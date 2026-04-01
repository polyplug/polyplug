// =============================================================================
// ABI FROZEN — pre-v1.0 (HostVTable rt_ctx refactoring)
// =============================================================================
//
// The following types and function signatures constitute the frozen polyplug ABI.
// NO CHANGES to #[repr(C)] structs, function pointer signatures, or the field
// order of HostVTable are permitted after this point.
//
// All new functionality must go through the host contract mechanism (get_host_contract).
// For rationale and trust model, see TRUST_MODEL.md.
// =============================================================================

//! ABI — `#[repr(C)]` types, constants, and FNV-1a hashing for the polyplug ABI boundary.
//!
//! Type definitions are sourced from `abi.toml` in this crate's root.

pub mod ffi;
pub mod tracking;

use core::ffi::c_void;

// Re-export hash functions from polyplug_utils
pub use polyplug_utils::{bundle_id, contract_id, fnv1a_64, host_contract_id, plugin_contract_id};

// ABI version sentinel — all bundles must export a function returning this value.
pub const POLYPLUG_ABI_VERSION: u32 = 1;

// ABI error codes (reserved: 0-255 runtime, 256+ plugin-defined)
pub const ABI_OK: u32 = 0;
pub const ABI_ERROR_GENERIC: u32 = 1;
pub const ABI_BUFFER_TOO_SMALL: u32 = 2; // caller must reallocate (see Buffer protocol)
pub const ABI_ERROR_PANIC: u32 = 3; // plugin panicked (caught by catch_unwind)
pub const ABI_ERROR_NOT_FOUND: u32 = 4; // plugin/contract not found
pub const ABI_ERROR_STALE_HANDLE: u32 = 5; // PluginHandle generation mismatch
pub const ABI_FUNCTION_NOT_AVAIL: u32 = 6; // function_id >= function_count
pub const ABI_ERROR_DUPLICATE_PROVIDER: u32 = 7; // same bundle already provides this contract
pub const ABI_ERROR_INVALID_POINTER: u32 = 8; // null or invalid pointer passed to ABI function

// Host contract error codes (reserved: 100-199 host contracts)
pub const ABI_HOST_CONTRACT_NOT_FOUND: u32 = 100; // host contract not found by contract_id
pub const ABI_HOST_CONTRACT_VERSION_MISMATCH: u32 = 101; // host contract version mismatch
pub const ABI_HOST_CONTRACT_CALL_FAILED: u32 = 102; // host contract function call failed

/// Non-owning UTF-8 string view.
///
/// OWNERSHIP: borrowed reference. `ptr` must remain valid for the duration
/// of the call. Never freed by the receiver.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct StringView {
    /// UTF-8 bytes, NOT null-terminated.
    pub ptr: *const u8,
    /// Byte count.
    pub len: usize,
}

// SAFETY: StringView is a read-only view into externally-owned data.
// The data pointed to is either 'static or valid for the lifetime of the call.
// Using StringView from multiple threads concurrently only reads the pointer —
// no mutation occurs. The caller guarantees the pointed-to data remains valid.
unsafe impl Send for StringView {}

// SAFETY: Same reasoning as Send — concurrent reads are safe.
unsafe impl Sync for StringView {}

/// Construct a StringView from a static byte slice.
pub const fn string_view_from_static(bytes: &'static [u8]) -> StringView {
    StringView {
        ptr: bytes.as_ptr(),
        len: bytes.len(),
    }
}

/// The null/empty StringView (ptr=null, len=0). Used for ABI_OK error messages.
pub const fn string_view_null() -> StringView {
    StringView {
        ptr: core::ptr::null(),
        len: 0,
    }
}

/// Returns the StringView contents as a `&str`.
///
/// # Safety
/// Caller must ensure `sv.ptr` is valid UTF-8 for `sv.len` bytes and the memory is live.
pub unsafe fn string_view_as_str(sv: &StringView) -> &str {
    // SAFETY: string_view_as_str is only called with host-owned StringViews created
    // via string_view_from_static — guarantees valid UTF-8.
    // Plugin-provided StringViews must never be passed to this function.
    unsafe {
        let slice: &[u8] = core::slice::from_raw_parts(sv.ptr, sv.len);
        core::str::from_utf8_unchecked(slice) // SAFETY: see comment above
    }
}

/// Copies the StringView contents into a new owned `String`.
///
/// # Safety
/// Caller must ensure `sv.ptr` is valid UTF-8 for `sv.len` bytes and the memory is live.
pub unsafe fn string_view_to_string_owned(sv: &StringView) -> String {
    // SAFETY: Caller guarantees ptr is valid, non-null, UTF-8, and live.
    unsafe { string_view_as_str(sv).to_owned() }
}

/// Owning byte buffer.
///
/// OWNERSHIP: `ptr` is always allocated via `polyplug_host_alloc`.
/// Owner calls `polyplug_host_free(ptr, cap, align)` when done.
#[repr(C)]
#[derive(Debug)]
pub struct Buffer {
    pub ptr: *mut u8,
    /// Bytes currently used.
    pub len: usize,
    /// Bytes allocated.
    pub cap: usize,
}

// SAFETY: Buffer owns its heap-allocated data through the host allocator.
// Sending between threads is safe because the host allocator is thread-safe.
unsafe impl Send for Buffer {}

/// Returns the buffer contents as a byte slice.
///
/// # Safety
/// Caller must ensure `buf.ptr` is valid for `buf.len` bytes and the memory is live.
pub unsafe fn buffer_as_slice(buf: &Buffer) -> &[u8] {
    // SAFETY: Caller guarantees ptr is non-null and valid for len bytes.
    unsafe { core::slice::from_raw_parts(buf.ptr, buf.len) }
}

/// Returns the buffer contents as a mutable byte slice.
///
/// # Safety
/// Caller must ensure `buf.ptr` is valid for `buf.cap` bytes, the memory is live, and no
/// other reference to the buffer exists.
pub unsafe fn buffer_as_mut_slice(buf: &mut Buffer) -> &mut [u8] {
    // SAFETY: Caller guarantees ptr is non-null, valid for cap bytes, and exclusively owned.
    unsafe { core::slice::from_raw_parts_mut(buf.ptr, buf.cap) }
}

/// ABI error — returned by value from all ABI calls.
///
/// OWNERSHIP: `code` is a value type. `message.ptr` is allocated by the callee
/// via `host_alloc`. Caller frees with `polyplug_host_free(message.ptr, message.len, 1)`
/// after reading. If `code == ABI_OK`, `message.ptr` is NULL — no free needed.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AbiError {
    /// 0 = success, non-zero = error.
    pub code: u32,
    /// Empty/NULL if success. UTF-8 message if non-zero code.
    pub message: StringView,
}

/// Construct a success AbiError.
pub const fn abi_error_ok() -> AbiError {
    AbiError {
        code: ABI_OK,
        message: string_view_null(),
    }
}

/// Construct a panic error with a static message.
pub const fn abi_error_panic_caught() -> AbiError {
    AbiError {
        code: ABI_ERROR_PANIC,
        message: string_view_from_static(b"plugin panicked"),
    }
}

/// Returns true if this represents success.
pub fn abi_error_is_ok(err: &AbiError) -> bool {
    err.code == ABI_OK
}

// SAFETY: AbiError contains a StringView which is Send+Sync, and a u32 code.
unsafe impl Send for AbiError {}

// SAFETY: AbiError contains a StringView which is Send+Sync (concurrent reads are safe), and a u32 code.
unsafe impl Sync for AbiError {}

/// Opaque handle to a loaded plugin — validated on use.
///
/// INTERNAL STRUCTURE: index into registry array + generation counter.
/// The generation counter detects use-after-unload.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PluginHandle {
    /// Slot in the registry array.
    pub index: u32,
    /// Incremented on unload — detects stale handles.
    pub generation: u32,
}

/// The null/invalid handle. Never returned by a successful lookup.
pub const fn plugin_handle_null() -> PluginHandle {
    PluginHandle {
        index: u32::MAX,
        generation: 0,
    }
}

/// Returns true if this is the null handle.
pub fn plugin_handle_is_null(handle: &PluginHandle) -> bool {
    handle.index == u32::MAX
}

/// Opaque host context passed to plugin functions via rt_ctx parameter.
///
/// Contains the runtime pointer and the bundle_id of the calling bundle.
/// The actual implementation is in the polyplug crate; this definition
/// establishes the ABI layout.
///
/// OWNERSHIP: `'static`, lives as long as the runtime.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct HostContext {
    /// Opaque pointer to the Runtime. Never dereferenced by plugins.
    pub runtime: *mut core::ffi::c_void,
    /// Bundle ID of the calling bundle for dependency enforcement.
    pub bundle_id: u64,
}

// SAFETY: HostContext contains a raw pointer (which is Send+Sync as raw ptr)
// and a u64. The pointer is only dereferenced by the host runtime.
unsafe impl Send for HostContext {}

// SAFETY: HostContext contains only a raw pointer and a u64.
// Concurrent reads are safe — no mutation occurs through shared references.
unsafe impl Sync for HostContext {}

// ─── Dispatch Types for Hybrid Native/VM Plugins ─────────────────────────────

/// Dispatch mechanism type — determines how function calls are routed.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchType {
    /// Native dispatch: direct function pointer calls (zero overhead).
    Native = 0,
    /// VM dispatch: call through a dispatch function with loader_data.
    VirtualMachine = 1,
}

// SAFETY: DispatchType is a simple C enum (Copy type). Safe to share across threads.
unsafe impl Send for DispatchType {}

// SAFETY: DispatchType is a simple C enum (Copy type). Concurrent reads are safe.
unsafe impl Sync for DispatchType {}

/// Native dispatch data — direct function pointer array.
///
/// Used when `dispatch_type == DispatchType::Native`.
/// The `functions` array contains `function_count` function pointers.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct NativeDispatch {
    /// Pointer to a static array of function pointers, indexed by function_id.
    pub functions: *const *const (),
}

// SAFETY: NativeDispatch contains only a pointer to static data.
// The function pointers are 'static and safe to call from any thread.
unsafe impl Send for NativeDispatch {}

// SAFETY: NativeDispatch contains only a pointer to static data.
// Concurrent reads of the pointer are safe.
unsafe impl Sync for NativeDispatch {}

/// VM dispatch data — call through a dispatch function.
///
/// Used when `dispatch_type == DispatchType::VirtualMachine`.
/// The `call` function receives `loader_data` which contains VM-specific state.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VmDispatch {
    /// Dispatch function called for every VM function invocation.
    ///
    /// # Arguments
    /// - `loader_data`: VM-specific data (cast from `*mut c_void`)
    /// - `fn_id`: Function index within the contract
    /// - `args`: Pointer to packed arguments (ABI-specific layout)
    /// - `out`: Pointer to output buffer for return value
    pub call: unsafe extern "C" fn(
        loader_data: *mut core::ffi::c_void,
        fn_id: u32,
        args: *const (),
        out: *mut (),
    ) -> AbiError,
    /// Loader-specific data (e.g., LuaLoaderData, JsLoaderData).
    /// Opaque to the host; interpreted by the dispatch function.
    pub loader_data: *mut core::ffi::c_void,
}

// SAFETY: VmDispatch contains a function pointer and a raw pointer.
// The function pointer is safe to call from any thread (the dispatch function
// must handle its own synchronization). The loader_data pointer is owned by
// the loader and must be thread-safe.
unsafe impl Send for VmDispatch {}

// SAFETY: VmDispatch contains only a function pointer and a raw pointer.
// Concurrent calls to the dispatch function must be safe (loader's responsibility).
unsafe impl Sync for VmDispatch {}

/// Union of dispatch mechanisms — use based on `dispatch_type`.
///
/// # Safety
/// Access the correct variant based on `PluginInterface::dispatch_type`:
/// - `dispatch_type == Native` → access `.native`
/// - `dispatch_type == VirtualMachine` → access `.vm`
#[repr(C)]
pub union PluginDispatch {
    /// Native dispatch data (when dispatch_type == Native).
    pub native: NativeDispatch,
    /// VM dispatch data (when dispatch_type == VirtualMachine).
    pub vm: VmDispatch,
}

// SAFETY: PluginDispatch is a union of Send+Sync types.
// The caller must access the correct variant based on dispatch_type.
unsafe impl Send for PluginDispatch {}

// SAFETY: PluginDispatch is a union of Send+Sync types.
// Concurrent access requires the caller to use the correct variant.
unsafe impl Sync for PluginDispatch {}

/// Plugin interface — one per contract implemented by a plugin.
///
/// OWNERSHIP: Must be `'static` or intentionally leaked.
/// Never stack-allocated. Never freed while runtime lives.
///
/// # Dispatch
/// - `dispatch_type == Native`: Call via `dispatch.native.functions[fn_id]`
/// - `dispatch_type == VirtualMachine`: Call via `dispatch.vm.call(...)`
#[repr(C)]
pub struct PluginInterface {
    /// Pointer to the host context for this plugin.
    /// Used for host function calls and dependency enforcement.
    pub rt_ctx: *const HostContext,
    /// FNV-1a hash of "contract_name@major_version".
    pub contract_id: u64,
    /// minor.patch encoded as `(minor << 16 | patch)`.
    pub contract_version: u32,
    /// Number of valid entries in the dispatch array.
    pub function_count: u32,
    /// Dispatch mechanism type (Native or VirtualMachine).
    pub dispatch_type: DispatchType,
    /// Union of dispatch mechanisms — access based on dispatch_type.
    pub dispatch: PluginDispatch,
}

// SAFETY: PluginInterface contains only data that is 'static or thread-safe.
// - rt_ctx: points to a HostContext owned by the runtime
// - contract_id, contract_version, function_count: plain data
// - dispatch_type: C enum (Copy)
// - dispatch: union of Send+Sync types
// Sending/sharing across threads only reads these values.
unsafe impl Send for PluginInterface {}

// SAFETY: PluginInterface contains only data that is 'static or thread-safe.
// Concurrent reads are safe — no mutation occurs through shared references.
unsafe impl Sync for PluginInterface {}

// ─── Host Runtime and Contract Types ─────────────────────────────────────────

/// Host runtime type identifier — identifies the language/runtime hosting plugins.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostRuntime {
    Rust = 0,
    Python = 1,
    Lua = 2,
    JavaScript = 3,
}

// SAFETY: HostRuntime is a simple C enum (Copy type). Safe to share across threads.
unsafe impl Send for HostRuntime {}

// SAFETY: HostRuntime is a simple C enum (Copy type). Concurrent reads are safe.
unsafe impl Sync for HostRuntime {}

/// Host contract vtable header — metadata for a host-provided contract.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct HostContractVTableHeader {
    /// VTable format version (for future compatibility).
    pub vtable_version: u32,
    /// FNV-1a hash of "contract_name@major_version".
    pub contract_id: u64,
    /// Contract major version.
    pub contract_major: u32,
    /// Contract minor version.
    pub contract_minor: u32,
    /// Number of functions in this contract.
    pub function_count: u32,
    /// Dispatch mechanism type (Native or VirtualMachine).
    pub dispatch_type: DispatchType,
}

// SAFETY: HostContractVTableHeader contains only plain data types (u32, u64, DispatchType).
// All fields are Copy types safe to share across threads.
unsafe impl Send for HostContractVTableHeader {}

// SAFETY: HostContractVTableHeader contains only plain data types.
// Concurrent reads are safe — no mutation occurs through shared references.
unsafe impl Sync for HostContractVTableHeader {}

/// Native dispatch for host contracts — direct function pointer array.
///
/// Used when `dispatch_type == DispatchType::Native`.
/// The `functions` array contains `function_count` function pointers.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct NativeHostContractDispatch {
    /// Pointer to the implementation (e.g., Box<dyn Trait> as *const c_void).
    /// This is passed as the first argument to all native dispatch functions.
    pub impl_ptr: *const c_void,
    /// Pointer to a static array of function pointers, indexed by function_id.
    pub functions: *const *const (),
}

// SAFETY: NativeHostContractDispatch contains only pointers to static data.
// The impl_ptr points to a 'static implementation or one owned by the host.
// The function pointers are 'static and safe to call from any thread.
unsafe impl Send for NativeHostContractDispatch {}

// SAFETY: NativeHostContractDispatch contains only pointers to static data.
// Concurrent reads of the pointers are safe.
unsafe impl Sync for NativeHostContractDispatch {}

/// VM dispatch for host contracts — call through a dispatch function.
///
/// Used when `dispatch_type == DispatchType::VirtualMachine`.
/// The `call` function receives `bridge_data` which contains VM-specific state.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VmHostContractDispatch {
    /// Dispatch function called for every VM function invocation.
    ///
    /// # Arguments
    /// - `bridge_data`: VM-specific data (cast from `*mut c_void`)
    /// - `fn_id`: Function index within the contract
    /// - `args`: Pointer to packed arguments (ABI-specific layout)
    /// - `out`: Pointer to output buffer for return value
    pub call: unsafe extern "C" fn(
        bridge_data: *mut core::ffi::c_void,
        fn_id: u32,
        args: *const (),
        out: *mut (),
    ) -> AbiError,
    /// VM-specific data (opaque to the host; interpreted by the dispatch function).
    pub bridge_data: *mut core::ffi::c_void,
}

// SAFETY: VmHostContractDispatch contains a function pointer and a raw pointer.
// The function pointer is safe to call from any thread (the dispatch function
// must handle its own synchronization). The bridge_data pointer is owned by
// the VM bridge and must be thread-safe.
unsafe impl Send for VmHostContractDispatch {}

// SAFETY: VmHostContractDispatch contains only a function pointer and a raw pointer.
// Concurrent calls to the dispatch function must be safe (VM bridge's responsibility).
unsafe impl Sync for VmHostContractDispatch {}

/// Union of host contract dispatch mechanisms — use based on `dispatch_type`.
///
/// # Safety
/// Access the correct variant based on `HostContractVTableHeader::dispatch_type`:
/// - `dispatch_type == Native` → access `.native`
/// - `dispatch_type == VirtualMachine` → access `.vm`
#[repr(C)]
pub union HostContractDispatch {
    /// Native dispatch data (when dispatch_type == Native).
    pub native: NativeHostContractDispatch,
    /// VM dispatch data (when dispatch_type == VirtualMachine).
    pub vm: VmHostContractDispatch,
}

// SAFETY: HostContractDispatch is a union of Send+Sync types.
// The caller must access the correct variant based on dispatch_type.
unsafe impl Send for HostContractDispatch {}

// SAFETY: HostContractDispatch is a union of Send+Sync types.
// Concurrent access requires the caller to use the correct variant.
unsafe impl Sync for HostContractDispatch {}

/// Host contract vtable — complete interface for a host-provided contract.
///
/// OWNERSHIP: Must be `'static` or intentionally leaked.
/// Never stack-allocated. Never freed while runtime lives.
///
/// # Dispatch
/// - `dispatch_type == Native`: Call via `dispatch.native.functions[fn_id]`
/// - `dispatch_type == VirtualMachine`: Call via `dispatch.vm.call(...)`
#[repr(C)]
pub struct HostContractVTable {
    /// Header containing contract metadata.
    pub header: HostContractVTableHeader,
    /// Union of dispatch mechanisms — access based on dispatch_type.
    pub dispatch: HostContractDispatch,
}

// SAFETY: HostContractVTable contains only data that is 'static or thread-safe.
// - header: plain data types (Send+Sync)
// - dispatch: union of Send+Sync types
// Sending/sharing across threads only reads these values.
unsafe impl Send for HostContractVTable {}

// SAFETY: HostContractVTable contains only data that is 'static or thread-safe.
// Concurrent reads are safe — no mutation occurs through shared references.
unsafe impl Sync for HostContractVTable {}

/// Host capabilities passed to every plugin at init time.
///
/// OWNERSHIP: `'static`, lives as long as the runtime.
///
/// All functions take `rt_ctx` as first parameter - an opaque pointer to the Runtime.
/// This allows each Runtime to have its own isolated state (no global registry).
#[repr(C)]
pub struct HostVTable {
    pub register_plugin: unsafe extern "C" fn(
        rt_ctx: *mut core::ffi::c_void,
        descriptor: *const PluginDescriptor,
        vtable: *const PluginInterface,
    ) -> AbiError,
    pub alloc:
        unsafe extern "C" fn(rt_ctx: *mut core::ffi::c_void, size: usize, align: usize) -> *mut u8,
    pub free: unsafe extern "C" fn(
        rt_ctx: *mut core::ffi::c_void,
        ptr: *mut u8,
        size: usize,
        align: usize,
    ),
    pub find_by_contract: unsafe extern "C" fn(
        rt_ctx: *mut core::ffi::c_void,
        contract_id: u64,
        min_version: u32,
    ) -> PluginHandle,
    pub find_by_bundle: unsafe extern "C" fn(
        rt_ctx: *mut core::ffi::c_void,
        bundle_id: u64,
        contract_id: u64,
        min_version: u32,
    ) -> PluginHandle,
    pub find_all_by_contract: unsafe extern "C" fn(
        rt_ctx: *mut core::ffi::c_void,
        contract_id: u64,
        min_version: u32,
        out: *mut PluginHandle,
        out_cap: usize,
    ) -> usize,
    pub resolve_plugin: unsafe extern "C" fn(
        rt_ctx: *mut core::ffi::c_void,
        handle: PluginHandle,
    ) -> *const PluginInterface,
    /// Get host contract vtable by contract_id and minimum version.
    /// Returns null if no host contract matches the criteria.
    pub get_host_contract: unsafe extern "C" fn(
        rt_ctx: *mut core::ffi::c_void,
        contract_id: u64,
        min_version: u32,
    ) -> *const HostContractVTable,
}

// SAFETY: HostVTable contains only function pointers.
// Function pointers are inherently thread-safe to call from any thread
// (the functions themselves must handle their own synchronization).
unsafe impl Send for HostVTable {}

// SAFETY: HostVTable contains only function pointers.
// Function pointers are inherently thread-safe to call from any thread
// (the functions themselves must handle their own synchronization).
unsafe impl Sync for HostVTable {}

/// Metadata about a plugin within a bundle.
///
/// OWNERSHIP: value type passed by pointer during init. The `name` and
/// `contract_name` StringViews are borrowed from the plugin's static memory.
/// The receiver must not free or outlive the plugin's library.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PluginDescriptor {
    /// Human-readable plugin name.
    pub name: StringView,
    /// Full contract name for collision detection.
    pub contract_name: StringView,
    pub version_major: u32,
    pub version_minor: u32,
    pub version_patch: u32,
}

// SAFETY: PluginDescriptor contains only StringViews (which are Send+Sync)
// and u32 values. All safe to share across threads.
unsafe impl Send for PluginDescriptor {}

// SAFETY: PluginDescriptor contains only StringViews (which are Send+Sync)
// and u32 values. All safe to share across threads.
unsafe impl Sync for PluginDescriptor {}

/// Context passed to every guest `polyplug_init()` function.
/// The `bundle_path` pointer is runtime-owned and valid for the lifetime of the `PluginRuntime`.
/// **Plugin must not store the raw pointer** — copy the string value if persistence is needed.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PluginContext {
    /// Absolute canonical path to the directory containing the loaded bundle.
    pub bundle_path: StringView,

    /// Host's supported ABI version for negotiation (Option C).
    /// Plugin can use this to determine available features.
    pub host_abi_version: u32,

    /// Bundle ID for dependency enforcement during init.
    pub bundle_id: u64,
}

/// Configuration passed to `polyplug_runtime_create` during runtime initialisation.
///
/// OWNERSHIP: borrowed for the duration of the runtime build only.
/// The caller may free all pointed-to memory after the build
/// returns. The runtime copies any data it needs to retain.
#[repr(C)]
pub struct RuntimeConfig {
    /// Plugin directories to scan (array of `plugin_dir_count` StringViews).
    pub plugin_dirs: *const StringView,
    pub plugin_dir_count: usize,
    /// Compatibility mode: 0 = Strict (only mode implemented in MVP).
    pub compatibility: u32,
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{align_of, offset_of, size_of};

    #[test]
    fn fnv1a_known_values() {
        // Known FNV-1a 64-bit value for empty string (FNV offset basis)
        assert_eq!(fnv1a_64(b""), 0xcbf29ce484222325);
        // Golden value: FNV-1a of "image.decode@1"
        assert_eq!(fnv1a_64(b"image.decode@1"), 0xa1ba05dd7da18569_u64);
        // Verify determinism
        assert_eq!(fnv1a_64(b"image.decode@1"), fnv1a_64(b"image.decode@1"));
        // Different inputs produce different hashes
        assert_ne!(fnv1a_64(b"image.decode@1"), fnv1a_64(b"image.decode@2"));
    }

    #[test]
    fn contract_id_canonical_format() {
        // Same name+major always produces same ID
        let id1: u64 = contract_id("image.decode", 1);
        let id2: u64 = contract_id("image.decode", 1);
        assert_eq!(id1, id2);
        // Different major versions produce different IDs
        let id3: u64 = contract_id("image.decode", 2);
        assert_ne!(id1, id3);
        // Different names produce different IDs
        let id4: u64 = contract_id("audio.decode", 1);
        assert_ne!(id1, id4);
    }

    #[test]
    fn contract_id_golden_values() {
        // Golden: FNV-1a of "image.decode@1"
        assert_eq!(contract_id("image.decode", 1), 0xa1ba05dd7da18569_u64);
        // Golden: FNV-1a of "audio.encode@2"
        assert_eq!(contract_id("audio.encode", 2), 0x7a7958404b1d72a5_u64);
    }

    #[test]
    fn contract_id_collision() {
        // Host and plugin contract IDs must never collide for same name+major
        let host_id: u64 = host_contract_id("logger", 1);
        let plugin_id: u64 = plugin_contract_id("logger", 1);
        assert_ne!(
            host_id, plugin_id,
            "host and plugin contract IDs must differ"
        );

        // Both must be deterministic
        assert_eq!(host_contract_id("logger", 1), host_contract_id("logger", 1));
        assert_eq!(
            plugin_contract_id("logger", 1),
            plugin_contract_id("logger", 1)
        );

        // Different names produce different IDs within same category
        assert_ne!(
            host_contract_id("logger", 1),
            host_contract_id("metrics", 1)
        );
        assert_ne!(
            plugin_contract_id("logger", 1),
            plugin_contract_id("metrics", 1)
        );

        // Different major versions produce different IDs within same category
        assert_ne!(host_contract_id("logger", 1), host_contract_id("logger", 2));
        assert_ne!(
            plugin_contract_id("logger", 1),
            plugin_contract_id("logger", 2)
        );
    }

    #[test]
    fn bundle_id_stability() {
        // Same input always yields same output
        assert_eq!(bundle_id("my-bundle"), bundle_id("my-bundle"));
        // Golden: FNV-1a of "my-bundle"
        assert_eq!(bundle_id("my-bundle"), 0xfe6226876e3a35b2_u64);
        // Golden: FNV-1a of "polyplug-core"
        assert_eq!(bundle_id("polyplug-core"), 0x6ef4aee714f5f991_u64);
        // Different bundle names produce different IDs
        assert_ne!(bundle_id("bundle-a"), bundle_id("bundle-b"));
    }

    #[test]
    fn test_plugin_handle_null() {
        let h: PluginHandle = plugin_handle_null();
        assert!(plugin_handle_is_null(&h));
        let valid: PluginHandle = PluginHandle {
            index: 0,
            generation: 1,
        };
        assert!(!plugin_handle_is_null(&valid));
    }

    #[test]
    fn test_abi_error_ok() {
        let e: AbiError = abi_error_ok();
        assert!(abi_error_is_ok(&e));
        assert_eq!(e.code, ABI_OK);
    }

    // ── ABI Layout Tests ─────────────────────────────────────────────────────
    //
    // These tests verify the frozen ABI struct layouts on the target platform.
    // If any assertion fails, it means the ABI has changed — a §7 violation.
    // Add offset_of tests for any new fields added to these structs.

    #[test]
    fn layout_string_view() {
        assert_eq!(size_of::<StringView>(), 16);
        assert_eq!(align_of::<StringView>(), 8);
        assert_eq!(offset_of!(StringView, ptr), 0);
        assert_eq!(offset_of!(StringView, len), 8);
    }

    #[test]
    fn layout_buffer() {
        assert_eq!(size_of::<Buffer>(), 24);
        assert_eq!(align_of::<Buffer>(), 8);
        assert_eq!(offset_of!(Buffer, ptr), 0);
        assert_eq!(offset_of!(Buffer, len), 8);
        assert_eq!(offset_of!(Buffer, cap), 16);
    }

    #[test]
    fn layout_abi_error() {
        assert_eq!(size_of::<AbiError>(), 24);
        assert_eq!(align_of::<AbiError>(), 8);
        assert_eq!(offset_of!(AbiError, code), 0);
        assert_eq!(offset_of!(AbiError, message), 8);
    }

    #[test]
    fn layout_plugin_handle() {
        assert_eq!(size_of::<PluginHandle>(), 8);
        assert_eq!(align_of::<PluginHandle>(), 4);
        assert_eq!(offset_of!(PluginHandle, index), 0);
        assert_eq!(offset_of!(PluginHandle, generation), 4);
    }

    #[test]
    fn layout_host_context() {
        assert_eq!(size_of::<HostContext>(), 16);
        assert_eq!(align_of::<HostContext>(), 8);
        assert_eq!(offset_of!(HostContext, runtime), 0);
        assert_eq!(offset_of!(HostContext, bundle_id), 8);
    }

    #[test]
    fn layout_dispatch_type() {
        assert_eq!(size_of::<DispatchType>(), 4);
        assert_eq!(align_of::<DispatchType>(), 4);
    }

    #[test]
    fn layout_native_dispatch() {
        assert_eq!(size_of::<NativeDispatch>(), 8);
        assert_eq!(align_of::<NativeDispatch>(), 8);
        assert_eq!(offset_of!(NativeDispatch, functions), 0);
    }

    #[test]
    fn layout_vm_dispatch() {
        assert_eq!(size_of::<VmDispatch>(), 16);
        assert_eq!(align_of::<VmDispatch>(), 8);
        assert_eq!(offset_of!(VmDispatch, call), 0);
        assert_eq!(offset_of!(VmDispatch, loader_data), 8);
    }

    #[test]
    fn layout_plugin_dispatch() {
        assert_eq!(size_of::<PluginDispatch>(), 16);
        assert_eq!(align_of::<PluginDispatch>(), 8);
    }

    #[test]
    fn layout_plugin_interface() {
        assert_eq!(size_of::<PluginInterface>(), 48);
        assert_eq!(align_of::<PluginInterface>(), 8);
        assert_eq!(offset_of!(PluginInterface, rt_ctx), 0);
        assert_eq!(offset_of!(PluginInterface, contract_id), 8);
        assert_eq!(offset_of!(PluginInterface, contract_version), 16);
        assert_eq!(offset_of!(PluginInterface, function_count), 20);
        assert_eq!(offset_of!(PluginInterface, dispatch_type), 24);
        assert_eq!(offset_of!(PluginInterface, dispatch), 32);
    }

    #[test]
    fn layout_host_vtable() {
        // HostVTable: 8 extern "C" fn pointers, each 8 bytes on x86_64.
        assert_eq!(size_of::<HostVTable>(), 64);
        assert_eq!(align_of::<HostVTable>(), 8);
        assert_eq!(offset_of!(HostVTable, register_plugin), 0);
        assert_eq!(offset_of!(HostVTable, alloc), 8);
        assert_eq!(offset_of!(HostVTable, free), 16);
        assert_eq!(offset_of!(HostVTable, find_by_contract), 24);
        assert_eq!(offset_of!(HostVTable, find_by_bundle), 32);
        assert_eq!(offset_of!(HostVTable, find_all_by_contract), 40);
        assert_eq!(offset_of!(HostVTable, resolve_plugin), 48);
        assert_eq!(offset_of!(HostVTable, get_host_contract), 56);
    }

    #[test]
    fn layout_plugin_descriptor() {
        // name(16) + contract_name(16) + version_major(4) + version_minor(4) + version_patch(4)
        // + 4 bytes tail padding = 48 bytes on x86_64.
        assert_eq!(size_of::<PluginDescriptor>(), 48);
        assert_eq!(align_of::<PluginDescriptor>(), 8);
        assert_eq!(offset_of!(PluginDescriptor, name), 0);
        assert_eq!(offset_of!(PluginDescriptor, contract_name), 16);
        assert_eq!(offset_of!(PluginDescriptor, version_major), 32);
        assert_eq!(offset_of!(PluginDescriptor, version_minor), 36);
        assert_eq!(offset_of!(PluginDescriptor, version_patch), 40);
    }

    #[test]
    fn layout_runtime_config() {
        // plugin_dirs ptr(8) + plugin_dir_count(8) + compatibility(4) + padding(4) = 24 bytes.
        assert_eq!(size_of::<RuntimeConfig>(), 24);
        assert_eq!(align_of::<RuntimeConfig>(), 8);
        assert_eq!(offset_of!(RuntimeConfig, plugin_dirs), 0);
        assert_eq!(offset_of!(RuntimeConfig, plugin_dir_count), 8);
        assert_eq!(offset_of!(RuntimeConfig, compatibility), 16);
    }

    #[test]
    #[cfg(target_pointer_width = "64")]
    fn plugin_context_layout() {
        // PluginContext: StringView(16) + u32(4) + padding(4) + u64(8) = 32 bytes
        assert_eq!(size_of::<PluginContext>(), 32);
        assert_eq!(align_of::<PluginContext>(), 8);
        assert_eq!(offset_of!(PluginContext, bundle_path), 0);
        assert_eq!(offset_of!(PluginContext, host_abi_version), 16);
        assert_eq!(offset_of!(PluginContext, bundle_id), 24);
    }

    // ── Host Contract Layout Tests ─────────────────────────────────────────────

    #[test]
    fn layout_host_runtime() {
        assert_eq!(size_of::<HostRuntime>(), 1);
        assert_eq!(align_of::<HostRuntime>(), 1);
    }

    #[test]
    fn layout_host_contract_vtable_header() {
        // vtable_version(4) + padding(4) + contract_id(8) + contract_major(4)
        // + contract_minor(4) + function_count(4) + dispatch_type(4) = 32 bytes
        assert_eq!(size_of::<HostContractVTableHeader>(), 32);
        assert_eq!(align_of::<HostContractVTableHeader>(), 8);
        assert_eq!(offset_of!(HostContractVTableHeader, vtable_version), 0);
        assert_eq!(offset_of!(HostContractVTableHeader, contract_id), 8);
        assert_eq!(offset_of!(HostContractVTableHeader, contract_major), 16);
        assert_eq!(offset_of!(HostContractVTableHeader, contract_minor), 20);
        assert_eq!(offset_of!(HostContractVTableHeader, function_count), 24);
        assert_eq!(offset_of!(HostContractVTableHeader, dispatch_type), 28);
    }

    #[test]
    fn layout_native_host_contract_dispatch() {
        // impl_ptr(8) + functions(8) = 16 bytes
        assert_eq!(size_of::<NativeHostContractDispatch>(), 16);
        assert_eq!(align_of::<NativeHostContractDispatch>(), 8);
        assert_eq!(offset_of!(NativeHostContractDispatch, impl_ptr), 0);
        assert_eq!(offset_of!(NativeHostContractDispatch, functions), 8);
    }

    #[test]
    fn layout_vm_host_contract_dispatch() {
        assert_eq!(size_of::<VmHostContractDispatch>(), 16);
        assert_eq!(align_of::<VmHostContractDispatch>(), 8);
        assert_eq!(offset_of!(VmHostContractDispatch, call), 0);
        assert_eq!(offset_of!(VmHostContractDispatch, bridge_data), 8);
    }

    #[test]
    fn layout_host_contract_dispatch() {
        assert_eq!(size_of::<HostContractDispatch>(), 16);
        assert_eq!(align_of::<HostContractDispatch>(), 8);
    }

    #[test]
    fn layout_host_contract_vtable() {
        // header(32) + dispatch(16) = 48 bytes
        assert_eq!(size_of::<HostContractVTable>(), 48);
        assert_eq!(align_of::<HostContractVTable>(), 8);
        assert_eq!(offset_of!(HostContractVTable, header), 0);
        assert_eq!(offset_of!(HostContractVTable, dispatch), 32);
    }

    // ── Send/Sync Tests for Host Contract Types ──────────────────────────────

    /// Compile-time assertion that a type implements Send.
    const fn assert_send<T: Send>() {}

    /// Compile-time assertion that a type implements Sync.
    const fn assert_sync<T: Sync>() {}

    #[test]
    fn host_contract_types_are_send() {
        // SAFETY: HostContractVTableHeader contains only plain data types (u32, u64, DispatchType).
        // All fields are Copy types safe to share across threads.
        assert_send::<HostContractVTableHeader>();

        // SAFETY: NativeHostContractDispatch contains only a pointer to static data.
        // The function pointers are 'static and safe to call from any thread.
        assert_send::<NativeHostContractDispatch>();

        // SAFETY: VmHostContractDispatch contains a function pointer and a raw pointer.
        // The function pointer is safe to call from any thread (the dispatch function
        // must handle its own synchronization). The bridge_data pointer is owned by
        // the VM bridge and must be thread-safe.
        assert_send::<VmHostContractDispatch>();

        // SAFETY: HostContractDispatch is a union of Send+Sync types.
        // The caller must access the correct variant based on dispatch_type.
        assert_send::<HostContractDispatch>();

        // SAFETY: HostContractVTable contains only data that is 'static or thread-safe.
        // - header: plain data types (Send+Sync)
        // - dispatch: union of Send+Sync types
        // Sending/sharing across threads only reads these values.
        assert_send::<HostContractVTable>();

        // SAFETY: DispatchType is a simple C enum (Copy type). Safe to share across threads.
        assert_send::<DispatchType>();
    }

    #[test]
    fn host_contract_types_are_sync() {
        // SAFETY: HostContractVTableHeader contains only plain data types.
        // Concurrent reads are safe — no mutation occurs through shared references.
        assert_sync::<HostContractVTableHeader>();

        // SAFETY: NativeHostContractDispatch contains only a pointer to static data.
        // Concurrent reads of the pointer are safe.
        assert_sync::<NativeHostContractDispatch>();

        // SAFETY: VmHostContractDispatch contains only a function pointer and a raw pointer.
        // Concurrent calls to the dispatch function must be safe (VM bridge's responsibility).
        assert_sync::<VmHostContractDispatch>();

        // SAFETY: HostContractDispatch is a union of Send+Sync types.
        // Concurrent access requires the caller to use the correct variant.
        assert_sync::<HostContractDispatch>();

        // SAFETY: HostContractVTable contains only data that is 'static or thread-safe.
        // Concurrent reads are safe — no mutation occurs through shared references.
        assert_sync::<HostContractVTable>();

        // SAFETY: DispatchType is a simple C enum (Copy type). Concurrent reads are safe.
        assert_sync::<DispatchType>();
    }
}

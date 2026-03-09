// =============================================================================
// ABI FROZEN AS OF EPIC 9.7
// =============================================================================
//
// The following types and function signatures constitute the frozen polyplug ABI.
// NO CHANGES to #[repr(C)] structs, function pointer signatures, or the field
// order of HostVTable are permitted after this point.
//
// All new functionality must go through the extension mechanism (get_extension).
// For rationale and trust model, see TRUST_MODEL.md.
// =============================================================================

//! ABI — `#[repr(C)]` types, constants, and FNV-1a hashing for the polyplug ABI boundary.

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

impl StringView {
    /// Construct a StringView from a static byte slice.
    pub const fn from_static(bytes: &'static [u8]) -> StringView {
        StringView {
            ptr: bytes.as_ptr(),
            len: bytes.len(),
        }
    }

    /// The null/empty StringView (ptr=null, len=0). Used for ABI_OK error messages.
    pub const fn null() -> StringView {
        StringView {
            ptr: core::ptr::null(),
            len: 0,
        }
    }
}

// SAFETY: StringView is a read-only view into externally-owned data.
// The data pointed to is either 'static or valid for the lifetime of the call.
// Using StringView from multiple threads concurrently only reads the pointer —
// no mutation occurs. The caller guarantees the pointed-to data remains valid.
unsafe impl Send for StringView {}
// SAFETY: Same reasoning as Send — concurrent reads are safe.
unsafe impl Sync for StringView {}

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

impl AbiError {
    /// Construct a success AbiError.
    pub const fn ok() -> AbiError {
        AbiError {
            code: ABI_OK,
            message: StringView::null(),
        }
    }

    /// Construct a panic error with a static message.
    pub const fn panic_caught() -> AbiError {
        AbiError {
            code: ABI_ERROR_PANIC,
            message: StringView::from_static(b"plugin panicked"),
        }
    }

    /// Returns true if this represents success.
    pub fn is_ok(self) -> bool {
        self.code == ABI_OK
    }
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

impl PluginHandle {
    /// The null/invalid handle. Never returned by a successful lookup.
    pub const fn null() -> PluginHandle {
        PluginHandle {
            index: u32::MAX,
            generation: 0,
        }
    }

    /// Returns true if this is the null handle.
    pub fn is_null(self) -> bool {
        self.index == u32::MAX
    }
}

/// Plugin VTable — one per contract implemented by a plugin.
///
/// OWNERSHIP: Must be `'static` or intentionally leaked.
/// Never stack-allocated. Never freed while runtime lives.
#[repr(C)]
pub struct PluginVTable {
    /// FNV-1a hash of "contract_name@major_version".
    pub contract_id: u64,
    /// minor.patch encoded as `(minor << 16 | patch)`.
    pub contract_version: u32,
    /// Number of valid entries in the `functions` array.
    pub function_count: u32,
    /// Pointer to a static array of function pointers, indexed by function_id.
    pub functions: *const *const (),
}

// SAFETY: PluginVTable only contains data that is 'static (the functions pointer
// points to a static array). All fields are copy types or static pointers.
// Sending/sharing across threads only reads these static pointers.
unsafe impl Send for PluginVTable {}
// SAFETY: PluginVTable only contains data that is 'static (the functions pointer
// points to a static array). All fields are copy types or static pointers.
// Sending/sharing across threads only reads these static pointers.
unsafe impl Sync for PluginVTable {}

/// Host capabilities passed to every plugin at init time.
///
/// OWNERSHIP: `'static`, lives as long as the runtime.
#[repr(C)]
pub struct HostVTable {
    pub alloc: unsafe extern "C" fn(size: usize, align: usize) -> *mut u8,
    pub free: unsafe extern "C" fn(ptr: *mut u8, size: usize, align: usize),
    pub find_by_contract: unsafe extern "C" fn(contract_id: u64, min_version: u32) -> PluginHandle,
    pub find_by_bundle:
        unsafe extern "C" fn(bundle_id: u64, contract_id: u64, min_version: u32) -> PluginHandle,
    pub find_all_by_contract: unsafe extern "C" fn(
        contract_id: u64,
        min_version: u32,
        out: *mut PluginHandle,
        out_cap: usize,
    ) -> usize,
    pub resolve_plugin: unsafe extern "C" fn(handle: PluginHandle) -> *const PluginVTable,
    pub get_extension: unsafe extern "C" fn(extension_id: u32) -> *const (),
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

/// Bridge used during `polyplug_init` only — not stored long-term.
///
/// OWNERSHIP: stack-allocated by the host, passed by pointer to the plugin.
/// Never stored by the plugin. The `host` pointer is `'static` (valid for
/// the runtime lifetime). Never freed by the plugin.
#[repr(C)]
pub struct PluginRegistrar {
    pub register_plugin: unsafe extern "C" fn(
        registrar: *mut PluginRegistrar,
        descriptor: *const PluginDescriptor,
        vtable: *const PluginVTable,
    ) -> AbiError,
    pub host: *const HostVTable,
}

/// A single extension entry in the runtime config.
///
/// OWNERSHIP: the `vtable` pointer must be `'static` (valid for the runtime
/// lifetime). `ExtensionEntry` arrays are passed by pointer to `RuntimeConfig`
/// and never owned or freed by the runtime.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ExtensionEntry {
    /// FNV-1a lower 32 bits of the extension name.
    pub extension_id: u32,
    /// Pointer to the extension's vtable struct.
    pub vtable: *const (),
}

// SAFETY: ExtensionEntry holds a u32 and a pointer to a static vtable.
unsafe impl Send for ExtensionEntry {}
// SAFETY: ExtensionEntry holds a u32 and a pointer to a static vtable.
// Sharing across threads only reads these values — no mutation after construction.
unsafe impl Sync for ExtensionEntry {}

/// Configuration passed to `polyplug_runtime_init`.
///
/// OWNERSHIP: borrowed for the duration of `polyplug_runtime_init` only.
/// The caller may free all pointed-to memory after `polyplug_runtime_init`
/// returns. The runtime copies any data it needs to retain.
#[repr(C)]
pub struct RuntimeConfig {
    /// Plugin directories to scan (array of `plugin_dir_count` StringViews).
    pub plugin_dirs: *const StringView,
    pub plugin_dir_count: usize,
    /// Compatibility mode: 0 = Strict (only mode implemented in MVP).
    pub compatibility: u32,
    /// Extensions provided by the host (array of `extension_count` entries).
    pub extensions: *const ExtensionEntry,
    pub extension_count: usize,
}

// ─── FNV-1a Hash ─────────────────────────────────────────────────────────────

/// FNV-1a 64-bit hash — used for contract IDs.
pub(crate) fn fnv1a_64(data: &[u8]) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x00000100000001B3;
    let mut hash: u64 = FNV_OFFSET;
    for &byte in data {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Compute the contract ID for `"name@major_version"` using FNV-1a 64-bit.
pub fn contract_id(name: &str, major_version: u32) -> u64 {
    let canonical: String = format!("{}@{}", name, major_version);
    fnv1a_64(canonical.as_bytes())
}

/// FNV-1a 32-bit hash (lower 32 bits) — used for extension IDs.
pub(crate) fn fnv1a_32(data: &[u8]) -> u32 {
    fnv1a_64(data) as u32
}

/// Compute an extension ID from its name using FNV-1a lower 32 bits.
pub fn extension_id(name: &str) -> u32 {
    fnv1a_32(name.as_bytes())
}

/// Compute a bundle ID from its name using FNV-1a 64-bit hash.
pub fn bundle_id(name: &str) -> u64 {
    fnv1a_64(name.as_bytes())
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv1a_known_values() {
        // Known FNV-1a 64-bit value for empty string
        assert_eq!(fnv1a_64(b""), 0xcbf29ce484222325);
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
    fn plugin_handle_null() {
        let h: PluginHandle = PluginHandle::null();
        assert!(h.is_null());
        let valid: PluginHandle = PluginHandle {
            index: 0,
            generation: 1,
        };
        assert!(!valid.is_null());
    }

    #[test]
    fn abi_error_ok() {
        let e: AbiError = AbiError::ok();
        assert!(e.is_ok());
        assert_eq!(e.code, ABI_OK);
    }

    // ── ABI Layout Tests ─────────────────────────────────────────────────────
    //
    // These tests verify the frozen ABI struct layouts on the target platform.
    // If any assertion fails, it means the ABI has changed — a §7 violation.
    // Add offset_of tests for any new fields added to these structs.

    #[test]
    fn layout_string_view() {
        use std::mem::align_of;
        use std::mem::offset_of;
        use std::mem::size_of;
        assert_eq!(size_of::<StringView>(), 16);
        assert_eq!(align_of::<StringView>(), 8);
        assert_eq!(offset_of!(StringView, ptr), 0);
        assert_eq!(offset_of!(StringView, len), 8);
    }

    #[test]
    fn layout_buffer() {
        use std::mem::align_of;
        use std::mem::offset_of;
        use std::mem::size_of;
        assert_eq!(size_of::<Buffer>(), 24);
        assert_eq!(align_of::<Buffer>(), 8);
        assert_eq!(offset_of!(Buffer, ptr), 0);
        assert_eq!(offset_of!(Buffer, len), 8);
        assert_eq!(offset_of!(Buffer, cap), 16);
    }

    #[test]
    fn layout_abi_error() {
        use std::mem::align_of;
        use std::mem::offset_of;
        use std::mem::size_of;
        assert_eq!(size_of::<AbiError>(), 24);
        assert_eq!(align_of::<AbiError>(), 8);
        assert_eq!(offset_of!(AbiError, code), 0);
        assert_eq!(offset_of!(AbiError, message), 8);
    }

    #[test]
    fn layout_plugin_handle() {
        use std::mem::align_of;
        use std::mem::offset_of;
        use std::mem::size_of;
        assert_eq!(size_of::<PluginHandle>(), 8);
        assert_eq!(align_of::<PluginHandle>(), 4);
        assert_eq!(offset_of!(PluginHandle, index), 0);
        assert_eq!(offset_of!(PluginHandle, generation), 4);
    }

    #[test]
    fn layout_plugin_vtable() {
        use std::mem::align_of;
        use std::mem::offset_of;
        use std::mem::size_of;
        assert_eq!(size_of::<PluginVTable>(), 24);
        assert_eq!(align_of::<PluginVTable>(), 8);
        assert_eq!(offset_of!(PluginVTable, contract_id), 0);
        assert_eq!(offset_of!(PluginVTable, contract_version), 8);
        assert_eq!(offset_of!(PluginVTable, function_count), 12);
        assert_eq!(offset_of!(PluginVTable, functions), 16);
    }

    #[test]
    fn layout_host_vtable() {
        use std::mem::align_of;
        use std::mem::offset_of;
        use std::mem::size_of;
        // HostVTable: 7 extern "C" fn pointers, each 8 bytes on x86_64.
        assert_eq!(size_of::<HostVTable>(), 56);
        assert_eq!(align_of::<HostVTable>(), 8);
        assert_eq!(offset_of!(HostVTable, alloc), 0);
        assert_eq!(offset_of!(HostVTable, free), 8);
        assert_eq!(offset_of!(HostVTable, find_by_contract), 16);
        assert_eq!(offset_of!(HostVTable, find_by_bundle), 24);
        assert_eq!(offset_of!(HostVTable, find_all_by_contract), 32);
        assert_eq!(offset_of!(HostVTable, resolve_plugin), 40);
        assert_eq!(offset_of!(HostVTable, get_extension), 48);
    }

    #[test]
    fn layout_plugin_descriptor() {
        use std::mem::align_of;
        use std::mem::offset_of;
        use std::mem::size_of;
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
    fn layout_plugin_registrar() {
        use std::mem::align_of;
        use std::mem::offset_of;
        use std::mem::size_of;
        // register_plugin fn ptr (8) + host ptr (8) = 16 bytes.
        assert_eq!(size_of::<PluginRegistrar>(), 16);
        assert_eq!(align_of::<PluginRegistrar>(), 8);
        assert_eq!(offset_of!(PluginRegistrar, host), 8);
    }

    #[test]
    fn layout_extension_entry() {
        use std::mem::align_of;
        use std::mem::offset_of;
        use std::mem::size_of;
        // extension_id(4) + padding(4) + vtable ptr(8) = 16 bytes.
        assert_eq!(size_of::<ExtensionEntry>(), 16);
        assert_eq!(align_of::<ExtensionEntry>(), 8);
        assert_eq!(offset_of!(ExtensionEntry, extension_id), 0);
        assert_eq!(offset_of!(ExtensionEntry, vtable), 8);
    }

    #[test]
    fn layout_runtime_config() {
        use std::mem::align_of;
        use std::mem::offset_of;
        use std::mem::size_of;
        // plugin_dirs ptr(8) + plugin_dir_count(8) + compatibility(4) + padding(4)
        // + extensions ptr(8) + extension_count(8) = 40 bytes.
        assert_eq!(size_of::<RuntimeConfig>(), 40);
        assert_eq!(align_of::<RuntimeConfig>(), 8);
        assert_eq!(offset_of!(RuntimeConfig, plugin_dirs), 0);
        assert_eq!(offset_of!(RuntimeConfig, plugin_dir_count), 8);
        assert_eq!(offset_of!(RuntimeConfig, compatibility), 16);
        assert_eq!(offset_of!(RuntimeConfig, extensions), 24);
        assert_eq!(offset_of!(RuntimeConfig, extension_count), 32);
    }
}

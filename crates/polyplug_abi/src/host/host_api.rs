//! Host Interface — function table passed to guests during initialization.
//!
//! This module defines `HostApi`, the primary interface guests use to
//! interact with the runtime. Guests receive this interface in `polyplug_init()`.
//!
//! # Who provides
//! The runtime creates this struct and passes it to `polyplug_init()`.
//!
//! # Who calls
//! Guest (plugin) code calls these functions to interact with the runtime.
//!
//! # Ownership
//! The struct is statically allocated by the runtime. The pointer is valid
//! until the runtime is destroyed. Guest must NOT free this pointer.
//!
//! # Lifetime
//! Lives as long as the runtime that created it.
//!
//! # Thread Safety
//! All functions are safe to call from any thread. The runtime handles
//! internal synchronization.
//!
//! # Self-Passing Pattern
//! All functions take `self: *const HostApi` as the first parameter.
//! SDKs hide this detail from users, automatically passing the interface pointer.

use core::ffi::c_void;

use polyplug_utils::BundleId;

use crate::{
    guest::{GuestContractInstance, GuestContractInterface},
    host::{HostContractInstance, HostContractInterface},
    plugin::{GuestContractHandle, PluginDescriptor},
    types::{AbiError, Array, DependencyInfo, StringView},
};

/// Host Interface — function table passed to guests during initialization.
///
/// Contains an opaque runtime pointer and function pointers for guest calls.
/// All functions use self-passing pattern (receive HostApi pointer as first parameter).
/// `HostApi` is `184 bytes` (1 opaque runtime pointer + 21 function pointer fields + 1 reserved data pointer).
/// Tail offsets: `create_guest_instance` @152, `destroy_guest_instance` @160, `revision_counter` @168, `reserved` @176.
///
/// # Who provides
/// The runtime creates this struct and passes it to `polyplug_init()`.
/// The struct is allocated using `Box::leak()` for `'static` lifetime.
///
/// # Nullability
/// Every function-pointer field is REQUIRED and ALWAYS non-null: the runtime
/// is the sole producer of this struct and populates all 21 callbacks at
/// creation. Consumers never construct or mutate a `HostApi`. Only the
/// `runtime` pointer can become null (it is swapped to null by
/// `polyplug_runtime_destroy`), and only the trailing `reserved` data pointer
/// is a null placeholder by contract.
///
/// # Who calls
/// Guest (plugin) code calls these functions to interact with the runtime.
/// SDK-generated wrappers handle the self-passing pattern automatically.
///
/// # Ownership
/// The struct is statically allocated by the runtime. The pointer is valid
/// until the runtime is destroyed. Guest must NOT free this pointer.
///
/// # Lifetime
/// Lives as long as the runtime that created it.
///
/// # Thread Safety
/// All functions are safe to call from any thread. The runtime uses
/// internal synchronization (RwLock/Mutex) for shared state.
///
/// # Self-passing pattern
/// Each function receives the interface pointer as its first parameter,
/// allowing guests to call: `host->find_guest_contract(host, id, ver)`
/// SDKs hide this pattern: `host.find_guest_contract(id, ver)`
#[repr(C)]
pub struct HostApi {
    /// Opaque pointer to Runtime.
    ///
    /// Set during interface creation. Provides access to runtime state
    /// for dependency enforcement and resource management.
    ///
    /// # Ownership
    /// Owned by the runtime. Guests must NOT free or modify this pointer.
    pub runtime: *mut c_void,
    /// Register a guest contract implementation.
    ///
    /// Called by plugins during `polyplug_init()` to register their contracts.
    /// Returns error if contract_id collision detected or ABI version mismatch.
    ///
    /// # Arguments
    /// - `this`: HostApi pointer (self-passing)
    /// - `descriptor`: Plugin descriptor with contract metadata
    /// - `interface`: GuestContractInterface to register
    /// - `out_err`: out-param; the result is written here (`AbiError::ok()` on
    ///   success, an error otherwise). Never null.
    pub register_guest_contract: unsafe extern "C" fn(
        this: *const HostApi,
        descriptor: *const PluginDescriptor,
        interface: *const GuestContractInterface,
        out_err: *mut AbiError,
    ),
    /// Allocate memory using the host allocator.
    ///
    /// Memory allocated here must be freed via `free`.
    /// Returns null on allocation failure.
    ///
    /// # Arguments
    /// - `this`: HostApi pointer (self-passing)
    /// - `size`: Number of bytes to allocate
    /// - `align`: Alignment requirement (must be power of 2)
    ///
    /// # Returns
    /// Pointer to allocated memory, or null on failure.
    pub alloc: unsafe extern "C" fn(this: *const HostApi, size: usize, align: usize) -> *mut u8,
    /// Free memory allocated via `alloc`.
    ///
    /// Must pass the same size and align used for allocation.
    ///
    /// # Arguments
    /// - `this`: HostApi pointer (self-passing)
    /// - `ptr`: Pointer to memory to free
    /// - `size`: Size used for allocation
    /// - `align`: Alignment used for allocation
    pub free: unsafe extern "C" fn(this: *const HostApi, ptr: *mut u8, size: usize, align: usize),
    /// Find a guest contract by contract_id and minimum version.
    ///
    /// Returns a GuestContractHandle that can be resolved to an interface.
    /// Returns null handle if no matching contract found.
    ///
    /// # Arguments
    /// - `this`: HostApi pointer (self-passing)
    /// - `contract_id`: Contract identifier hash
    /// - `min_version`: Minimum version required
    ///
    /// # Returns
    /// GuestContractHandle for the first matching contract, or null handle.
    pub find_guest_contract: unsafe extern "C" fn(
        this: *const HostApi,
        contract_id: u64,
        min_version: u32,
    ) -> GuestContractHandle,
    /// Find all guest contracts matching contract_id and minimum version.
    ///
    /// Returns an Array of GuestContractHandle. Caller must free via `host->free`.
    /// Use when multiple implementations of the same contract may exist.
    ///
    /// # Arguments
    /// - `this`: HostApi pointer (self-passing)
    /// - `contract_id`: Contract identifier hash
    /// - `min_version`: Minimum version required
    ///
    /// # Returns
    /// Array of GuestContractHandle. Caller owns and must free.
    pub find_all_guest_contracts: unsafe extern "C" fn(
        this: *const HostApi,
        contract_id: u64,
        min_version: u32,
    ) -> Array<GuestContractHandle>,
    /// Resolve a GuestContractHandle to a GuestContractInterface pointer.
    ///
    /// Returns null if the handle is invalid or contract was unloaded.
    ///
    /// # Arguments
    /// - `this`: HostApi pointer (self-passing)
    /// - `handle`: GuestContractHandle from find_guest_contract
    ///
    /// # Returns
    /// Pointer to GuestContractInterface, or null if invalid/stale.
    pub resolve_guest_contract: unsafe extern "C" fn(
        this: *const HostApi,
        handle: GuestContractHandle,
    ) -> *const GuestContractInterface,
    /// Get a host contract instance by contract_id and minimum version.
    ///
    /// For singleton host contracts, returns the same instance every time.
    /// For multi-instance host contracts, returns a new instance each time.
    ///
    /// # Arguments
    /// - `this`: HostApi pointer (self-passing)
    /// - `contract_id`: Host contract identifier hash
    /// - `min_version`: Minimum version required
    ///
    /// # Returns
    /// HostContractInstance for the contract.
    pub get_host_contract: unsafe extern "C" fn(
        this: *const HostApi,
        contract_id: u64,
        min_version: u32,
    ) -> HostContractInstance,
    /// Resolve a host contract interface by contract_id and minimum version.
    ///
    /// Returns the HostContractInterface pointer for the contract.
    /// This is needed to access dispatch metadata (dispatch_type, function_count, functions).
    /// Returns null if no matching contract found.
    ///
    /// # Arguments
    /// - `this`: HostApi pointer (self-passing)
    /// - `contract_id`: Host contract identifier hash
    /// - `min_version`: Minimum version required
    ///
    /// # Returns
    /// Pointer to HostContractInterface, or null if invalid/not found.
    pub resolve_host_contract_interface: unsafe extern "C" fn(
        this: *const HostApi,
        contract_id: u64,
        min_version: u32,
    ) -> *const HostContractInterface,
    /// List all loaded bundles.
    ///
    /// Returns an Array of BundleId. Caller must free via `host->free`.
    /// Bundle IDs are stable for the lifetime of the runtime.
    ///
    /// # Arguments
    /// - `this`: HostApi pointer (self-passing)
    ///
    /// # Returns
    /// Array of BundleId. Caller owns and must free.
    pub list_bundles: unsafe extern "C" fn(this: *const HostApi) -> Array<BundleId>,
    /// Get dependencies for the calling bundle.
    ///
    /// Uses bundle_id from current BundleInitContext (TLS) to look up declared deps.
    /// Returns an Array of DependencyInfo. Caller must free via `host->free`.
    ///
    /// # Arguments
    /// - `this`: HostApi pointer (self-passing)
    ///
    /// # Returns
    /// Array of DependencyInfo. Caller owns and must free.
    /// Returns empty array if called outside bundle init context.
    pub get_dependencies: unsafe extern "C" fn(this: *const HostApi) -> Array<DependencyInfo>,
    /// Load a plugin bundle from a path.
    ///
    /// Host applications call this to load a bundle at runtime.
    /// The loader matching the bundle's runtime type is used.
    ///
    /// # Arguments
    /// - `this`: HostApi pointer (self-passing)
    /// - `path`: UTF-8 path to bundle directory (not null-terminated)
    /// - `path_len`: Length of path in bytes
    /// - `out_err`: out-param; the result is written here (`AbiError::ok()` on
    ///   success, an error otherwise). Never null.
    pub load_bundle: unsafe extern "C" fn(
        this: *const HostApi,
        path: *const u8,
        path_len: usize,
        out_err: *mut AbiError,
    ),
    /// Reload a plugin bundle (hot-reload).
    ///
    /// Replaces the bundle's contracts with new versions from the updated binary.
    /// All instances must be destroyed before calling this.
    ///
    /// # Arguments
    /// - `this`: HostApi pointer (self-passing)
    /// - `path`: UTF-8 path to bundle directory (not null-terminated)
    /// - `path_len`: Length of path in bytes
    /// - `out_err`: out-param; the result is written here (`AbiError::ok()` on
    ///   success, an error otherwise). Never null.
    pub reload_bundle: unsafe extern "C" fn(
        this: *const HostApi,
        path: *const u8,
        path_len: usize,
        out_err: *mut AbiError,
    ),
    /// Register a host contract interface.
    ///
    /// Host applications register their contracts for plugins to consume.
    ///
    /// # Arguments
    /// - `this`: HostApi pointer (self-passing)
    /// - `interface`: HostContractInterface to register
    /// - `out_err`: out-param; the result is written here (`AbiError::ok()` on
    ///   success, an error otherwise). Never null.
    pub register_host_contract: unsafe extern "C" fn(
        this: *const HostApi,
        interface: *const HostContractInterface,
        out_err: *mut AbiError,
    ),
    /// Register a language loader.
    ///
    /// Host applications register loaders for each runtime language they support.
    ///
    /// # Arguments
    /// - `this`: HostApi pointer (self-passing)
    /// - `loader_ptr`: Opaque pointer to the loader implementation
    /// - `out_err`: out-param; the result is written here (`AbiError::ok()` on
    ///   success, an error otherwise). Never null.
    ///
    /// The loader's name comes from its own `BundleLoader::loader_name()`
    /// implementation — the single source of truth — so it is not passed here.
    pub register_loader:
        unsafe extern "C" fn(this: *const HostApi, loader_ptr: *mut c_void, out_err: *mut AbiError),
    /// Get last error message.
    ///
    /// Returns the most recent error message from this runtime.
    /// Copies up to buf_len bytes into buf.
    ///
    /// # Arguments
    /// - `this`: HostApi pointer (self-passing)
    /// - `buf`: Buffer to write error message into
    /// - `buf_len`: Maximum bytes to write
    ///
    /// # Returns
    /// Number of bytes written (0 if no error or buffer too small).
    pub get_last_error:
        unsafe extern "C" fn(this: *const HostApi, buf: *mut u8, buf_len: usize) -> usize,
    /// Get last error message length.
    ///
    /// Returns the byte length of the most recent error message.
    /// Use to allocate buffer before calling get_last_error.
    ///
    /// # Arguments
    /// - `this`: HostApi pointer (self-passing)
    ///
    /// # Returns
    /// Length of last error message (0 if no error).
    pub get_error_len: unsafe extern "C" fn(this: *const HostApi) -> usize,
    /// Unload a guest bundle, invalidating its handles and freeing its resources.
    ///
    /// This performs **true unload**: the bundle's slots have their generation bumped,
    /// the bundle is removed from the registry indices, and the superseded interface
    /// `Arc` together with the underlying dylib mapping or VM state are reclaimed through
    /// crossbeam-epoch deferred reclamation — the memory is freed once no reader is still
    /// pinned in the epoch that preceded the unload. There is no opt-in mode; unload
    /// always reclaims when safe.
    ///
    /// A raw `GuestContractInterface` pointer cached before the unload and dereferenced
    /// after it is **undefined behaviour**: the host must quiesce the bundle (ensure no
    /// thread is calling into it or holds a pointer into it) before unloading. Runtime-
    /// mediated calls — `create_guest_instance`, `destroy_guest_instance` — pin the epoch
    /// across dispatch and are therefore safe against a concurrent unload.
    ///
    /// After unload, every handle that was minted for this bundle resolves to
    /// `AbiErrorCode::StaleHandle`, and `find_guest_contract` / `find_all_guest_contracts`
    /// no longer return it.
    ///
    /// # Obtaining the bundle_id
    /// The host obtains the `bundle_id` either from `list_bundles`, or by hashing the
    /// bundle name via `BundleId::new(name)`.
    ///
    /// # Arguments
    /// - `this`: HostApi pointer (self-passing)
    /// - `bundle_id`: Identifier of the bundle to unload
    /// - `out_err`: out-param; the result is written here — `AbiError::ok()` on
    ///   success, an error on failure (e.g. the bundle is not loaded, or a
    ///   still-loaded bundle declared a dependency on a contract this bundle
    ///   provides). Never null.
    pub unload_bundle:
        unsafe extern "C" fn(this: *const HostApi, bundle_id: BundleId, out_err: *mut AbiError),
    /// Log a guest diagnostic into the host's logging funnel.
    ///
    /// Routes to the same sink as `RuntimeConfig::log`: the host-installed
    /// callback when one is set, otherwise the stderr default (Error/Warn
    /// visibility only). `level` is a `LogLevel` discriminant; unknown values
    /// are clamped to `LogLevel::Error`. `scope` is a short stable tag — guest
    /// plugins should use `"guest.<plugin-name>"` or similar.
    ///
    /// The runtime always provides this function — guests may call it
    /// unconditionally via the self-passing pattern:
    /// `host->log(host, level, scope, message)`.
    ///
    /// # Ownership
    /// `scope` and `message` are borrowed views, read only for the duration of
    /// the call; the runtime copies what it needs. Null/empty views are legal.
    ///
    /// # Arguments
    /// - `this`: HostApi pointer (self-passing)
    /// - `level`: `LogLevel` discriminant (`1 = Error` .. `5 = Trace`)
    /// - `scope`: short UTF-8 subsystem tag
    /// - `message`: UTF-8 log message
    pub log: unsafe extern "C" fn(
        this: *const HostApi,
        level: u32,
        scope: StringView,
        message: StringView,
    ),
    /// Create a guest contract instance through the runtime (host-mediated).
    ///
    /// The host/peer caller passes the resolved `interface` pointer (from
    /// `resolve_guest_contract`) and the constructor `args`. The runtime invokes the
    /// interface's `create_instance` under an epoch pin (so a concurrent unload cannot
    /// free the interface mid-construction) and tracks the resulting instance for its
    /// live-instance accounting. The new `GuestContractInstance` is written through
    /// `out_instance` (a null `data` denotes a stateless instance).
    pub create_guest_instance: unsafe extern "C" fn(
        this: *const HostApi,
        interface: *const GuestContractInterface,
        args: *const c_void,
        out_instance: *mut GuestContractInstance,
    ),
    /// Destroy a guest contract instance through the runtime (host-mediated).
    ///
    /// Mirror of `create_guest_instance`: the runtime invokes the interface's
    /// `destroy_instance` under an epoch pin and updates its live-instance accounting.
    pub destroy_guest_instance: unsafe extern "C" fn(
        this: *const HostApi,
        interface: *const GuestContractInterface,
        instance: GuestContractInstance,
    ),
    /// Return a pointer to the runtime's monotonic registry revision counter — the
    /// shared word a generated host→guest caller polls to keep its cached interface
    /// safe with NO per-call function call.
    ///
    /// Generated callers resolve a contract once and cache the interface pointer so
    /// the hot path is a direct indirect call with no per-call resolve. To keep that
    /// cache safe without making the user track dangling pointers, the caller invokes
    /// THIS function exactly once — at construction — to obtain the address of the
    /// counter, caches that pointer alongside the counter's current value, and then
    /// before every dispatch reads the counter *directly through the cached pointer*
    /// (a single aligned atomic load — one instruction, no call into the runtime).
    /// While the value is unchanged the cached interface is guaranteed current and the
    /// caller dispatches directly; when it changes (a bundle was loaded, hot-reloaded,
    /// or unloaded) the caller re-resolves and refreshes its cache. This is what turns
    /// the "raw interface pointer cached after reload/unload is UB" footgun into an
    /// automatically managed, safe cache that costs a memory load per call, not a
    /// function call.
    ///
    /// The pointed-to value is an opaque monotonic counter; callers must treat it only
    /// as "equal ⇒ unchanged" and never ascribe meaning to its magnitude or deltas. It
    /// is deliberately a runtime-wide revision rather than a per-slot generation: a
    /// hot-reload swaps a new interface into the *same* slot WITHOUT bumping that
    /// slot's generation (so existing handles survive a reload), which a generation
    /// check would miss — the revision changes on every mutation, so reload is
    /// detected.
    ///
    /// # Memory model
    /// The runtime bumps the counter under its write lock with `Release` ordering;
    /// readers should load with `Acquire` where the language allows (Rust/C++/C#). On
    /// every supported 64-bit target an aligned 64-bit load is itself atomic, so
    /// languages that cannot express ordering (Python/Lua) still observe a coherent,
    /// monotonically advancing value. The counter lives inside the `Runtime` (held
    /// behind an `Arc`, so its address is stable) and is valid for the whole lifetime
    /// of the runtime — i.e. for as long as any caller may dispatch.
    ///
    /// # Arguments
    /// - `this`: HostApi pointer (self-passing)
    ///
    /// # Returns
    /// A pointer to the `u64` revision counter, or null if `this` is null. A caller
    /// that receives null treats its cache as always-current (it has no runtime to
    /// mediate reloads against).
    pub revision_counter: unsafe extern "C" fn(this: *const HostApi) -> *const u64,
    /// Reserved. Producers must set this to null; consumers must not read it.
    pub reserved: *const c_void,
}

// SAFETY: HostApi contains an opaque pointer and function pointers.
// The opaque pointer is managed by the runtime.
// Function pointers are inherently thread-safe to call from any thread
// (the functions themselves must handle their own synchronization).
unsafe impl Send for HostApi {}

// SAFETY: HostApi contains an opaque pointer and function pointers.
// Concurrent calls to the same interface are safe because the runtime
// handles internal synchronization.
unsafe impl Sync for HostApi {}

#[cfg(test)]
mod tests {
    use core::ffi::c_void;
    use core::mem::{align_of, offset_of, size_of};

    use crate::host::host_api::HostApi;

    #[test]
    fn layout_host_api() {
        // HostApi: runtime pointer (8 bytes) + 21 extern "C" fn pointers (168 bytes) + 1 reserved data pointer (8 bytes).
        // Total: 184 bytes (23 pointer-sized fields)
        // Fields: runtime, register_guest_contract, alloc, free, find_guest_contract,
        //         find_all_guest_contracts, resolve_guest_contract,
        //         get_host_contract, resolve_host_contract_interface, list_bundles,
        //         get_dependencies, load_bundle, reload_bundle, register_host_contract,
        //         register_loader, get_last_error, get_error_len,
        //         unload_bundle, log, create_guest_instance, destroy_guest_instance,
        //         revision_counter, reserved
        assert_eq!(size_of::<HostApi>(), 184);
        assert_eq!(align_of::<HostApi>(), 8);
        // Existing fields (unchanged offsets)
        assert_eq!(offset_of!(HostApi, runtime), 0);
        assert_eq!(offset_of!(HostApi, register_guest_contract), 8);
        assert_eq!(offset_of!(HostApi, alloc), 16);
        assert_eq!(offset_of!(HostApi, free), 24);
        assert_eq!(offset_of!(HostApi, find_guest_contract), 32);
        assert_eq!(offset_of!(HostApi, find_all_guest_contracts), 40);
        assert_eq!(offset_of!(HostApi, resolve_guest_contract), 48);
        assert_eq!(offset_of!(HostApi, get_host_contract), 56);
        assert_eq!(offset_of!(HostApi, resolve_host_contract_interface), 64);
        assert_eq!(offset_of!(HostApi, list_bundles), 72);
        assert_eq!(offset_of!(HostApi, get_dependencies), 80);
        assert_eq!(offset_of!(HostApi, load_bundle), 88);
        assert_eq!(offset_of!(HostApi, reload_bundle), 96);
        assert_eq!(offset_of!(HostApi, register_host_contract), 104);
        assert_eq!(offset_of!(HostApi, register_loader), 112);
        assert_eq!(offset_of!(HostApi, get_last_error), 120);
        assert_eq!(offset_of!(HostApi, get_error_len), 128);
        assert_eq!(offset_of!(HostApi, unload_bundle), 136);
        assert_eq!(offset_of!(HostApi, log), 144);
        assert_eq!(offset_of!(HostApi, create_guest_instance), 152);
        assert_eq!(offset_of!(HostApi, destroy_guest_instance), 160);
        assert_eq!(offset_of!(HostApi, revision_counter), 168);
        assert_eq!(offset_of!(HostApi, reserved), 176);
    }

    /// Verify HostApi has runtime: *mut c_void field at offset 0.
    #[test]
    fn host_api_has_runtime_field() {
        assert_eq!(offset_of!(HostApi, runtime), 0);
        assert_eq!(size_of::<*mut c_void>(), 8);
    }

    /// Verify list_bundles field exists.
    #[test]
    fn list_bundles_field_exists() {
        assert_eq!(offset_of!(HostApi, list_bundles), 72);
    }

    /// Verify get_dependencies field exists.
    #[test]
    fn get_dependencies_field_exists() {
        assert_eq!(offset_of!(HostApi, get_dependencies), 80);
    }
}

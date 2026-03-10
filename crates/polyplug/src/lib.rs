//! polyplug — core plugin runtime for the polyplug platform.
//!
//! This crate exports:
//! - 8 C ABI functions (the complete public contract of polyplug)
//! - Module-level access to all subsystems

pub mod abi;
pub mod allocator;
pub mod error;
pub mod extensions;
pub mod graph;
pub mod loader;
pub mod registry;
pub mod reload;
pub mod runtime;
pub use reload::ReloadEvent;

pub mod version;

// Re-export the allocator functions at crate level for convenience.
// These are also exported with #[unsafe(no_mangle)] from allocator/mod.rs.
pub use allocator::polyplug_host_alloc;
pub use allocator::polyplug_host_free;

// ─── C ABI Exports ───────────────────────────────────────────────────────────

/// ABI version sentinel. Bundles MUST export this function.
/// The loader checks this before calling polyplug_init.
#[unsafe(no_mangle)]
pub extern "C" fn polyplug_abi_version() -> u32 {
    abi::POLYPLUG_ABI_VERSION
}

/// Initialize the runtime with the given configuration.
/// Returns an opaque RuntimeHandle (pointer to heap-allocated state).
//
//  For MVP: creates a Runtime using RuntimeBuilder with empty config.
//  Full implementation processes config.plugin_dirs and config.extensions.
#[unsafe(no_mangle)]
pub extern "C" fn polyplug_runtime_init(
    _config: *const abi::RuntimeConfig,
) -> *mut runtime::Runtime {
    let rt: runtime::Runtime = match runtime::Runtime::builder().build() {
        Ok(r) => r,
        Err(_) => return core::ptr::null_mut(),
    };
    // SAFETY: Box::into_raw gives the caller ownership. They must call
    // polyplug_runtime_destroy to free it.
    Box::into_raw(Box::new(rt))
}

/// Destroy the runtime and free all associated resources.
///
/// # Safety
/// `runtime` must be a valid non-null pointer previously returned by
/// `polyplug_runtime_init`. Must not be called more than once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_runtime_destroy(runtime: *mut runtime::Runtime) {
    if runtime.is_null() {
        return;
    }
    // SAFETY: runtime was created with Box::new() in polyplug_runtime_init.
    // Dropping the Box is correct here.
    drop(unsafe { Box::from_raw(runtime) });
}

/// Find the first plugin providing `contract_id` at or above `min_version`.
///
/// # Safety
/// Callable from any thread after `polyplug_runtime_init` returns.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_find_by_contract(
    contract_id: u64,
    min_version: u32,
) -> abi::PluginHandle {
    // SAFETY: Delegates to host_find_by_contract. No pointer args — only plain integers.
    // The outer function is unsafe; the inner call requires an explicit unsafe block (Rust 2024).
    unsafe { crate::runtime::host_find_by_contract(contract_id, min_version) }
}

/// Find a specific bundle's provider of `contract_id`.
///
/// # Safety
/// Callable from any thread after `polyplug_runtime_init` returns.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_find_by_bundle(
    bundle_id: u64,
    contract_id: u64,
    min_version: u32,
) -> abi::PluginHandle {
    // SAFETY: Delegates to host_find_by_bundle. No pointer args — only plain integers.
    // The outer function is unsafe; the inner call requires an explicit unsafe block (Rust 2024).
    unsafe { crate::runtime::host_find_by_bundle(bundle_id, contract_id, min_version) }
}

/// Fill `out` with up to `out_cap` handles providing `contract_id`. Returns count written.
///
/// # Safety
/// `out` must point to a valid buffer of at least `out_cap` `PluginHandle` elements.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_find_all_by_contract(
    contract_id: u64,
    min_version: u32,
    out: *mut abi::PluginHandle,
    out_cap: usize,
) -> usize {
    // SAFETY: out is valid for out_cap PluginHandle elements per the ABI contract (Rust 2024).
    unsafe { crate::runtime::host_find_all_by_contract(contract_id, min_version, out, out_cap) }
}

/// Resolve a `PluginHandle` to its vtable pointer.
///
/// # Safety
/// `handle` must be a valid, non-stale handle. The returned pointer is valid
/// as long as the host runtime is alive.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_resolve_plugin(
    handle: abi::PluginHandle,
) -> *const abi::PluginVTable {
    // SAFETY: handle validity is the caller's responsibility per the ABI contract (Rust 2024).
    unsafe { crate::runtime::host_resolve_plugin(handle) }
}

/// Retrieve an extension vtable by extension_id.
///
/// Full implementation looks up extensions registered via RuntimeBuilder::extension().
/// Returns null if the extension is not registered.
///
/// # Safety
/// `runtime` must be a valid non-null pointer to a live Runtime.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_get_extension(
    _runtime: *const runtime::Runtime,
    _extension_id: u32,
) -> *const () {
    // SAFETY: host_get_extension reads from GLOBAL_EXTENSION_MAP (OnceLock, read-only after init).
    // No pointer dereferences; safe to call from any thread.
    unsafe { crate::runtime::host_get_extension(_extension_id) }
}

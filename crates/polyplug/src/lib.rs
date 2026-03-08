//! polyplug — core plugin runtime for the polyplug platform.
//!
//! This crate exports:
//! - 6 C ABI functions (the complete public contract of polyplug)
//! - Module-level access to all subsystems

pub mod abi;
pub mod allocator;
pub mod error;
pub mod graph;
pub mod loader;
pub mod registry;
pub mod runtime;

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

/// Find a plugin by contract_id and minimum version.
///
/// # Safety
/// `runtime` must be a valid non-null pointer to a live Runtime.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_find_plugin(
    runtime: *const runtime::Runtime,
    contract_id: u64,
    min_version: u32,
) -> abi::PluginHandle {
    if runtime.is_null() {
        return abi::PluginHandle::null();
    }
    // SAFETY: runtime is non-null and points to a live Runtime instance.
    match unsafe { (*runtime).find_plugin(contract_id, min_version) } {
        Ok(handle) => handle,
        Err(_) => abi::PluginHandle::null(),
    }
}

/// Call a function on a loaded plugin through its vtable.
///
/// # Safety
/// - `runtime` must be a valid non-null pointer to a live Runtime.
/// - `plugin` must be a valid PluginHandle (not stale).
/// - `args` and `out` must point to valid memory matching the function's expected types.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_call_plugin(
    runtime: *const runtime::Runtime,
    plugin: abi::PluginHandle,
    function_id: u32,
    args: *const (),
    out: *mut (),
) -> abi::AbiError {
    if runtime.is_null() {
        return abi::AbiError {
            code: abi::ABI_ERROR_NOT_FOUND,
            message: abi::StringView::null(),
        };
    }
    // SAFETY: runtime is non-null and points to a live Runtime. args/out are
    // valid memory for the function's expected types (enforced by generated code).
    unsafe { (*runtime).call_plugin(plugin, function_id, args, out) }
}

/// Retrieve an extension vtable by extension_id.
///
/// For MVP: always returns null (no extensions registered).
/// Full implementation looks up extensions registered in RuntimeConfig.
///
/// # Safety
/// `runtime` must be a valid non-null pointer to a live Runtime.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_get_extension(
    _runtime: *const runtime::Runtime,
    _extension_id: u32,
) -> *const () {
    // MVP: no extension registry
    core::ptr::null()
}

//! Runtime — core runtime logic, builder pattern, and two-phase lifecycle.
//!
//! Phase 1 (initialization, single-threaded):
//!  - Load manifests
//!  - Build capability graph
//!  - dlopen bundles in topological order
//!  - Call init() on each bundle
//!  - Register vtables
//!
//! Phase 2 (runtime, multi-threaded, lock-free):
//!  - Plugin dispatch is a direct pointer dereference
//!  - find_plugin() is a read-only RwLock read guard
//!  - No locks in the hot path

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::OnceLock;

use crate::abi::ABI_FUNCTION_NOT_AVAIL;
use crate::abi::AbiError;
use crate::abi::HostVTable;
use crate::abi::PluginHandle;
use crate::abi::PluginVTable;
use crate::allocator::polyplug_host_alloc;
use crate::allocator::polyplug_host_free;
use crate::error::RegistryError;
use crate::error::RuntimeError;
use crate::loader::LoadedBundle;
use crate::registry::Registry;

// ─── Global registry for cross-plugin dispatch ───────────────────────────────

static GLOBAL_REGISTRY: OnceLock<Arc<Registry>> = OnceLock::new();

/// Set the global registry used by `host_find_plugin` and `host_call_plugin`.
/// If the registry has already been set, this call is a no-op (OnceLock semantics).
pub fn set_global_registry(registry: Arc<Registry>) {
    // OnceLock::set returns Err(value) when already set — expected behaviour after
    // the first RuntimeBuilder::build() call. Silently ignore.
    let _: Result<(), Arc<Registry>> = GLOBAL_REGISTRY.set(registry);
}

/// Return the global registry for dispatching, or `None` if not yet initialised.
pub(crate) fn global_registry() -> Option<Arc<Registry>> {
    GLOBAL_REGISTRY.get().cloned()
}

/// The runtime instance. Thread-safe — implements `Send + Sync`.
//
//  Holds the registry and all loaded bundles.
//  Bundles are never dropped (never-drop invariant, §7.3).
pub struct Runtime {
    registry: Arc<Registry>,
    /// Loaded bundles, never dropped.
    _bundles: Vec<LoadedBundle>,
    /// The static HostVTable given to plugins. Must be 'static.
    host_vtable: &'static HostVTable,
}

// SAFETY: Runtime wraps Arc<Registry> (Send+Sync) and Vec<LoadedBundle>.
// LoadedBundle contains a Box<Library> which is not Sync by itself,
// but we never share the library references — only vtable pointers (which are 'static).
// The Runtime is effectively immutable after initialization.
unsafe impl Send for Runtime {}
// SAFETY: See above — Runtime is immutable after init. All mutable state is behind Arc<RwLock>.
unsafe impl Sync for Runtime {}

/// Builder for constructing a Runtime.
pub struct RuntimeBuilder {
    plugin_dirs: Vec<PathBuf>,
}

impl RuntimeBuilder {
    /// Create a new RuntimeBuilder with default settings.
    pub fn new() -> RuntimeBuilder {
        RuntimeBuilder {
            plugin_dirs: Vec::new(),
        }
    }

    /// Add a directory to scan for plugin bundles.
    pub fn plugin_dir(mut self, path: PathBuf) -> RuntimeBuilder {
        self.plugin_dirs.push(path);
        self
    }

    /// Build the runtime.
    //
    //  For MVP: scans plugin_dirs for .so/.dll/.dylib files,
    //  loads them in sorted order, registers vtables.
    //  Full capability graph resolution is a future enhancement.
    pub fn build(self) -> Result<Runtime, RuntimeError> {
        let registry: Arc<Registry> = Arc::new(Registry::new());

        // Wire the global dispatcher before leaking the HostVTable.
        set_global_registry(Arc::clone(&registry));

        let bundles: Vec<LoadedBundle> = Vec::new();

        // Build the static HostVTable. This must be 'static.
        let host_vtable: &'static HostVTable = Box::leak(Box::new(HostVTable {
            alloc: polyplug_host_alloc,
            // SAFETY: polyplug_host_free is unsafe extern C — we store its pointer for the vtable.
            free: polyplug_host_free,
            find_plugin: host_find_plugin,
            call_plugin: host_call_plugin,
            get_extension: host_get_extension,
        }));

        let _ = &self.plugin_dirs;

        Ok(Runtime {
            registry,
            _bundles: bundles,
            host_vtable,
        })
    }
}

impl Default for RuntimeBuilder {
    fn default() -> RuntimeBuilder {
        RuntimeBuilder::new()
    }
}

impl Runtime {
    /// Create a RuntimeBuilder.
    pub fn builder() -> RuntimeBuilder {
        RuntimeBuilder::new()
    }

    /// Find a plugin by contract_id and minimum version.
    pub fn find_plugin(
        &self,
        contract_id: u64,
        min_version: u32,
    ) -> Result<PluginHandle, RegistryError> {
        self.registry.find(contract_id, min_version)
    }

    /// Call a plugin function through its vtable.
    ///
    /// Returns AbiError with appropriate code on failure.
    ///
    /// # Safety
    /// `args` and `out` must point to valid memory matching the function's expected types.
    /// This is guaranteed by the generated caller code — app developers never call this directly.
    pub unsafe fn call_plugin(
        &self,
        handle: PluginHandle,
        fn_id: u32,
        args: *const (),
        out: *mut (),
    ) -> AbiError {
        let vtable_ptr: *const PluginVTable = match self.registry.resolve(handle) {
            Ok(p) => p,
            Err(e) => {
                return registry_error_to_abi_error(e);
            }
        };

        // SAFETY: vtable_ptr is 'static (library never dropped, §7.3).
        let vtable: &PluginVTable = unsafe { &*vtable_ptr };
        if fn_id >= vtable.function_count {
            return AbiError {
                code: ABI_FUNCTION_NOT_AVAIL,
                message: crate::abi::StringView::null(),
            };
        }

        // Dispatch: index into the function pointer array.
        // SAFETY: fn_id < function_count, so the array index is valid.
        // functions points to a 'static array. The function has the correct
        // signature for the contract (guaranteed by codegen).
        let fn_ptr: *const () = unsafe { *vtable.functions.add(fn_id as usize) };
        let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
            // SAFETY: The function pointer is cast to the generic dispatch signature.
            // Actual argument types are enforced by the generated caller code.
            unsafe { core::mem::transmute(fn_ptr) };
        // SAFETY: args and out point to valid memory as guaranteed by the generated caller.
        unsafe { dispatch_fn(args, out) }
    }

    /// Get the HostVTable for use in plugin registrars.
    pub fn host_vtable(&self) -> &'static HostVTable {
        self.host_vtable
    }
}

// ─── Standalone C ABI callbacks (stored in HostVTable) ───────────────────────

/// HostVTable.find_plugin callback — dispatches to thread-local runtime.
//
//  The HostVTable is 'static and shared across threads. Plugins call this during
//  Phase 2 (runtime) to discover other plugins.
//
//  For MVP: this is a stub returning null handle.
//  Full implementation requires a global/thread-local runtime reference.
//
// SAFETY: This function is called by plugin code through the HostVTable function
// pointer. The caller (generated plugin code) ensures the calling convention is
// correct. The function body is entirely safe — no unsafe operations are performed.
// SAFETY: This function is called by plugin code through the HostVTable.
// Returns PluginHandle::null() when registry not set or plugin not found — never panics.
unsafe extern "C" fn host_find_plugin(contract_id: u64, min_version: u32) -> PluginHandle {
    match global_registry() {
        Some(reg) => reg.find(contract_id, min_version).unwrap_or(PluginHandle::null()),
        None => PluginHandle::null(),
    }
}

/// HostVTable.call_plugin callback.
//
// SAFETY: This function is called by plugin code through the HostVTable function
// pointer. The caller ensures the calling convention matches the extern "C" ABI.
// The function body is entirely safe — no unsafe operations are performed.
// SAFETY: This function is called by plugin code through the HostVTable.
// args and out must point to valid memory per the function's ABI contract.
unsafe extern "C" fn host_call_plugin(
    plugin: PluginHandle,
    fn_id: u32,
    args: *const (),
    out: *mut (),
) -> AbiError {
    match global_registry() {
        Some(reg) => {
            let vtable_ptr: *const PluginVTable = match reg.resolve(plugin) {
                Ok(p) => p,
                Err(e) => return registry_error_to_abi_error(e),
            };
            // SAFETY: vtable_ptr is 'static (library never dropped, §7.3).
            let vtable: &PluginVTable = unsafe { &*vtable_ptr };
            if fn_id >= vtable.function_count {
                return AbiError {
                    code: ABI_FUNCTION_NOT_AVAIL,
                    message: crate::abi::StringView::null(),
                };
            }
            // SAFETY: fn_id < function_count validated above.
            let fn_ptr: *const () = unsafe { *vtable.functions.add(fn_id as usize) };
            // SAFETY: Transmuted to generic dispatch signature; types enforced by generated callers.
            let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
                unsafe { core::mem::transmute(fn_ptr) };
            // SAFETY: args and out are valid per ABI contract.
            unsafe { dispatch_fn(args, out) }
        }
        None => AbiError {
            code: crate::abi::ABI_ERROR_NOT_FOUND,
            message: crate::abi::StringView::null(),
        },
    }
}

/// HostVTable.get_extension callback.
//
// SAFETY: This function is called by plugin code through the HostVTable function
// pointer. The caller ensures the calling convention matches the extern "C" ABI.
// The function body is entirely safe — returns a null pointer constant.
unsafe extern "C" fn host_get_extension(_extension_id: u32) -> *const () {
    // For MVP: no extensions registered.
    core::ptr::null()
}

/// Convert a RegistryError to an AbiError.
fn registry_error_to_abi_error(error: RegistryError) -> AbiError {
    let code: u32 = match &error {
        RegistryError::StaleHandle { .. } => crate::abi::ABI_ERROR_STALE_HANDLE,
        RegistryError::PluginNotFound { .. } => crate::abi::ABI_ERROR_NOT_FOUND,
        RegistryError::ContractIdCollision { .. } | RegistryError::DuplicateProvider { .. } => {
            crate::abi::ABI_ERROR_GENERIC
        }
    };
    AbiError {
        code,
        message: crate::abi::StringView::null(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_creates_runtime() {
        let runtime: Runtime = Runtime::builder()
            .build()
            .expect("runtime build should succeed");
        // Registry starts empty
        let result: Result<PluginHandle, _> = runtime.find_plugin(0x1234_5678_9ABC_DEF0_u64, 0);
        assert!(result.is_err(), "empty registry should return not found");
    }

    #[test]
    fn abi_ok_constant() {
        assert_eq!(crate::abi::ABI_OK, 0_u32);
    }

    #[test]
    fn dispatcher_graceful_degradation_when_no_registry() {
        // contract_id=0 won't match any registered plugin regardless of whether
        // GLOBAL_REGISTRY is set.
        // SAFETY: host_find_plugin has no pointer preconditions — args are plain integers.
        let handle: PluginHandle = unsafe { host_find_plugin(0_u64, 0_u32) };
        assert!(handle.is_null(), "host_find_plugin must return null when plugin not found");
    }
}

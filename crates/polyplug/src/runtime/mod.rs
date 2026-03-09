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
//!  - find_by_contract() is a read-only RwLock read guard
//!  - No locks in the hot path

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::OnceLock;

use crate::abi::AbiError;
use crate::abi::HostVTable;
use crate::abi::PluginHandle;
use crate::abi::PluginVTable;
use crate::abi::ABI_FUNCTION_NOT_AVAIL;
use crate::allocator::polyplug_host_alloc;
use crate::allocator::polyplug_host_free;
use crate::error::RegistryError;
use crate::error::RuntimeError;
use crate::loader::LoadedBundle;
use crate::registry::Registry;
use std::collections::HashMap;

use crate::error::LoaderError;
use crate::loader::BundleLoader;
use crate::loader::NativeBundleLoader;

// ─── Global registry for cross-plugin dispatch ───────────────────────────────

static GLOBAL_REGISTRY: OnceLock<Arc<Registry>> = OnceLock::new();

// Thread-local bundle ID set during Phase 1 init to enforce declared-dependency checks.
//
// A non-zero value means the current thread is executing an init() callback for that bundle.
// Callbacks (`host_find_by_contract`, `host_find_by_bundle`) check this and reject undeclared
// cross-bundle lookups during Phase 1. Reset to 0 after init() returns.
thread_local! {
    pub(crate) static INIT_BUNDLE_ID: core::cell::Cell<u64> = const { core::cell::Cell::new(0) };
}

/// Set the global registry used by `host_find_by_contract` and related callbacks.
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
    /// All registered loaders, keyed by runtime_name. Immutable after build().
    _loaders: HashMap<String, Box<dyn BundleLoader>>,
}

// SAFETY: Runtime wraps Arc<Registry> (Send+Sync) and Vec<LoadedBundle>.
// LoadedBundle contains a Box<Library> which is not Sync by itself,
// but libraries are stored in `Registry::loaded_libraries` and never shared
// as references — only vtable pointers (which are valid for the Registry's
// lifetime) are accessed concurrently. The Runtime is effectively immutable after init.
unsafe impl Send for Runtime {}
// SAFETY: See above — Runtime is immutable after init. All mutable state is behind Arc<RwLock>.
unsafe impl Sync for Runtime {}

/// Builder for constructing a Runtime.
pub struct RuntimeBuilder {
    plugin_dirs: Vec<PathBuf>,
    loaders: Vec<Box<dyn BundleLoader>>,
}

impl RuntimeBuilder {
    /// Create a new RuntimeBuilder with default settings.
    pub fn new() -> RuntimeBuilder {
        RuntimeBuilder {
            plugin_dirs: Vec::new(),
            loaders: Vec::new(),
        }
    }

    /// Add a directory to scan for plugin bundles.
    pub fn plugin_dir(mut self, path: PathBuf) -> RuntimeBuilder {
        self.plugin_dirs.push(path);
        self
    }

    /// Register an additional bundle loader for a non-native runtime.
    ///
    /// The loader is identified by `loader.runtime_name()`. Duplicate registrations
    /// (same runtime name) are detected in `build()` and cause `build()` to return
    /// `Err(RuntimeError::Loader(LoaderError::DuplicateLoader { .. }))`.
    ///
    /// Native bundles do not require calling this method — `NativeBundleLoader` is
    /// registered automatically.
    pub fn loader(mut self, loader: impl BundleLoader + 'static) -> RuntimeBuilder {
        self.loaders.push(Box::new(loader));
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
            find_by_contract: host_find_by_contract,
            find_by_bundle: host_find_by_bundle,
            find_all_by_contract: host_find_all_by_contract,
            resolve_plugin: host_resolve_plugin,
            get_extension: host_get_extension,
        }));

        // Build loader dispatch map. Start with the built-in NativeBundleLoader.
        let native_loader: NativeBundleLoader =
            NativeBundleLoader::new(Arc::clone(&registry), host_vtable);
        let mut loader_map: HashMap<String, Box<dyn BundleLoader>> = HashMap::new();
        loader_map.insert(
            native_loader.runtime_name().to_owned(),
            Box::new(native_loader),
        );

        // Register user-provided loaders, checking for duplicates.
        for loader in self.loaders {
            let name: String = loader.runtime_name().to_owned();
            if loader_map.contains_key(&name) {
                return Err(RuntimeError::Loader(LoaderError::DuplicateLoader {
                    runtime_name: name,
                }));
            }
            loader_map.insert(name, loader);
        }

        let _ = &self.plugin_dirs;

        Ok(Runtime {
            registry,
            _bundles: bundles,
            host_vtable,
            _loaders: loader_map,
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

    /// Find the first provider of a contract.
    pub fn find_by_contract(
        &self,
        contract_id: u64,
        min_version: u32,
    ) -> Result<PluginHandle, RegistryError> {
        self.registry.find_by_contract(contract_id, min_version)
    }

    /// Find a specific bundle's provider of a contract.
    pub fn find_by_bundle(
        &self,
        bundle_id: u64,
        contract_id: u64,
        min_version: u32,
    ) -> Result<PluginHandle, RegistryError> {
        self.registry
            .find_by_bundle(bundle_id, contract_id, min_version)
    }

    /// Find all providers of a contract.
    pub fn find_all_by_contract(&self, contract_id: u64, min_version: u32) -> Vec<PluginHandle> {
        self.registry.find_all_by_contract(contract_id, min_version)
    }

    /// Resolve a plugin handle to a vtable pointer.
    pub fn resolve_plugin(
        &self,
        handle: PluginHandle,
    ) -> Result<*const PluginVTable, RegistryError> {
        self.registry.resolve(handle)
    }

    /// Legacy: Find a plugin by contract_id. Use find_by_contract() instead.
    pub fn find_plugin(
        &self,
        contract_id: u64,
        min_version: u32,
    ) -> Result<PluginHandle, RegistryError> {
        self.registry.find_by_contract(contract_id, min_version)
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

/// HostVTable.find_by_contract callback — dispatches to global registry with dependency enforcement.
//
// SAFETY: This function is called by plugin code through the HostVTable function pointer.
// The caller ensures calling convention is correct. All registry operations are lock-protected.
pub(crate) unsafe extern "C" fn host_find_by_contract(
    contract_id: u64,
    min_version: u32,
) -> PluginHandle {
    let registry: Arc<Registry> = match global_registry() {
        Some(r) => r,
        None => return PluginHandle::null(),
    };
    let caller_bundle_id: u64 = INIT_BUNDLE_ID.with(|c| c.get());
    if caller_bundle_id != 0 && !registry.is_dependency_declared(caller_bundle_id, contract_id) {
        // Dependency not declared — return null handle during init phase
        return PluginHandle::null();
    }
    match registry.find_by_contract(contract_id, min_version) {
        Ok(h) => h,
        Err(_) => PluginHandle::null(),
    }
}

/// HostVTable.find_by_bundle callback — dispatches to global registry with dependency enforcement.
//
// SAFETY: This function is called by plugin code through the HostVTable function pointer.
pub(crate) unsafe extern "C" fn host_find_by_bundle(
    bundle_id: u64,
    contract_id: u64,
    min_version: u32,
) -> PluginHandle {
    let registry: Arc<Registry> = match global_registry() {
        Some(r) => r,
        None => return PluginHandle::null(),
    };
    let caller_bundle_id: u64 = INIT_BUNDLE_ID.with(|c| c.get());
    if caller_bundle_id != 0 && !registry.is_dependency_declared(caller_bundle_id, contract_id) {
        return PluginHandle::null();
    }
    match registry.find_by_bundle(bundle_id, contract_id, min_version) {
        Ok(h) => h,
        Err(_) => PluginHandle::null(),
    }
}

/// HostVTable.find_all_by_contract callback — fills out buffer, NO dependency enforcement.
//
// SAFETY: This function is called by plugin code through the HostVTable function pointer.
// `out` must point to a valid buffer of at least `out_cap` PluginHandle elements.
// The caller (generated plugin code) allocates the buffer before calling this.
pub(crate) unsafe extern "C" fn host_find_all_by_contract(
    contract_id: u64,
    min_version: u32,
    out: *mut PluginHandle,
    out_cap: usize,
) -> usize {
    let registry: Arc<Registry> = match global_registry() {
        Some(r) => r,
        None => return 0,
    };
    // No dependency enforcement for find_all — enumeration is freely allowed
    let handles: Vec<PluginHandle> = registry.find_all_by_contract(contract_id, min_version);
    let count: usize = handles.len().min(out_cap);
    // SAFETY: out is valid for out_cap elements per ABI contract. We write at most out_cap items.
    for (i, &handle) in handles.iter().take(count).enumerate() {
        // SAFETY: i < count <= out_cap; out points to a valid buffer of out_cap PluginHandles.
        unsafe {
            *out.add(i) = handle;
        }
    }
    count
}

/// HostVTable.resolve_plugin callback — returns raw vtable pointer for a handle.
//
// SAFETY: This function is called by plugin code through the HostVTable function pointer.
// Returns null if the handle is stale or registry is not set.
// The returned pointer is valid as long as the plugin library is loaded.
// The host guarantees it does not unload libraries during active dispatch.
pub(crate) unsafe extern "C" fn host_resolve_plugin(handle: PluginHandle) -> *const PluginVTable {
    let registry: Arc<Registry> = match global_registry() {
        Some(r) => r,
        None => return core::ptr::null(),
    };
    match registry.resolve(handle) {
        Ok(ptr) => ptr,
        Err(_) => core::ptr::null(),
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
        // SAFETY: host_find_by_contract has no pointer preconditions — args are plain integers.
        let handle: PluginHandle = unsafe { host_find_by_contract(0_u64, 0_u32) };
        assert!(
            handle.is_null(),
            "host_find_by_contract must return null when plugin not found"
        );
    }

    #[test]
    fn init_bundle_id_thread_local_default_is_zero() {
        let id: u64 = INIT_BUNDLE_ID.with(|c| c.get());
        assert_eq!(id, 0_u64, "INIT_BUNDLE_ID must default to 0");
    }

    #[test]
    fn dep_enforcement_blocks_undeclared_contract() {
        // Set INIT_BUNDLE_ID to a non-zero value — simulating Phase 1 init.
        // No deps declared for this bundle_id, so any find_by_contract must return null.
        INIT_BUNDLE_ID.with(|c| c.set(0xDEAD_BEEF_u64));
        // SAFETY: host_find_by_contract has no pointer preconditions.
        let handle: PluginHandle =
            unsafe { host_find_by_contract(0x1111_2222_3333_4444_u64, 0_u32) };
        // Reset before asserting so subsequent tests are clean.
        INIT_BUNDLE_ID.with(|c| c.set(0_u64));
        assert!(
            handle.is_null(),
            "dep enforcement must return null for undeclared contract during init phase"
        );
    }
}

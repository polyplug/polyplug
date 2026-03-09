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

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::OnceLock;

use crate::abi::HostVTable;
use crate::abi::PluginHandle;
use crate::abi::PluginVTable;
use crate::allocator::polyplug_host_alloc;
use crate::allocator::polyplug_host_free;
use crate::error::RegistryError;
use crate::error::RuntimeError;
use crate::loader::LoadedBundle;
use crate::registry::Registry;
use std::collections::HashMap;

use crate::error::LoaderError;
use crate::abi::PluginRegistrar;
use crate::error::GraphError;
use crate::error::PolyplugError;
use crate::graph::CapabilityGraph;
use crate::loader::BundleInitGuard;
use crate::loader::manifest::ManifestData;
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
    loaders: HashMap<String, Box<dyn BundleLoader>>,
}

// SAFETY: Runtime wraps Arc<Registry> (Send+Sync) and Vec<LoadedBundle>.
// LoadedBundle contains a Box<Library> which is not Sync by itself,
// but libraries are stored in `Registry::loaded_libraries` and never shared
// as references — only vtable pointers (which are valid for the Registry's
// lifetime) are accessed concurrently. The Runtime is effectively immutable after init.
unsafe impl Send for Runtime {}
// SAFETY: See above — Runtime is immutable after init. All mutable state is behind Arc<RwLock>.
unsafe impl Sync for Runtime {}

/// Options for `Runtime::load_bundle_with`.
///
/// Currently empty — placeholder for future compatibility options (Epic 14).
pub struct LoadOptions {}

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
            let names: Vec<String> = loader.runtime_names();
            // Check for duplicates across all runtime names this loader handles.
            for name in &names {
                if loader_map.contains_key(name.as_str()) {
                    return Err(RuntimeError::Loader(LoaderError::DuplicateLoader {
                        runtime_name: name.clone(),
                    }));
                }
            }
            // Insert under the first name. Each JsLoader instance handles exactly ONE name
            // (JsLoader::runtime_names() uses the default which returns vec![self.runtime_name()]).
            // For all single-name loaders, this is semantically identical to the original code.
            if let Some(primary_name) = names.into_iter().next() {
                loader_map.insert(primary_name, loader);
            }
        }

        // Phase 1: Scan plugin directories for bundles
        let discovered: Vec<(PathBuf, ManifestData)> = crate::loader::scanner::scan_dirs(&self.plugin_dirs);

        // If nothing discovered, build Runtime with empty bundles (no graph needed)
        if !discovered.is_empty() {
            // Phase 2: Build capability graph
            let graph: CapabilityGraph =
                CapabilityGraph::from_manifests(&discovered).map_err(|e: GraphError| RuntimeError::Graph(e))?;

            // Phase 3: Get topological load order (providers first)
            let load_order: Vec<String> =
                graph.topological_order().map_err(|e: GraphError| RuntimeError::Graph(e))?;

            // Phase 4: Build lookup map bundle_name -> (path, manifest)
            let mut bundle_map: HashMap<String, (PathBuf, ManifestData)> = HashMap::new();
            for entry in discovered {
                bundle_map.insert(entry.1.bundle_name.clone(), entry);
            }

            // Phase 5: Dispatch each bundle to its loader in topo order
            for bundle_name in &load_order {
                let (bundle_path, manifest): &(PathBuf, ManifestData) = bundle_map
                    .get(bundle_name)
                    .ok_or_else(|| RuntimeError::Loader(LoaderError::InitFailed {
                        bundle: bundle_name.clone(),
                        error: "bundle in topo order but not found in map".to_owned(),
                    }))?;

                let loader: &dyn BundleLoader = loader_map
                    .get(&manifest.runtime)
                    .map(Box::as_ref)
                    .ok_or_else(|| RuntimeError::Loader(LoaderError::NoLoaderForRuntime {
                        bundle: bundle_path.display().to_string(),
                        runtime_name: manifest.runtime.clone(),
                    }))?;

                let (mut registrar, _guard): (PluginRegistrar, BundleInitGuard) =
                    crate::loader::make_registrar_context(&registry, manifest.bundle_id, host_vtable);

                loader.load(bundle_path, &mut registrar)
                    .map_err(|e: PolyplugError| match e {
                        PolyplugError::Loader(le) => RuntimeError::Loader(le),
                        other => RuntimeError::Loader(LoaderError::InitFailed {
                            bundle: bundle_path.display().to_string(),
                            error: other.to_string(),
                        }),
                    })?;
            }
        }

        Ok(Runtime {
            registry,
            _bundles: bundles,
            host_vtable,
            loaders: loader_map,
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

    /// Get the HostVTable for use in plugin registrars.
    pub fn host_vtable(&self) -> &'static HostVTable {
        self.host_vtable
    }

    /// Load a single plugin bundle explicitly by path.
    ///
    /// Reads the companion manifest, finds the matching loader, and dispatches.
    /// Does NOT perform graph pre-validation — intended for programmatic loads.
    pub fn load_bundle(&self, path: &Path) -> Result<(), PolyplugError> {
        self.load_bundle_with(path, LoadOptions {})
    }

    /// Load a single plugin bundle explicitly with options.
    pub fn load_bundle_with(
        &self,
        path: &Path,
        _opts: LoadOptions,
    ) -> Result<(), PolyplugError> {
        // Check companion manifest exists
        let manifest_path: PathBuf = path.with_extension("manifest.toml");
        if !manifest_path.exists() {
            return Err(PolyplugError::Loader(LoaderError::ManifestParse {
                path: manifest_path.display().to_string(),
                reason: "manifest file not found".to_owned(),
            }));
        }
        // Parse manifest and compute bundle_id
        let mut manifest: ManifestData = crate::loader::parse_manifest(path)
            .map_err(|e: LoaderError| PolyplugError::Loader(e))?;
        manifest.bundle_id = crate::abi::bundle_id(&manifest.bundle_name);
        // Find the loader for this runtime
        let runtime_name: &str = &manifest.runtime;
        let loader: &dyn BundleLoader = self
            .loaders
            .get(runtime_name)
            .map(Box::as_ref)
            .ok_or_else(|| PolyplugError::Loader(LoaderError::NoLoaderForRuntime {
                bundle: path.display().to_string(),
                runtime_name: runtime_name.to_owned(),
            }))?;
        // Build registrar and dispatch
        let mut registrar: PluginRegistrar = PluginRegistrar {
            register_plugin: crate::loader::registrar_callback,
            host: self.host_vtable as *const HostVTable,
        };
        crate::runtime::INIT_BUNDLE_ID.with(|c: &core::cell::Cell<u64>| c.set(manifest.bundle_id));
        let result: Result<(), PolyplugError> = loader.load(path, &mut registrar);
        crate::runtime::INIT_BUNDLE_ID.with(|c: &core::cell::Cell<u64>| c.set(0));
        result
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_creates_runtime() {
        let runtime: Runtime = Runtime::builder()
            .build()
            .expect("runtime build should succeed");
        // Registry starts empty
        let result: Result<PluginHandle, _> =
            runtime.find_by_contract(0x1234_5678_9ABC_DEF0_u64, 0);
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

    // ── Tests f/g: dep enforcement with a registered plugin ─────────────────
    //
    // These use a module-level OnceLock to register the test plugin in the
    // global registry exactly once. Cargo may run unit tests in parallel within
    // a binary, so the OnceLock ensures idempotent setup.

    use std::sync::OnceLock;

    /// One-time setup: register a plugin for contract 0xF00D_CAFE in the global registry.
    fn ensure_test_plugin_registered() {
        static SETUP: OnceLock<()> = OnceLock::new();
        SETUP.get_or_init(|| {
            let vtable: &'static crate::abi::PluginVTable =
                Box::leak(Box::new(crate::abi::PluginVTable {
                    contract_id: 0xF00D_CAFE_0000_0001_u64,
                    contract_version: 0,
                    function_count: 0,
                    functions: core::ptr::null(),
                }));
            let desc: crate::abi::PluginDescriptor = crate::abi::PluginDescriptor {
                name: crate::abi::StringView::from_static(b"test-dep-plugin"),
                contract_name: crate::abi::StringView::from_static(b"test.dep.Contract"),
                version_major: 1,
                version_minor: 0,
                version_patch: 0,
            };
            // Use the existing global registry if already set; otherwise create and set one.
            let registry: Arc<crate::registry::Registry> = global_registry().unwrap_or_else(|| {
                let r: Arc<crate::registry::Registry> = Arc::new(crate::registry::Registry::new());
                set_global_registry(Arc::clone(&r));
                r
            });
            // SAFETY: vtable is 'static and valid for the registry lifetime.
            unsafe {
                registry
                    .register(
                        desc,
                        vtable,
                        "test.dep.Contract".to_owned(),
                        0xBEEF_0001_u64,
                    )
                    .expect("setup: register test-dep-plugin");
            }
        });
    }

    /// Test f — declared dependency passes dep enforcement.
    #[test]
    fn declared_dep_passes_enforcement() {
        ensure_test_plugin_registered();
        let caller_bid: u64 = crate::abi::bundle_id("caller-bundle-f");
        let dep_cid: u64 = 0xF00D_CAFE_0000_0001_u64;
        // Declare the dependency so enforcement allows lookup.
        if let Some(reg) = global_registry() {
            reg.declare_deps(caller_bid, vec![dep_cid])
                .expect("declare_deps should succeed");
        }
        INIT_BUNDLE_ID.with(|c| c.set(caller_bid));
        // SAFETY: host_find_by_contract has no pointer preconditions.
        let handle: PluginHandle = unsafe { host_find_by_contract(dep_cid, 0_u32) };
        // Reset INIT_BUNDLE_ID before asserting so other tests are unaffected.
        INIT_BUNDLE_ID.with(|c| c.set(0_u64));
        assert!(
            !handle.is_null(),
            "declared dependency must return a valid handle during init phase"
        );
    }

    /// Test g — find_all_by_contract skips dependency enforcement.
    #[test]
    fn find_all_skips_dep_enforcement() {
        ensure_test_plugin_registered();
        let caller_bid: u64 = crate::abi::bundle_id("caller-bundle-g-no-deps");
        let dep_cid: u64 = 0xF00D_CAFE_0000_0001_u64;
        // Do NOT declare any deps for this bundle — enforcement would block find_by_contract.
        INIT_BUNDLE_ID.with(|c| c.set(caller_bid));
        // Use a local buffer for the out parameter.
        let mut handles: [PluginHandle; 8] = [PluginHandle {
            index: 0,
            generation: 0,
        }; 8];
        // SAFETY: handles is a valid local array; out pointer and cap are consistent.
        let count: usize =
            unsafe { host_find_all_by_contract(dep_cid, 0_u32, handles.as_mut_ptr(), 8_usize) };
        INIT_BUNDLE_ID.with(|c| c.set(0_u64));
        assert!(
            count >= 1,
            "find_all must return providers even without declared deps (no enforcement)"
        );
    }
}

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

use core::time::Duration;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use crate::error::GraphError;
use crate::error::HostContractError;
use crate::error::LoaderError;
use crate::error::PolyplugError;
use crate::error::RegistryError;
use crate::error::RuntimeError;
use crate::graph::CapabilityGraph;
use crate::loader::BundleLoader;
use crate::loader::LoadedBundle;
use crate::loader::manifest::ManifestData;
use crate::registry::Registry;
use crate::version::Compatibility;
use crate::version::Version;
use polyplug_abi::HostContractVTable;
use polyplug_abi::HostRuntime;
use polyplug_abi::HostVTable;
use polyplug_abi::PluginDescriptor;
use polyplug_abi::PluginHandle;
use polyplug_abi::PluginInterface;

use core::sync::atomic::Ordering;
use notify::Watcher;

// ─── HostContext for rt_ctx parameter ─────────────────────────────────────────

/// Context passed to host functions via rt_ctx parameter.
/// Contains the runtime pointer and the bundle_id of the calling bundle.
#[repr(C)]
pub struct HostContext {
    pub runtime: *mut Runtime,
    pub bundle_id: u64,
}

// SAFETY: HostContext contains a raw pointer to Runtime (which is Send+Sync)
// and a u64. The runtime pointer is only dereferenced in host functions that
// receive it from the loader, which guarantees the Runtime is live.
unsafe impl Send for HostContext {}
// SAFETY: Same reasoning as Send — HostContext contains only a raw pointer to Runtime
// (which is Sync) and a u64. Concurrent reads of the pointer are safe.
unsafe impl Sync for HostContext {}

// ─── Runtime Configuration ───────────────────────────────────────────────────

/// Configuration for the polyplug runtime.
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    /// Whether hot-reload is enabled for this runtime.
    /// When disabled, reload_bundle() returns ReloadDisabled error.
    pub hot_reload_enabled: bool,
    /// Maximum number of retry attempts for hot-reload operations.
    pub hot_reload_max_retries: u32,
    /// Interval between hot-reload retry attempts.
    pub hot_reload_retry_interval: Duration,
    /// Whether to abort the runtime when max retries are exhausted.
    pub hot_reload_abort_on_max_retries: bool,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            hot_reload_enabled: false,
            hot_reload_max_retries: 3,
            hot_reload_retry_interval: Duration::from_secs(1),
            hot_reload_abort_on_max_retries: true,
        }
    }
}

/// Type alias for the warning callback to avoid repetition.
type WarningCb = Box<dyn Fn(&str) + Send + Sync>;

/// Type alias for the reload callback.
type ReloadCb = std::sync::Arc<dyn Fn(crate::reload::ReloadPhase) + Send + Sync>;

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
    /// ManifestData for all loaded bundles, keyed by bundle_name.
    /// Used by reload_bundle() for cascade detection.
    pub(crate) bundle_manifests:
        std::sync::Mutex<HashMap<String, crate::loader::manifest::ManifestData>>,
    /// Library handles for reloaded native bundles — these ARE droppable (unlike loaded_libraries).
    /// Keyed by bundle_id. On each reload the old handle is removed and dropped after quiescence.
    pub(crate) reload_libraries: std::sync::Mutex<HashMap<u64, libloading::Library>>,
    /// Optional callback fired after vtable swap, before dlclose.
    pub(crate) on_reload_cb: Option<ReloadCb>,
    /// Background watcher thread handle. Joined on Drop.
    watcher_thread: std::sync::Mutex<Option<std::thread::JoinHandle<()>>>,
    /// Stop flag sent to watcher thread.
    watcher_stop: std::sync::Mutex<Option<std::sync::Arc<core::sync::atomic::AtomicBool>>>,
    config: RuntimeConfig,
    /// Optional warning callback. If None, warnings go to stderr.
    warning_cb: Option<WarningCb>,
    /// Last error message for FFI error reporting.
    last_error: std::sync::Mutex<String>,
    /// Captured vtables during hot-reload. Used by reload_register_callback.
    pub(crate) reload_captured_vtables: std::sync::Mutex<Vec<crate::reload::VTablePtr>>,
    /// Registered host contracts, keyed by contract_id.
    host_contracts: std::sync::RwLock<HashMap<u64, &'static HostContractVTable>>,
    /// Host runtime type identifier.
    host_runtime: HostRuntime,
}

/// Options for `Runtime::load_bundle_with`.
///
/// The `compatibility` field overrides the global `RuntimeBuilder::compatibility` setting
/// for this specific bundle load only.
pub struct LoadOptions {
    pub compatibility: Compatibility,
    pub ignore_function_count_mismatch: bool,
}

/// Builder for constructing a Runtime.
pub struct RuntimeBuilder {
    plugin_dirs: Vec<PathBuf>,
    loaders: Vec<Box<dyn BundleLoader>>,
    compatibility: Compatibility,
    warning_cb: Option<WarningCb>,
    on_reload_cb: Option<ReloadCb>,
    config: RuntimeConfig,
    host_runtime: HostRuntime,
}

impl RuntimeBuilder {
    /// Create a new RuntimeBuilder with default settings.
    pub fn new() -> RuntimeBuilder {
        RuntimeBuilder {
            plugin_dirs: Vec::new(),
            loaders: Vec::new(),
            compatibility: Compatibility::default(),
            warning_cb: None,
            on_reload_cb: None,
            config: RuntimeConfig::default(),
            host_runtime: HostRuntime::Rust,
        }
    }

    /// Add a directory to scan for plugin bundles during `build()`.
    pub fn plugin_dir(mut self, path: PathBuf) -> RuntimeBuilder {
        self.plugin_dirs.push(path);
        self
    }

    /// Register a bundle loader for a runtime.
    ///
    /// The loader is identified by `loader.runtime_name()`. Duplicate registrations
    /// (same runtime name) are detected in `build()` and cause `build()` to return
    /// `Err(RuntimeError::Loader(LoaderError::DuplicateLoader { .. }))`.
    ///
    /// The native loader (`"native"`) is registered automatically during `build()`
    /// unless a user-provided loader already claims that name.
    pub fn loader(mut self, loader: impl BundleLoader + 'static) -> RuntimeBuilder {
        self.loaders.push(Box::new(loader));
        self
    }

    /// Set the global compatibility mode for version negotiation.
    /// Defaults to `Compatibility::Strict`.
    pub fn compatibility(mut self, c: Compatibility) -> RuntimeBuilder {
        self.compatibility = c;
        self
    }

    /// Register a warning callback.
    ///
    /// Only the first registered callback takes effect (OnceLock semantics).
    /// The callback receives human-readable warning strings.
    pub fn on_warning(mut self, cb: impl Fn(&str) + Send + Sync + 'static) -> RuntimeBuilder {
        self.warning_cb = Some(Box::new(cb));
        self
    }

    /// Register a callback fired after each successful vtable swap, before dlclose.
    ///
    /// The callback receives a `ReloadPhase` describing the reload phase.
    pub fn on_reload(
        mut self,
        cb: impl Fn(crate::reload::ReloadPhase) + Send + Sync + 'static,
    ) -> RuntimeBuilder {
        self.on_reload_cb = Some(std::sync::Arc::new(cb));
        self
    }

    pub fn config(mut self, config: RuntimeConfig) -> RuntimeBuilder {
        self.config = config;
        self
    }

    /// Set the host runtime type.
    /// Defaults to `HostRuntime::Rust`.
    pub fn host_runtime(mut self, runtime: HostRuntime) -> RuntimeBuilder {
        self.host_runtime = runtime;
        self
    }

    /// Build the runtime.
    //
    //  For MVP: scans plugin_dirs for .so/.dll/.dylib files,
    //  loads them in sorted order, registers vtables.
    //  Full capability graph resolution is a future enhancement.
    pub fn build(self) -> Result<Runtime, RuntimeError> {
        let registry: Arc<Registry> = Arc::new(Registry::new());

        // Build the static HostVTable. This must be 'static.
        let host_vtable: &'static HostVTable = Box::leak(Box::new(HostVTable {
            register_plugin: host_register_plugin,
            alloc: host_alloc,
            free: host_free,
            find_by_contract: host_find_by_contract,
            find_by_bundle: host_find_by_bundle,
            find_all_by_contract: host_find_all_by_contract,
            resolve_plugin: host_resolve_plugin,
            get_host_contract: host_get_host_contract,
        }));

        let mut loader_map: HashMap<String, Box<dyn BundleLoader>> = HashMap::new();

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

        if !loader_map.contains_key("native") {
            let native_loader: crate::loader::NativeBundleLoader =
                crate::loader::NativeBundleLoader::new(Arc::clone(&registry), host_vtable);
            loader_map.insert("native".to_owned(), Box::new(native_loader));
        }

        // Phase 1: Scan plugin directories for bundles
        let discovered: Vec<(PathBuf, ManifestData)> =
            crate::loader::scanner::scan_dirs(&self.plugin_dirs);

        // Snapshot manifests for hot-reload cascade detection.
        let mut manifests_map: HashMap<String, crate::loader::manifest::ManifestData> =
            HashMap::new();
        for (path, manifest) in &discovered {
            let mut stored_manifest: ManifestData = manifest.clone();
            stored_manifest.path = path.clone();
            manifests_map.insert(stored_manifest.name.clone(), stored_manifest);
        }

        // Create Runtime first (before loading bundles) so we can pass it to loaders
        let runtime: Runtime = Runtime {
            registry: Arc::clone(&registry),
            _bundles: Vec::new(),
            host_vtable,
            loaders: loader_map,
            bundle_manifests: std::sync::Mutex::new(manifests_map),
            reload_libraries: std::sync::Mutex::new(HashMap::new()),
            on_reload_cb: self.on_reload_cb,
            watcher_thread: std::sync::Mutex::new(None),
            watcher_stop: std::sync::Mutex::new(None),
            config: self.config,
            warning_cb: self.warning_cb,
            last_error: std::sync::Mutex::new(String::new()),
            reload_captured_vtables: std::sync::Mutex::new(Vec::new()),
            host_contracts: std::sync::RwLock::new(HashMap::new()),
            host_runtime: self.host_runtime,
        };

        // If nothing discovered, return Runtime with empty bundles (no graph needed)
        if !discovered.is_empty() {
            // Phase 2: Build capability graph
            let graph: CapabilityGraph = CapabilityGraph::from_manifests(&discovered)
                .map_err(|e: GraphError| RuntimeError::Graph(e))?;

            // Phase 2.5: Validate version compatibility
            validate_bundle_compatibility(
                &discovered,
                self.compatibility,
                runtime.warning_cb.as_ref(),
            )?;

            // Phase 3: Get topological load order (providers first)
            let load_order: Vec<String> = graph
                .topological_order()
                .map_err(|e: GraphError| RuntimeError::Graph(e))?;

            // Phase 4: Build lookup map bundle_name -> (path, manifest)
            let mut bundle_map: HashMap<String, (PathBuf, ManifestData)> = HashMap::new();
            for entry in discovered {
                bundle_map.insert(entry.1.name.clone(), entry);
            }

            // Phase 5: Dispatch each bundle to its loader in topo order
            for bundle_name in &load_order {
                let (bundle_path, manifest): &(PathBuf, ManifestData) =
                    bundle_map.get(bundle_name).ok_or_else(|| {
                        RuntimeError::Loader(LoaderError::InitFailed {
                            bundle: bundle_name.clone(),
                            error: "bundle in topo order but not found in map".to_owned(),
                        })
                    })?;

                let loader: &dyn BundleLoader = runtime
                    .loaders
                    .get(&manifest.runtime)
                    .map(Box::as_ref)
                    .ok_or_else(|| {
                        RuntimeError::Loader(LoaderError::NoLoaderForRuntime {
                            bundle: bundle_path.display().to_string(),
                            runtime_name: manifest.runtime.clone(),
                        })
                    })?;

                loader
                    .load(manifest, &runtime)
                    .map_err(|e: PolyplugError| match e {
                        PolyplugError::Loader(le) => RuntimeError::Loader(le),
                        other => RuntimeError::Loader(LoaderError::InitFailed {
                            bundle: manifest.name.clone(),
                            error: other.to_string(),
                        }),
                    })?;
            }
        }

        Ok(runtime)
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
    #[inline(always)]
    pub fn find_by_contract(
        &self,
        contract_id: u64,
        min_version: u32,
    ) -> Result<PluginHandle, RegistryError> {
        self.registry.find_by_contract(contract_id, min_version)
    }

    /// Find a specific bundle's provider of a contract.
    #[inline(always)]
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
    #[inline(always)]
    pub fn find_all_by_contract(
        &self,
        contract_id: u64,
        min_version: u32,
        out: &mut [PluginHandle],
    ) -> usize {
        self.registry
            .find_all_by_contract(contract_id, min_version, out)
    }

    /// Find all providers of a contract, packing handles directly into a u64 buffer.
    #[inline(always)]
    pub fn find_all_by_contract_packed(
        &self,
        contract_id: u64,
        min_version: u32,
        out: &mut [u64],
    ) -> usize {
        self.registry
            .find_all_by_contract_packed(contract_id, min_version, out)
    }

    /// Resolve a plugin handle to a vtable guard.
    #[inline(always)]
    pub fn resolve_plugin(
        &self,
        handle: PluginHandle,
    ) -> Result<crate::registry::PluginGuard, RegistryError> {
        self.registry.resolve_guard(handle)
    }

    /// Register a host contract vtable.
    /// Returns `Err(HostContractError::DuplicateContract)` if a contract with the same ID is already registered.
    pub fn register_host_contract(
        &self,
        contract_id: u64,
        vtable: &'static HostContractVTable,
    ) -> Result<(), HostContractError> {
        let mut guard: std::sync::RwLockWriteGuard<'_, HashMap<u64, &'static HostContractVTable>> =
            self.host_contracts.write().unwrap_or_else(|e| {
                eprintln!("[polyplug] RwLock poisoned, recovering: {}", e);
                e.into_inner()
            });
        if guard.contains_key(&contract_id) {
            return Err(HostContractError::DuplicateContract { contract_id });
        }
        guard.insert(contract_id, vtable);
        Ok(())
    }

    /// Unregister a host contract vtable.
    /// Returns `true` if the contract was registered and removed, `false` if it was not found.
    pub fn unregister_host_contract(&self, contract_id: u64) -> bool {
        let mut guard: std::sync::RwLockWriteGuard<'_, HashMap<u64, &'static HostContractVTable>> =
            self.host_contracts.write().unwrap_or_else(|e| {
                eprintln!("[polyplug] RwLock poisoned, recovering: {}", e);
                e.into_inner()
            });
        guard.remove(&contract_id).is_some()
    }

    /// Get a host contract vtable by contract_id and minimum version.
    /// Returns `None` if no matching contract is found or if the version is too low.
    pub fn get_host_contract(
        &self,
        contract_id: u64,
        min_version: u32,
    ) -> Option<&'static HostContractVTable> {
        let guard: std::sync::RwLockReadGuard<'_, HashMap<u64, &'static HostContractVTable>> =
            self.host_contracts.read().unwrap_or_else(|e| {
                eprintln!("[polyplug] RwLock poisoned, recovering: {}", e);
                e.into_inner()
            });
        guard.get(&contract_id).and_then(|vtable| {
            let header: &polyplug_abi::HostContractVTableHeader = &vtable.header;
            let version: u32 = (header.contract_major << 16) | header.contract_minor;
            if version >= min_version {
                Some(*vtable)
            } else {
                None
            }
        })
    }

    /// Get the host runtime type.
    #[inline(always)]
    pub fn host_runtime(&self) -> HostRuntime {
        self.host_runtime
    }

    /// Get the HostVTable for use in plugin registrars.
    #[inline(always)]
    pub fn host_vtable(&self) -> &'static HostVTable {
        self.host_vtable
    }

    #[inline(always)]
    pub fn registry(&self) -> &std::sync::Arc<Registry> {
        &self.registry
    }

    /// Get the runtime configuration.
    #[inline(always)]
    pub fn config(&self) -> &RuntimeConfig {
        &self.config
    }

    /// Emit a warning message via the registered warning callback, or to stderr if none.
    pub fn emit_warning(&self, msg: &str) {
        match &self.warning_cb {
            Some(cb) => cb(msg),
            None => eprintln!("[polyplug] {msg}"),
        }
    }

    /// Set the last error message for FFI error reporting.
    pub(crate) fn set_last_error(&self, msg: impl Into<String>) {
        let mut guard: std::sync::MutexGuard<'_, String> =
            self.last_error.lock().unwrap_or_else(|e| {
                eprintln!("[polyplug] Mutex poisoned, recovering: {}", e);
                e.into_inner()
            });
        *guard = msg.into();
    }

    /// Get the last error message for FFI error reporting.
    /// Returns the number of bytes written to the buffer.
    pub(crate) fn get_last_error(&self, buf: &mut [u8]) -> usize {
        let guard: std::sync::MutexGuard<'_, String> = self.last_error.lock().unwrap_or_else(|e| {
            eprintln!("[polyplug] Mutex poisoned, recovering: {}", e);
            e.into_inner()
        });
        let bytes: &[u8] = guard.as_bytes();
        let write_n: usize = bytes.len().min(buf.len());
        if write_n > 0 {
            buf[..write_n].copy_from_slice(&bytes[..write_n]);
        }
        write_n
    }

    /// Clear the last error message.
    pub(crate) fn clear_last_error(&self) {
        let mut guard: std::sync::MutexGuard<'_, String> =
            self.last_error.lock().unwrap_or_else(|e| {
                eprintln!("[polyplug] Mutex poisoned, recovering: {}", e);
                e.into_inner()
            });
        guard.clear();
    }

    /// Get the length of the last error message.
    pub(crate) fn last_error_len(&self) -> usize {
        let guard: std::sync::MutexGuard<'_, String> = self.last_error.lock().unwrap_or_else(|e| {
            eprintln!("[polyplug] Mutex poisoned, recovering: {}", e);
            e.into_inner()
        });
        guard.len()
    }

    /// Register an additional bundle loader into this runtime after build.
    ///
    /// `loader` must be a `Box<dyn BundleLoader>` produced by a loader cdylib compiled
    /// against the same polyplug rlib. Ownership is transferred — the caller must not
    /// free the loader after a successful call.
    ///
    /// Returns `Err(RuntimeError::Loader(LoaderError::DuplicateLoader { .. }))` if a
    /// loader for the same runtime name is already registered.
    pub fn register_loader(&mut self, loader: Box<dyn BundleLoader>) -> Result<(), RuntimeError> {
        let names: Vec<String> = loader.runtime_names();
        for name in &names {
            if self.loaders.contains_key(name.as_str()) {
                return Err(RuntimeError::Loader(LoaderError::DuplicateLoader {
                    runtime_name: name.clone(),
                }));
            }
        }
        if let Some(primary_name) = names.into_iter().next() {
            self.loaders.insert(primary_name, loader);
        }
        Ok(())
    }

    /// Load a single plugin bundle explicitly by path.
    ///
    /// Reads the companion manifest, finds the matching loader, and dispatches.
    /// Does NOT perform graph pre-validation — intended for programmatic loads.
    pub fn load_bundle(&self, path: &Path) -> Result<(), PolyplugError> {
        self.load_bundle_with(
            path,
            LoadOptions {
                compatibility: Compatibility::default(),
                ignore_function_count_mismatch: false,
            },
        )
    }

    /// Load a single plugin bundle explicitly with options.
    pub fn load_bundle_with(&self, path: &Path, opts: LoadOptions) -> Result<(), PolyplugError> {
        // Determine the bundle directory: if path is a file, use its parent; otherwise use path as-is.
        let bundle_dir: &Path = if path.is_file() {
            path.parent().unwrap_or(path)
        } else {
            path
        };

        let manifest: ManifestData = crate::loader::parse_manifest(bundle_dir)
            .map_err(|e: LoaderError| PolyplugError::Loader(e))?;
        if manifest.id == 0 {
            return Err(PolyplugError::Loader(LoaderError::InitFailed {
                bundle: path.display().to_string(),
                error: "manifest.id is required but was 0 or missing".to_owned(),
            }));
        }

        // Validate function_count entries for this explicit load
        if !opts.ignore_function_count_mismatch {
            let major_str: &str = match manifest.version.split_once('.') {
                Some((maj, _)) => maj,
                None => "0",
            };
            for contract in &manifest.provides {
                // Extract contract name without version (e.g., "data.Reporter" from "data.Reporter@1.0")
                let contract_name: &str = match contract.split_once('@') {
                    Some((name, _)) => name,
                    None => contract,
                };
                let key: String = format!("{}@{}", contract_name, major_str);
                if !manifest.function_count.contains_key(&key)
                    && opts.compatibility != Compatibility::Yolo
                {
                    let msg: String = format!(
                        "bundle {:?} provides {:?} but has no function_count entry for key {:?}",
                        manifest.name, contract, key
                    );
                    if opts.compatibility == Compatibility::Strict {
                        return Err(PolyplugError::Loader(LoaderError::FunctionCountMismatch {
                            contract: contract.clone(),
                            // sentinel 0/0: entry is missing entirely; actual count is unknown without loading the .so
                            expected: 0,
                            found: 0,
                        }));
                    } else {
                        self.emit_warning(&msg);
                    }
                }
            }
        }

        // Find the loader for this runtime
        let runtime_name: &str = &manifest.runtime;
        let loader: &dyn BundleLoader = self
            .loaders
            .get(runtime_name)
            .map(Box::as_ref)
            .ok_or_else(|| {
                PolyplugError::Loader(LoaderError::NoLoaderForRuntime {
                    bundle: path.display().to_string(),
                    runtime_name: runtime_name.to_owned(),
                })
            })?;

        let result: Result<(), PolyplugError> = loader.load(&manifest, self);
        if result.is_ok() {
            let bundle_name: String = manifest.name.clone();
            let mut manifests: std::sync::MutexGuard<'_, HashMap<String, ManifestData>> =
                self.bundle_manifests.lock().unwrap_or_else(|e| {
                    eprintln!("[polyplug] Mutex poisoned, recovering: {}", e);
                    e.into_inner()
                });
            manifests.insert(bundle_name, manifest);
        }
        result
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.watcher_stop.lock()
            && let Some(flag) = guard.take()
        {
            flag.store(true, core::sync::atomic::Ordering::Relaxed);
        }
        if let Ok(mut guard) = self.watcher_thread.lock()
            && let Some(handle) = guard.take()
        {
            let _: std::thread::Result<()> = handle.join();
        }
    }
}

impl Runtime {
    /// Start a background file watcher on `dir`.
    ///
    /// Automatically calls `reload_bundle()` when a `.so` / `.dll` / `.dylib`
    /// file in the watched directory is modified or created.
    /// Uses a 100ms debounce to suppress duplicate events.
    ///
    /// The caller must hold this `Runtime` in an `Arc`. Pass `Arc::clone(&rt)`
    /// as `self_arc` — e.g. `Runtime::watch_plugin_dir(Arc::clone(&rt), dir)`.
    pub fn watch_plugin_dir(
        self_arc: std::sync::Arc<Runtime>,
        dir: &std::path::Path,
    ) -> Result<(), crate::error::PolyplugError> {
        let canonical_dir: PathBuf = dir.canonicalize().map_err(|e: std::io::Error| {
            crate::error::PolyplugError::WatcherFailed {
                reason: e.to_string(),
            }
        })?;

        let stop_flag: Arc<core::sync::atomic::AtomicBool> =
            Arc::new(core::sync::atomic::AtomicBool::new(false));
        let stop_flag_thread: Arc<core::sync::atomic::AtomicBool> = Arc::clone(&stop_flag);

        let debounce: Arc<std::sync::Mutex<HashMap<PathBuf, std::time::Instant>>> =
            Arc::new(std::sync::Mutex::new(HashMap::new()));
        let debounce_cb: Arc<std::sync::Mutex<HashMap<PathBuf, std::time::Instant>>> =
            Arc::clone(&debounce);

        let runtime_weak: std::sync::Weak<Runtime> = Arc::downgrade(&self_arc);

        let mut watcher: notify::RecommendedWatcher =
            notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                let event: notify::Event = match res {
                    Ok(e) => e,
                    Err(_) => return,
                };
                if !matches!(
                    event.kind,
                    notify::EventKind::Modify(_) | notify::EventKind::Create(_)
                ) {
                    return;
                }
                for path in &event.paths {
                    let ext: &str = path
                        .extension()
                        .and_then(|s: &std::ffi::OsStr| s.to_str())
                        .unwrap_or("");
                    if !matches!(ext, "so" | "dll" | "dylib") {
                        continue;
                    }
                    let now: std::time::Instant = std::time::Instant::now();
                    let mut debounce_map: std::sync::MutexGuard<
                        '_,
                        HashMap<PathBuf, std::time::Instant>,
                    > = debounce_cb.lock().unwrap_or_else(|e| {
                        eprintln!("[polyplug] Mutex poisoned, recovering: {}", e);
                        e.into_inner()
                    });
                    let last: std::time::Instant =
                        debounce_map.get(path).copied().unwrap_or_else(|| {
                            now.checked_sub(core::time::Duration::from_secs(1_u64))
                                .unwrap_or(now)
                        });
                    if now.duration_since(last) < core::time::Duration::from_millis(100_u64) {
                        continue;
                    }
                    debounce_map.insert(path.clone(), now);
                    drop(debounce_map);
                    let bundle_path_str: String = path.to_string_lossy().into_owned();
                    if let Some(rt) = runtime_weak.upgrade() {
                        match crate::reload::reload_bundle_impl(
                            &rt,
                            std::path::Path::new(&bundle_path_str),
                            0_usize,
                        ) {
                            Ok(()) => {}
                            Err(e) => {
                                rt.emit_warning(&format!(
                                    "hot-reload: auto-reload failed for {bundle_path_str}: {e}"
                                ));
                            }
                        }
                    }
                }
            })
            .map_err(|e: notify::Error| {
                crate::error::PolyplugError::WatcherFailed {
                    reason: e.to_string(),
                }
            })?;

        watcher
            .watch(&canonical_dir, notify::RecursiveMode::Recursive)
            .map_err(
                |e: notify::Error| crate::error::PolyplugError::WatcherFailed {
                    reason: e.to_string(),
                },
            )?;

        let handle: std::thread::JoinHandle<()> = std::thread::spawn(move || {
            // Keep watcher alive for the thread lifetime.
            let _watcher: notify::RecommendedWatcher = watcher;
            while !stop_flag_thread.load(Ordering::Relaxed) {
                std::thread::sleep(core::time::Duration::from_millis(10_u64));
            }
        });

        self_arc
            .watcher_stop
            .lock()
            .unwrap_or_else(|e| {
                eprintln!("[polyplug] Mutex poisoned, recovering: {}", e);
                e.into_inner()
            })
            .replace(stop_flag);
        self_arc
            .watcher_thread
            .lock()
            .unwrap_or_else(|e| {
                eprintln!("[polyplug] Mutex poisoned, recovering: {}", e);
                e.into_inner()
            })
            .replace(handle);
        Ok(())
    }
}

// ─── Module-level validation helpers ────────────────────────────────────────

/// Validate version compatibility for all discovered bundles.
///
/// Iterates each bundle's dependencies. For each dependency with a `min_version`,
/// finds the provider bundle and compares versions.
/// Also checks that each provided contract has a `function_count` entry.
///
/// Behaviour depends on `compatibility`:
/// - `Strict`: returns `Err` on any mismatch
/// - `Relaxed`: emits warning, continues
/// - `Yolo`: silently ignores all mismatches
pub(crate) fn validate_bundle_compatibility(
    manifests: &[(PathBuf, ManifestData)],
    compatibility: Compatibility,
    warning_cb: Option<&WarningCb>,
) -> Result<(), RuntimeError> {
    // Build provider_map: contract_name -> &ManifestData
    let mut provider_map: HashMap<String, &ManifestData> = HashMap::new();
    for (_path, manifest) in manifests {
        for contract in &manifest.provides {
            provider_map.insert(contract.clone(), manifest);
        }
    }

    for (_path, manifest) in manifests {
        // Check version compatibility for each dependency
        let resolved: Vec<crate::loader::manifest::ManifestDependency> =
            manifest.resolved_dependencies();
        for dep in &resolved {
            let (dep_contract, dep_min_version_str): (&str, &str) = match dep {
                crate::loader::manifest::ManifestDependency::ByContract {
                    contract,
                    min_version,
                    ..
                } => (contract.as_str(), min_version.as_str()),
                crate::loader::manifest::ManifestDependency::ByBundle {
                    contract,
                    min_version,
                    ..
                } => (contract.as_str(), min_version.as_str()),
            };

            if dep_min_version_str.is_empty() {
                continue;
            }

            let provider: &ManifestData = match provider_map.get(dep_contract) {
                Some(p) => p,
                None => continue, // graph already validates this
            };

            let required: Version = match Version::parse(dep_min_version_str, &manifest.name) {
                Ok(v) => v,
                Err(e) => return Err(RuntimeError::Loader(e)),
            };

            let provided: Version = parse_manifest_version(&provider.version, &provider.name)?;

            if !provided.is_compatible_with(&required) {
                match compatibility {
                    Compatibility::Strict => {
                        return Err(RuntimeError::Loader(LoaderError::VersionMismatch {
                            contract: dep_contract.to_owned(),
                            required,
                            found: provided,
                        }));
                    }
                    Compatibility::Relaxed => {
                        let msg: String = format!(
                            "version mismatch for contract `{}`: required={}, found={} (bundle `{}`)",
                            dep_contract, required, provided, provider.name
                        );
                        match warning_cb {
                            Some(cb) => cb(&msg),
                            None => eprintln!("[polyplug] {msg}"),
                        }
                    }
                    Compatibility::Yolo => {} // intentionally silent — Yolo mode skips all version checks
                }
            }
        }

        // Check function_count entries for provided contracts
        for contract in &manifest.provides {
            let major_str: &str = match manifest.version.split_once('.') {
                Some((maj, _)) => maj,
                None => "0",
            };
            let key: String = format!("{}@{}", contract, major_str);
            if !manifest.function_count.contains_key(&key) {
                match compatibility {
                    Compatibility::Strict => {
                        return Err(RuntimeError::Loader(LoaderError::FunctionCountMismatch {
                            contract: contract.clone(),
                            // sentinel 0/0: entry is missing entirely; actual count is unknown without loading the .so
                            expected: 0,
                            found: 0,
                        }));
                    }
                    Compatibility::Relaxed => {
                        let msg: String = format!(
                            "bundle `{}` provides `{}` but has no function_count entry for key `{}`",
                            manifest.name, contract, key
                        );
                        match warning_cb {
                            Some(cb) => cb(&msg),
                            None => eprintln!("[polyplug] {msg}"),
                        }
                    }
                    Compatibility::Yolo => {} // intentionally silent — Yolo mode skips all function_count checks
                }
            }
        }
    }

    Ok(())
}

fn parse_manifest_version(v: &str, bundle_name: &str) -> Result<Version, RuntimeError> {
    if v.is_empty() {
        Ok(Version { major: 0, minor: 0 })
    } else {
        Version::parse(v, bundle_name).map_err(RuntimeError::Loader)
    }
}

// ─── HostVTable C ABI callbacks ───────────────────────────────────────────────

/// HostVTable.register_plugin callback — registers a plugin vtable with the runtime.
///
/// # Safety
/// - rt_ctx must be a valid pointer to a HostContext
/// - descriptor must point to a valid PluginDescriptor
/// - vtable must point to a valid PluginInterface that remains valid for the Runtime lifetime
pub(crate) unsafe extern "C" fn host_register_plugin(
    rt_ctx: *mut core::ffi::c_void,
    descriptor: *const PluginDescriptor,
    vtable: *const PluginInterface,
) -> polyplug_abi::AbiError {
    if rt_ctx.is_null() {
        return polyplug_abi::AbiError {
            code: polyplug_abi::ABI_ERROR_GENERIC,
            message: polyplug_abi::StringView::null(),
        };
    }
    // SAFETY: rt_ctx is a valid *mut HostContext passed by the host during polyplug_init
    let ctx: &HostContext = unsafe { &*(rt_ctx as *const HostContext) };
    // SAFETY: ctx.runtime is a valid pointer to a Runtime that is guaranteed to be live
    // during the plugin init call.
    // SAFETY: ctx.runtime is a valid pointer to a Runtime that is guaranteed to be live
    // during the plugin init call.
    let runtime: &Runtime = unsafe { &*ctx.runtime };
    let registry: &Registry = &runtime.registry;
    let bundle_id: u64 = ctx.bundle_id;

    // SAFETY: descriptor is provided by the plugin's polyplug_init function
    let desc: PluginDescriptor = unsafe { *descriptor };

    if desc.contract_name.ptr.is_null() || desc.contract_name.len == 0 {
        return polyplug_abi::AbiError {
            code: polyplug_abi::ABI_ERROR_GENERIC,
            message: polyplug_abi::StringView::from_static(
                b"PluginDescriptor.contract_name is null or empty",
            ),
        };
    }

    // SAFETY: desc.contract_name.ptr is non-null, valid UTF-8 for len bytes
    let contract_name: String = unsafe { desc.contract_name.to_string_owned() };

    // SAFETY: vtable is a valid 'static PluginInterface from the plugin binary
    match unsafe { registry.register(desc, vtable, contract_name, bundle_id) } {
        Ok(_handle) => polyplug_abi::AbiError::ok(),
        Err(e) => {
            eprintln!("[polyplug] registration failed for bundle {bundle_id}: {e}");
            polyplug_abi::AbiError {
                code: polyplug_abi::ABI_ERROR_GENERIC,
                message: polyplug_abi::StringView::null(),
            }
        }
    }
}

/// HostVTable.alloc callback — allocate memory via the host allocator.
///
/// # Safety
/// rt_ctx is ignored (system allocator is global). Standard alloc safety applies.
pub(crate) unsafe extern "C" fn host_alloc(
    _rt_ctx: *mut core::ffi::c_void,
    size: usize,
    align: usize,
) -> *mut u8 {
    polyplug_abi::ffi::polyplug_host_alloc(size, align)
}

/// HostVTable.free callback — free memory via the host allocator.
///
/// # Safety
/// rt_ctx is ignored (system allocator is global). Standard free safety applies.
pub(crate) unsafe extern "C" fn host_free(
    _rt_ctx: *mut core::ffi::c_void,
    ptr: *mut u8,
    size: usize,
    align: usize,
) {
    // SAFETY: polyplug_host_free is a safe wrapper around the system allocator.
    unsafe { polyplug_abi::ffi::polyplug_host_free(ptr, size, align) }
}

/// HostVTable.find_by_contract callback — dispatches to runtime's registry with dependency enforcement.
///
/// # Safety
/// rt_ctx must be a valid pointer to a HostContext.
pub(crate) unsafe extern "C" fn host_find_by_contract(
    rt_ctx: *mut core::ffi::c_void,
    contract_id: u64,
    min_version: u32,
) -> PluginHandle {
    if rt_ctx.is_null() {
        return PluginHandle::null();
    }
    // SAFETY: rt_ctx is a valid *mut HostContext passed by the host
    let ctx: &HostContext = unsafe { &*(rt_ctx as *const HostContext) };
    // SAFETY: ctx.runtime is a valid pointer to a Runtime that is guaranteed to be live
    // during the plugin init call.
    // SAFETY: ctx.runtime is a valid pointer to a Runtime that is guaranteed to be live
    // during the plugin init call.
    let runtime: &Runtime = unsafe { &*ctx.runtime };
    let registry: &Registry = &runtime.registry;
    let caller_bundle_id: u64 = ctx.bundle_id;

    if caller_bundle_id != 0 && !registry.is_dependency_declared(caller_bundle_id, contract_id) {
        return PluginHandle::null();
    }
    match registry.find_by_contract(contract_id, min_version) {
        Ok(h) => h,
        Err(_) => PluginHandle::null(),
    }
}

/// HostVTable.find_by_bundle callback — dispatches to runtime's registry with dependency enforcement.
///
/// # Safety
/// rt_ctx must be a valid pointer to a HostContext.
pub(crate) unsafe extern "C" fn host_find_by_bundle(
    rt_ctx: *mut core::ffi::c_void,
    bundle_id: u64,
    contract_id: u64,
    min_version: u32,
) -> PluginHandle {
    if rt_ctx.is_null() {
        return PluginHandle::null();
    }
    // SAFETY: rt_ctx is a valid *mut HostContext passed by the host
    let ctx: &HostContext = unsafe { &*(rt_ctx as *const HostContext) };
    // SAFETY: ctx.runtime is a valid pointer to a Runtime that is guaranteed to be live
    // during the plugin init call.
    let runtime: &Runtime = unsafe { &*ctx.runtime };
    let registry: &Registry = &runtime.registry;
    let caller_bundle_id: u64 = ctx.bundle_id;

    if caller_bundle_id != 0 && !registry.is_dependency_declared(caller_bundle_id, contract_id) {
        return PluginHandle::null();
    }
    match registry.find_by_bundle(bundle_id, contract_id, min_version) {
        Ok(h) => h,
        Err(_) => PluginHandle::null(),
    }
}

/// HostVTable.find_all_by_contract callback — fills out buffer, NO dependency enforcement.
///
/// # Safety
/// - rt_ctx must be a valid pointer to a HostContext
/// - out must point to a valid buffer of at least out_cap PluginHandle elements
pub(crate) unsafe extern "C" fn host_find_all_by_contract(
    rt_ctx: *mut core::ffi::c_void,
    contract_id: u64,
    min_version: u32,
    out: *mut PluginHandle,
    out_cap: usize,
) -> usize {
    if rt_ctx.is_null() {
        return 0usize;
    }
    // SAFETY: rt_ctx is a valid *mut HostContext passed by the host
    let ctx: &HostContext = unsafe { &*(rt_ctx as *const HostContext) };
    // SAFETY: ctx.runtime is a valid pointer to a Runtime that is guaranteed to be live
    // during the plugin init call.
    // SAFETY: ctx.runtime is a valid pointer to a Runtime that is guaranteed to be live
    // during the plugin init call.
    let runtime: &Runtime = unsafe { &*ctx.runtime };
    let registry: &Registry = &runtime.registry;

    if out_cap == 0usize {
        return 0usize;
    }
    // SAFETY: out is valid for out_cap PluginHandle elements per ABI contract
    let out_slice: &mut [PluginHandle] = unsafe { core::slice::from_raw_parts_mut(out, out_cap) };
    registry.find_all_by_contract(contract_id, min_version, out_slice)
}

/// HostVTable.resolve_plugin callback — returns raw vtable pointer for a handle.
///
/// # Safety
/// rt_ctx must be a valid pointer to a HostContext.
pub(crate) unsafe extern "C" fn host_resolve_plugin(
    rt_ctx: *mut core::ffi::c_void,
    handle: PluginHandle,
) -> *const PluginInterface {
    if rt_ctx.is_null() {
        return core::ptr::null();
    }
    // SAFETY: rt_ctx is a valid *mut HostContext passed by the host
    let ctx: &HostContext = unsafe { &*(rt_ctx as *const HostContext) };
    // SAFETY: ctx.runtime is a valid pointer to a Runtime that is guaranteed to be live
    // during the plugin init call.
    let runtime: &Runtime = unsafe { &*ctx.runtime };
    let registry: &Registry = &runtime.registry;

    match registry.resolve(handle) {
        Ok(ptr) => ptr,
        Err(_) => core::ptr::null(),
    }
}

/// HostVTable.get_host_contract callback — returns the vtable for a host contract.
///
/// # Safety
/// rt_ctx must be a valid pointer to a HostContext.
pub(crate) unsafe extern "C" fn host_get_host_contract(
    rt_ctx: *mut core::ffi::c_void,
    contract_id: u64,
    min_version: u32,
) -> *const polyplug_abi::HostContractVTable {
    if rt_ctx.is_null() {
        return core::ptr::null();
    }
    // SAFETY: rt_ctx is a valid *mut HostContext passed by the host
    let ctx: &HostContext = unsafe { &*(rt_ctx as *const HostContext) };
    // SAFETY: ctx.runtime is a valid pointer to a Runtime that is guaranteed to be live
    let runtime: &Runtime = unsafe { &*ctx.runtime };

    match runtime.get_host_contract(contract_id, min_version) {
        Some(vtable) => vtable as *const HostContractVTable,
        None => core::ptr::null(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;

    #[test]
    fn builder_creates_runtime() {
        let runtime: Runtime = Runtime::builder()
            .build()
            .expect("runtime build should succeed");
        let result: Result<PluginHandle, _> =
            runtime.find_by_contract(0x1234_5678_9ABC_DEF0_u64, 0);
        assert!(result.is_err(), "empty registry should return not found");
    }

    #[test]
    fn abi_ok_constant() {
        assert_eq!(polyplug_abi::ABI_OK, 0_u32);
    }

    #[test]
    fn host_find_by_contract_null_rt_ctx_returns_null() {
        // SAFETY: host_find_by_contract handles null rt_ctx gracefully
        let handle: PluginHandle =
            unsafe { host_find_by_contract(core::ptr::null_mut(), 0_u64, 0_u32) };
        assert!(
            handle.is_null(),
            "host_find_by_contract must return null when rt_ctx is null"
        );
    }

    #[test]
    fn dep_enforcement_blocks_undeclared_contract() {
        let runtime: Runtime = Runtime::builder()
            .build()
            .expect("runtime build should succeed");

        // Create a HostContext with a non-zero bundle_id to simulate init phase
        let host_ctx: HostContext = HostContext {
            runtime: &runtime as *const Runtime as *mut Runtime,
            bundle_id: 0xDEAD_BEEF_u64,
        };
        let rt_ptr: *mut core::ffi::c_void =
            &host_ctx as *const HostContext as *mut core::ffi::c_void;

        // SAFETY: rt_ptr is a valid HostContext pointer
        let handle: PluginHandle =
            unsafe { host_find_by_contract(rt_ptr, 0x1111_2222_3333_4444_u64, 0_u32) };
        assert!(
            handle.is_null(),
            "dep enforcement must return null for undeclared contract during init phase"
        );
    }

    fn create_bundle_dir(temp: &tempfile::TempDir, bundle_name: &str, runtime: &str) -> PathBuf {
        let bundle_dir: PathBuf = temp.path().join(bundle_name);
        if let Err(e) = std::fs::create_dir_all(&bundle_dir) {
            panic!("failed to create bundle dir {}: {e}", bundle_dir.display());
        }
        let so_path: PathBuf = bundle_dir.join("dummy.so");
        if let Err(e) = std::fs::write(&so_path, b"") {
            panic!("failed to write dummy so {}: {e}", so_path.display());
        }
        let manifest: String = format!(
            "id = 12345\nname = \"{}\"\nruntime = \"{}\"\nfile = \"dummy.so\"\n",
            bundle_name, runtime
        );
        let manifest_path: PathBuf = bundle_dir.join("manifest.toml");
        if let Err(e) = std::fs::write(&manifest_path, manifest) {
            panic!("failed to write manifest {}: {e}", manifest_path.display());
        }
        bundle_dir
    }

    fn register_contract(
        registry: &crate::registry::Registry,
        contract_id: u64,
        bundle_id: u64,
    ) -> PluginHandle {
        use polyplug_abi::{DispatchType, NativeDispatch, PluginDispatch, PluginInterface};
        let vtable: &'static PluginInterface = Box::leak(Box::new(PluginInterface {
            rt_ctx: core::ptr::null(),
            contract_id,
            contract_version: 0_u32,
            function_count: 0_u32,
            dispatch_type: DispatchType::Native,
            dispatch: PluginDispatch {
                native: NativeDispatch {
                    functions: core::ptr::null(),
                },
            },
        }));
        let descriptor: polyplug_abi::PluginDescriptor = polyplug_abi::PluginDescriptor {
            name: polyplug_abi::StringView::from_static(b"stub"),
            contract_name: polyplug_abi::StringView::from_static(b"stub.contract"),
            version_major: 1_u32,
            version_minor: 0_u32,
            version_patch: 0_u32,
        };
        // SAFETY: vtable is leaked and lives for the process lifetime.
        let result: Result<PluginHandle, crate::error::RegistryError> =
            unsafe { registry.register(descriptor, vtable, "stub.contract".to_owned(), bundle_id) };
        match result {
            Ok(handle) => handle,
            Err(e) => panic!("failed to register contract: {e}"),
        }
    }

    struct EnforceLoader {
        contract_id: u64,
        error_bundle_id: u64,
    }

    impl crate::loader::BundleLoader for EnforceLoader {
        fn runtime_name(&self) -> &'static str {
            "enforce"
        }

        fn load(
            &self,
            _manifest: &crate::loader::manifest::ManifestData,
            _runtime: &Runtime,
        ) -> Result<(), crate::error::PolyplugError> {
            Err(RuntimeError::UndeclaredDependency {
                bundle_id: self.error_bundle_id,
                contract_id: self.contract_id,
            })
        }
    }

    struct ProbeLoader {
        observed_init: Arc<std::sync::Mutex<Option<bool>>>,
    }

    impl crate::loader::BundleLoader for ProbeLoader {
        fn runtime_name(&self) -> &'static str {
            "probe"
        }

        fn load(
            &self,
            _manifest: &crate::loader::manifest::ManifestData,
            _runtime: &Runtime,
        ) -> Result<(), crate::error::PolyplugError> {
            let mut guard: std::sync::MutexGuard<'_, Option<bool>> = match self.observed_init.lock()
            {
                Ok(g) => g,
                Err(e) => e.into_inner(),
            };
            *guard = Some(true);
            Ok(())
        }
    }

    struct PanicLoader;

    impl crate::loader::BundleLoader for PanicLoader {
        fn runtime_name(&self) -> &'static str {
            "panic"
        }

        fn load(
            &self,
            _manifest: &crate::loader::manifest::ManifestData,
            _runtime: &Runtime,
        ) -> Result<(), crate::error::PolyplugError> {
            panic!("intentional panic in PanicLoader");
        }
    }

    struct ReentrantState {
        runtime_ptr: usize,
        inner_bundle: PathBuf,
        inner_load_completed: Option<bool>,
    }

    struct ReentrantLoader {
        state: Arc<std::sync::Mutex<ReentrantState>>,
    }

    impl crate::loader::BundleLoader for ReentrantLoader {
        fn runtime_name(&self) -> &'static str {
            "reentrant"
        }

        fn load(
            &self,
            _manifest: &crate::loader::manifest::ManifestData,
            _runtime: &Runtime,
        ) -> Result<(), crate::error::PolyplugError> {
            let state: std::sync::MutexGuard<'_, ReentrantState> = match self.state.lock() {
                Ok(g) => g,
                Err(e) => e.into_inner(),
            };
            let runtime_ptr: usize = state.runtime_ptr;
            if runtime_ptr == 0 {
                return Err(RuntimeError::Loader(
                    crate::error::LoaderError::InitFailed {
                        bundle: "reentrant".to_owned(),
                        error: "runtime pointer not initialized".to_owned(),
                    },
                ));
            }
            let inner_bundle: PathBuf = state.inner_bundle.clone();
            let already_set: bool = state.inner_load_completed.is_some();
            drop(state);
            // SAFETY: runtime_ptr was set from a valid &Runtime during load_bundle.
            let runtime_ref: &Runtime = unsafe { &*(runtime_ptr as *const Runtime) };
            let inner_result: Result<(), crate::error::PolyplugError> = runtime_ref
                .load_bundle_with(
                    inner_bundle.as_path(),
                    LoadOptions {
                        compatibility: crate::version::Compatibility::default(),
                        ignore_function_count_mismatch: false,
                    },
                );
            inner_result?;
            let mut st2: std::sync::MutexGuard<'_, ReentrantState> = match self.state.lock() {
                Ok(g) => g,
                Err(e) => e.into_inner(),
            };
            if !already_set {
                st2.inner_load_completed = Some(true);
            }
            Ok(())
        }
    }

    struct LazyState {
        observed_init: Option<bool>,
    }

    struct LazyLoader {
        state: Arc<std::sync::Mutex<LazyState>>,
    }

    impl crate::loader::BundleLoader for LazyLoader {
        fn runtime_name(&self) -> &'static str {
            "lazy"
        }

        fn load(
            &self,
            _manifest: &crate::loader::manifest::ManifestData,
            _runtime: &Runtime,
        ) -> Result<(), crate::error::PolyplugError> {
            let mut state: std::sync::MutexGuard<'_, LazyState> = match self.state.lock() {
                Ok(g) => g,
                Err(e) => e.into_inner(),
            };
            if state.observed_init.is_none() {
                state.observed_init = Some(true);
            }
            Ok(())
        }
    }

    #[test]
    fn bundle_id_zero_escape_returns_undeclared_dependency_error() {
        let temp: tempfile::TempDir = match tempfile::TempDir::new() {
            Ok(t) => t,
            Err(e) => panic!("failed to create temp dir: {e}"),
        };
        let contract: u64 = polyplug_abi::contract_id("trust.test", 1_u32);
        let bundle_name: &str = "enforce_bundle";
        let bundle_path: PathBuf = create_bundle_dir(&temp, bundle_name, "enforce");
        let runtime: Runtime = match Runtime::builder()
            .loader(EnforceLoader {
                contract_id: contract,
                error_bundle_id: 0_u64,
            })
            .build()
        {
            Ok(rt) => rt,
            Err(e) => panic!("failed to build runtime: {e}"),
        };
        let registry: &Arc<Registry> = runtime.registry();
        let _handle: PluginHandle = register_contract(registry.as_ref(), contract, 0xBEEF_u64);
        let result: Result<(), crate::error::PolyplugError> =
            runtime.load_bundle(bundle_path.as_path());
        match result {
            Err(PolyplugError::UndeclaredDependency {
                bundle_id,
                contract_id,
            }) => {
                assert_eq!(bundle_id, 0_u64);
                assert_eq!(contract_id, contract);
            }
            Err(other) => panic!("unexpected error: {other}"),
            Ok(()) => panic!("expected undeclared dependency error"),
        }
    }

    #[test]
    fn tls_state_cleared_after_init_completes() {
        let temp: tempfile::TempDir = match tempfile::TempDir::new() {
            Ok(t) => t,
            Err(e) => panic!("failed to create temp dir: {e}"),
        };
        let contract: u64 = polyplug_abi::contract_id("trust.tls", 1_u32);
        let observed: Arc<std::sync::Mutex<Option<bool>>> = Arc::new(std::sync::Mutex::new(None));
        let bundle_path: PathBuf = create_bundle_dir(&temp, "probe_bundle", "probe");
        let runtime: Runtime = match Runtime::builder()
            .loader(ProbeLoader {
                observed_init: Arc::clone(&observed),
            })
            .build()
        {
            Ok(rt) => rt,
            Err(e) => panic!("failed to build runtime: {e}"),
        };
        let registry: &Arc<Registry> = runtime.registry();
        let _handle: PluginHandle = register_contract(registry.as_ref(), contract, 0xCAFE_u64);
        let result: Result<(), crate::error::PolyplugError> =
            runtime.load_bundle(bundle_path.as_path());
        if let Err(e) = result {
            panic!("load_bundle failed: {e}");
        }
        let observed_value: Option<bool> = match observed.lock() {
            Ok(g) => *g,
            Err(e) => *e.into_inner(),
        };
        assert_eq!(
            observed_value,
            Some(true),
            "loader should have been called during init"
        );
        let handle_after: Result<PluginHandle, _> = runtime.find_by_contract(contract, 0_u32);
        assert!(
            handle_after.is_ok(),
            "after init, find_by_contract should succeed"
        );
    }

    #[test]
    fn panic_during_init_is_caught() {
        let temp: tempfile::TempDir = match tempfile::TempDir::new() {
            Ok(t) => t,
            Err(e) => panic!("failed to create temp dir: {e}"),
        };
        let _bundle_root: PathBuf = create_bundle_dir(&temp, "panic_bundle", "panic");
        let plugin_dir: PathBuf = temp.path().to_path_buf();
        let result = std::panic::catch_unwind(|| {
            let _rt: Runtime = Runtime::builder()
                .plugin_dir(plugin_dir)
                .loader(PanicLoader)
                .build()
                .unwrap_or_else(|e| panic!("runtime build failed: {e}"));
        });
        if result.is_ok() {
            panic!("expected panic from PanicLoader");
        }
    }

    #[test]
    fn reentrant_load_on_same_thread_works() {
        let temp: tempfile::TempDir = match tempfile::TempDir::new() {
            Ok(t) => t,
            Err(e) => panic!("failed to create temp dir: {e}"),
        };
        let contract: u64 = polyplug_abi::contract_id("trust.reentrant", 1_u32);
        let outer_bundle: PathBuf = create_bundle_dir(&temp, "outer_bundle", "reentrant");
        let inner_bundle: PathBuf = create_bundle_dir(&temp, "inner_bundle", "probe");
        let state: Arc<std::sync::Mutex<ReentrantState>> =
            Arc::new(std::sync::Mutex::new(ReentrantState {
                runtime_ptr: 0,
                inner_bundle: inner_bundle.clone(),
                inner_load_completed: None,
            }));
        let runtime: Runtime = match Runtime::builder()
            .loader(ReentrantLoader {
                state: Arc::clone(&state),
            })
            .loader(ProbeLoader {
                observed_init: Arc::new(std::sync::Mutex::new(None)),
            })
            .build()
        {
            Ok(rt) => rt,
            Err(e) => panic!("failed to build runtime: {e}"),
        };
        let registry: &Arc<Registry> = runtime.registry();
        let _handle: PluginHandle = register_contract(registry.as_ref(), contract, 0xABCD_u64);
        {
            let mut guard: std::sync::MutexGuard<'_, ReentrantState> = match state.lock() {
                Ok(g) => g,
                Err(e) => e.into_inner(),
            };
            guard.runtime_ptr = &runtime as *const Runtime as usize;
        }
        let result: Result<(), crate::error::PolyplugError> = runtime.load_bundle_with(
            outer_bundle.as_path(),
            LoadOptions {
                compatibility: crate::version::Compatibility::default(),
                ignore_function_count_mismatch: false,
            },
        );
        if let Err(e) = result {
            panic!("outer load failed: {e}");
        }
        let inner_completed: Option<bool> = match state.lock() {
            Ok(g) => g.inner_load_completed,
            Err(e) => e.into_inner().inner_load_completed,
        };
        assert_eq!(
            inner_completed,
            Some(true),
            "inner load should have completed successfully"
        );
        let _ = inner_bundle;
    }

    #[test]
    fn lazy_load_during_init_works() {
        let temp: tempfile::TempDir = match tempfile::TempDir::new() {
            Ok(t) => t,
            Err(e) => panic!("failed to create temp dir: {e}"),
        };
        let contract: u64 = polyplug_abi::contract_id("trust.lazy", 1_u32);
        let outer_bundle: PathBuf = create_bundle_dir(&temp, "lazy_outer", "lazy");
        let inner_bundle: PathBuf = create_bundle_dir(&temp, "lazy_inner", "probe");
        let state: Arc<std::sync::Mutex<LazyState>> = Arc::new(std::sync::Mutex::new(LazyState {
            observed_init: None,
        }));
        let runtime: Runtime = match Runtime::builder()
            .loader(LazyLoader {
                state: Arc::clone(&state),
            })
            .loader(ProbeLoader {
                observed_init: Arc::new(std::sync::Mutex::new(None)),
            })
            .build()
        {
            Ok(rt) => rt,
            Err(e) => panic!("failed to build runtime: {e}"),
        };
        let registry: &Arc<Registry> = runtime.registry();
        let _handle: PluginHandle = register_contract(registry.as_ref(), contract, 0xFACE_u64);
        let result: Result<(), crate::error::PolyplugError> =
            runtime.load_bundle(outer_bundle.as_path());
        if let Err(e) = result {
            panic!("outer load failed: {e}");
        }
        let observed_init: Option<bool> = match state.lock() {
            Ok(g) => g.observed_init,
            Err(e) => e.into_inner().observed_init,
        };
        assert_eq!(
            observed_init,
            Some(true),
            "init should have been observed during lazy loader init"
        );
        let inner_result: Result<(), crate::error::PolyplugError> = runtime.load_bundle_with(
            inner_bundle.as_path(),
            LoadOptions {
                compatibility: crate::version::Compatibility::default(),
                ignore_function_count_mismatch: false,
            },
        );
        if let Err(e) = inner_result {
            panic!("lazy inner load failed: {e}");
        }
    }

    // --- Host Contract Tests ---

    fn create_host_contract_vtable(
        contract_id: u64,
        major: u32,
        minor: u32,
    ) -> &'static HostContractVTable {
        Box::leak(Box::new(HostContractVTable {
            header: polyplug_abi::HostContractVTableHeader {
                vtable_version: 1,
                contract_id,
                contract_major: major,
                contract_minor: minor,
                function_count: 1,
                dispatch_type: polyplug_abi::DispatchType::Native,
            },
            dispatch: polyplug_abi::HostContractDispatch {
                native: polyplug_abi::NativeHostContractDispatch {
                    impl_ptr: core::ptr::null(),
                    functions: core::ptr::null(),
                },
            },
        }))
    }

    #[test]
    fn runtime_host_contracts_register_and_lookup() {
        let runtime: Runtime = Runtime::builder()
            .build()
            .expect("runtime build should succeed");

        let contract_id: u64 = polyplug_abi::host_contract_id("host.logger", 1);
        let vtable: &'static HostContractVTable = create_host_contract_vtable(contract_id, 1, 0);

        let result: Result<(), HostContractError> =
            runtime.register_host_contract(contract_id, vtable);
        assert!(result.is_ok(), "registration should succeed");

        let found: Option<&'static HostContractVTable> = runtime.get_host_contract(contract_id, 0);
        assert!(found.is_some(), "contract should be found");
        let found_vtable: &HostContractVTable =
            found.expect("contract should be present after is_some check");
        assert_eq!(found_vtable.header.contract_id, contract_id);
    }

    #[test]
    fn runtime_host_contracts_duplicate_registration_fails() {
        let runtime: Runtime = Runtime::builder()
            .build()
            .expect("runtime build should succeed");

        let contract_id: u64 = polyplug_abi::host_contract_id("host.logger", 1);
        let vtable1: &'static HostContractVTable = create_host_contract_vtable(contract_id, 1, 0);
        let vtable2: &'static HostContractVTable = create_host_contract_vtable(contract_id, 1, 1);

        let result1: Result<(), HostContractError> =
            runtime.register_host_contract(contract_id, vtable1);
        assert!(result1.is_ok(), "first registration should succeed");

        let result2: Result<(), HostContractError> =
            runtime.register_host_contract(contract_id, vtable2);
        assert!(result2.is_err(), "duplicate registration should fail");
        match result2 {
            Err(HostContractError::DuplicateContract { contract_id: id }) => {
                assert_eq!(id, contract_id);
            }
            Err(other) => panic!("unexpected error: {other}"),
            Ok(()) => panic!("expected error"),
        }
    }

    #[test]
    fn runtime_host_contracts_unregister() {
        let runtime: Runtime = Runtime::builder()
            .build()
            .expect("runtime build should succeed");

        let contract_id: u64 = polyplug_abi::host_contract_id("host.logger", 1);
        let vtable: &'static HostContractVTable = create_host_contract_vtable(contract_id, 1, 0);

        runtime
            .register_host_contract(contract_id, vtable)
            .expect("registration should succeed");

        let removed: bool = runtime.unregister_host_contract(contract_id);
        assert!(
            removed,
            "unregister should return true for existing contract"
        );

        let removed_again: bool = runtime.unregister_host_contract(contract_id);
        assert!(
            !removed_again,
            "unregister should return false for non-existent contract"
        );

        let found: Option<&'static HostContractVTable> = runtime.get_host_contract(contract_id, 0);
        assert!(
            found.is_none(),
            "contract should not be found after unregister"
        );
    }

    #[test]
    fn runtime_host_contracts_version_check() {
        let runtime: Runtime = Runtime::builder()
            .build()
            .expect("runtime build should succeed");

        let contract_id: u64 = polyplug_abi::host_contract_id("host.logger", 2);
        let vtable: &'static HostContractVTable = create_host_contract_vtable(contract_id, 2, 5);

        runtime
            .register_host_contract(contract_id, vtable)
            .expect("registration should succeed");

        let found_low: Option<&'static HostContractVTable> =
            runtime.get_host_contract(contract_id, 0);
        assert!(found_low.is_some(), "should find with min_version=0");

        let found_exact: Option<&'static HostContractVTable> =
            runtime.get_host_contract(contract_id, (2 << 16) | 5);
        assert!(found_exact.is_some(), "should find with exact version");

        let found_higher_minor: Option<&'static HostContractVTable> =
            runtime.get_host_contract(contract_id, (2 << 16) | 3);
        assert!(
            found_higher_minor.is_some(),
            "should find with lower minor version requirement"
        );

        let found_higher_major: Option<&'static HostContractVTable> =
            runtime.get_host_contract(contract_id, 3 << 16);
        assert!(
            found_higher_major.is_none(),
            "should not find with higher major version requirement"
        );
    }

    #[test]
    fn runtime_host_runtime_default_is_rust() {
        let runtime: Runtime = Runtime::builder()
            .build()
            .expect("runtime build should succeed");
        assert_eq!(runtime.host_runtime(), HostRuntime::Rust);
    }

    #[test]
    fn runtime_host_runtime_can_be_set() {
        let runtime: Runtime = Runtime::builder()
            .host_runtime(HostRuntime::Python)
            .build()
            .expect("runtime build should succeed");
        assert_eq!(runtime.host_runtime(), HostRuntime::Python);
    }

    #[test]
    fn host_get_host_contract_callback_returns_registered_contract() {
        let runtime: Runtime = Runtime::builder()
            .build()
            .expect("runtime build should succeed");

        let contract_id: u64 = polyplug_abi::host_contract_id("host.test", 1);
        let vtable: &'static HostContractVTable = create_host_contract_vtable(contract_id, 1, 0);

        runtime
            .register_host_contract(contract_id, vtable)
            .expect("registration should succeed");

        let host_ctx: HostContext = HostContext {
            runtime: &runtime as *const Runtime as *mut Runtime,
            bundle_id: 0,
        };
        let rt_ptr: *mut core::ffi::c_void =
            &host_ctx as *const HostContext as *mut core::ffi::c_void;

        // SAFETY: rt_ptr is a valid HostContext pointer, runtime is live
        let result: *const HostContractVTable =
            unsafe { host_get_host_contract(rt_ptr, contract_id, 0) };
        assert!(
            !result.is_null(),
            "callback should return non-null for registered contract"
        );

        // SAFETY: result is a valid HostContractVTable pointer
        let found_vtable: &HostContractVTable = unsafe { &*result };
        assert_eq!(found_vtable.header.contract_id, contract_id);
    }

    #[test]
    fn host_get_host_contract_callback_returns_null_for_unregistered() {
        let runtime: Runtime = Runtime::builder()
            .build()
            .expect("runtime build should succeed");

        let contract_id: u64 = polyplug_abi::host_contract_id("host.nonexistent", 1);

        let host_ctx: HostContext = HostContext {
            runtime: &runtime as *const Runtime as *mut Runtime,
            bundle_id: 0,
        };
        let rt_ptr: *mut core::ffi::c_void =
            &host_ctx as *const HostContext as *mut core::ffi::c_void;

        // SAFETY: rt_ptr is a valid HostContext pointer, runtime is live
        let result: *const HostContractVTable =
            unsafe { host_get_host_contract(rt_ptr, contract_id, 0) };
        assert!(
            result.is_null(),
            "callback should return null for unregistered contract"
        );
    }
}

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
use std::sync::OnceLock;

use crate::error::GraphError;
use crate::error::LoaderError;
use crate::error::PolyplugError;
use crate::error::RegistryError;
use crate::error::RuntimeError;
use crate::extensions::Extension;
use crate::extensions::SendPtr;
use crate::graph::CapabilityGraph;
use crate::loader::BundleInitGuard;
use crate::loader::BundleLoader;
use crate::loader::LoadedBundle;
use crate::loader::manifest::ManifestData;
use crate::registry::Registry;
use crate::version::Compatibility;
use crate::version::Version;
use polyplug_abi::HostVTable;
use polyplug_abi::PluginHandle;
use polyplug_abi::PluginRegistrar;
use polyplug_abi::PluginVTable;
use polyplug_abi::ffi::polyplug_host_alloc;
use polyplug_abi::ffi::polyplug_host_free;

#[cfg(feature = "hot-reload")]
use core::sync::atomic::Ordering;
#[cfg(feature = "hot-reload")]
use notify::Watcher;

// ─── Runtime Configuration ───────────────────────────────────────────────────

/// Configuration for the polyplug runtime.
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
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
            hot_reload_max_retries: 3,
            hot_reload_retry_interval: Duration::from_secs(1),
            hot_reload_abort_on_max_retries: true,
        }
    }
}

// ─── Global registry for cross-plugin dispatch ───────────────────────────────

static GLOBAL_REGISTRY: OnceLock<Arc<Registry>> = OnceLock::new();

/// Extension map: extension_id -> raw vtable pointer.
/// Set once during RuntimeBuilder::build(). Immutable after that.
static GLOBAL_EXTENSION_MAP: OnceLock<HashMap<u32, SendPtr>> = OnceLock::new();

/// Type alias for the warning callback to avoid repetition.
type WarningCb = Box<dyn Fn(&str) + Send + Sync>;

/// Type alias for the reload callback.
type ReloadCb = std::sync::Arc<dyn Fn(crate::reload::ReloadPhase) + Send + Sync>;

/// Global warning callback. Set once via `RuntimeBuilder::on_warning()`.
///
/// Only the first registered warning callback takes effect.
/// Subsequent registrations are silently ignored (OnceLock semantics).
/// Test binaries needing different callbacks must be separate test binaries.
static GLOBAL_WARNING_CB: OnceLock<WarningCb> = OnceLock::new();

/// Emit a warning through the registered callback, or fall back to stderr.
pub(crate) fn emit_warning(msg: &str) {
    match GLOBAL_WARNING_CB.get() {
        Some(cb) => cb(msg),
        None => eprintln!("[polyplug] warning: {msg}"),
    }
}

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
pub fn global_registry() -> Option<Arc<Registry>> {
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
    /// Extension impls. Never dropped — keeps vtable memory alive for the Runtime's lifetime.
    _extensions: Vec<Box<dyn Extension>>,
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
    /// Background watcher thread handle. Feature-gated. Joined on Drop.
    #[cfg(feature = "hot-reload")]
    watcher_thread: std::sync::Mutex<Option<std::thread::JoinHandle<()>>>,
    /// Stop flag sent to watcher thread. Feature-gated.
    #[cfg(feature = "hot-reload")]
    watcher_stop: std::sync::Mutex<Option<std::sync::Arc<core::sync::atomic::AtomicBool>>>,
    config: RuntimeConfig,
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
    extensions: Vec<Box<dyn Extension>>,
    compatibility: Compatibility,
    warning_cb: Option<WarningCb>,
    on_reload_cb: Option<ReloadCb>,
    config: RuntimeConfig,
}

impl RuntimeBuilder {
    /// Create a new RuntimeBuilder with default settings.
    pub fn new() -> RuntimeBuilder {
        RuntimeBuilder {
            plugin_dirs: Vec::new(),
            loaders: Vec::new(),
            extensions: Vec::new(),
            compatibility: Compatibility::default(),
            warning_cb: None,
            on_reload_cb: None,
            config: RuntimeConfig::default(),
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

    /// Register an extension. If two extensions share the same extension_id, the last one wins.
    ///
    /// Extensions provide optional host-side vtables queryable by plugins at init time.
    pub fn extension(mut self, ext: Box<dyn Extension>) -> RuntimeBuilder {
        self.extensions.push(ext);
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

    /// Build the runtime.
    //
    //  For MVP: scans plugin_dirs for .so/.dll/.dylib files,
    //  loads them in sorted order, registers vtables.
    //  Full capability graph resolution is a future enhancement.
    pub fn build(self) -> Result<Runtime, RuntimeError> {
        let registry: Arc<Registry> = Arc::new(Registry::new());

        // Wire the global dispatcher before leaking the HostVTable.
        set_global_registry(Arc::clone(&registry));

        // Install warning callback if provided (OnceLock::set returns Err when already set — expected).
        if let Some(cb) = self.warning_cb {
            let _: Result<(), WarningCb> = GLOBAL_WARNING_CB.set(cb);
        }

        // Build extension map: extension_id -> vtable pointer.
        // If GLOBAL_EXTENSION_MAP is already set (e.g., second build() call in tests), silently skip.
        let mut ext_map: HashMap<u32, SendPtr> = HashMap::new();
        for ext in &self.extensions {
            let id: u32 = ext.extension_id();
            let ptr: *const () = ext.vtable_ptr();
            ext_map.insert(id, SendPtr(ptr));
        }
        // OnceLock::set returns Err(value) when already set — expected after first build().
        let _: Result<(), HashMap<u32, SendPtr>> = GLOBAL_EXTENSION_MAP.set(ext_map);

        let bundles: Vec<LoadedBundle> = Vec::new();

        // Build the static HostVTable. This must be 'static.
        let host_vtable: &'static HostVTable = Box::leak(Box::new(HostVTable {
            alloc: polyplug_host_alloc,
            free: polyplug_host_free,
            find_by_contract: host_find_by_contract,
            find_by_bundle: host_find_by_bundle,
            find_all_by_contract: host_find_all_by_contract,
            resolve_plugin: host_resolve_plugin,
            get_extension: host_get_extension,
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
            manifests_map.insert(stored_manifest.bundle_name.clone(), stored_manifest);
        }
        // If nothing discovered, build Runtime with empty bundles (no graph needed)
        if !discovered.is_empty() {
            // Phase 2: Build capability graph
            let graph: CapabilityGraph = CapabilityGraph::from_manifests(&discovered)
                .map_err(|e: GraphError| RuntimeError::Graph(e))?;

            // Phase 2.5: Validate version compatibility
            validate_bundle_compatibility(&discovered, self.compatibility)?;

            // Phase 3: Get topological load order (providers first)
            let load_order: Vec<String> = graph
                .topological_order()
                .map_err(|e: GraphError| RuntimeError::Graph(e))?;

            // Phase 4: Build lookup map bundle_name -> (path, manifest)
            let mut bundle_map: HashMap<String, (PathBuf, ManifestData)> = HashMap::new();
            for entry in discovered {
                bundle_map.insert(entry.1.bundle_name.clone(), entry);
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

                let loader: &dyn BundleLoader = loader_map
                    .get(&manifest.runtime)
                    .map(Box::as_ref)
                    .ok_or_else(|| {
                        RuntimeError::Loader(LoaderError::NoLoaderForRuntime {
                            bundle: bundle_path.display().to_string(),
                            runtime_name: manifest.runtime.clone(),
                        })
                    })?;

                let (mut registrar, _guard): (PluginRegistrar, BundleInitGuard) =
                    crate::loader::make_registrar_context(
                        &registry,
                        manifest.bundle_id,
                        host_vtable,
                    );

                // For directory bundles, resolve the actual file path from manifest.file.
                // Loaders (Python, Lua, JS, dotnet) expect a direct file path — they
                // call file_stem(), read_to_string(), canonicalize(), etc. on the path.
                // The scanner passes the directory path for directory bundles; here we
                // join manifest.file to get the real file inside the bundle directory.
                let effective_path: PathBuf = if !manifest.file.is_empty() {
                    bundle_path.join(&manifest.file)
                } else {
                    bundle_path.clone()
                };

                loader.load(&effective_path, &mut registrar).map_err(
                    |e: PolyplugError| match e {
                        PolyplugError::Loader(le) => RuntimeError::Loader(le),
                        other => RuntimeError::Loader(LoaderError::InitFailed {
                            bundle: effective_path.display().to_string(),
                            error: other.to_string(),
                        }),
                    },
                )?;
            }
        }

        Ok(Runtime {
            registry,
            _bundles: bundles,
            host_vtable,
            loaders: loader_map,
            _extensions: self.extensions,
            bundle_manifests: std::sync::Mutex::new(manifests_map),
            reload_libraries: std::sync::Mutex::new(HashMap::new()),
            on_reload_cb: self.on_reload_cb,
            #[cfg(feature = "hot-reload")]
            watcher_thread: std::sync::Mutex::new(None),
            #[cfg(feature = "hot-reload")]
            watcher_stop: std::sync::Mutex::new(None),
            config: self.config,
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

    /// Resolve a plugin handle to a vtable pointer.
    #[inline(always)]
    pub fn resolve_plugin(
        &self,
        handle: PluginHandle,
    ) -> Result<*const PluginVTable, RegistryError> {
        self.registry.resolve(handle)
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

    #[inline(always)]
    pub(crate) fn host_vtable_ref(&self) -> &'static HostVTable {
        self.host_vtable
    }

    /// Get the runtime configuration.
    #[inline(always)]
    pub fn config(&self) -> &RuntimeConfig {
        &self.config
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
        // Require bundle path to be a directory
        if !path.is_dir() {
            return Err(PolyplugError::Loader(LoaderError::BundleNotADirectory {
                path: path.to_path_buf(),
            }));
        }
        let manifest_path: PathBuf = path.join("manifest.toml");
        if !manifest_path.exists() {
            return Err(PolyplugError::Loader(LoaderError::ManifestParse {
                path: manifest_path.display().to_string(),
                reason: "manifest.toml not found in bundle directory".to_owned(),
            }));
        }
        // Parse manifest and compute bundle_id
        let mut manifest: ManifestData = crate::loader::parse_manifest(path)
            .map_err(|e: LoaderError| PolyplugError::Loader(e))?;
        manifest.bundle_id = polyplug_abi::bundle_id(&manifest.bundle_name);
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
                        manifest.bundle_name, contract, key
                    );
                    if opts.compatibility == Compatibility::Strict {
                        return Err(PolyplugError::Loader(LoaderError::FunctionCountMismatch {
                            contract: contract.clone(),
                            // sentinel 0/0: entry is missing entirely; actual count is unknown without loading the .so
                            expected: 0,
                            found: 0,
                        }));
                    } else {
                        emit_warning(&msg);
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
        // Build registrar and dispatch — use make_registrar_context to set REGISTRAR_REGISTRY_PTR
        // and REGISTRAR_BUNDLE_ID thread-locals. This is required for non-native loaders
        // (Python, Lua, JS, dotnet) whose register_plugin callbacks depend on these TLS values.
        let (mut registrar, _guard): (PluginRegistrar, crate::loader::BundleInitGuard) =
            crate::loader::make_registrar_context(
                &self.registry,
                manifest.bundle_id,
                self.host_vtable,
            );
        let effective_path: PathBuf = if !manifest.file.is_empty() {
            path.join(&manifest.file)
        } else {
            path.to_path_buf()
        };
        let result: Result<(), PolyplugError> = loader.load(&effective_path, &mut registrar);
        if result.is_ok() {
            let bundle_name: String = manifest.bundle_name.clone();
            let mut manifests: std::sync::MutexGuard<'_, HashMap<String, ManifestData>> = self
                .bundle_manifests
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            manifests.insert(bundle_name, manifest);
        }
        result
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        #[cfg(feature = "hot-reload")]
        {
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
}

#[cfg(feature = "hot-reload")]
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
                    > = debounce_cb.lock().unwrap_or_else(|e| e.into_inner());
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
                                crate::runtime::emit_warning(&format!(
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
            .unwrap_or_else(|e| e.into_inner())
            .replace(stop_flag);
        self_arc
            .watcher_thread
            .lock()
            .unwrap_or_else(|e| e.into_inner())
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

            let required: Version = match Version::parse(dep_min_version_str, &manifest.bundle_name)
            {
                Ok(v) => v,
                Err(e) => return Err(RuntimeError::Loader(e)),
            };

            let provided: Version =
                parse_manifest_version(&provider.version, &provider.bundle_name)?;

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
                        emit_warning(&format!(
                            "version mismatch for contract `{}`: required={}, found={} (bundle `{}`)",
                            dep_contract, required, provided, provider.bundle_name
                        ));
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
                        emit_warning(&format!(
                            "bundle `{}` provides `{}` but has no function_count entry for key `{}`",
                            manifest.bundle_name, contract, key
                        ));
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
    if out_cap == 0usize {
        return 0usize;
    }
    // SAFETY: out is valid for out_cap PluginHandle elements per ABI contract.
    let out_slice: &mut [PluginHandle] = unsafe { core::slice::from_raw_parts_mut(out, out_cap) };
    registry.find_all_by_contract(contract_id, min_version, out_slice)
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

/// HostVTable.get_extension callback — returns the vtable pointer for a registered extension.
//
// SAFETY: GLOBAL_EXTENSION_MAP is initialized during RuntimeBuilder::build() and
// never mutated after that. Reading from OnceLock::get() is lock-free and safe
// from any thread. SendPtr wraps a *const () to a 'static extension vtable.
pub(crate) unsafe extern "C" fn host_get_extension(extension_id: u32) -> *const () {
    match GLOBAL_EXTENSION_MAP.get() {
        Some(map) => match map.get(&extension_id) {
            Some(ptr) => ptr.0,
            None => core::ptr::null(),
        },
        None => core::ptr::null(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::OnceLock;

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
        assert_eq!(polyplug_abi::ABI_OK, 0_u32);
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

    #[test]
    fn host_get_extension_returns_null_for_unknown_id() {
        // SAFETY: host_get_extension reads from OnceLock; no pointer preconditions.
        let ptr: *const () = unsafe { host_get_extension(0xDEAD_BEEF_u32) };
        assert!(ptr.is_null(), "unknown extension_id must return null");
    }

    // ── Tests f/g: dep enforcement with a registered plugin ─────────────────
    //
    // These use a module-level OnceLock to register the test plugin in the
    // global registry exactly once. Cargo may run unit tests in parallel within
    // a binary, so the OnceLock ensures idempotent setup.

    /// One-time setup: register a plugin for contract 0xF00D_CAFE in the global registry.
    fn ensure_test_plugin_registered() {
        static SETUP: OnceLock<()> = OnceLock::new();
        SETUP.get_or_init(|| {
            let vtable: &'static polyplug_abi::PluginVTable =
                Box::leak(Box::new(polyplug_abi::PluginVTable {
                    contract_id: 0xF00D_CAFE_0000_0001_u64,
                    contract_version: 0,
                    function_count: 0,
                    functions: core::ptr::null(),
                }));
            let desc: polyplug_abi::PluginDescriptor = polyplug_abi::PluginDescriptor {
                name: polyplug_abi::StringView::from_static(b"test-dep-plugin"),
                contract_name: polyplug_abi::StringView::from_static(b"test.dep.Contract"),
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
        let caller_bid: u64 = polyplug_abi::bundle_id("caller-bundle-f");
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
        let caller_bid: u64 = polyplug_abi::bundle_id("caller-bundle-g-no-deps");
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

    // ── Trust boundary tests (moved from integration_trust_boundary.rs) ─────

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
            "bundle_name = \"{}\"\nruntime = \"{}\"\nfile = \"dummy.so\"\n",
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
        let vtable: &'static PluginVTable = Box::leak(Box::new(PluginVTable {
            contract_id,
            contract_version: 0_u32,
            function_count: 0_u32,
            functions: core::ptr::null(),
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
            _path: &Path,
            _registrar: &mut polyplug_abi::PluginRegistrar,
        ) -> Result<(), crate::error::PolyplugError> {
            // SAFETY: host_find_by_contract takes plain integers only.
            let handle: PluginHandle = unsafe { host_find_by_contract(self.contract_id, 0_u32) };
            if handle.is_null() {
                return Err(RuntimeError::UndeclaredDependency {
                    bundle_id: self.error_bundle_id,
                    contract_id: self.contract_id,
                });
            }
            Ok(())
        }
    }

    struct ProbeLoader {
        contract_id: u64,
        observed_null: Arc<std::sync::Mutex<Option<bool>>>,
    }

    impl crate::loader::BundleLoader for ProbeLoader {
        fn runtime_name(&self) -> &'static str {
            "probe"
        }

        fn load(
            &self,
            _path: &Path,
            _registrar: &mut polyplug_abi::PluginRegistrar,
        ) -> Result<(), crate::error::PolyplugError> {
            // SAFETY: host_find_by_contract takes plain integers only.
            let handle: PluginHandle = unsafe { host_find_by_contract(self.contract_id, 0_u32) };
            let mut guard: std::sync::MutexGuard<'_, Option<bool>> = match self.observed_null.lock()
            {
                Ok(g) => g,
                Err(e) => e.into_inner(),
            };
            *guard = Some(handle.is_null());
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
            _path: &Path,
            _registrar: &mut polyplug_abi::PluginRegistrar,
        ) -> Result<(), crate::error::PolyplugError> {
            panic!("intentional panic in PanicLoader");
        }
    }

    struct ReentrantState {
        runtime_ptr: usize,
        inner_bundle: PathBuf,
        tls_after_inner_load: Option<u64>,
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
            _path: &Path,
            _registrar: &mut polyplug_abi::PluginRegistrar,
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
            let already_set: bool = state.tls_after_inner_load.is_some();
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
            let tls_val: u64 = INIT_BUNDLE_ID.with(|c| c.get());
            let mut st2: std::sync::MutexGuard<'_, ReentrantState> = match self.state.lock() {
                Ok(g) => g,
                Err(e) => e.into_inner(),
            };
            if !already_set {
                st2.tls_after_inner_load = Some(tls_val);
            }
            Ok(())
        }
    }

    struct LazyState {
        contract_id: u64,
        observed_null: Option<bool>,
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
            _path: &Path,
            _registrar: &mut polyplug_abi::PluginRegistrar,
        ) -> Result<(), crate::error::PolyplugError> {
            let mut state: std::sync::MutexGuard<'_, LazyState> = match self.state.lock() {
                Ok(g) => g,
                Err(e) => e.into_inner(),
            };
            // SAFETY: host_find_by_contract takes plain integers only.
            let handle: PluginHandle = unsafe { host_find_by_contract(state.contract_id, 0_u32) };
            if state.observed_null.is_none() {
                state.observed_null = Some(handle.is_null());
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
            Err(RuntimeError::UndeclaredDependency {
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
                contract_id: contract,
                observed_null: Arc::clone(&observed),
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
            "during init, dep enforcement must block undeclared lookup (observed_null=true)"
        );
        let handle_after: Result<PluginHandle, _> = runtime.find_by_contract(contract, 0_u32);
        assert!(
            handle_after.is_ok(),
            "after init, TLS must be cleared so find_by_contract succeeds without enforcement"
        );
    }

    #[test]
    fn panic_during_init_triggers_guard_drop() {
        let temp: tempfile::TempDir = match tempfile::TempDir::new() {
            Ok(t) => t,
            Err(e) => panic!("failed to create temp dir: {e}"),
        };
        let contract: u64 = polyplug_abi::contract_id("trust.panic", 1_u32);
        // Use existing global registry if set, otherwise create and set a new one.
        // This handles test isolation when multiple tests share the same binary.
        let registry: Arc<Registry> = match global_registry() {
            Some(r) => r,
            None => {
                let r: Arc<Registry> = Arc::new(Registry::new());
                set_global_registry(Arc::clone(&r));
                r
            }
        };
        let _handle: PluginHandle = register_contract(registry.as_ref(), contract, 0xD00D_u64);
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
        // SAFETY: host_find_by_contract takes plain integers only.
        let handle_after: PluginHandle = unsafe { host_find_by_contract(contract, 0_u32) };
        assert!(
            !handle_after.is_null(),
            "BundleInitGuard should clear TLS even when load panics"
        );
    }

    #[test]
    fn reentrant_load_on_same_thread_does_not_leak_bundle_id() {
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
                tls_after_inner_load: None,
            }));
        let runtime: Runtime = match Runtime::builder()
            .loader(ReentrantLoader {
                state: Arc::clone(&state),
            })
            .loader(ProbeLoader {
                contract_id: contract,
                observed_null: Arc::new(std::sync::Mutex::new(None)),
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
        let tls_captured: Option<u64> = match state.lock() {
            Ok(g) => g.tls_after_inner_load,
            Err(e) => e.into_inner().tls_after_inner_load,
        };
        assert_eq!(
            tls_captured,
            Some(0_u64),
            "after inner guard drops, INIT_BUNDLE_ID must be 0"
        );
        let _ = inner_bundle;
    }

    #[test]
    fn lazy_load_during_init_does_not_corrupt_tls() {
        let temp: tempfile::TempDir = match tempfile::TempDir::new() {
            Ok(t) => t,
            Err(e) => panic!("failed to create temp dir: {e}"),
        };
        let contract: u64 = polyplug_abi::contract_id("trust.lazy", 1_u32);
        let outer_bundle: PathBuf = create_bundle_dir(&temp, "lazy_outer", "lazy");
        let inner_bundle: PathBuf = create_bundle_dir(&temp, "lazy_inner", "probe");
        let state: Arc<std::sync::Mutex<LazyState>> = Arc::new(std::sync::Mutex::new(LazyState {
            contract_id: contract,
            observed_null: None,
        }));
        let runtime: Runtime = match Runtime::builder()
            .loader(LazyLoader {
                state: Arc::clone(&state),
            })
            .loader(ProbeLoader {
                contract_id: contract,
                observed_null: Arc::new(std::sync::Mutex::new(None)),
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
            Ok(g) => g.observed_null,
            Err(e) => e.into_inner().observed_null,
        };
        assert_eq!(
            observed_init,
            Some(true),
            "init should observe enforcement during lazy loader init"
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
        let init_id_after: u64 = INIT_BUNDLE_ID.with(|c| c.get());
        assert_eq!(
            init_id_after, 0_u64,
            "lazy load must not corrupt TLS: INIT_BUNDLE_ID must be 0 after all loads"
        );
    }
}

use core::ffi::c_void;
use std::{collections::HashMap, path::PathBuf, sync::Arc};

use polyplug_abi::runtime::{Compatibility, RuntimeConfig};
use polyplug_abi::types::{LogLevel, StringView};
use polyplug_abi::{HostApi, RuntimeLanguage};

use crate::{
    compatibility::CapabilityGraph,
    error::{GraphError, LoaderError, RuntimeError},
    loader::{BundleLoader, ManifestData},
    logger::{LoggerClosure, LoggerHandle},
    runtime::{ReloadCallback, Runtime},
    runtime_store::RuntimeStore,
};

/// `RuntimeConfig::log` trampoline that forwards to the boxed Rust closure
/// installed via [`RuntimeBuilder::logger`].
///
/// # Safety
/// `user_data` must point to the [`LoggerClosure`] owned by the `Runtime`
/// (kept alive for the runtime's lifetime); `scope` and `message` must be
/// valid UTF-8 views for the duration of the call — both are guaranteed by
/// the runtime's logger plumbing, the only producer of these calls.
unsafe extern "C" fn rust_logger_trampoline(
    user_data: *mut c_void,
    level: u32,
    scope: StringView,
    message: StringView,
) {
    if user_data.is_null() {
        return;
    }
    // SAFETY: user_data points to the runtime-owned LoggerClosure (see function
    // docs); the box lives for the runtime's lifetime, which covers every log call.
    let callback: &LoggerClosure = unsafe { &*(user_data as *const LoggerClosure) };
    // Unknown level values cannot occur from the runtime's own logger, but the
    // conversion stays total: collapse anything unexpected to Error.
    let level: LogLevel = match LogLevel::from_u32(level) {
        Some(l) => l,
        None => LogLevel::Error,
    };
    // SAFETY: the runtime's LoggerHandle built both views from live, UTF-8 Rust
    // string data that outlives this call (documented callback contract).
    let (scope_str, message_str): (&str, &str) = unsafe { (scope.as_str(), message.as_str()) };
    callback.0.emit(level, scope_str, message_str);
}

/// Builder for constructing a Runtime.
pub struct RuntimeBuilder {
    plugin_dirs: Vec<PathBuf>,
    loaders: Vec<Box<dyn BundleLoader>>,
    compatibility: Compatibility,
    /// Boxed Rust logger closure (boxed for a thin, stable `user_data`
    /// pointer); ownership moves into the Runtime so it outlives every log call.
    logger_closure: Option<Box<LoggerClosure>>,
    on_reload_cb: Option<ReloadCallback>,
    config: RuntimeConfig,
    host_runtime: RuntimeLanguage,
}

impl RuntimeBuilder {
    /// Create a new RuntimeBuilder with default settings.
    pub fn new() -> RuntimeBuilder {
        RuntimeBuilder {
            plugin_dirs: Vec::new(),
            loaders: Vec::new(),
            compatibility: Compatibility::default(),
            logger_closure: None,
            on_reload_cb: None,
            config: RuntimeConfig::default(),
            host_runtime: RuntimeLanguage::Rust,
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

    /// Install a Rust closure as the runtime logger.
    ///
    /// Ergonomic wrapper over `RuntimeConfig::log` for Rust hosts: the closure
    /// is boxed, owned by the built `Runtime`, and reached through an
    /// `extern "C"` trampoline. All levels are delivered
    /// (`log_max_level = LogLevel::Trace`) — filter inside the closure if you
    /// want less.
    ///
    /// # Callback contract
    /// - May be invoked from any thread.
    /// - Must NOT re-enter the runtime (calling any runtime/HostApi function
    ///   from inside the closure may deadlock).
    /// - The `scope` and `message` slices are valid only for the duration of
    ///   the call — copy them (`to_owned`) to retain.
    /// - Scope examples: `"registry"`, `"loader.lua"`, `"reload"`.
    ///
    /// Note: a later [`RuntimeBuilder::config`] call overwrites the
    /// `log` / `log_user_data` / `log_max_level` fields this installs — set the
    /// config first, then the logger.
    pub fn logger(
        mut self,
        cb: impl Fn(LogLevel, &str, &str) + Send + Sync + 'static,
    ) -> RuntimeBuilder {
        let holder: Box<LoggerClosure> = Box::new(LoggerClosure(Box::new(cb)));
        self.config.log = Some(rust_logger_trampoline);
        self.config.log_user_data = (&*holder) as *const LoggerClosure as *mut c_void;
        self.config.log_max_level = LogLevel::Trace as u32;
        self.logger_closure = Some(holder);
        self
    }

    /// Register a callback fired after each successful interface swap, before dlclose.
    ///
    /// The callback receives the opaque `RuntimeConfig::on_reload_user_data` pointer
    /// (forwarded unchanged) and a `ReloadPhase` describing the reload phase. Set the
    /// user-data pointer through [`RuntimeBuilder::config`].
    pub fn on_reload(
        mut self,
        cb: impl Fn(*mut core::ffi::c_void, polyplug_abi::runtime::ReloadPhase) + Send + Sync + 'static,
    ) -> RuntimeBuilder {
        self.on_reload_cb = Some(ReloadCallback(std::sync::Arc::new(cb)));
        self
    }

    pub fn config(mut self, config: RuntimeConfig) -> RuntimeBuilder {
        self.config = config;
        self
    }

    /// Set the host runtime type.
    /// Defaults to `RuntimeLanguage::Rust`.
    pub fn host_runtime(mut self, runtime: RuntimeLanguage) -> RuntimeBuilder {
        self.host_runtime = runtime;
        self
    }

    /// Build the runtime.
    //
    //  For MVP: scans plugin_dirs for .so/.dll/.dylib files,
    //  loads them in sorted order, registers interfaces.
    //  Full capability graph resolution is a future enhancement.
    pub fn build(self) -> Result<Arc<Runtime>, RuntimeError> {
        let logger: LoggerHandle = LoggerHandle::from_config(&self.config);
        let registry: Arc<RuntimeStore> = Arc::new(RuntimeStore::with_logger(logger));

        // Build the static HostApi. This must be 'static.
        // The `runtime` field is null here and patched once below, after the
        // Runtime is placed inside its Arc, so callbacks can recover the Runtime
        // via `(*this).runtime`.
        let host_abi: &'static HostApi = Box::leak(Box::new(HostApi {
            runtime: core::ptr::null_mut(),
            register_guest_contract: crate::runtime::host_register_guest_contract,
            alloc: crate::runtime::host_alloc,
            free: crate::runtime::host_free,
            find_guest_contract: crate::runtime::host_find_guest_contract,
            find_all_guest_contracts: crate::runtime::host_find_all_guest_contracts,
            resolve_guest_contract: crate::runtime::host_resolve_guest_contract,
            get_host_contract: crate::runtime::host_get_host_contract,
            resolve_host_contract_interface: crate::runtime::host_resolve_host_contract_interface,
            list_bundles: crate::runtime::host_list_bundles,
            get_dependencies: crate::runtime::host_get_dependencies,
            // Host operations
            load_bundle: crate::runtime::host_load_bundle,
            reload_bundle: crate::runtime::host_reload_bundle,
            register_host_contract: crate::runtime::host_register_host_contract,
            register_loader: crate::runtime::host_register_loader,
            get_last_error: crate::runtime::host_get_last_error,
            get_error_len: crate::runtime::host_get_error_len,
            call_guest_method: crate::runtime::host_call_guest_method,
            reserved: core::ptr::null(),
            unload_bundle: crate::runtime::host_unload_bundle,
        }));

        let mut loader_map: HashMap<String, Box<dyn BundleLoader>> = HashMap::new();

        // Register user-provided loaders, checking for duplicates.
        for loader in self.loaders {
            let name: &str = loader.runtime_name();
            if loader_map.contains_key(name) {
                return Err(RuntimeError::Loader(LoaderError::DuplicateLoader {
                    runtime_name: name.to_string(),
                }));
            }

            loader_map.insert(name.to_string(), loader);
        }

        // Phase 1: Scan plugin directories for bundles
        let scan: crate::loader::ScanResult = crate::loader::scan_dirs(&self.plugin_dirs);

        // Surface every scan failure as a warning. Scanning is best-effort: a
        // corrupt or unreadable bundle must not hide the others, but it must be
        // visible to the host.
        for diagnostic in &scan.diagnostics {
            logger.log(LogLevel::Warn, "builder", || format!("scan: {diagnostic}"));
        }

        let discovered: Vec<(PathBuf, ManifestData)> = scan.found;

        // Snapshot manifests for hot-reload cascade detection.
        let mut manifests_map: HashMap<String, crate::loader::ManifestData> = HashMap::new();
        for (path, manifest) in &discovered {
            let mut stored_manifest: ManifestData = manifest.clone();
            stored_manifest.path = path.clone();
            manifests_map.insert(stored_manifest.name.clone(), stored_manifest);
        }

        // Create Runtime first (before loading bundles) so we can pass it to loaders
        let runtime: Runtime = Runtime {
            registry: Arc::clone(&registry),
            host_abi,
            loaders: std::sync::RwLock::new(loader_map),
            bundle_manifests: std::sync::Mutex::new(manifests_map),
            on_reload_cb: self.on_reload_cb,
            config: self.config,
            logger,
            _logger_closure: self.logger_closure,
            last_error: std::sync::Mutex::new(String::new()),
            host_contracts: std::sync::RwLock::new(HashMap::new()),
            singleton_instances: std::sync::RwLock::new(HashMap::new()),
            host_runtime: self.host_runtime,
            init_bundle_stack: std::sync::Mutex::new(HashMap::new()),
            active_init_count: core::sync::atomic::AtomicUsize::new(0),
        };

        let runtime: Arc<Runtime> = Arc::new(runtime);

        // Patch the HostApi.runtime field to point at the Arc's target.
        // SAFETY: `host_abi` is the unique leaked HostApi for this Runtime and
        // no plugin has received it yet (bundle loading happens after this write), so
        // this is a single writer with no concurrent reader. `Arc::as_ptr` stays valid
        // for the lifetime of the Arc, and the HostApi lives at least as long
        // (callbacks only run while the Runtime — and thus the Arc — is alive).
        unsafe {
            (*(host_abi as *const HostApi as *mut HostApi)).runtime =
                Arc::as_ptr(&runtime) as *mut c_void;
        }

        // If nothing discovered, return Runtime with no loaded bundles (no graph needed)
        if !discovered.is_empty() {
            // Phase 2: Build capability graph
            let graph: CapabilityGraph =
                CapabilityGraph::from_manifests_with_logger(&discovered, logger)
                    .map_err(|e: GraphError| RuntimeError::Graph(e))?;

            // Phase 2.5: Validate version compatibility
            crate::runtime::validate_bundle_compatibility(&discovered, self.compatibility, logger)?;

            // Phase 3: Get topological load order (providers first)
            let load_order: Vec<String> = graph
                .topological_order()
                .map_err(|e: GraphError| RuntimeError::Graph(e))?;

            // Phase 4: Build lookup map bundle_name -> (path, manifest)
            let mut bundle_map: HashMap<String, (PathBuf, ManifestData)> = HashMap::new();
            for entry in discovered {
                bundle_map.insert(entry.1.name.clone(), entry);
            }

            // Phase 5: Dispatch each bundle to its loader in topo order.
            //
            // Route every discovered bundle through the shared explicit-load path
            // (`Runtime::load_manifest_with_source`) so it receives the exact same
            // treatment as a bundle loaded via `Runtime::load_bundle`: manifest
            // validation, init-time dependency declaration (so the plugin's
            // `polyplug_init` can resolve declared dependencies), bundle-metadata
            // registration (non-empty descriptors), function-count validation, and
            // the `bundle_manifests` insert. The earlier `manifests_map`
            // pre-population is overwritten with identical data by that insert.
            for bundle_name in &load_order {
                let (bundle_path, manifest): &(PathBuf, ManifestData) =
                    bundle_map.get(bundle_name).ok_or_else(|| {
                        RuntimeError::Loader(LoaderError::InitFailed {
                            bundle: bundle_name.clone(),
                            error: "bundle in topo order but not found in map".to_owned(),
                        })
                    })?;

                let source: crate::loader::BundleSource =
                    crate::loader::BundleSource::Path(bundle_path.clone());
                runtime
                    .load_manifest_with_source(
                        manifest.clone(),
                        source,
                        crate::runtime::LoadOptions {
                            compatibility: self.compatibility,
                            ignore_function_count_mismatch: false,
                        },
                    )
                    .map_err(|e: RuntimeError| match e {
                        RuntimeError::Loader(le) => RuntimeError::Loader(le),
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

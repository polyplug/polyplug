use std::{collections::HashMap, path::PathBuf};

use crate::{compatibility::Compatibility, error::{LoaderError, RuntimeError}, loader::BundleLoader};

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

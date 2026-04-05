use std::{collections::HashMap, path::PathBuf, sync::Arc};

use polyplug_abi::{RuntimeAbi, RuntimeLanguage};

use crate::{
    compatibility::{CapabilityGraph, Compatibility},
    error::{GraphError, LoaderError, RuntimeError},
    loader::{BundleLoader, ManifestData},
    registry::plugin_registry::PluginRegistry,
    runtime::{Runtime, WarningCb, ReloadCb},
    RuntimeConfig,
};

/// Builder for constructing a Runtime.
pub struct RuntimeBuilder {
    plugin_dirs: Vec<PathBuf>,
    loaders: Vec<Box<dyn BundleLoader>>,
    compatibility: Compatibility,
    warning_cb: Option<WarningCb>,
    on_reload_cb: Option<ReloadCb>,
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
            warning_cb: None,
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

    /// Register a warning callback.
    ///
    /// Only the first registered callback takes effect (OnceLock semantics).
    /// The callback receives human-readable warning strings.
    pub fn on_warning(mut self, cb: impl Fn(&str) + Send + Sync + 'static) -> RuntimeBuilder {
        self.warning_cb = Some(Box::new(cb));
        self
    }

    /// Register a callback fired after each successful interface swap, before dlclose.
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
    pub fn build(self) -> Result<Runtime, RuntimeError> {
        let registry: Arc<PluginRegistry> = Arc::new(PluginRegistry::new());

        // Build the static RuntimeAbi. This must be 'static.
        let host_abi: &'static RuntimeAbi = Box::leak(Box::new(RuntimeAbi {
            register_contract: crate::runtime::host_register_contract,
            alloc: crate::runtime::host_alloc,
            free: crate::runtime::host_free,
            find_by_contract: crate::runtime::host_find_by_contract,
            find_all_by_contract: crate::runtime::host_find_all_by_contract,
            resolve_contract: crate::runtime::host_resolve_contract,
            call_method: crate::runtime::host_call_method,
            get_host_contract: crate::runtime::host_get_host_contract,
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

        // Phase 1: Scan plugin directories for bundles
        let discovered: Vec<(PathBuf, ManifestData)> =
            crate::loader::scanner::scan_dirs(&self.plugin_dirs);

        // Snapshot manifests for hot-reload cascade detection.
        let mut manifests_map: HashMap<String, crate::loader::ManifestData> =
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
            host_abi,
            loaders: loader_map,
            bundle_manifests: std::sync::Mutex::new(manifests_map),
            on_reload_cb: self.on_reload_cb,
            config: self.config,
            warning_cb: self.warning_cb,
            last_error: std::sync::Mutex::new(String::new()),
            host_contracts: std::sync::RwLock::new(HashMap::new()),
            singleton_instances: std::sync::RwLock::new(HashMap::new()),
            host_runtime: self.host_runtime.into(),
        };

        // If nothing discovered, return Runtime with empty bundles (no graph needed)
        if !discovered.is_empty() {
            // Phase 2: Build capability graph
            let graph: CapabilityGraph = CapabilityGraph::from_manifests(&discovered)
                .map_err(|e: GraphError| RuntimeError::Graph(e))?;

            // Phase 2.5: Validate version compatibility
            crate::runtime::validate_bundle_compatibility(
                &discovered,
                self.compatibility,
                runtime.warning_cb(),
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

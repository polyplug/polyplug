use core::ffi::c_void;
use core::ptr;
use core::slice;
use core::sync::atomic::AtomicUsize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use ed25519_dalek::VerifyingKey;
use polyplug_abi::runtime::{Compatibility, ReloadPhase, RuntimeConfig, SignaturePolicy};
use polyplug_abi::types::{Array, Ed25519PublicKey, LogLevel, StringView};
use polyplug_abi::{HostApi, SupportedLanguage};

use polyplug_common::ManifestData;

use crate::{
    compatibility::CapabilityGraph,
    error::{GraphError, LoaderError, RuntimeError},
    loader::{BundleLoader, BundleSource, ScanResult, scan_dirs},
    logger::{LoggerClosure, LoggerHandle},
    runtime::{
        LoadOptions, ReloadCallback, Runtime, host_alloc, host_create_guest_instance,
        host_destroy_guest_instance, host_find_all_guest_contracts, host_find_guest_contract,
        host_free, host_get_dependencies, host_get_error_len, host_get_host_contract,
        host_get_last_error, host_list_bundles, host_load_bundle, host_log,
        host_register_guest_contract, host_register_host_contract, host_register_loader,
        host_registry_revision, host_reload_bundle, host_resolve_guest_contract,
        host_resolve_host_contract_interface, host_unload_bundle, validate_bundle_compatibility,
    },
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
    (callback.0)(level, scope_str, message_str);
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
    host_language: SupportedLanguage,
    /// Host-configured trusted Ed25519 verifying keys for bundle key pinning,
    /// stored as owned ABI key structs. Empty = TOFU (no pinning). At `build()`
    /// this `Vec` moves into the runtime and `config.trusted_keys` points at its
    /// heap buffer, so the persisted `Array` never dangles.
    trusted_keys: Vec<Ed25519PublicKey>,
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
            host_language: SupportedLanguage::Rust,
            trusted_keys: Vec::new(),
        }
    }

    /// Add a directory to scan for plugin bundles during `build()`.
    pub fn plugin_dir(mut self, path: PathBuf) -> RuntimeBuilder {
        self.plugin_dirs.push(path);
        self
    }

    /// Register a bundle loader.
    ///
    /// The loader is identified by `loader.loader_name()`. Duplicate registrations
    /// (same loader name) are detected in `build()` and cause `build()` to return
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
        cb: impl Fn(*mut c_void, ReloadPhase) + Send + Sync + 'static,
    ) -> RuntimeBuilder {
        self.on_reload_cb = Some(ReloadCallback(Arc::new(cb)));
        self
    }

    pub fn config(mut self, config: RuntimeConfig) -> RuntimeBuilder {
        self.config = config;
        self
    }

    /// Set the bundle signature enforcement policy.
    /// Defaults to `SignaturePolicy::Off` (unsigned bundles load normally).
    pub fn signature_policy(mut self, policy: SignaturePolicy) -> RuntimeBuilder {
        self.config.signature_policy = policy;
        self
    }

    /// Pin the set of trusted Ed25519 verifying keys (signing key pinning).
    ///
    /// With an empty set (the default) the runtime uses Trust-On-First-Use: it
    /// trusts the key embedded in each `bundle.sig`, so signature verification
    /// proves integrity but not authenticity. Supplying one or more keys here
    /// switches to key pinning: after the normal signature check, the runtime
    /// also requires the bundle's embedded key to be one of these, rejecting a
    /// bundle re-signed with any other key.
    ///
    /// Only effective alongside a non-`Off` [`signature_policy`]; under
    /// [`SignaturePolicy::Off`] no verification runs. The keys are copied into
    /// the builder, so the borrowed slice need not outlive this call.
    ///
    /// [`signature_policy`]: RuntimeBuilder::signature_policy
    pub fn trusted_keys(mut self, keys: &[VerifyingKey]) -> RuntimeBuilder {
        self.trusted_keys = keys
            .iter()
            .map(|key: &VerifyingKey| Ed25519PublicKey {
                bytes: *key.as_bytes(),
            })
            .collect();
        self
    }

    /// Set the host language type.
    /// Defaults to `SupportedLanguage::Rust`.
    pub fn host_language(mut self, language: SupportedLanguage) -> RuntimeBuilder {
        self.host_language = language;
        self
    }

    /// Build the runtime.
    //
    //  For MVP: scans plugin_dirs for .so/.dll/.dylib files,
    //  loads them in sorted order, registers interfaces.
    //  Full capability graph resolution is a future enhancement.
    pub fn build(mut self) -> Result<Arc<Runtime>, RuntimeError> {
        // Resolve the trusted-key allowlist into runtime-owned storage. Keys reach
        // the builder two ways, and BOTH are copied into the `_trusted_keys` `Vec`
        // so the runtime owns them for its whole lifetime and `config.trusted_keys`
        // points at that owned copy. This honors the documented
        // `RuntimeConfig.trusted_keys` contract: the host's own buffer is only
        // borrowed for the duration of `create`, and the host may free it as soon
        // as `create` returns. (Copying is cheap — a handful of 32-byte keys, once,
        // on the rare construction path.)
        //
        // * Rust builder API (`trusted_keys()`): `self.trusted_keys` is already an
        //   owned `Vec` — use it directly.
        // * FFI / `config()` path: a host (any language) populated
        //   `config.trusted_keys` with its OWN `Array` and did not call the Rust
        //   API, so copy those elements out into runtime-owned storage here. Not
        //   copying would either dangle (host frees per contract) or silently drop
        //   the keys, disabling pinning for every non-Rust host.
        //
        // Moving a `Vec` relocates only its 3-word header, never the heap buffer the
        // `Array` pointer addresses, so the pointer stays valid across the move into
        // the `Runtime` field below. An empty result leaves `config.trusted_keys`
        // as the default empty `Array` (TOFU).
        let mut trusted_keys: Vec<Ed25519PublicKey> = self.trusted_keys;
        if trusted_keys.is_empty() && !self.config.trusted_keys.is_empty() {
            let raw: &Array<Ed25519PublicKey> = &self.config.trusted_keys;
            // SAFETY: on the FFI/`config()` path the host guarantees, per the
            // `RuntimeConfig.trusted_keys` ownership contract, that `items` is valid
            // for `len` elements for the duration of this `create` call (where
            // `build` runs). `is_empty()` above rules out a null pointer / zero
            // length. We only read the elements and copy their bytes out; the host
            // pointer is never retained past this block.
            let host_keys: &[Ed25519PublicKey] =
                unsafe { slice::from_raw_parts(raw.items, raw.len) };
            trusted_keys = host_keys.to_vec();
        }
        if trusted_keys.is_empty() {
            self.config.trusted_keys = Array::empty();
        } else {
            self.config.trusted_keys = Array::new(
                trusted_keys.as_ptr() as *mut Ed25519PublicKey,
                trusted_keys.len(),
            );
        }

        let logger: LoggerHandle = LoggerHandle::from_config(&self.config);
        let registry: Arc<RuntimeStore> = Arc::new(RuntimeStore::with_logger(logger));

        // Build the owned HostApi. The `Box` gives it a stable heap address that
        // is independent of where the `Runtime` value lives, so the pointer handed
        // to plugins survives the runtime's move into its `Arc`. Ownership lives in
        // the `Runtime` (its last-declared field) and is reclaimed on teardown.
        // The `runtime` field is null here and patched once below, after the
        // Runtime is placed inside its Arc, so callbacks can recover the Runtime
        // via `(*this).runtime`.
        let host_abi: Box<HostApi> = Box::new(HostApi {
            runtime: ptr::null_mut(),
            register_guest_contract: host_register_guest_contract,
            alloc: host_alloc,
            free: host_free,
            find_guest_contract: host_find_guest_contract,
            find_all_guest_contracts: host_find_all_guest_contracts,
            resolve_guest_contract: host_resolve_guest_contract,
            get_host_contract: host_get_host_contract,
            resolve_host_contract_interface: host_resolve_host_contract_interface,
            list_bundles: host_list_bundles,
            get_dependencies: host_get_dependencies,
            load_bundle: host_load_bundle,
            reload_bundle: host_reload_bundle,
            register_host_contract: host_register_host_contract,
            register_loader: host_register_loader,
            get_last_error: host_get_last_error,
            get_error_len: host_get_error_len,
            unload_bundle: host_unload_bundle,
            log: host_log,
            create_guest_instance: host_create_guest_instance,
            destroy_guest_instance: host_destroy_guest_instance,
            registry_revision: host_registry_revision,
            reserved: ptr::null(),
        });

        let mut loader_map: HashMap<String, Box<dyn BundleLoader>> = HashMap::new();

        // Register user-provided loaders, checking for duplicates.
        for loader in self.loaders {
            let name: &str = loader.loader_name();
            if loader_map.contains_key(name) {
                return Err(RuntimeError::Loader(LoaderError::DuplicateLoader {
                    loader_name: name.to_string(),
                }));
            }

            loader_map.insert(name.to_string(), loader);
        }

        // Phase 1: Scan plugin directories for bundles
        let scan: ScanResult = scan_dirs(&self.plugin_dirs);

        // Surface every scan failure as a warning. Scanning is best-effort: a
        // corrupt or unreadable bundle must not hide the others, but it must be
        // visible to the host.
        for diagnostic in &scan.diagnostics {
            logger.log(LogLevel::Warn, "builder", || format!("scan: {diagnostic}"));
        }

        let discovered: Vec<(PathBuf, ManifestData)> = scan.found;

        // Snapshot manifests for hot-reload cascade detection.
        let mut manifests_map: HashMap<String, ManifestData> = HashMap::new();
        for (path, manifest) in &discovered {
            let mut stored_manifest: ManifestData = manifest.clone();
            stored_manifest.path = path.clone();
            manifests_map.insert(stored_manifest.name.clone(), stored_manifest);
        }

        // Create Runtime first (before loading bundles) so we can pass it to loaders
        let runtime: Runtime = Runtime {
            registry: Arc::clone(&registry),
            host_abi,
            loaders: RwLock::new(loader_map),
            bundle_manifests: Mutex::new(manifests_map),
            on_reload_cb: self.on_reload_cb,
            config: self.config,
            logger,
            _logger_closure: self.logger_closure,
            _trusted_keys: trusted_keys,
            last_error: Mutex::new(String::new()),
            host_contracts: RwLock::new(HashMap::new()),
            singleton_instances: RwLock::new(HashMap::new()),
            host_language: self.host_language,
            init_bundle_stack: Mutex::new(HashMap::new()),
            active_init_count: AtomicUsize::new(0),
            reload_serialize: Mutex::new(()),
            instance_counts: Mutex::new(HashMap::new()),
            in_process_residents: Mutex::new(HashMap::new()),
        };

        let runtime: Arc<Runtime> = Arc::new(runtime);

        // Patch the owned HostApi's `runtime` field to point at the Arc's target.
        // The patch pointer is derived ENTIRELY through raw pointers from
        // `Arc::as_ptr` — no intermediate `&`/`&mut` to the Runtime or HostApi is
        // formed — so the write does not violate Stacked Borrows. `Box<HostApi>` is
        // layout-identical to `*mut HostApi` (a single non-null pointer for a sized
        // payload), so reading the field as `*mut HostApi` yields the Box's stable
        // heap address — the same pointer plugins later receive via `host_abi()`.
        //
        // SAFETY: `rt_ptr` comes from `Arc::as_ptr` and is valid for the Arc's
        // lifetime; `&raw const (*rt_ptr).host_abi` addresses the `Box<HostApi>`
        // field in-bounds. Reading it as `*mut HostApi` is sound by the layout
        // identity above and yields the live owned HostApi. No plugin has received
        // that HostApi yet (bundle loading happens after this write), so this is a
        // single writer with no concurrent reader and no aliasing live reference.
        // The HostApi is owned by the Runtime the Arc holds, so it outlives the
        // runtime pointer written here.
        unsafe {
            let rt_ptr: *const Runtime = Arc::as_ptr(&runtime);
            let box_field_ptr: *const Box<HostApi> = &raw const (*rt_ptr).host_abi;
            let host_abi_ptr: *mut HostApi = (box_field_ptr as *const *mut HostApi).read();
            (*host_abi_ptr).runtime = rt_ptr as *mut c_void;
        }

        // If nothing discovered, return Runtime with no loaded bundles (no graph needed)
        if !discovered.is_empty() {
            // Phase 2: Build capability graph
            let graph: CapabilityGraph =
                CapabilityGraph::from_manifests_with_logger(&discovered, logger)
                    .map_err(|e: GraphError| RuntimeError::Graph(e))?;

            // Phase 2.5: Validate version compatibility
            validate_bundle_compatibility(&discovered, self.compatibility, logger)?;

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

                let source: BundleSource = BundleSource::Path(bundle_path.clone());
                runtime
                    .load_manifest_with_source(
                        manifest.clone(),
                        source,
                        LoadOptions {
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

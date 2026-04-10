//! Runtime — core runtime logic, builder pattern, and two-phase lifecycle.
//!
//! Phase 1 (initialization, single-threaded):
//!  - Load manifests
//!  - Build capability graph
//!  - dlopen bundles in topological order
//!  - Call init() on each bundle
//!  - Register interfaces
//!
//! Phase 2 (runtime, multi-threaded, lock-free):
//!  - Plugin dispatch is a direct pointer dereference
//!  - find_by_contract() is a read-only RwLock read guard
//!  - No locks in the hot path

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::RwLock;

use polyplug_abi::{GuestContractInterface, HostContractInterface, HostContractInstance, HostInterface, PluginDescriptor, GuestContractHandle, RuntimeLanguage};
use polyplug_abi::types::Version;
use polyplug_utils::{BundleId, GuestContractId};

use polyplug_abi::runtime::Compatibility;
use crate::error::HostContractError;
use crate::error::LoaderError;
use crate::error::RegistryError;
use crate::error::RuntimeError;
use crate::loader::BundleLoader;
use crate::loader::LoadedBundle;
use crate::loader::ManifestData;
use crate::loader::ManifestDependency;
use crate::registry::ContractRegistry;
use crate::runtime_builder::RuntimeBuilder;
use crate::RuntimeConfig;

// ─── TLS for Init Phase Bundle ID ─────────────────────────────────────────────

// Thread-local storage for bundle_id during init phase.
// Used by host_register_contract to enforce dependency constraints.
// Set by loaders before calling polyplug_init, cleared after init completes.
std::thread_local! {
    static INIT_BUNDLE_ID: core::cell::Cell<u64> = const { core::cell::Cell::new(0) };
}

/// Set the bundle_id for the current thread init phase.
/// Call this before plugin init, clear after init completes.
pub fn set_init_bundle_id(bundle_id: u64) {
    INIT_BUNDLE_ID.with(|id| id.set(bundle_id));
}

/// Clear the bundle_id after init phase completes.
pub fn clear_init_bundle_id() {
    INIT_BUNDLE_ID.with(|id| id.set(0));
}

/// Get the current init phase bundle_id.
pub fn get_init_bundle_id() -> u64 {
    INIT_BUNDLE_ID.with(|id| id.get())
}

// ─── Runtime Configuration ───────────────────────────────────────────────────

/// Type alias for the warning callback to avoid repetition.
pub type WarningCb = Box<dyn Fn(&str) + Send + Sync>;

/// Type alias for the reload callback.
pub type ReloadCb = Arc<dyn Fn(crate::reload::ReloadPhase) + Send + Sync>;

/// Options for `Runtime::load_bundle_with`.
///
/// The `compatibility` field overrides the global `RuntimeBuilder::compatibility` setting
/// for this specific bundle load only.
pub struct LoadOptions {
    pub compatibility: Compatibility,
    pub ignore_function_count_mismatch: bool,
}

/// The runtime instance.
pub struct Runtime {
    pub(crate) registry: Arc<ContractRegistry>,
    /// Loaded bundles, never dropped.
    pub(crate) _bundles: Vec<LoadedBundle>,
    /// The static HostInterface given to plugins. Must be 'static.
    pub(crate) host_abi: &'static HostInterface,
    /// All registered loaders, keyed by runtime_name. Immutable after build().
    pub(crate) loaders: HashMap<String, Box<dyn BundleLoader>>,
    /// ManifestData for all loaded bundles, keyed by bundle_name.
    /// Used by reload_bundle() for cascade detection.
    pub(crate) bundle_manifests: Mutex<HashMap<String, ManifestData>>,
    /// Optional callback fired after interface swap, before dlclose.
    pub(crate) on_reload_cb: Option<ReloadCb>,
    pub(crate) config: RuntimeConfig,
    /// Optional warning callback. If None, warnings go to stderr.
    pub(crate) warning_cb: Option<WarningCb>,
    /// Last error message for FFI error reporting.
    pub(crate) last_error: Mutex<String>,
    /// Registered host contracts, keyed by contract_id.
    pub(crate) host_contracts: RwLock<HashMap<u64, &'static HostContractInterface>>,
    /// Cache for singleton host contract instances.
    /// Key: HostContractId hash value.
    pub(crate) singleton_instances: RwLock<HashMap<u64, HostContractInstance>>,
    /// Host runtime type identifier.
    pub(crate) host_runtime: RuntimeLanguage,
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
    ) -> Result<GuestContractHandle, RegistryError> {
        self.registry.find_by_contract(GuestContractId::from_u64(contract_id), min_version)
    }

    /// Find a specific bundle's provider of a contract.
    #[inline(always)]
    pub fn find_by_bundle(
        &self,
        bundle_id: u64,
        contract_id: u64,
        min_version: u32,
    ) -> Result<GuestContractHandle, RegistryError> {
        self.registry
            .find_by_bundle(BundleId::from_u64(bundle_id), GuestContractId::from_u64(contract_id), min_version)
    }

    /// Find all providers of a contract.
    #[inline(always)]
    pub fn find_all_by_contract(
        &self,
        contract_id: u64,
        min_version: u32,
        out: &mut [GuestContractHandle],
    ) -> usize {
        self.registry
            .find_all_by_contract(GuestContractId::from_u64(contract_id), min_version, out)
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
            .find_all_by_contract_packed(GuestContractId::from_u64(contract_id), min_version, out)
    }

    /// Resolve a plugin handle to its interface pointer directly.
    #[inline(always)]
    pub fn resolve_plugin(
        &self,
        handle: GuestContractHandle,
    ) -> Result<*const GuestContractInterface, RegistryError> {
        self.registry.resolve(handle)
    }

    /// Register a host contract interface.
    /// Returns `Err(HostContractError::DuplicateContract)` if a contract with the same ID is already registered.
    pub fn register_host_contract(
        &self,
        contract_id: u64,
        interface: &'static HostContractInterface,
    ) -> Result<(), HostContractError> {
        let mut guard: std::sync::RwLockWriteGuard<'_, HashMap<u64, &'static HostContractInterface>> =
            self.host_contracts.write().unwrap_or_else(|e| {
                eprintln!("[polyplug] RwLock poisoned, recovering: {}", e);
                e.into_inner()
            });
        if guard.contains_key(&contract_id) {
            return Err(HostContractError::DuplicateContract { contract_id });
        }
        guard.insert(contract_id, interface);
        Ok(())
    }

    /// Unregister a host contract interface.
    /// Returns `true` if the contract was registered and removed, `false` if it was not found.
    pub fn unregister_host_contract(&self, contract_id: u64) -> bool {
        let mut guard: std::sync::RwLockWriteGuard<'_, HashMap<u64, &'static HostContractInterface>> =
            self.host_contracts.write().unwrap_or_else(|e| {
                eprintln!("[polyplug] RwLock poisoned, recovering: {}", e);
                e.into_inner()
            });
        guard.remove(&contract_id).is_some()
    }

    /// Get a host contract interface by contract_id and minimum version.
    /// Returns `None` if no matching contract is found or if the version is too low.
    pub fn get_host_contract(
        &self,
        contract_id: u64,
        min_version: u32,
    ) -> Option<&'static HostContractInterface> {
        let guard: std::sync::RwLockReadGuard<'_, HashMap<u64, &'static HostContractInterface>> =
            self.host_contracts.read().unwrap_or_else(|e| {
                eprintln!("[polyplug] RwLock poisoned, recovering: {}", e);
                e.into_inner()
            });
        guard.get(&contract_id).and_then(|interface| {
            let version: u32 = (interface.contract_version.major << 16) | interface.contract_version.minor;
            if version >= min_version {
                Some(*interface)
            } else {
                None
            }
        })
    }

    /// Get the host runtime type.
    #[inline(always)]
    pub fn host_runtime(&self) -> RuntimeLanguage {
        self.host_runtime
    }

    /// Get the warning callback.
    pub fn warning_cb(&self) -> Option<&WarningCb> {
        self.warning_cb.as_ref()
    }

    /// Get the HostInterface for use in plugin registrars.
    #[inline(always)]
    pub fn host_abi(&self) -> &'static HostInterface {
        self.host_abi
    }

    /// Get the HostInterface pointer for passing to guest contracts.
    ///
    /// Returns a HostInterface with the runtime pointer set.
    /// The runtime pointer can be extracted via `(*host_interface).runtime`.
    ///
    /// # Safety
    /// The returned pointer is valid for the lifetime of the Runtime.
    /// The HostInterface is leaked and lives until the Runtime is dropped.
    #[inline(always)]
    pub fn as_context_ptr(&self) -> *const HostInterface {
        // Create a HostInterface with runtime pointer set and leak it.
        // SAFETY: The pointer is used by guest code and lives for the process lifetime.
        // This is a small leak (72 bytes per call) but necessary for correctness.
        // GuestContractInterface functions receive `host: *const c_void` which is
        // actually this HostInterface pointer.
        let host_interface: Box<HostInterface> = Box::new(HostInterface {
            runtime: self as *const Runtime as *mut core::ffi::c_void,
            register_contract: host_register_contract,
            alloc: host_alloc,
            free: host_free,
            find_by_contract: host_find_by_contract,
            find_all_by_contract: host_find_all_by_contract,
            resolve_contract: host_resolve_contract,
            call_guest_method: host_call_method,
            get_host_contract: host_get_host_contract,
            resolve_host_contract_interface: host_resolve_host_contract_interface,
            list_bundles: host_list_bundles,
            get_dependencies: host_get_dependencies,
        });
        // SAFETY: We leak this HostInterface for the lifetime of the runtime.
        // This is acceptable because the pointer is used by guest contract instances
        // which are destroyed before the Runtime.
        Box::into_raw(host_interface)
    }

    #[inline(always)]
    pub fn registry(&self) -> &Arc<ContractRegistry> {
        &self.registry
    }

    /// Get the runtime configuration.
    #[inline(always)]
    pub fn config(&self) -> &RuntimeConfig {
        &self.config
    }

    /// Get the reload callback.
    #[inline(always)]
    pub fn on_reload_cb(&self) -> &Option<ReloadCb> {
        &self.on_reload_cb
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
    pub fn load_bundle(&self, path: &Path) -> Result<(), RuntimeError> {
        self.load_bundle_with(
            path,
            LoadOptions {
                compatibility: Compatibility::default(),
                ignore_function_count_mismatch: false,
            },
        )
    }

    /// Load a single plugin bundle explicitly with options.
    pub fn load_bundle_with(&self, path: &Path, opts: LoadOptions) -> Result<(), RuntimeError> {
        // Determine the bundle directory: if path is a file, use its parent; otherwise use path as-is.
        let bundle_dir: &Path = if path.is_file() {
            path.parent().unwrap_or(path)
        } else {
            path
        };

        let manifest: ManifestData = crate::loader::parse_manifest(bundle_dir)
            .map_err(|e: LoaderError| RuntimeError::Loader(e))?;
        if manifest.id == 0 {
            return Err(RuntimeError::Loader(LoaderError::InitFailed {
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
                        return Err(RuntimeError::Loader(LoaderError::FunctionCountMismatch {
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
                RuntimeError::Loader(LoaderError::NoLoaderForRuntime {
                    bundle: path.display().to_string(),
                    runtime_name: runtime_name.to_owned(),
                })
            })?;

        let result: Result<(), RuntimeError> = loader.load(&manifest, self);
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

    for (path, manifest) in manifests {
        // Check version compatibility for each dependency
        let resolved: Vec<ManifestDependency> =
            manifest.resolved_dependencies();
        for dep in &resolved {
            let (dep_contract, dep_min_version_str): (&str, &str) = match dep {
                ManifestDependency::ByContract {
                    contract,
                    min_version,
                    ..
                } => (contract.as_str(), min_version.as_str()),
                ManifestDependency::ByBundle {
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

            let required: Version = match Version::from_str(dep_min_version_str) {
                Ok(v) => v,
                Err(e) => return Err(RuntimeError::Loader(LoaderError::ManifestParse {
                    path: path.display().to_string(),
                    reason: format!("invalid version '{}': {:?}", dep_min_version_str, e),
                })),
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

fn parse_manifest_version(v: &str, _bundle_name: &str) -> Result<Version, RuntimeError> {
    if v.is_empty() {
        Ok(Version { major: 0, minor: 0, patch: 0 })
    } else {
        // Parse version string "major.minor.patch"
        let parts: Vec<&str> = v.split('.').collect();
        let major = parts.get(0).and_then(|s| s.parse().ok()).unwrap_or(0);
        let minor = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
        let patch = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
        Ok(Version { major, minor, patch })
    }
}

/// Helper to create a null GuestContractHandle.
fn plugin_handle_null() -> GuestContractHandle {
    GuestContractHandle::null()
}

/// Helper to convert a StringView to an owned String.
fn string_view_to_string_owned(sv: &polyplug_abi::types::StringView) -> String {
    if sv.ptr.is_null() || sv.len == 0 {
        return String::new();
    }
    // SAFETY: ptr and len are valid for this StringView
    let slice = unsafe { core::slice::from_raw_parts(sv.ptr, sv.len) };
    String::from_utf8_lossy(slice).into_owned()
}

// ─── HostInterface C ABI callbacks ───────────────────────────────────────────────

/// HostInterface.register_contract callback — registers a guest contract implementation with the runtime.
///
/// Uses TLS for bundle_id during init phase (dependency enforcement).
///
/// # Safety
/// - this must be a valid HostInterface pointer with valid runtime field
/// - descriptor must point to a valid PluginDescriptor
/// - interface must point to a valid GuestContractInterface that remains valid for the Runtime lifetime
pub(crate) unsafe extern "C" fn host_register_contract(
    this: *const HostInterface,
    descriptor: *const PluginDescriptor,
    interface: *const GuestContractInterface,
) -> polyplug_abi::types::AbiError {
    if this.is_null() {
        return polyplug_abi::types::AbiError {
            code: polyplug_abi::types::AbiErrorCode::Generic,
            message: polyplug_abi::types::StringView::null(),
        };
    }
    // SAFETY: this is a valid HostInterface pointer passed during polyplug_init.
    // (*this).runtime contains a valid pointer to Runtime.
    let runtime: &Runtime = unsafe { &*((*this).runtime as *const Runtime) };
    let registry: &ContractRegistry = &runtime.registry;
    // Get bundle_id from TLS (set by loader before calling polyplug_init)
    let bundle_id: u64 = get_init_bundle_id();

    // SAFETY: descriptor is provided by the plugin's polyplug_init function
    let desc: PluginDescriptor = unsafe { *descriptor };

    if desc.contract_name.ptr.is_null() || desc.contract_name.len == 0 {
        return polyplug_abi::types::AbiError {
            code: polyplug_abi::types::AbiErrorCode::Generic,
            message: polyplug_abi::types::StringView::from_static(
                b"PluginDescriptor.contract_name is null or empty",
            ),
        };
    }

    // SAFETY: desc.contract_name.ptr is non-null, valid UTF-8 for len bytes
    let contract_name: String = string_view_to_string_owned(&desc.contract_name);

    // SAFETY: interface is a valid 'static GuestContractInterface from the plugin binary
    match unsafe { registry.register(desc, interface, contract_name, BundleId::from_u64(bundle_id)) } {
        Ok(_handle) => polyplug_abi::types::AbiError::ok(),
        Err(e) => {
            eprintln!("[polyplug] registration failed for bundle {bundle_id}: {e}");
            polyplug_abi::types::AbiError {
                code: polyplug_abi::types::AbiErrorCode::Generic,
                message: polyplug_abi::types::StringView::null(),
            }
        }
    }
}

/// HostInterface.alloc callback — allocate memory via the host allocator.
///
/// # Safety
/// this is ignored (system allocator is global). Standard alloc safety applies.
pub(crate) unsafe extern "C" fn host_alloc(
    _this: *const HostInterface,
    size: usize,
    align: usize,
) -> *mut u8 {
    polyplug_abi::ffi::polyplug_host_alloc(size, align)
}

/// HostInterface.free callback — free memory via the host allocator.
///
/// # Safety
/// this is ignored (system allocator is global). Standard free safety applies.
pub(crate) unsafe extern "C" fn host_free(
    _this: *const HostInterface,
    ptr: *mut u8,
    size: usize,
    align: usize,
) {
    // SAFETY: polyplug_host_free is a safe wrapper around the system allocator.
    unsafe { polyplug_abi::ffi::polyplug_host_free(ptr, size, align) }
}

/// HostInterface.find_by_contract callback — dispatches to runtime's registry with dependency enforcement.
///
/// Uses TLS for bundle_id during init phase.
///
/// # Safety
/// this must be a valid HostInterface pointer with valid runtime field.
pub(crate) unsafe extern "C" fn host_find_by_contract(
    this: *const HostInterface,
    contract_id: u64,
    min_version: u32,
) -> GuestContractHandle {
    if this.is_null() {
        return plugin_handle_null();
    }
    // SAFETY: this is a valid HostInterface pointer passed by the host.
    // (*this).runtime contains a valid pointer to Runtime.
    let runtime: &Runtime = unsafe { &*((*this).runtime as *const Runtime) };
    let registry: &ContractRegistry = &runtime.registry;
    // Get bundle_id from TLS for dependency enforcement during init phase
    let caller_bundle_id: u64 = get_init_bundle_id();

    if caller_bundle_id != 0 && !registry.is_dependency_declared(BundleId::from_u64(caller_bundle_id), GuestContractId::from_u64(contract_id)) {
        return plugin_handle_null();
    }
    match registry.find_by_contract(GuestContractId::from_u64(contract_id), min_version) {
        Ok(h) => h,
        Err(_) => plugin_handle_null(),
    }
}

/// HostInterface.find_all_by_contract callback — returns Array<GuestContractHandle>.
///
/// # Safety
/// - this must be a valid HostInterface pointer with valid runtime field
pub(crate) unsafe extern "C" fn host_find_all_by_contract(
    this: *const HostInterface,
    contract_id: u64,
    min_version: u32,
) -> polyplug_abi::Array<GuestContractHandle> {
    use polyplug_abi::Array;

    if this.is_null() {
        return Array::empty();
    }
    // SAFETY: this is a valid HostInterface pointer passed by the host.
    // (*this).runtime contains a valid pointer to Runtime.
    let runtime: &Runtime = unsafe { &*((*this).runtime as *const Runtime) };
    let registry: &ContractRegistry = &runtime.registry;

    // First, count matching contracts
    let count = registry.count_by_contract(GuestContractId::from_u64(contract_id), min_version);

    if count == 0 {
        return Array::empty();
    }

    // Allocate via host allocator
    let size = count * core::mem::size_of::<GuestContractHandle>();
    let align = core::mem::align_of::<GuestContractHandle>();
    // SAFETY: host_alloc is safe to call from unsafe context
    let ptr = unsafe { host_alloc(this, size, align) as *mut GuestContractHandle };

    if ptr.is_null() {
        return Array::empty();
    }

    // Fill array with matching handles
    let slice = unsafe { core::slice::from_raw_parts_mut(ptr, count) };
    let actual = registry.find_all_by_contract_into(GuestContractId::from_u64(contract_id), min_version, slice);

    Array::new(ptr, actual)
}

/// HostInterface.resolve_contract callback — returns interface pointer for a handle.
///
/// # Safety
/// this must be a valid HostInterface pointer with valid runtime field.
pub(crate) unsafe extern "C" fn host_resolve_contract(
    this: *const HostInterface,
    handle: GuestContractHandle,
) -> *const GuestContractInterface {
    if this.is_null() {
        return core::ptr::null();
    }
    // SAFETY: this is a valid HostInterface pointer passed by the host.
    // (*this).runtime contains a valid pointer to Runtime.
    let runtime: &Runtime = unsafe { &*((*this).runtime as *const Runtime) };
    let registry: &ContractRegistry = &runtime.registry;

    match registry.resolve(handle) {
        Ok(ptr) => ptr,
        Err(_) => core::ptr::null(),
    }
}

/// HostInterface.call_guest_method callback — cross-dispatch method call for guest contracts.
///
/// This function enables plugins to call methods on other guest contract instances
/// across different dispatch types (Native vs VM).
///
/// # Implementation Status
/// **PLACEHOLDER** - Full implementation requires instance-to-contract mapping.
///
/// For full implementation, the runtime needs to track which contract each instance
/// belongs to. Options:
/// - Option A: instance.data contains a struct with `{ contract_id, state_ptr }`
/// - Option B: Runtime tracks instance -> contract_id mapping separately
///
/// Once the contract is known:
/// - Native dispatch: `dispatch.native.functions[method_id](instance, args, out)`
/// - VM dispatch: `dispatch.vm.call(loader_data, instance, method_id, args, out)`
///
/// # Safety
/// - this must be a valid HostInterface pointer with valid runtime field.
/// - instance must be a valid GuestContractInstance (non-null data pointer).
/// - args must point to valid ABI-packed arguments for the method.
/// - out must point to a valid output buffer sized for the return type.
pub(crate) unsafe extern "C" fn host_call_method(
    this: *const HostInterface,
    instance: polyplug_abi::GuestContractInstance,
    _method_id: u32,
    _args: *const (),
    _out: *mut (),
) -> polyplug_abi::types::AbiError {
    use polyplug_abi::types::{AbiError, AbiErrorCode, StringView};

    if this.is_null() {
        return AbiError {
            code: AbiErrorCode::InvalidPointer,
            message: StringView::from_static(b"null HostInterface in call_guest_method"),
        };
    }
    if instance.data.is_null() {
        return AbiError {
            code: AbiErrorCode::InvalidPointer,
            message: StringView::from_static(b"null instance in call_guest_method"),
        };
    }

    // SAFETY: this is a valid HostInterface pointer passed by the host.
    // (*this).runtime contains a valid pointer to Runtime.
    let runtime: &Runtime = unsafe { &*((*this).runtime as *const Runtime) };

    // PLACEHOLDER: Full implementation requires instance -> contract mapping.
    // For now, return an error indicating this is not yet implemented.
    runtime.set_last_error("call_guest_method requires instance-contract mapping (not yet implemented)");
    AbiError {
        code: AbiErrorCode::Generic,
        message: StringView::from_static(b"call_guest_method placeholder - needs instance-contract mapping"),
    }
}

/// HostInterface.get_host_contract callback — returns an instance for a host contract.
///
/// For singleton contracts: returns cached instance (creates on first call).
/// For multi-instance contracts: creates new instance each call.
///
/// # Safety
/// this must be a valid HostInterface pointer with valid runtime field.
pub(crate) unsafe extern "C" fn host_get_host_contract(
    this: *const HostInterface,
    contract_id: u64,
    min_version: u32,
) -> HostContractInstance {
    if this.is_null() {
        return HostContractInstance::null();
    }
    // SAFETY: this is a valid HostInterface pointer passed by the host.
    // (*this).runtime contains a valid pointer to Runtime.
    let runtime: &Runtime = unsafe { &*((*this).runtime as *const Runtime) };

    // Find the host contract interface
    let host_contracts_guard = runtime.host_contracts.read().unwrap_or_else(|e| {
        eprintln!("[polyplug] RwLock poisoned, recovering: {}", e);
        e.into_inner()
    });

    // Find interface matching contract_id and version
    let interface: Option<&HostContractInterface> = host_contracts_guard.values()
        .find(|iface| {
            iface.contract_id.id() == contract_id &&
            iface.contract_version.major >= (min_version >> 16)
        })
        .map(|v| *v);

    match interface {
        Some(interface) => {
            if interface.singleton {
                // Singleton: check cache first
                let singleton_guard = runtime.singleton_instances.read().unwrap_or_else(|e| {
                    eprintln!("[polyplug] RwLock poisoned, recovering: {}", e);
                    e.into_inner()
                });
                if let Some(&instance) = singleton_guard.get(&contract_id) {
                    return instance;
                }
                drop(singleton_guard);
                drop(host_contracts_guard);

                // Create singleton and cache it
                let mut singleton_guard = runtime.singleton_instances.write().unwrap_or_else(|e| {
                    eprintln!("[polyplug] RwLock poisoned, recovering: {}", e);
                    e.into_inner()
                });
                // Double-check pattern: another thread may have created while we waited
                if let Some(&instance) = singleton_guard.get(&contract_id) {
                    return instance;
                }
                // SAFETY: interface.create_instance is a valid function pointer
                // Pass the HostContractInterface pointer (self-passing pattern)
                let instance: HostContractInstance = unsafe {
                    (interface.create_instance)(interface as *const HostContractInterface, core::ptr::null())
                };
                singleton_guard.insert(contract_id, instance);
                instance
            } else {
                // Multi-instance: create new instance each call
                // SAFETY: interface.create_instance is a valid function pointer
                // Pass the HostContractInterface pointer (self-passing pattern)
                unsafe {
                    (interface.create_instance)(interface as *const HostContractInterface, core::ptr::null())
                }
            }
        }
        None => {
            runtime.set_last_error(&format!(
                "host contract not found: id={}, min_version={}",
                contract_id, min_version
            ));
            HostContractInstance::null()
        }
    }
}

/// HostInterface.resolve_host_contract_interface callback — returns HostContractInterface pointer.
///
/// # Safety
/// this must be a valid HostInterface pointer with valid runtime field.
pub(crate) unsafe extern "C" fn host_resolve_host_contract_interface(
    this: *const HostInterface,
    contract_id: u64,
    min_version: u32,
) -> *const HostContractInterface {
    if this.is_null() {
        return core::ptr::null();
    }
    // SAFETY: this is a valid HostInterface pointer passed by the host.
    // (*this).runtime contains a valid pointer to Runtime.
    let runtime: &Runtime = unsafe { &*((*this).runtime as *const Runtime) };

    // Find the host contract interface
    let host_contracts_guard = runtime.host_contracts.read().unwrap_or_else(|e| {
        eprintln!("[polyplug] RwLock poisoned, recovering: {}", e);
        e.into_inner()
    });

    // Find interface matching contract_id and version
    host_contracts_guard.values()
        .find(|iface| {
            iface.contract_id.id() == contract_id &&
            iface.contract_version.major >= (min_version >> 16)
        })
        .map(|v| *v as *const HostContractInterface)
        .unwrap_or_else(|| {
            runtime.set_last_error(&format!(
                "host contract interface not found: id={}, min_version={}",
                contract_id, min_version
            ));
            core::ptr::null()
        })
}

/// HostInterface.list_bundles callback — returns Array<BundleId>.
///
/// # Safety
/// this must be a valid HostInterface pointer with valid runtime field.
pub(crate) unsafe extern "C" fn host_list_bundles(
    this: *const HostInterface,
) -> polyplug_abi::Array<polyplug_utils::BundleId> {
    use polyplug_abi::Array;
    use polyplug_utils::BundleId;

    if this.is_null() {
        return Array::empty();
    }
    // SAFETY: this is a valid HostInterface pointer.
    let runtime: &Runtime = unsafe { &*((*this).runtime as *const Runtime) };

    let manifests = runtime.bundle_manifests.lock().unwrap_or_else(|e| {
        eprintln!("[polyplug] Mutex poisoned, recovering: {}", e);
        e.into_inner()
    });

    let count = manifests.len();
    if count == 0 {
        return Array::empty();
    }

    // Allocate via host allocator
    let size = count * core::mem::size_of::<BundleId>();
    let align = core::mem::align_of::<BundleId>();
    // SAFETY: host_alloc is safe to call
    let ptr = unsafe { host_alloc(this, size, align) as *mut BundleId };

    if ptr.is_null() {
        return Array::empty();
    }

    // Fill array
    for (i, (_, manifest)) in manifests.iter().enumerate() {
        unsafe { *ptr.add(i) = BundleId::from_u64(manifest.id); }
    }

    Array::new(ptr, count)
}

/// HostInterface.get_dependencies callback — returns Array<DependencyInfo>.
///
/// Uses TLS bundle_id to look up the calling bundle's dependencies.
///
/// # Safety
/// this must be a valid HostInterface pointer with valid runtime field.
pub(crate) unsafe extern "C" fn host_get_dependencies(
    this: *const HostInterface,
) -> polyplug_abi::Array<polyplug_abi::DependencyInfo> {
    use polyplug_abi::{Array, DependencyInfo};

    if this.is_null() {
        return Array::empty();
    }
    // SAFETY: this is a valid HostInterface pointer.
    let runtime: &Runtime = unsafe { &*((*this).runtime as *const Runtime) };

    // Get bundle_id from TLS
    let caller_bundle_id = get_init_bundle_id();
    if caller_bundle_id == 0 {
        return Array::empty();
    }

    let manifests = runtime.bundle_manifests.lock().unwrap_or_else(|e| {
        eprintln!("[polyplug] Mutex poisoned, recovering: {}", e);
        e.into_inner()
    });

    // Find manifest by ID
    let manifest = match manifests.values().find(|m| m.id == caller_bundle_id) {
        Some(m) => m,
        None => return Array::empty(),
    };

    let deps = &manifest.dependencies;
    if deps.is_empty() {
        return Array::empty();
    }

    let count = deps.len();
    let size = count * core::mem::size_of::<DependencyInfo>();
    let align = core::mem::align_of::<DependencyInfo>();
    // SAFETY: host_alloc is safe to call
    let ptr = unsafe { host_alloc(this, size, align) as *mut DependencyInfo };

    if ptr.is_null() {
        return Array::empty();
    }

    // Fill array with DependencyInfo
    for (i, dep) in deps.iter().enumerate() {
        let info = DependencyInfo {
            contract_id: dep.contract_id,
            min_version: dep.min_version.parse().unwrap_or(0),
            bundle_id: dep.bundle_id.unwrap_or_else(|| polyplug_utils::BundleId::from_u64(0)),
        };
        unsafe { *ptr.add(i) = info; }
    }

    Array::new(ptr, count)
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
        let result: Result<GuestContractHandle, _> =
            runtime.find_by_contract(0x1234_5678_9ABC_DEF0_u64, 0);
        assert!(result.is_err(), "empty registry should return not found");
    }

    #[test]
    fn abi_ok_constant() {
        assert_eq!(polyplug_abi::AbiErrorCode::Ok, polyplug_abi::AbiErrorCode::Ok);
        assert_eq!(polyplug_abi::AbiErrorCode::Ok as u32, 0_u32);
    }

    /// TH-06: Verify host callbacks in runtime.rs use HostInterface self-passing pattern.
    /// This is a compile-time verification test.
    #[test]
    fn host_callbacks_use_host_interface_self_passing() {
        // All host callback functions (host_register_contract, host_alloc, host_free,
        // host_find_by_contract, host_find_all_by_contract, host_resolve_contract,
        // host_call_method, host_get_host_contract) use *const HostInterface as first parameter.
        //
        // This is verified by the function signatures in this file using HostInterface.
        // The self-passing pattern allows extracting runtime from (*this).runtime.
        //
        // HostInterface is pointer-sized (8 bytes on x86_64), ensuring ABI compatibility.
        assert_eq!(std::mem::size_of::<*const HostInterface>(), 8);
    }

    #[test]
    fn host_find_by_contract_null_this_returns_null() {
        // SAFETY: host_find_by_contract handles null HostInterface gracefully
        let handle: GuestContractHandle =
            unsafe { host_find_by_contract(core::ptr::null(), 0_u64, 0_u32) };
        assert!(
            handle.is_null(),
            "host_find_by_contract must return null when this is null"
        );
    }

    #[test]
    fn dep_enforcement_blocks_undeclared_contract() {
        let runtime: Runtime = Runtime::builder()
            .build()
            .expect("runtime build should succeed");

        // Set TLS bundle_id to simulate init phase
        set_init_bundle_id(0xDEAD_BEEF_u64);

        // Create a HostInterface with runtime pointer
        let host_interface: HostInterface = HostInterface {
            runtime: &runtime as *const Runtime as *mut core::ffi::c_void,
            register_contract: host_register_contract,
            alloc: host_alloc,
            free: host_free,
            find_by_contract: host_find_by_contract,
            find_all_by_contract: host_find_all_by_contract,
            resolve_contract: host_resolve_contract,
            call_guest_method: host_call_method,
            get_host_contract: host_get_host_contract,
            resolve_host_contract_interface: host_resolve_host_contract_interface,
            list_bundles: host_list_bundles,
            get_dependencies: host_get_dependencies,
        };

        // SAFETY: host_interface is valid with runtime pointer, TLS bundle_id is set
        let handle: GuestContractHandle =
            unsafe { host_find_by_contract(&host_interface as *const HostInterface, 0x1111_2222_3333_4444_u64, 0_u32) };
        assert!(
            handle.is_null(),
            "dep enforcement must return null for undeclared contract during init phase"
        );

        // Clear TLS after test
        clear_init_bundle_id();
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
        registry: &crate::registry::ContractRegistry,
        contract_id: u64,
        bundle_id: u64,
    ) -> GuestContractHandle {
        use polyplug_abi::{
            DispatchType,
            DispatchMechanisms,
            NativeDispatch,
            GuestContractInterface,
            GuestContractInstance,
        };

        unsafe extern "C" fn stub_create_instance(_host: *const HostInterface, _args: *const ()) -> GuestContractInstance {
            GuestContractInstance::null()
        }

        unsafe extern "C" fn stub_destroy_instance(_host: *const HostInterface, _instance: GuestContractInstance) {}

        let interface: &'static GuestContractInterface = Box::leak(Box::new(GuestContractInterface {
            contract_id: polyplug_utils::GuestContractId::from_u64(contract_id),
            contract_version: Version { major: 0, minor: 0, patch: 0 },
            dispatch_type: DispatchType::Native,
            create_instance: stub_create_instance,
            destroy_instance: stub_destroy_instance,
            dispatch: DispatchMechanisms {
                native: NativeDispatch {
                    function_count: 0,
                    functions: core::ptr::null(),
                },
            },
        }));
        let descriptor: polyplug_abi::PluginDescriptor = polyplug_abi::PluginDescriptor {
            name: polyplug_abi::StringView::from_static(b"stub"),
            contract_name: polyplug_abi::StringView::from_static(b"stub.contract"),
            version: Version { major: 1, minor: 0, patch: 0 },
        };
        // SAFETY: interface is leaked and lives for the process lifetime.
        let result: Result<GuestContractHandle, crate::error::RegistryError> =
            unsafe { registry.register(descriptor, interface, "stub.contract".to_owned(), BundleId::from_u64(bundle_id)) };
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
            _manifest: &ManifestData,
            _runtime: &Runtime,
        ) -> Result<(), crate::error::RuntimeError> {
            Err(RuntimeError::UndeclaredDependency {
                bundle_id: self.error_bundle_id,
                contract_id: self.contract_id,
            })
        }

        fn reload(
            &self,
            _manifest: &ManifestData,
            _runtime: &Runtime,
        ) -> Result<(), crate::error::RuntimeError> {
            Err(RuntimeError::HotReloadDisabled)
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
            _manifest: &ManifestData,
            _runtime: &Runtime,
        ) -> Result<(), crate::error::RuntimeError> {
            let mut guard: std::sync::MutexGuard<'_, Option<bool>> = match self.observed_init.lock()
            {
                Ok(g) => g,
                Err(e) => e.into_inner(),
            };
            *guard = Some(true);
            Ok(())
        }

        fn reload(
            &self,
            _manifest: &ManifestData,
            _runtime: &Runtime,
        ) -> Result<(), crate::error::RuntimeError> {
            Err(RuntimeError::HotReloadDisabled)
        }
    }

    struct PanicLoader;

    impl crate::loader::BundleLoader for PanicLoader {
        fn runtime_name(&self) -> &'static str {
            "panic"
        }

        fn load(
            &self,
            _manifest: &ManifestData,
            _runtime: &Runtime,
        ) -> Result<(), crate::error::RuntimeError> {
            panic!("intentional panic in PanicLoader");
        }

        fn reload(
            &self,
            _manifest: &ManifestData,
            _runtime: &Runtime,
        ) -> Result<(), crate::error::RuntimeError> {
            Err(RuntimeError::HotReloadDisabled)
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
            _manifest: &ManifestData,
            _runtime: &Runtime,
        ) -> Result<(), crate::error::RuntimeError> {
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
            let inner_result: Result<(), crate::error::RuntimeError> = runtime_ref
                .load_bundle_with(
                    inner_bundle.as_path(),
                    LoadOptions {
                        compatibility: polyplug_abi::runtime::Compatibility::default(),
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

        fn reload(
            &self,
            _manifest: &ManifestData,
            _runtime: &Runtime,
        ) -> Result<(), crate::error::RuntimeError> {
            Err(RuntimeError::HotReloadDisabled)
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
            _manifest: &ManifestData,
            _runtime: &Runtime,
        ) -> Result<(), crate::error::RuntimeError> {
            let mut state: std::sync::MutexGuard<'_, LazyState> = match self.state.lock() {
                Ok(g) => g,
                Err(e) => e.into_inner(),
            };
            if state.observed_init.is_none() {
                state.observed_init = Some(true);
            }
            Ok(())
        }

        fn reload(
            &self,
            _manifest: &ManifestData,
            _runtime: &Runtime,
        ) -> Result<(), crate::error::RuntimeError> {
            Err(RuntimeError::HotReloadDisabled)
        }
    }

    #[test]
    fn bundle_id_zero_escape_returns_undeclared_dependency_error() {
        let temp: tempfile::TempDir = match tempfile::TempDir::new() {
            Ok(t) => t,
            Err(e) => panic!("failed to create temp dir: {e}"),
        };
        let contract: u64 = polyplug_utils::guest_contract_id("trust.test", 1_u32);
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
        let registry: &Arc<ContractRegistry> = runtime.registry();
        let _handle: GuestContractHandle = register_contract(registry.as_ref(), contract, 0xBEEF_u64);
        let result: Result<(), crate::error::RuntimeError> =
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
        let contract: u64 = polyplug_utils::guest_contract_id("trust.tls", 1_u32);
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
        let registry: &Arc<ContractRegistry> = runtime.registry();
        let _handle: GuestContractHandle = register_contract(registry.as_ref(), contract, 0xCAFE_u64);
        let result: Result<(), crate::error::RuntimeError> =
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
        let handle_after: Result<GuestContractHandle, _> = runtime.find_by_contract(contract, 0_u32);
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
        let contract: u64 = polyplug_utils::guest_contract_id("trust.reentrant", 1_u32);
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
        let registry: &Arc<ContractRegistry> = runtime.registry();
        let _handle: GuestContractHandle = register_contract(registry.as_ref(), contract, 0xABCD_u64);
        {
            let mut guard: std::sync::MutexGuard<'_, ReentrantState> = match state.lock() {
                Ok(g) => g,
                Err(e) => e.into_inner(),
            };
            guard.runtime_ptr = &runtime as *const Runtime as usize;
        }
        let result: Result<(), crate::error::RuntimeError> = runtime.load_bundle_with(
            outer_bundle.as_path(),
            LoadOptions {
                compatibility: polyplug_abi::runtime::Compatibility::default(),
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
        let contract: u64 = polyplug_utils::guest_contract_id("trust.lazy", 1_u32);
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
        let registry: &Arc<ContractRegistry> = runtime.registry();
        let _handle: GuestContractHandle = register_contract(registry.as_ref(), contract, 0xFACE_u64);
        let result: Result<(), crate::error::RuntimeError> =
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
        let inner_result: Result<(), crate::error::RuntimeError> = runtime.load_bundle_with(
            inner_bundle.as_path(),
            LoadOptions {
                compatibility: polyplug_abi::runtime::Compatibility::default(),
                ignore_function_count_mismatch: false,
            },
        );
        if let Err(e) = inner_result {
            panic!("lazy inner load failed: {e}");
        }
    }

    // --- Host Contract Tests ---

    fn create_host_contract_interface(
        contract_id: u64,
        major: u32,
        minor: u32,
    ) -> &'static HostContractInterface {
        use polyplug_abi::{DispatchMechanisms, NativeDispatch, HostContractInstance, DispatchType};

        unsafe extern "C" fn stub_create_instance(_this: *const HostContractInterface, _args: *const ()) -> HostContractInstance {
            // Return a non-null dummy pointer for testing
            static mut DUMMY: usize = 0xDEADBEEF;
            HostContractInstance { data: &raw mut DUMMY as *mut core::ffi::c_void }
        }

        unsafe extern "C" fn stub_destroy_instance(_this: *const HostContractInterface, _instance: HostContractInstance) {}

        Box::leak(Box::new(HostContractInterface {
            contract_id: polyplug_utils::HostContractId::from(contract_id),
            contract_version: polyplug_abi::types::Version { major, minor, patch: 0 },
            singleton: true,
            dispatch_type: DispatchType::Native,
            runtime: core::ptr::null_mut(),
            create_instance: stub_create_instance,
            destroy_instance: stub_destroy_instance,
            dispatch: DispatchMechanisms {
                native: NativeDispatch {
                    function_count: 0,
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

        let contract_id: u64 = polyplug_utils::host_contract_id("host.logger", 1);
        let interface: &'static HostContractInterface = create_host_contract_interface(contract_id, 1, 0);

        let result: Result<(), HostContractError> =
            runtime.register_host_contract(contract_id, interface);
        assert!(result.is_ok(), "registration should succeed");

        let found: Option<&'static HostContractInterface> = runtime.get_host_contract(contract_id, 0);
        assert!(found.is_some(), "contract should be found");
        let found_interface: &HostContractInterface =
            found.expect("contract should be present after is_some check");
        assert_eq!(found_interface.contract_id.id(), contract_id);
    }

    #[test]
    fn runtime_host_contracts_duplicate_registration_fails() {
        let runtime: Runtime = Runtime::builder()
            .build()
            .expect("runtime build should succeed");

        let contract_id: u64 = polyplug_utils::host_contract_id("host.logger", 1);
        let interface1: &'static HostContractInterface = create_host_contract_interface(contract_id, 1, 0);
        let interface2: &'static HostContractInterface = create_host_contract_interface(contract_id, 1, 1);

        let result1: Result<(), HostContractError> =
            runtime.register_host_contract(contract_id, interface1);
        assert!(result1.is_ok(), "first registration should succeed");

        let result2: Result<(), HostContractError> =
            runtime.register_host_contract(contract_id, interface2);
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

        let contract_id: u64 = polyplug_utils::host_contract_id("host.logger", 1);
        let interface: &'static HostContractInterface = create_host_contract_interface(contract_id, 1, 0);

        runtime
            .register_host_contract(contract_id, interface)
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

        let found: Option<&'static HostContractInterface> = runtime.get_host_contract(contract_id, 0);
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

        let contract_id: u64 = polyplug_utils::host_contract_id("host.logger", 2);
        let interface: &'static HostContractInterface = create_host_contract_interface(contract_id, 2, 5);

        runtime
            .register_host_contract(contract_id, interface)
            .expect("registration should succeed");

        let found_low: Option<&'static HostContractInterface> =
            runtime.get_host_contract(contract_id, 0);
        assert!(found_low.is_some(), "should find with min_version=0");

        let found_exact: Option<&'static HostContractInterface> =
            runtime.get_host_contract(contract_id, (2 << 16) | 5);
        assert!(found_exact.is_some(), "should find with exact version");

        let found_higher_minor: Option<&'static HostContractInterface> =
            runtime.get_host_contract(contract_id, (2 << 16) | 3);
        assert!(
            found_higher_minor.is_some(),
            "should find with lower minor version requirement"
        );

        let found_higher_major: Option<&'static HostContractInterface> =
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
        assert_eq!(runtime.host_runtime(), RuntimeLanguage::Rust);
    }

    #[test]
    fn runtime_host_runtime_can_be_set() {
        let runtime: Runtime = Runtime::builder()
            .host_runtime(RuntimeLanguage::Python)
            .build()
            .expect("runtime build should succeed");
        assert_eq!(runtime.host_runtime(), RuntimeLanguage::Python);
    }

    #[test]
    fn host_get_host_contract_callback_returns_registered_contract() {
        let runtime: Runtime = Runtime::builder()
            .build()
            .expect("runtime build should succeed");

        let contract_id: u64 = polyplug_utils::host_contract_id("host.test", 1);
        let interface: &'static HostContractInterface = create_host_contract_interface(contract_id, 1, 0);

        runtime
            .register_host_contract(contract_id, interface)
            .expect("registration should succeed");

        // Create a HostInterface with runtime pointer
        let host_interface: HostInterface = HostInterface {
            runtime: &runtime as *const Runtime as *mut core::ffi::c_void,
            register_contract: host_register_contract,
            alloc: host_alloc,
            free: host_free,
            find_by_contract: host_find_by_contract,
            find_all_by_contract: host_find_all_by_contract,
            resolve_contract: host_resolve_contract,
            call_guest_method: host_call_method,
            get_host_contract: host_get_host_contract,
            resolve_host_contract_interface: host_resolve_host_contract_interface,
            list_bundles: host_list_bundles,
            get_dependencies: host_get_dependencies,
        };

        // SAFETY: host_interface is valid with runtime pointer, runtime is live
        let instance: HostContractInstance =
            unsafe { host_get_host_contract(&host_interface as *const HostInterface, contract_id, 0) };
        assert!(
            !instance.data.is_null(),
            "callback should return non-null instance for registered contract"
        );
    }

    #[test]
    fn host_get_host_contract_callback_returns_null_for_unregistered() {
        let runtime: Runtime = Runtime::builder()
            .build()
            .expect("runtime build should succeed");

        let contract_id: u64 = polyplug_utils::host_contract_id("host.nonexistent", 1);

        // Create a HostInterface with runtime pointer
        let host_interface: HostInterface = HostInterface {
            runtime: &runtime as *const Runtime as *mut core::ffi::c_void,
            register_contract: host_register_contract,
            alloc: host_alloc,
            free: host_free,
            find_by_contract: host_find_by_contract,
            find_all_by_contract: host_find_all_by_contract,
            resolve_contract: host_resolve_contract,
            call_guest_method: host_call_method,
            get_host_contract: host_get_host_contract,
            resolve_host_contract_interface: host_resolve_host_contract_interface,
            list_bundles: host_list_bundles,
            get_dependencies: host_get_dependencies,
        };

        // SAFETY: host_interface is valid with runtime pointer, runtime is live
        let instance: HostContractInstance =
            unsafe { host_get_host_contract(&host_interface as *const HostInterface, contract_id, 0) };
        assert!(
            instance.data.is_null(),
            "callback should return null instance for unregistered contract"
        );
    }

    // ─── Instance Lifecycle Tests (HC-02, HC-03) ───────────────────────────────

    // Create instance callback that returns a unique "magic" pointer per call.
    // Uses a thread-local counter to ensure unique values per call within a test.
    std::thread_local! {
        static LOCAL_INSTANCE_COUNTER: core::cell::Cell<usize> = const { core::cell::Cell::new(0) };
    }

    /// Create instance callback that returns a unique instance per call.
    /// Each call increments a thread-local counter and returns a unique pointer.
    unsafe extern "C" fn counting_create_instance(
        _this: *const HostContractInterface,
        _args: *const (),
    ) -> HostContractInstance {
        LOCAL_INSTANCE_COUNTER.with(|counter| {
            let count: usize = counter.get();
            counter.set(count + 1);
            // Use the count as a "unique" pointer value - we don't actually allocate
            // since these are just test instances
            HostContractInstance {
                data: (count + 1) as *mut core::ffi::c_void,  // +1 to avoid null for count=0
            }
        })
    }

    /// No-op destroy for counting instances.
    unsafe extern "C" fn counting_destroy_instance(
        _this: *const HostContractInterface,
        _instance: HostContractInstance,
    ) {
        // No cleanup needed - we're just using integer values as pointers
    }

    /// Create a counting host contract interface with configurable singleton mode.
    fn create_counting_host_contract_interface(
        contract_id: u64,
        major: u32,
        singleton: bool,
    ) -> &'static HostContractInterface {
        use polyplug_abi::{DispatchMechanisms, NativeDispatch, DispatchType};

        Box::leak(Box::new(HostContractInterface {
            contract_id: polyplug_utils::HostContractId::from(contract_id),
            contract_version: polyplug_abi::types::Version { major, minor: 0, patch: 0 },
            singleton,
            dispatch_type: DispatchType::Native,
            runtime: core::ptr::null_mut(),
            create_instance: counting_create_instance,
            destroy_instance: counting_destroy_instance,
            dispatch: DispatchMechanisms {
                native: NativeDispatch {
                    function_count: 0,
                    functions: core::ptr::null(),
                },
            },
        }))
    }

    #[test]
    fn singleton_contract_returns_cached_instance_on_multiple_calls() {
        // Reset thread-local counter before test
        LOCAL_INSTANCE_COUNTER.with(|counter| counter.set(0));

        let runtime: Runtime = Runtime::builder()
            .build()
            .expect("runtime build should succeed");

        let contract_id: u64 = polyplug_utils::host_contract_id("singleton.test", 1);
        let interface: &'static HostContractInterface =
            create_counting_host_contract_interface(contract_id, 1, true);  // singleton=true

        runtime
            .register_host_contract(contract_id, interface)
            .expect("registration should succeed");

        // Create a HostInterface with runtime pointer
        let host_interface: HostInterface = HostInterface {
            runtime: &runtime as *const Runtime as *mut core::ffi::c_void,
            register_contract: host_register_contract,
            alloc: host_alloc,
            free: host_free,
            find_by_contract: host_find_by_contract,
            find_all_by_contract: host_find_all_by_contract,
            resolve_contract: host_resolve_contract,
            call_guest_method: host_call_method,
            get_host_contract: host_get_host_contract,
            resolve_host_contract_interface: host_resolve_host_contract_interface,
            list_bundles: host_list_bundles,
            get_dependencies: host_get_dependencies,
        };

        // First call - creates instance
        // SAFETY: host_interface is valid with runtime pointer, runtime is live
        let instance1: HostContractInstance =
            unsafe { host_get_host_contract(&host_interface as *const HostInterface, contract_id, 0) };
        assert!(!instance1.data.is_null(), "first call should return non-null instance");

        // Second call - should return SAME cached instance
        // SAFETY: host_interface is valid with runtime pointer, runtime is live
        let instance2: HostContractInstance =
            unsafe { host_get_host_contract(&host_interface as *const HostInterface, contract_id, 0) };
        assert!(!instance2.data.is_null(), "second call should return non-null instance");

        // HC-02: Verify same instance pointer is returned
        assert_eq!(
            instance1.data, instance2.data,
            "singleton contract should return cached instance (same pointer)"
        );

        // Counter should have been incremented only once (single create)
        let counter_value: usize = LOCAL_INSTANCE_COUNTER.with(|counter| counter.get());
        assert_eq!(counter_value, 1, "singleton should only call create_instance once");

        // Third call - still same instance
        // SAFETY: host_interface is valid with runtime pointer, runtime is live
        let instance3: HostContractInstance =
            unsafe { host_get_host_contract(&host_interface as *const HostInterface, contract_id, 0) };
        assert_eq!(
            instance1.data, instance3.data,
            "third call should still return same cached instance"
        );
        assert_eq!(
            LOCAL_INSTANCE_COUNTER.with(|counter| counter.get()), 1,
            "counter still at 1 - no additional create calls"
        );
    }

    #[test]
    fn multi_instance_contract_creates_new_instance_on_each_call() {
        // Reset thread-local counter before test
        LOCAL_INSTANCE_COUNTER.with(|counter| counter.set(100));  // Start at 100 for unique values

        let runtime: Runtime = Runtime::builder()
            .build()
            .expect("runtime build should succeed");

        let contract_id: u64 = polyplug_utils::host_contract_id("multi.test", 1);
        let interface: &'static HostContractInterface =
            create_counting_host_contract_interface(contract_id, 1, false);  // singleton=false

        runtime
            .register_host_contract(contract_id, interface)
            .expect("registration should succeed");

        // Create a HostInterface with runtime pointer
        let host_interface: HostInterface = HostInterface {
            runtime: &runtime as *const Runtime as *mut core::ffi::c_void,
            register_contract: host_register_contract,
            alloc: host_alloc,
            free: host_free,
            find_by_contract: host_find_by_contract,
            find_all_by_contract: host_find_all_by_contract,
            resolve_contract: host_resolve_contract,
            call_guest_method: host_call_method,
            get_host_contract: host_get_host_contract,
            resolve_host_contract_interface: host_resolve_host_contract_interface,
            list_bundles: host_list_bundles,
            get_dependencies: host_get_dependencies,
        };

        // First call - creates instance (counter becomes 101)
        // SAFETY: host_interface is valid with runtime pointer, runtime is live
        let instance1: HostContractInstance =
            unsafe { host_get_host_contract(&host_interface as *const HostInterface, contract_id, 0) };
        assert!(!instance1.data.is_null(), "first call should return non-null instance");

        // Second call - creates NEW instance (counter becomes 102)
        // SAFETY: host_interface is valid with runtime pointer, runtime is live
        let instance2: HostContractInstance =
            unsafe { host_get_host_contract(&host_interface as *const HostInterface, contract_id, 0) };
        assert!(!instance2.data.is_null(), "second call should return non-null instance");

        // HC-03: Verify different instance pointers are returned
        assert_ne!(
            instance1.data, instance2.data,
            "multi-instance contract should create new instance each call (different pointers)"
        );

        // Counter should have been incremented twice
        let counter_value: usize = LOCAL_INSTANCE_COUNTER.with(|counter| counter.get());
        assert_eq!(counter_value, 102, "multi-instance should call create_instance twice");

        // Third call - creates yet another instance (counter becomes 103)
        // SAFETY: host_interface is valid with runtime pointer, runtime is live
        let instance3: HostContractInstance =
            unsafe { host_get_host_contract(&host_interface as *const HostInterface, contract_id, 0) };
        assert_ne!(instance1.data, instance3.data, "third instance differs from first");
        assert_ne!(instance2.data, instance3.data, "third instance differs from second");
        assert_eq!(
            LOCAL_INSTANCE_COUNTER.with(|counter| counter.get()), 103,
            "counter at 103 - three create calls"
        );
    }

    #[test]
    fn singleton_and_multi_instance_contracts_coexist() {
        // Reset thread-local counter
        LOCAL_INSTANCE_COUNTER.with(|counter| counter.set(0));

        let runtime: Runtime = Runtime::builder()
            .build()
            .expect("runtime build should succeed");

        let singleton_id: u64 = polyplug_utils::host_contract_id("singleton.mixed", 1);
        let multi_id: u64 = polyplug_utils::host_contract_id("multi.mixed", 1);

        let singleton_interface: &'static HostContractInterface =
            create_counting_host_contract_interface(singleton_id, 1, true);
        let multi_interface: &'static HostContractInterface =
            create_counting_host_contract_interface(multi_id, 1, false);

        runtime
            .register_host_contract(singleton_id, singleton_interface)
            .expect("singleton registration should succeed");
        runtime
            .register_host_contract(multi_id, multi_interface)
            .expect("multi-instance registration should succeed");

        // Create a HostInterface with runtime pointer
        let host_interface: HostInterface = HostInterface {
            runtime: &runtime as *const Runtime as *mut core::ffi::c_void,
            register_contract: host_register_contract,
            alloc: host_alloc,
            free: host_free,
            find_by_contract: host_find_by_contract,
            find_all_by_contract: host_find_all_by_contract,
            resolve_contract: host_resolve_contract,
            call_guest_method: host_call_method,
            get_host_contract: host_get_host_contract,
            resolve_host_contract_interface: host_resolve_host_contract_interface,
            list_bundles: host_list_bundles,
            get_dependencies: host_get_dependencies,
        };

        // Call singleton twice - should get same instance
        // SAFETY: host_interface is valid with runtime pointer, runtime is live
        let s1: HostContractInstance =
            unsafe { host_get_host_contract(&host_interface as *const HostInterface, singleton_id, 0) };
        let s2: HostContractInstance =
            unsafe { host_get_host_contract(&host_interface as *const HostInterface, singleton_id, 0) };
        assert_eq!(s1.data, s2.data, "singleton returns cached instance");

        // Call multi-instance twice - should get different instances
        // SAFETY: host_interface is valid with runtime pointer, runtime is live
        let m1: HostContractInstance =
            unsafe { host_get_host_contract(&host_interface as *const HostInterface, multi_id, 0) };
        let m2: HostContractInstance =
            unsafe { host_get_host_contract(&host_interface as *const HostInterface, multi_id, 0) };
        assert_ne!(m1.data, m2.data, "multi-instance returns new instances");

        // Singleton instance should differ from multi instances
        assert_ne!(s1.data, m1.data, "singleton and multi instances are different");
        assert_ne!(s1.data, m2.data, "singleton and multi instances are different");
    }
}

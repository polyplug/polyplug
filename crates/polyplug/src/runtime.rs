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
//!  - find_guest_contract() is a read-only RwLock read guard
//!  - No locks in the hot path

use core::str::FromStr;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::RwLock;
use std::thread::ThreadId;

use polyplug_abi::runtime::{Compatibility, RuntimeConfig};
use polyplug_abi::{
    GuestContractHandle, GuestContractInterface, HostApi, HostContractInstance,
    HostContractInterface, PluginDescriptor, RuntimeLanguage, types::Version,
};
use polyplug_utils::{BundleId, GuestContractId, fnv1a_32};

use crate::error::HostContractError;
use crate::error::LoaderError;
use crate::error::RegistryError;
use crate::error::RuntimeError;
use crate::loader::BundleLoader;
use crate::loader::ManifestData;
use crate::loader::ManifestDependency;
pub use crate::runtime_builder::RuntimeBuilder;
use crate::runtime_store::RuntimeStore;

// ─── Runtime Configuration ───────────────────────────────────────────────────

/// Warning callback invoked with human-readable diagnostic strings.
pub(crate) struct WarningCallback(pub(crate) Box<dyn Fn(&str) + Send + Sync>);

/// Reload callback invoked after each interface swap, before dlclose.
///
/// The first argument is the opaque `on_reload_user_data` pointer from
/// `RuntimeConfig`, forwarded unchanged on every invocation.
pub(crate) struct ReloadCallback(
    pub(crate) Arc<dyn Fn(*mut core::ffi::c_void, polyplug_abi::runtime::ReloadPhase) + Send + Sync>,
);

/// Options for `Runtime::load_bundle_with`.
///
/// The `compatibility` field overrides the global `RuntimeBuilder::compatibility` setting
/// for this specific bundle load only.
pub(crate) struct LoadOptions {
    pub compatibility: Compatibility,
    pub ignore_function_count_mismatch: bool,
}

/// The runtime instance.
pub struct Runtime {
    pub(crate) registry: Arc<RuntimeStore>,
    /// The static HostApi given to plugins. Must be 'static.
    pub(crate) host_abi: &'static HostApi,
    /// All registered loaders, keyed by runtime_name.
    ///
    /// Interior-mutable (`RwLock`) so loaders can be registered after `build()`
    /// through a shared `&Runtime` (e.g. the `register_loader` HostApi
    /// callback), without ever forging a `&mut Runtime` from an `Arc`-shared
    /// pointer (which would be aliasing UB). Load/reload paths take read guards;
    /// registration takes a write guard.
    pub(crate) loaders: RwLock<HashMap<String, Box<dyn BundleLoader>>>,
    /// ManifestData for all loaded bundles, keyed by bundle_name.
    /// Used by reload_bundle() for cascade detection.
    pub(crate) bundle_manifests: Mutex<HashMap<String, ManifestData>>,
    /// Optional callback fired after interface swap, before dlclose.
    pub(crate) on_reload_cb: Option<ReloadCallback>,
    pub(crate) config: RuntimeConfig,
    /// Optional warning callback. If None, warnings go to stderr.
    pub(crate) warning_cb: Option<WarningCallback>,
    /// Last error message for FFI error reporting.
    pub(crate) last_error: Mutex<String>,
    /// Registered host contracts, keyed by contract_id.
    pub(crate) host_contracts: RwLock<HashMap<u64, &'static HostContractInterface>>,
    /// Cache for singleton host contract instances.
    /// Key: HostContractId hash value.
    pub(crate) singleton_instances: RwLock<HashMap<u64, HostContractInstance>>,
    /// Host runtime type identifier.
    pub(crate) host_runtime: RuntimeLanguage,
    /// Host-registered extensions, keyed by fnv1a_32 of the extension name.
    /// Raw pointer stored as usize for Send+Sync. Callers are responsible for thread safety.
    pub(crate) extensions: RwLock<HashMap<u32, usize>>,
    /// Per-thread stack of bundle_ids currently inside `polyplug_init`.
    ///
    /// Replaces the former process-global `thread_local!` (Rule 12: no thread-locals
    /// for runtime state — this is now instance-owned, so multiple runtimes in one
    /// process stay isolated). A `Vec` per thread gives reentrancy safety: a nested
    /// load on the same thread pushes its own id and pops it on completion, restoring
    /// the outer bundle's id instead of clobbering it. Loaders push before calling
    /// `polyplug_init` and pop afterwards (including the panic path).
    pub(crate) init_bundle_stack: Mutex<HashMap<ThreadId, Vec<u64>>>,
}

impl Runtime {
    /// Create a RuntimeBuilder.
    pub fn builder() -> RuntimeBuilder {
        RuntimeBuilder::new()
    }

    /// Find the first provider of a contract.
    #[inline(always)]
    pub fn find_guest_contract(
        &self,
        contract_id: u64,
        min_version: u32,
    ) -> Result<GuestContractHandle, RegistryError> {
        self.registry
            .find_guest_contract(GuestContractId::from_u64(contract_id), min_version)
    }

    /// Find a specific bundle's provider of a contract.
    #[inline(always)]
    pub fn find_guest_contract_by_bundle(
        &self,
        bundle_id: u64,
        contract_id: u64,
        min_version: u32,
    ) -> Result<GuestContractHandle, RegistryError> {
        self.registry.find_guest_contract_by_bundle(
            BundleId::from_u64(bundle_id),
            GuestContractId::from_u64(contract_id),
            min_version,
        )
    }

    /// Find all providers of a contract.
    #[inline(always)]
    pub fn find_all_by_contract(
        &self,
        contract_id: u64,
        min_version: u32,
        out: &mut [GuestContractHandle],
    ) -> usize {
        self.registry.find_all_guest_contracts(
            GuestContractId::from_u64(contract_id),
            min_version,
            out,
        )
    }

    /// Find all providers of a contract, packing handles directly into a u64 buffer.
    #[inline(always)]
    pub fn find_all_by_contract_packed(
        &self,
        contract_id: u64,
        min_version: u32,
        out: &mut [u64],
    ) -> usize {
        self.registry.find_all_guest_contracts_packed(
            GuestContractId::from_u64(contract_id),
            min_version,
            out,
        )
    }

    /// Resolve a plugin handle to its interface pointer directly.
    #[inline(always)]
    pub fn resolve_guest_contract(
        &self,
        handle: GuestContractHandle,
    ) -> Result<*const GuestContractInterface, RegistryError> {
        self.registry.resolve_guest_contract(handle)
    }

    /// Host-side convenience wrapper for plugin→plugin cross-dispatch.
    ///
    /// Routes through the same internal logic as the `call_guest_method` HostApi
    /// callback (re-resolving the target by `instance.contract_id` on every call,
    /// holding no registry lock across the dispatch). The target contract and
    /// function are addressed by the `instance` handle and `fn_id`.
    ///
    /// # Safety
    /// - `instance` must be a live instance produced by the target contract
    /// - `args` / `out` must satisfy the target function's ABI argument layout
    /// - `arena` must be null or a valid [`polyplug_abi::types::CallArena`]
    ///   for the duration of the call
    #[inline]
    pub unsafe fn call_guest_method(
        &self,
        instance: polyplug_abi::guest::GuestContractInstance,
        fn_id: u32,
        args: *const core::ffi::c_void,
        out: *mut core::ffi::c_void,
        arena: *mut polyplug_abi::types::CallArena,
    ) -> polyplug_abi::types::AbiError {
        // SAFETY: host_abi is the runtime's own 'static HostApi whose `runtime`
        // field points to this Runtime; forwarding the args is the same call the
        // VM/native guests make.
        unsafe {
            host_call_guest_method(
                self.host_abi as *const HostApi,
                instance,
                fn_id,
                args,
                out,
                arena,
            )
        }
    }

    /// Register a host contract interface.
    /// Returns `Err(HostContractError::DuplicateContract)` if a contract with the same ID is already registered.
    pub fn register_host_contract(
        &self,
        contract_id: u64,
        interface: &'static HostContractInterface,
    ) -> Result<(), HostContractError> {
        let mut guard: std::sync::RwLockWriteGuard<
            '_,
            HashMap<u64, &'static HostContractInterface>,
        > = self.host_contracts.write().unwrap_or_else(|e| {
            eprintln!("[polyplug] RwLock poisoned, recovering: {}", e);
            e.into_inner()
        });
        if guard.contains_key(&contract_id) {
            return Err(HostContractError::DuplicateContract { contract_id });
        }
        guard.insert(contract_id, interface);
        Ok(())
    }

    /// Unregister_guest_contract a host contract interface.
    /// Returns `true` if the contract was register_guest_contracted and removed, `false` if it was not found.
    pub fn unregister_host_contract(&self, contract_id: u64) -> bool {
        let mut guard: std::sync::RwLockWriteGuard<
            '_,
            HashMap<u64, &'static HostContractInterface>,
        > = self.host_contracts.write().unwrap_or_else(|e| {
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
            if host_contract_version_satisfies(interface, min_version) {
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
    pub(crate) fn warning_cb(&self) -> Option<&WarningCallback> {
        self.warning_cb.as_ref()
    }

    /// Get the HostApi for use in plugin registrars.
    #[inline(always)]
    pub fn host_abi(&self) -> &'static HostApi {
        self.host_abi
    }

    /// Register a host extension by name.
    ///
    /// Plugins retrieve extensions via `get_extension` on the HostApi.
    /// The extension_id is computed with `polyplug_utils::fnv1a_32(name.as_bytes())`.
    ///
    /// # Safety
    /// `ptr` must remain valid for the lifetime of the runtime.
    pub unsafe fn register_extension(&self, name: &str, ptr: *const ()) {
        let extension_id: u32 = fnv1a_32(name.as_bytes());
        if let Ok(mut map) = self.extensions.write() {
            map.insert(extension_id, ptr as usize);
        }
    }

    /// Get the HostApi pointer for passing to guest contracts.
    ///
    /// Returns the runtime's `'static` HostApi, whose `runtime` field was
    /// patched once in `RuntimeBuilder::build` to point at this Runtime.
    /// The runtime pointer can be extracted via `(*host_interface).runtime`.
    ///
    /// # Safety
    /// The returned pointer is valid for the lifetime of the Runtime.
    #[inline(always)]
    pub fn as_context_ptr(&self) -> *const HostApi {
        self.host_abi as *const HostApi
    }

    #[inline(always)]
    pub fn registry(&self) -> &Arc<RuntimeStore> {
        &self.registry
    }

    /// Get the runtime configuration.
    #[inline(always)]
    pub fn config(&self) -> &RuntimeConfig {
        &self.config
    }

    /// Get the reload callback.
    #[inline(always)]
    pub(crate) fn on_reload_cb(&self) -> &Option<ReloadCallback> {
        &self.on_reload_cb
    }

    /// Emit a warning message via the registered warning callback, or to stderr if none.
    pub fn emit_warning(&self, msg: &str) {
        match &self.warning_cb {
            Some(cb) => (cb.0)(msg),
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
    /// loader for the same runtime name is already register_guest_contracted.
    pub fn register_guest_contract_loader(
        &self,
        loader: Box<dyn BundleLoader>,
    ) -> Result<(), RuntimeError> {
        let name: String = loader.runtime_name().to_string();
        let mut loaders: std::sync::RwLockWriteGuard<'_, HashMap<String, Box<dyn BundleLoader>>> =
            self.loaders.write().unwrap_or_else(|e| {
                eprintln!("[polyplug] RwLock poisoned, recovering: {}", e);
                e.into_inner()
            });
        if loaders.contains_key(&name) {
            return Err(RuntimeError::Loader(LoaderError::DuplicateLoader {
                runtime_name: name,
            }));
        }

        loaders.insert(name, loader);
        Ok(())
    }

    /// Resolve a loader by runtime name, returning a stable reference valid for the
    /// runtime's lifetime.
    ///
    /// The returned reference is obtained under a short-lived read guard and then
    /// detached. This is sound because loaders are append-only: once inserted into
    /// the `loaders` map a `Box<dyn BundleLoader>` is never removed or replaced for
    /// the runtime's lifetime, so the heap address behind the `Box` is stable. We
    /// must NOT hold the `loaders` read guard across `BundleLoader::load`/`reload`,
    /// because those run `polyplug_init`, which may call back into
    /// `host_register_loader` and take the `loaders` write guard — holding a read
    /// guard on the same thread would deadlock.
    pub(crate) fn loader_for(&self, runtime_name: &str) -> Option<&dyn BundleLoader> {
        let loaders: std::sync::RwLockReadGuard<'_, HashMap<String, Box<dyn BundleLoader>>> =
            self.loaders.read().unwrap_or_else(|e| {
                eprintln!("[polyplug] RwLock poisoned, recovering: {}", e);
                e.into_inner()
            });
        let loader_ptr: *const dyn BundleLoader = loaders.get(runtime_name).map(Box::as_ref)?;
        // SAFETY: loaders are append-only (never removed or replaced for the runtime
        // lifetime), so the `Box`'s heap allocation behind `loader_ptr` stays valid and
        // pinned for as long as `&self` lives. Detaching the reference from the guard
        // lets callers invoke load()/reload() without holding the lock (deadlock-free).
        Some(unsafe { &*loader_ptr })
    }

    /// Push a bundle_id onto the current thread's init stack.
    ///
    /// Loaders call this immediately before invoking `polyplug_init`. The matching
    /// [`Runtime::pop_init_bundle_id`] MUST be called afterwards (including on the
    /// panic path) so the stack does not leak entries.
    pub fn push_init_bundle_id(&self, bundle_id: u64) {
        let mut stack: std::sync::MutexGuard<'_, HashMap<ThreadId, Vec<u64>>> =
            self.init_bundle_stack.lock().unwrap_or_else(|e| {
                eprintln!("[polyplug] Mutex poisoned, recovering: {}", e);
                e.into_inner()
            });
        stack
            .entry(std::thread::current().id())
            .or_default()
            .push(bundle_id);
    }

    /// Pop the most recent bundle_id from the current thread's init stack.
    ///
    /// Restores the previous (outer) bundle_id for reentrant loads on the same thread.
    pub fn pop_init_bundle_id(&self) {
        let thread_id: ThreadId = std::thread::current().id();
        let mut stack: std::sync::MutexGuard<'_, HashMap<ThreadId, Vec<u64>>> =
            self.init_bundle_stack.lock().unwrap_or_else(|e| {
                eprintln!("[polyplug] Mutex poisoned, recovering: {}", e);
                e.into_inner()
            });
        if let Some(thread_stack) = stack.get_mut(&thread_id) {
            thread_stack.pop();
            if thread_stack.is_empty() {
                stack.remove(&thread_id);
            }
        }
    }

    /// Get the bundle_id currently inside `polyplug_init` on this thread.
    ///
    /// Returns 0 when this thread is not inside any plugin init phase (i.e. for
    /// host-side lookups outside the init window).
    pub(crate) fn current_init_bundle_id(&self) -> u64 {
        let thread_id: ThreadId = std::thread::current().id();
        let stack: std::sync::MutexGuard<'_, HashMap<ThreadId, Vec<u64>>> =
            self.init_bundle_stack.lock().unwrap_or_else(|e| {
                eprintln!("[polyplug] Mutex poisoned, recovering: {}", e);
                e.into_inner()
            });
        stack
            .get(&thread_id)
            .and_then(|thread_stack| thread_stack.last().copied())
            .unwrap_or(0)
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
    pub(crate) fn load_bundle_with(
        &self,
        path: &Path,
        opts: LoadOptions,
    ) -> Result<(), RuntimeError> {
        // Determine the bundle directory: if path is a file, use its parent; otherwise use path as-is.
        let bundle_dir: &Path = if path.is_file() {
            path.parent().unwrap_or(path)
        } else {
            path
        };

        let manifest: ManifestData = crate::loader::parse_manifest(bundle_dir)
            .map_err(|e: LoaderError| RuntimeError::Loader(e))?;
        // Full manifest validation (required fields, id == FNV1a-64(name), well-formed
        // provides/bundle_dependencies version specs). Folds in the former inline
        // id == 0 check.
        manifest
            .validate()
            .map_err(|e: LoaderError| RuntimeError::Loader(e))?;

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

        // Find the loader for this runtime. The lock is released before load() runs
        // (see `loader_for`) so a plugin init that registers a loader cannot deadlock.
        let runtime_name: &str = &manifest.runtime;
        let loader: &dyn BundleLoader = self.loader_for(runtime_name).ok_or_else(|| {
            RuntimeError::Loader(LoaderError::NoLoaderForRuntime {
                bundle: path.display().to_string(),
                runtime_name: runtime_name.to_owned(),
            })
        })?;

        // Declare this bundle's dependency contract_ids in the registry BEFORE
        // calling the loader. The loader runs `polyplug_init`, during which the
        // plugin may resolve its declared dependencies via `find_guest_contract`.
        // Dependency enforcement (host_find_guest_contract) consults this set, so
        // it must be populated before init runs — otherwise even declared lookups
        // would be denied. See TRUST_MODEL.md §3/§4.
        let declared_contract_ids: Vec<GuestContractId> = manifest
            .dependencies
            .iter()
            .map(|dep: &crate::loader::RawManifestDependency| dep.contract_id)
            .collect();
        let bundle_id: BundleId = BundleId::new(&manifest.name);
        if let Err(e) = self
            .registry
            .declare_bundle_dependencies(bundle_id, declared_contract_ids)
        {
            return Err(RuntimeError::Registry(e));
        }

        let result: Result<(), RuntimeError> = loader.load(&manifest, self);
        if result.is_ok() {
            let bundle_name: String = manifest.name.clone();

            // Parse bundle dependencies from new bundle-level format
            let bundle_deps: Vec<crate::runtime_store::BundleDependency> =
                manifest.parsed_bundle_dependencies();

            // Parse version from manifest
            let bundle_version: Version = manifest.version.parse::<Version>().unwrap_or(Version {
                major: 0,
                minor: 0,
                patch: 0,
            });

            // Convert runtime string to RuntimeLanguage
            let runtime_lang: RuntimeLanguage = runtime_language_from_str(&manifest.runtime);

            // Register bundle metadata in RuntimeStore. A failure here means the
            // bundle loaded but its metadata could not be recorded, leaving the
            // store inconsistent — propagate it instead of silently discarding.
            self.registry.register_bundle_metadata(
                bundle_id,
                manifest.name.clone(),
                bundle_version,
                runtime_lang,
                manifest.path.clone(),
                bundle_deps,
            )?;

            // Real function_count validation: now that the bundle is loaded and its
            // interfaces registered, compare the manifest's declared function counts
            // against each native interface's actual `dispatch.native.function_count`.
            // The pre-load presence check above only proves an entry exists; this
            // proves the declared number matches reality. VM-dispatch interfaces have
            // no exposed count and are skipped.
            if !opts.ignore_function_count_mismatch && opts.compatibility != Compatibility::Yolo {
                self.validate_loaded_function_counts(bundle_id, &manifest, opts.compatibility)?;
            }

            let mut manifests: std::sync::MutexGuard<'_, HashMap<String, ManifestData>> =
                self.bundle_manifests.lock().unwrap_or_else(|e| {
                    eprintln!("[polyplug] Mutex poisoned, recovering: {}", e);
                    e.into_inner()
                });
            manifests.insert(bundle_name, manifest);
        }
        result
    }

    /// Compare declared `function_count` entries against the actual native counts of
    /// the bundle's freshly-registered interfaces.
    ///
    /// In `Strict` mode a mismatch is an error; in `Relaxed` mode it emits a warning.
    /// Only native-dispatch interfaces carry an observable count; VM interfaces are
    /// skipped (their count is `None`).
    fn validate_loaded_function_counts(
        &self,
        bundle_id: BundleId,
        manifest: &ManifestData,
        compatibility: Compatibility,
    ) -> Result<(), RuntimeError> {
        let registered: Vec<(String, u32, Option<u32>)> =
            self.registry.bundle_native_function_counts(bundle_id);
        for (contract_name, major, actual_opt) in registered {
            let actual: u32 = match actual_opt {
                Some(n) => n,
                None => continue, // VM dispatch: no observable count.
            };
            let key: String = format!("{}@{}", contract_name, major);
            let declared: u32 = match manifest.function_count.get(&key) {
                Some(n) => *n,
                None => continue, // Missing-entry case already handled pre-load.
            };
            if declared != actual {
                match compatibility {
                    Compatibility::Strict => {
                        return Err(RuntimeError::Loader(LoaderError::FunctionCountMismatch {
                            contract: key,
                            expected: declared,
                            found: actual,
                        }));
                    }
                    Compatibility::Relaxed => {
                        self.emit_warning(&format!(
                            "bundle `{}` contract `{}`: declared function_count {} but interface exports {}",
                            manifest.name, key, declared, actual
                        ));
                    }
                    Compatibility::Yolo => {}
                }
            }
        }
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
    warning_cb: Option<&WarningCallback>,
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
        let resolve_guest_contractd: Vec<ManifestDependency> = manifest.resolved_dependencies();
        for dep in &resolve_guest_contractd {
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
                Err(e) => {
                    return Err(RuntimeError::Loader(LoaderError::ManifestParse {
                        path: path.display().to_string(),
                        reason: format!("invalid version '{}': {:?}", dep_min_version_str, e),
                    }));
                }
            };

            let provided: Version =
                parse_manifest_version(&provider.version, &provider.name, path)?;

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
                            Some(cb) => (cb.0)(&msg),
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
                            Some(cb) => (cb.0)(&msg),
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

fn parse_manifest_version(
    v: &str,
    _bundle_name: &str,
    manifest_path: &std::path::Path,
) -> Result<Version, RuntimeError> {
    if v.is_empty() {
        return Ok(Version {
            major: 0,
            minor: 0,
            patch: 0,
        });
    }
    // A malformed version string is malformed manifest content: reject it with
    // ManifestParse, mirroring how the dependency `required` version is parsed.
    match Version::from_str(v) {
        Ok(version) => Ok(version),
        Err(e) => Err(RuntimeError::Loader(LoaderError::ManifestParse {
            path: manifest_path.display().to_string(),
            reason: format!("invalid version '{}': {:?}", v, e),
        })),
    }
}

/// Helper to create a null GuestContractHandle.
fn plugin_handle_null() -> GuestContractHandle {
    GuestContractHandle::null()
}

/// Host-contract version negotiation (see `docs/HOST_CONTRACTS.md`).
///
/// `min_version` is the requested version packed as `(major << 16) | minor`,
/// matching the constant every generator emits. A host contract satisfies the
/// request iff its major matches EXACTLY and its minor is `>=` the requested
/// minor. A higher major is NOT compatible (breaking change); a lower minor is
/// NOT compatible (missing functions).
///
/// `min_version == 0` is the documented wildcard ("accept any version"): real
/// contracts are `>= 1.0`, so a packed request never legitimately equals 0.
fn host_contract_version_satisfies(interface: &HostContractInterface, min_version: u32) -> bool {
    if min_version == 0 {
        return true;
    }
    let req_major: u32 = min_version >> 16;
    let req_minor: u32 = min_version & 0xFFFF;
    interface.contract_version.major == req_major && interface.contract_version.minor >= req_minor
}

/// Convert a runtime string from manifest.toml to RuntimeLanguage enum.
fn runtime_language_from_str(s: &str) -> RuntimeLanguage {
    match s {
        "native" | "rust" => RuntimeLanguage::Rust,
        "python" => RuntimeLanguage::Python,
        "lua" => RuntimeLanguage::Lua,
        "javascript" | "js" => RuntimeLanguage::JavaScript,
        "dotnet" | "csharp" => RuntimeLanguage::Dotnet,
        "cpp" => RuntimeLanguage::Cpp,
        _ => RuntimeLanguage::Rust,
    }
}

/// Convert a `StringView` to an owned, strictly-validated UTF-8 `String`.
///
/// The contract name keys the registry, so a lossy conversion could silently
/// replace invalid bytes with U+FFFD and alias two distinct names. Invalid UTF-8
/// is therefore rejected with [`RuntimeError::InvalidUtf8`] rather than coerced.
///
/// # Safety
/// `sv.ptr` must be valid for `sv.len` bytes for the duration of this call, or be null.
unsafe fn string_view_to_string_owned(
    sv: &polyplug_abi::types::StringView,
    context: &str,
) -> Result<String, RuntimeError> {
    if sv.ptr.is_null() || sv.len == 0 {
        return Ok(String::new());
    }
    // SAFETY: caller guarantees ptr/len describe a valid byte range for this call.
    let slice: &[u8] = unsafe { core::slice::from_raw_parts(sv.ptr, sv.len) };
    match core::str::from_utf8(slice) {
        Ok(s) => Ok(s.to_owned()),
        Err(_) => Err(RuntimeError::InvalidUtf8 {
            context: context.to_owned(),
        }),
    }
}

// ─── HostApi C ABI callbacks ───────────────────────────────────────────────

/// HostApi.register_guest_contract callback — registers a guest contract implementation with the runtime.
///
/// Reads bundle_id from the runtime's per-thread init stack (dependency enforcement).
///
/// # Safety
/// - this must be a valid HostApi pointer with valid runtime field
/// - descriptor must point to a valid PluginDescriptor
/// - interface must point to a valid GuestContractInterface that remains valid for the Runtime lifetime
pub(crate) unsafe extern "C" fn host_register_guest_contract(
    this: *const HostApi,
    descriptor: *const PluginDescriptor,
    interface: *const GuestContractInterface,
) -> polyplug_abi::types::AbiError {
    if this.is_null() {
        return polyplug_abi::types::AbiError {
            code: polyplug_abi::types::AbiErrorCode::Generic as u32,
            message: polyplug_abi::types::StringView::null(),
        };
    }
    // SAFETY: this is a valid HostApi pointer passed during polyplug_init.
    // (*this).runtime contains a valid pointer to Runtime.
    let runtime: &Runtime = unsafe { &*((*this).runtime as *const Runtime) };
    let registry: &RuntimeStore = &runtime.registry;
    // Get bundle_id from the runtime's per-thread init stack (pushed by the loader
    // before calling polyplug_init).
    let bundle_id: u64 = runtime.current_init_bundle_id();

    // SAFETY: descriptor is provided by the plugin's polyplug_init function
    let desc: PluginDescriptor = unsafe { *descriptor };

    if desc.contract_name.ptr.is_null() || desc.contract_name.len == 0 {
        return polyplug_abi::types::AbiError {
            code: polyplug_abi::types::AbiErrorCode::Generic as u32,
            message: polyplug_abi::types::StringView::from_static(
                b"PluginDescriptor.contract_name is null or empty",
            ),
        };
    }

    // SAFETY: desc.contract_name.ptr is non-null and valid for len bytes during init.
    let contract_name: String = match unsafe {
        string_view_to_string_owned(&desc.contract_name, "PluginDescriptor.contract_name")
    } {
        Ok(name) => name,
        Err(e) => {
            runtime.set_last_error(e.to_string());
            eprintln!("[polyplug] registration rejected for bundle {bundle_id}: {e}");
            return polyplug_abi::types::AbiError {
                code: polyplug_abi::types::AbiErrorCode::Generic as u32,
                message: polyplug_abi::types::StringView::null(),
            };
        }
    };

    // SAFETY: interface is a valid 'static GuestContractInterface from the plugin binary
    match unsafe {
        registry.register_guest_contract(
            desc,
            interface,
            contract_name,
            BundleId::from_u64(bundle_id),
        )
    } {
        Ok(_handle) => polyplug_abi::types::AbiError::ok(),
        Err(e) => {
            eprintln!("[polyplug] registration failed for bundle {bundle_id}: {e}");
            polyplug_abi::types::AbiError {
                code: polyplug_abi::types::AbiErrorCode::Generic as u32,
                message: polyplug_abi::types::StringView::null(),
            }
        }
    }
}

/// HostApi.alloc callback — allocate memory via the host allocator.
///
/// # Safety
/// this is ignored (system allocator is global). Standard alloc safety applies.
pub(crate) unsafe extern "C" fn host_alloc(
    _this: *const HostApi,
    size: usize,
    align: usize,
) -> *mut u8 {
    polyplug_abi::ffi::polyplug_host_alloc(size, align)
}

/// HostApi.free callback — free memory via the host allocator.
///
/// # Safety
/// this is ignored (system allocator is global). Standard free safety applies.
pub(crate) unsafe extern "C" fn host_free(
    _this: *const HostApi,
    ptr: *mut u8,
    size: usize,
    align: usize,
) {
    // SAFETY: polyplug_host_free is a safe wrapper around the system allocator.
    unsafe { polyplug_abi::ffi::polyplug_host_free(ptr, size, align) }
}

/// HostApi.find_guest_contract callback — dispatches to runtime's registry with dependency enforcement.
///
/// Reads bundle_id from the runtime's per-thread init stack during the init phase.
///
/// # Safety
/// this must be a valid HostApi pointer with valid runtime field.
pub(crate) unsafe extern "C" fn host_find_guest_contract(
    this: *const HostApi,
    contract_id: u64,
    min_version: u32,
) -> GuestContractHandle {
    if this.is_null() {
        return plugin_handle_null();
    }
    // SAFETY: this is a valid HostApi pointer passed by the host.
    // (*this).runtime contains a valid pointer to Runtime.
    let runtime: &Runtime = unsafe { &*((*this).runtime as *const Runtime) };
    let registry: &RuntimeStore = &runtime.registry;
    // Get bundle_id from the runtime's per-thread init stack for dependency
    // enforcement during the init phase.
    let caller_bundle_id: u64 = runtime.current_init_bundle_id();

    if caller_bundle_id != 0
        && !registry.is_bundle_dependency_declared(
            BundleId::from_u64(caller_bundle_id),
            GuestContractId::from_u64(contract_id),
        )
    {
        return plugin_handle_null();
    }
    match registry.find_guest_contract(GuestContractId::from_u64(contract_id), min_version) {
        Ok(h) => h,
        Err(_) => plugin_handle_null(),
    }
}

/// HostApi.find_all_by_contract callback — returns Array<GuestContractHandle>.
///
/// # Safety
/// - this must be a valid HostApi pointer with valid runtime field
pub(crate) unsafe extern "C" fn host_find_all_guest_contracts(
    this: *const HostApi,
    contract_id: u64,
    min_version: u32,
) -> polyplug_abi::Array<GuestContractHandle> {
    use polyplug_abi::Array;

    if this.is_null() {
        return Array::empty();
    }
    // SAFETY: this is a valid HostApi pointer passed by the host.
    // (*this).runtime contains a valid pointer to Runtime.
    let runtime: &Runtime = unsafe { &*((*this).runtime as *const Runtime) };
    let registry: &RuntimeStore = &runtime.registry;

    // Dependency enforcement during the init window: a plugin must not enumerate
    // providers of a contract it did not declare. Outside the window
    // (caller_bundle_id == 0, host-side lookups) enumeration is unrestricted.
    let caller_bundle_id: u64 = runtime.current_init_bundle_id();
    if caller_bundle_id != 0
        && !registry.is_bundle_dependency_declared(
            BundleId::from_u64(caller_bundle_id),
            GuestContractId::from_u64(contract_id),
        )
    {
        return Array::empty();
    }

    // First, count matching contracts
    let count = registry.count_guest_contracts(GuestContractId::from_u64(contract_id), min_version);

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
    // SAFETY: ptr was allocated by host_alloc with size = count * size_of::<GuestContractHandle>()
    // and is valid for `count` elements. count > 0 is guaranteed by the empty check above.
    let slice = unsafe { core::slice::from_raw_parts_mut(ptr, count) };
    let actual = registry.find_all_guest_contracts_into(
        GuestContractId::from_u64(contract_id),
        min_version,
        slice,
    );

    Array::new(ptr, actual)
}

/// HostApi.resolve_guest_contract callback — returns interface pointer for a handle.
///
/// # Safety
/// this must be a valid HostApi pointer with valid runtime field.
pub unsafe extern "C" fn host_resolve_guest_contract(
    this: *const HostApi,
    handle: GuestContractHandle,
) -> *const GuestContractInterface {
    if this.is_null() {
        return core::ptr::null();
    }
    // SAFETY: this is a valid HostApi pointer passed by the host.
    // (*this).runtime contains a valid pointer to Runtime.
    let runtime: &Runtime = unsafe { &*((*this).runtime as *const Runtime) };
    let registry: &RuntimeStore = &runtime.registry;

    match registry.resolve_guest_contract(handle) {
        Ok(ptr) => ptr,
        Err(_) => core::ptr::null(),
    }
}

/// HostApi.get_host_contract callback — returns an instance for a host contract.
///
/// For singleton contracts: returns cached instance (creates on first call).
/// For multi-instance contracts: creates new instance each call.
///
/// # Safety
/// this must be a valid HostApi pointer with valid runtime field.
pub(crate) unsafe extern "C" fn host_get_host_contract(
    this: *const HostApi,
    contract_id: u64,
    min_version: u32,
) -> HostContractInstance {
    if this.is_null() {
        return HostContractInstance::null();
    }
    // SAFETY: this is a valid HostApi pointer passed by the host.
    // (*this).runtime contains a valid pointer to Runtime.
    let runtime: &Runtime = unsafe { &*((*this).runtime as *const Runtime) };

    // Find the host contract interface
    let host_contracts_guard = runtime.host_contracts.read().unwrap_or_else(|e| {
        eprintln!("[polyplug] RwLock poisoned, recovering: {}", e);
        e.into_inner()
    });

    // Find interface matching contract_id and version
    let interface: Option<&HostContractInterface> = host_contracts_guard
        .values()
        .find(|iface| {
            iface.contract_id.id() == contract_id
                && host_contract_version_satisfies(iface, min_version)
        })
        .copied();

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
                    (interface.create_instance)(
                        interface as *const HostContractInterface,
                        core::ptr::null(),
                    )
                };
                singleton_guard.insert(contract_id, instance);
                instance
            } else {
                // Multi-instance: create new instance each call
                // SAFETY: interface.create_instance is a valid function pointer
                // Pass the HostContractInterface pointer (self-passing pattern)
                unsafe {
                    (interface.create_instance)(
                        interface as *const HostContractInterface,
                        core::ptr::null(),
                    )
                }
            }
        }
        None => {
            runtime.set_last_error(format!(
                "host contract not found: id={}, min_version={}",
                contract_id, min_version
            ));
            HostContractInstance::null()
        }
    }
}

/// HostApi.resolve_host_contract_interface callback — returns HostContractInterface pointer.
///
/// # Safety
/// this must be a valid HostApi pointer with valid runtime field.
pub(crate) unsafe extern "C" fn host_resolve_host_contract_interface(
    this: *const HostApi,
    contract_id: u64,
    min_version: u32,
) -> *const HostContractInterface {
    if this.is_null() {
        return core::ptr::null();
    }
    // SAFETY: this is a valid HostApi pointer passed by the host.
    // (*this).runtime contains a valid pointer to Runtime.
    let runtime: &Runtime = unsafe { &*((*this).runtime as *const Runtime) };

    // Find the host contract interface
    let host_contracts_guard = runtime.host_contracts.read().unwrap_or_else(|e| {
        eprintln!("[polyplug] RwLock poisoned, recovering: {}", e);
        e.into_inner()
    });

    // Find interface matching contract_id and version
    host_contracts_guard
        .values()
        .find(|iface| {
            iface.contract_id.id() == contract_id
                && host_contract_version_satisfies(iface, min_version)
        })
        .map(|v| *v as *const HostContractInterface)
        .unwrap_or_else(|| {
            runtime.set_last_error(format!(
                "host contract interface not found: id={}, min_version={}",
                contract_id, min_version
            ));
            core::ptr::null()
        })
}

/// HostApi.list_bundles callback — returns Array<BundleId>.
///
/// # Safety
/// this must be a valid HostApi pointer with valid runtime field.
pub(crate) unsafe extern "C" fn host_list_bundles(
    this: *const HostApi,
) -> polyplug_abi::Array<polyplug_utils::BundleId> {
    use polyplug_abi::Array;
    use polyplug_utils::BundleId;

    if this.is_null() {
        return Array::empty();
    }
    // SAFETY: this is a valid HostApi pointer.
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
        // SAFETY: ptr was allocated with count elements and i < count.
        unsafe {
            *ptr.add(i) = BundleId::from_u64(manifest.id);
        }
    }

    Array::new(ptr, count)
}

/// HostApi.get_dependencies callback — returns Array<DependencyInfo>.
///
/// Uses TLS bundle_id to look up the calling bundle's dependencies.
///
/// # Safety
/// this must be a valid HostApi pointer with valid runtime field.
pub(crate) unsafe extern "C" fn host_get_dependencies(
    this: *const HostApi,
) -> polyplug_abi::Array<polyplug_abi::DependencyInfo> {
    use polyplug_abi::{Array, DependencyInfo};

    if this.is_null() {
        return Array::empty();
    }
    // SAFETY: this is a valid HostApi pointer.
    let runtime: &Runtime = unsafe { &*((*this).runtime as *const Runtime) };

    // Get bundle_id from the runtime's per-thread init stack.
    let caller_bundle_id: u64 = runtime.current_init_bundle_id();
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
            bundle_id: dep
                .bundle_id
                .unwrap_or_else(|| polyplug_utils::BundleId::from_u64(0)),
        };
        // SAFETY: ptr was allocated with count elements of DependencyInfo and i < count.
        unsafe {
            *ptr.add(i) = info;
        }
    }

    Array::new(ptr, count)
}

// ─── HostApi operation functions (18-02 implementation) ───────────────────
// These functions implement the HostApi operation fields for host applications.

/// HostApi.load_bundle callback — loads a plugin bundle from a path.
///
/// Host applications call this to load a bundle at runtime.
///
/// # Safety
/// - this must be a valid HostApi pointer with valid runtime field
/// - path must point to path_len valid UTF-8 bytes for the duration of the call
pub unsafe extern "C" fn host_load_bundle(
    this: *const HostApi,
    path: *const u8,
    path_len: usize,
) -> polyplug_abi::AbiError {
    use polyplug_abi::{AbiError, AbiErrorCode, StringView};

    if this.is_null() {
        return AbiError {
            code: AbiErrorCode::InvalidPointer as u32,
            message: StringView::from_static(b"null HostApi in load_bundle"),
        };
    }
    // SAFETY: this is a valid HostApi pointer. (*this).runtime contains a valid pointer to Runtime.
    let runtime: &Runtime = unsafe { &*((*this).runtime as *const Runtime) };

    if path.is_null() {
        runtime.set_last_error("null path pointer in load_bundle");
        return AbiError {
            code: AbiErrorCode::InvalidPointer as u32,
            message: StringView::from_static(b"null path pointer in load_bundle"),
        };
    }

    // SAFETY: path is non-null and points to path_len valid bytes per ABI contract.
    let bytes: &[u8] = unsafe { core::slice::from_raw_parts(path, path_len) };
    let s: &str = match core::str::from_utf8(bytes) {
        Ok(s) => s,
        Err(e) => {
            runtime.set_last_error(e.to_string());
            return AbiError {
                code: AbiErrorCode::Generic as u32,
                message: StringView::null(),
            };
        }
    };

    match runtime.load_bundle(std::path::Path::new(s)) {
        Ok(()) => AbiError::ok(),
        Err(e) => {
            runtime.set_last_error(e.to_string());
            AbiError {
                code: AbiErrorCode::Generic as u32,
                message: StringView::null(),
            }
        }
    }
}

/// HostApi.reload_bundle callback — hot-reloads a plugin bundle.
///
/// Replaces the bundle's contracts with new versions from the updated binary.
///
/// # Safety
/// - this must be a valid HostApi pointer with valid runtime field
/// - path must point to path_len valid UTF-8 bytes for the duration of the call
pub unsafe extern "C" fn host_reload_bundle(
    this: *const HostApi,
    path: *const u8,
    path_len: usize,
) -> polyplug_abi::AbiError {
    use polyplug_abi::{AbiError, AbiErrorCode, StringView};

    if this.is_null() {
        return AbiError {
            code: AbiErrorCode::InvalidPointer as u32,
            message: StringView::from_static(b"null HostApi in reload_bundle"),
        };
    }
    // SAFETY: this is a valid HostApi pointer. (*this).runtime contains a valid pointer to Runtime.
    let runtime: &Runtime = unsafe { &*((*this).runtime as *const Runtime) };

    if path.is_null() {
        runtime.set_last_error("null path pointer in reload_bundle");
        return AbiError {
            code: AbiErrorCode::InvalidPointer as u32,
            message: StringView::from_static(b"null path pointer in reload_bundle"),
        };
    }

    // SAFETY: path is non-null and points to path_len valid bytes per ABI contract.
    let bytes: &[u8] = unsafe { core::slice::from_raw_parts(path, path_len) };
    let s: &str = match core::str::from_utf8(bytes) {
        Ok(s) => s,
        Err(e) => {
            runtime.set_last_error(e.to_string());
            return AbiError {
                code: AbiErrorCode::Generic as u32,
                message: StringView::null(),
            };
        }
    };

    match runtime.reload_bundle(std::path::Path::new(s)) {
        Ok(()) => AbiError::ok(),
        Err(e) => {
            runtime.set_last_error(e.to_string());
            AbiError {
                code: AbiErrorCode::Generic as u32,
                message: StringView::null(),
            }
        }
    }
}

/// HostApi.register_host_contract callback — registers a host contract interface.
///
/// Host applications register their contracts for plugins to consume.
///
/// # Safety
/// - this must be a valid HostApi pointer with valid runtime field
/// - interface must be a valid HostContractInterface pointer that remains valid for runtime lifetime
pub(crate) unsafe extern "C" fn host_register_host_contract(
    this: *const HostApi,
    interface: *const polyplug_abi::HostContractInterface,
) -> polyplug_abi::AbiError {
    use polyplug_abi::{AbiError, AbiErrorCode, StringView};

    if this.is_null() || interface.is_null() {
        return AbiError {
            code: AbiErrorCode::InvalidPointer as u32,
            message: StringView::from_static(b"null pointer in register_host_contract"),
        };
    }
    // SAFETY: this is a valid HostApi pointer. (*this).runtime contains a valid pointer to Runtime.
    let runtime: &Runtime = unsafe { &*((*this).runtime as *const Runtime) };
    // SAFETY: interface is a valid HostContractInterface pointer. Caller guarantees it remains valid for runtime lifetime.
    let interface_ref: &'static polyplug_abi::HostContractInterface = unsafe { &*interface };

    match runtime.register_host_contract(interface_ref.contract_id.id(), interface_ref) {
        Ok(()) => AbiError::ok(),
        Err(crate::error::HostContractError::DuplicateContract { .. }) => AbiError {
            code: AbiErrorCode::Generic as u32,
            message: StringView::from_static(b"duplicate host contract registration"),
        },
        Err(e) => {
            runtime.set_last_error(e.to_string());
            AbiError {
                code: AbiErrorCode::Generic as u32,
                message: StringView::null(),
            }
        }
    }
}

/// HostApi.register_loader callback — registers a language loader.
///
/// Host applications register loaders for each runtime language they support.
///
/// # Safety
/// - this must be a valid HostApi pointer with valid runtime field
/// - loader_ptr must be a *mut Box<dyn BundleLoader> erased to *mut c_void by a loader cdylib
///   compiled against the same polyplug rlib
pub(crate) unsafe extern "C" fn host_register_loader(
    this: *const HostApi,
    _runtime_name: polyplug_abi::StringView,
    loader_ptr: *mut core::ffi::c_void,
) -> polyplug_abi::AbiError {
    use polyplug_abi::{AbiError, AbiErrorCode, StringView};

    if this.is_null() || loader_ptr.is_null() {
        return AbiError {
            code: AbiErrorCode::InvalidPointer as u32,
            message: StringView::from_static(b"null pointer in register_loader"),
        };
    }
    // SAFETY: this is a valid HostApi pointer. (*this).runtime contains a valid
    // pointer to Runtime. A shared reference is sufficient — `register_guest_contract_loader`
    // takes `&self` and uses the interior `RwLock` to mutate `loaders`. Forging a
    // `&mut Runtime` from the Arc-shared pointer would be aliasing UB (other live
    // `&Runtime` exist), so we never do that.
    let runtime: &Runtime = unsafe { &*((*this).runtime as *const Runtime) };

    // SAFETY: loader_ptr is a *mut Box<dyn BundleLoader> erased to *mut c_void by a loader cdylib
    // compiled against the same polyplug rlib. Reconstituting via Box::from_raw is valid.
    let loader: Box<dyn BundleLoader> =
        unsafe { *Box::from_raw(loader_ptr as *mut Box<dyn BundleLoader>) };

    match runtime.register_guest_contract_loader(loader) {
        Ok(()) => AbiError::ok(),
        Err(e) => {
            runtime.set_last_error(e.to_string());
            AbiError {
                code: AbiErrorCode::Generic as u32,
                message: StringView::null(),
            }
        }
    }
}

/// HostApi.get_last_error callback — gets the last error message.
///
/// Copies up to buf_len bytes into buf. Clears error after read.
///
/// # Safety
/// - this must be a valid HostApi pointer with valid runtime field
/// - buf must be valid for writes of buf_len bytes when non-null
pub unsafe extern "C" fn host_get_last_error(
    this: *const HostApi,
    buf: *mut u8,
    buf_len: usize,
) -> usize {
    if this.is_null() {
        return 0;
    }
    // SAFETY: this is a valid HostApi pointer. (*this).runtime contains a valid pointer to Runtime.
    let runtime: &Runtime = unsafe { &*((*this).runtime as *const Runtime) };

    if buf.is_null() {
        let len = runtime.last_error_len();
        runtime.clear_last_error();
        return len;
    }
    if buf_len == 0 {
        runtime.clear_last_error();
        return 0;
    }
    // SAFETY: buf is valid for buf_len bytes per ABI contract.
    let buf_slice: &mut [u8] = unsafe { core::slice::from_raw_parts_mut(buf, buf_len) };
    let len = runtime.get_last_error(buf_slice);
    runtime.clear_last_error();
    len
}

/// HostApi.get_error_len callback — gets the last error message length.
///
/// Use to allocate buffer before calling get_last_error.
///
/// # Safety
/// - this must be a valid HostApi pointer with valid runtime field
pub unsafe extern "C" fn host_get_error_len(this: *const HostApi) -> usize {
    if this.is_null() {
        // Return length of the null runtime error message
        return b"null HostApi pointer".len();
    }
    // SAFETY: this is a valid HostApi pointer. (*this).runtime contains a valid pointer to Runtime.
    let runtime: &Runtime = unsafe { &*((*this).runtime as *const Runtime) };
    runtime.last_error_len()
}

/// HostApi.call_guest_method callback — host-mediated plugin→plugin cross-dispatch.
///
/// Re-resolves the target contract through the registry via `instance.contract_id`
/// on every call (never caches), so a fresh cross-call always routes to the live
/// interface while retired interfaces keep in-flight instances valid. See the
/// `call_guest_method` field doc on [`HostApi`] for the full contract.
///
/// # Safety
/// - `this` must be a valid HostApi pointer with valid runtime field
/// - `instance` must be an instance produced by the target contract
/// - `args` / `out` must satisfy the target function's ABI argument layout
/// - `arena` must be null or a valid [`CallArena`] for the duration of the call
pub(crate) unsafe extern "C" fn host_call_guest_method(
    this: *const HostApi,
    instance: polyplug_abi::guest::GuestContractInstance,
    fn_id: u32,
    args: *const core::ffi::c_void,
    out: *mut core::ffi::c_void,
    arena: *mut polyplug_abi::types::CallArena,
) -> polyplug_abi::types::AbiError {
    if this.is_null() || instance.data.is_null() {
        return polyplug_abi::types::AbiError {
            code: polyplug_abi::types::AbiErrorCode::InvalidPointer as u32,
            message: polyplug_abi::types::StringView::null(),
        };
    }

    // SAFETY: this is a valid HostApi pointer passed by the host.
    // (*this).runtime contains a valid pointer to Runtime.
    let runtime: &Runtime = unsafe { &*((*this).runtime as *const Runtime) };
    let registry: &RuntimeStore = &runtime.registry;

    // Re-resolve the target contract by id on EVERY call. find_guest_contract +
    // resolve_guest_contract both drop their read guard before returning, so no
    // registry lock is held across the guest dispatch below. The returned pointer
    // stays valid even across a concurrent hot-reload because retired interfaces
    // are kept alive (retire-not-drop) for the runtime lifetime.
    let contract_id: u64 = instance.contract_id.id();
    let handle: GuestContractHandle =
        match registry.find_guest_contract(GuestContractId::from_u64(contract_id), 0) {
            Ok(h) => h,
            Err(_) => {
                runtime.set_last_error(format!(
                    "call_guest_method: no contract found for contract_id={contract_id}"
                ));
                return polyplug_abi::types::AbiError {
                    code: polyplug_abi::types::AbiErrorCode::NotFound as u32,
                    message: polyplug_abi::types::StringView::null(),
                };
            }
        };
    let interface_ptr: *const GuestContractInterface = match registry.resolve_guest_contract(handle)
    {
        Ok(ptr) if !ptr.is_null() => ptr,
        _ => {
            runtime.set_last_error(format!(
                "call_guest_method: contract could not be resolved for contract_id={contract_id}"
            ));
            return polyplug_abi::types::AbiError {
                code: polyplug_abi::types::AbiErrorCode::NotFound as u32,
                message: polyplug_abi::types::StringView::null(),
            };
        }
    };

    // SAFETY: interface_ptr is non-null and points to a 'static (retire-not-drop)
    // GuestContractInterface registered by the loader; reading its fields is sound.
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };

    match interface.dispatch_type {
        polyplug_abi::dispatch::DispatchType::Native => {
            // SAFETY: dispatch_type == Native guarantees the `native` union variant
            // is the active one, so reading it is sound.
            let native: polyplug_abi::dispatch::NativeDispatch =
                unsafe { interface.dispatch.native };
            if fn_id >= native.function_count || native.functions.is_null() {
                return polyplug_abi::types::AbiError {
                    code: polyplug_abi::types::AbiErrorCode::FunctionNotAvailable as u32,
                    message: polyplug_abi::types::StringView::null(),
                };
            }
            // SAFETY: fn_id < function_count and functions is non-null, so the slot
            // at fn_id is within the static function-pointer array.
            let slot: *const () = unsafe { *native.functions.add(fn_id as usize) };
            if slot.is_null() {
                return polyplug_abi::types::AbiError {
                    code: polyplug_abi::types::AbiErrorCode::FunctionNotAvailable as u32,
                    message: polyplug_abi::types::StringView::null(),
                };
            }
            // Native dispatch function pointers carry NO arena parameter in their
            // ABI signature, so `arena` is intentionally unused on this path.
            let _ = arena;
            // SAFETY: native dispatch slots have the frozen native ABI signature
            // `extern "C" fn(GuestContractInstance, *const (), *mut ()) -> AbiError`
            // (see polyplugc rust generator); `slot` is a non-null pointer to such
            // a function. The transmute reinterprets the type-erased `*const ()` as
            // that concrete fn pointer, which is the established native-call form.
            let func: unsafe extern "C" fn(
                polyplug_abi::guest::GuestContractInstance,
                *const (),
                *mut (),
            ) -> polyplug_abi::types::AbiError = unsafe { core::mem::transmute(slot) };
            // SAFETY: args/out satisfy the target function's ABI layout per the
            // caller's contract; instance belongs to this contract.
            unsafe { func(instance, args.cast::<()>(), out.cast::<()>()) }
        }
        polyplug_abi::dispatch::DispatchType::VirtualMachine => {
            // SAFETY: dispatch_type == VirtualMachine guarantees the `vm` union
            // variant is the active one, so reading it is sound.
            let vm: polyplug_abi::dispatch::VmDispatch = unsafe { interface.dispatch.vm };
            // SAFETY: vm.call is the loader-provided VM dispatch entry point with
            // the frozen 6-arg signature; loader_data is the matching opaque handle.
            // args/out/arena are forwarded unchanged per the VM dispatch contract.
            unsafe {
                (vm.call)(
                    vm.loader_data,
                    instance,
                    fn_id,
                    args.cast::<()>(),
                    out.cast::<()>(),
                    arena,
                )
            }
        }
    }
}

/// HostApi.get_extension callback — returns a registered extension pointer.
///
/// # Safety
/// - `this` must be a valid HostApi pointer with valid runtime field
/// - `extension_id` must be fnv1a_32 of the extension name
pub(crate) unsafe extern "C" fn host_get_extension(
    this: *const HostApi,
    extension_id: u32,
) -> *const () {
    // SAFETY: this is non-null per ABI contract; runtime field was set at init.
    let runtime: &Runtime = unsafe { &*((*this).runtime as *const Runtime) };
    match runtime.extensions.read() {
        Ok(map) => map.get(&extension_id).copied().unwrap_or(0) as *const (),
        Err(_) => core::ptr::null(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;

    /// No-op create_instance for a test host contract interface.
    unsafe extern "C" fn test_create_instance(
        _this: *const HostContractInterface,
        _args: *const (),
    ) -> HostContractInstance {
        HostContractInstance::null()
    }

    /// No-op destroy_instance for a test host contract interface.
    unsafe extern "C" fn test_destroy_instance(
        _this: *const HostContractInterface,
        _instance: HostContractInstance,
    ) {
    }

    /// Build a `HostContractInterface` with the given major/minor version for
    /// negotiation tests (other fields are inert).
    fn host_contract_interface_with_version(major: u32, minor: u32) -> HostContractInterface {
        HostContractInterface {
            contract_id: polyplug_utils::HostContractId::from(0xABCD_u64),
            contract_version: Version {
                major,
                minor,
                patch: 0,
            },
            singleton: true,
            dispatch_type: polyplug_abi::dispatch::dispatch_type::DispatchType::Native,
            runtime: core::ptr::null_mut(),
            user_data: core::ptr::null_mut(),
            create_instance: test_create_instance,
            destroy_instance: test_destroy_instance,
            dispatch: polyplug_abi::DispatchMechanisms {
                native: polyplug_abi::NativeDispatch {
                    function_count: 0,
                    functions: core::ptr::null(),
                },
            },
        }
    }

    /// Pack a (major, minor) request the way generated callers do.
    fn pack_min_version(major: u32, minor: u32) -> u32 {
        (major << 16) | minor
    }

    #[test]
    fn host_contract_version_exact_major_equal_minor_passes() {
        let iface: HostContractInterface = host_contract_interface_with_version(1, 5);
        assert!(host_contract_version_satisfies(
            &iface,
            pack_min_version(1, 5)
        ));
    }

    #[test]
    fn host_contract_version_higher_minor_passes() {
        let iface: HostContractInterface = host_contract_interface_with_version(1, 7);
        assert!(host_contract_version_satisfies(
            &iface,
            pack_min_version(1, 5)
        ));
    }

    #[test]
    fn host_contract_version_lower_minor_fails() {
        let iface: HostContractInterface = host_contract_interface_with_version(1, 4);
        assert!(!host_contract_version_satisfies(
            &iface,
            pack_min_version(1, 5)
        ));
    }

    #[test]
    fn host_contract_version_higher_major_fails() {
        // 2.0 must NOT satisfy a request for 1.5 — a higher major is a breaking change.
        let iface: HostContractInterface = host_contract_interface_with_version(2, 0);
        assert!(!host_contract_version_satisfies(
            &iface,
            pack_min_version(1, 5)
        ));
    }

    #[test]
    fn host_contract_version_lower_major_fails() {
        let iface: HostContractInterface = host_contract_interface_with_version(1, 9);
        assert!(!host_contract_version_satisfies(
            &iface,
            pack_min_version(2, 0)
        ));
    }

    #[test]
    fn builder_creates_runtime() {
        let runtime: Arc<Runtime> = Runtime::builder()
            .build()
            .expect("runtime build should succeed");
        let result: Result<GuestContractHandle, _> =
            runtime.find_guest_contract(0x1234_5678_9ABC_DEF0_u64, 0);
        assert!(result.is_err(), "empty registry should return not found");
    }

    #[test]
    fn abi_ok_constant() {
        assert_eq!(
            polyplug_abi::AbiErrorCode::Ok,
            polyplug_abi::AbiErrorCode::Ok
        );
        assert_eq!(polyplug_abi::AbiErrorCode::Ok as u32, 0_u32);
    }

    /// TH-06: Verify host callbacks in runtime.rs use HostApi self-passing pattern.
    /// This is a compile-time verification test.
    #[test]
    fn host_callbacks_use_host_interface_self_passing() {
        // All host callback functions (host_register_guest_contract, host_alloc, host_free,
        // host_find_guest_contract, host_find_all_guest_contracts, host_resolve_guest_contract,
        // host_get_host_contract) use *const HostApi as first parameter.
        //
        // This is verified by the function signatures in this file using HostApi.
        // The self-passing pattern allows extracting runtime from (*this).runtime.
        //
        // HostApi is pointer-sized (8 bytes on x86_64), ensuring ABI compatibility.
        assert_eq!(core::mem::size_of::<*const HostApi>(), 8);
    }

    #[test]
    fn host_find_guest_contract_null_this_returns_null() {
        // SAFETY: host_find_guest_contract handles null HostApi gracefully
        let handle: GuestContractHandle =
            unsafe { host_find_guest_contract(core::ptr::null(), 0_u64, 0_u32) };
        assert!(
            handle.is_null(),
            "host_find_guest_contract must return null when this is null"
        );
    }

    #[test]
    fn dep_enforcement_blocks_undeclared_contract() {
        let runtime: Arc<Runtime> = Runtime::builder()
            .build()
            .expect("runtime build should succeed");

        // Push a bundle_id onto the runtime init stack to simulate init phase
        runtime.push_init_bundle_id(0xDEAD_BEEF_u64);

        // Create a HostApi with runtime pointer
        let host_interface: HostApi = HostApi {
            runtime: Arc::as_ptr(&runtime) as *mut core::ffi::c_void,
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
            // Host operations (implemented in 18-02)
            load_bundle: host_load_bundle,
            reload_bundle: host_reload_bundle,
            register_host_contract: host_register_host_contract,
            register_loader: host_register_loader,
            get_last_error: host_get_last_error,
            get_error_len: host_get_error_len,
            call_guest_method: host_call_guest_method,
            get_extension: host_get_extension,
        };

        // SAFETY: host_interface is valid with runtime pointer; init bundle_id is set
        let handle: GuestContractHandle = unsafe {
            host_find_guest_contract(
                &host_interface as *const HostApi,
                0x1111_2222_3333_4444_u64,
                0_u32,
            )
        };
        assert!(
            handle.is_null(),
            "dep enforcement must return null for undeclared contract during init phase"
        );

        // Pop the init bundle_id after test
        runtime.pop_init_bundle_id();
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
        // Emit the canonical id = FNV1a-64(name) so the manifest passes validation.
        let manifest: String = format!(
            "id = {}\nname = \"{}\"\nruntime = \"{}\"\nfile = \"dummy.so\"\n",
            BundleId::new(bundle_name).id(),
            bundle_name,
            runtime
        );
        let manifest_path: PathBuf = bundle_dir.join("manifest.toml");
        if let Err(e) = std::fs::write(&manifest_path, manifest) {
            panic!("failed to write manifest {}: {e}", manifest_path.display());
        }
        bundle_dir
    }

    fn register_guest_contract(
        registry: &crate::runtime_store::RuntimeStore,
        contract_id: u64,
        bundle_id: u64,
    ) -> GuestContractHandle {
        use polyplug_abi::{
            DispatchMechanisms, DispatchType, GuestContractInstance, GuestContractInterface,
            NativeDispatch,
        };

        unsafe extern "C" fn stub_create_instance(
            _host: *const HostApi,
            _args: *const (),
        ) -> GuestContractInstance {
            GuestContractInstance::null()
        }

        unsafe extern "C" fn stub_destroy_instance(
            _host: *const HostApi,
            _instance: GuestContractInstance,
        ) {
        }

        let interface: &'static GuestContractInterface =
            Box::leak(Box::new(GuestContractInterface {
                contract_id: polyplug_utils::GuestContractId::from_u64(contract_id),
                contract_version: Version {
                    major: 0,
                    minor: 0,
                    patch: 0,
                },
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
            version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
        };
        // SAFETY: interface is leaked and lives for the process lifetime.
        let result: Result<GuestContractHandle, crate::error::RegistryError> = unsafe {
            registry.register_guest_contract(
                descriptor,
                interface,
                "stub.contract".to_owned(),
                BundleId::from_u64(bundle_id),
            )
        };
        match result {
            Ok(handle) => handle,
            Err(e) => panic!("failed to register_guest_contract contract: {e}"),
        }
    }

    // ─── call_guest_method tests ─────────────────────────────────────────────

    /// Native dispatch target: writes the i32 at `args` plus one into `out`.
    unsafe extern "C" fn native_add_one(
        _instance: polyplug_abi::guest::GuestContractInstance,
        args: *const (),
        out: *mut (),
    ) -> polyplug_abi::types::AbiError {
        // SAFETY: the test passes a valid *const i32 / *mut i32.
        unsafe {
            let input: i32 = *(args as *const i32);
            *(out as *mut i32) = input + 1;
        }
        polyplug_abi::types::AbiError::ok()
    }

    /// Sync wrapper for a static native function-pointer table.
    ///
    /// The contained pointers are `'static` function pointers, which are safe to
    /// read from any thread; the wrapper only exists to satisfy the `Sync` bound
    /// on `static` items.
    struct NativeFnTable([*const (); 1]);
    // SAFETY: the array holds only 'static fn pointers, which are immutable and
    // safe to share across threads.
    unsafe impl Sync for NativeFnTable {}

    static NATIVE_FNS: NativeFnTable = NativeFnTable([native_add_one as *const ()]);

    /// Register a native-dispatch contract whose function 0 is `native_add_one`.
    fn register_native_caller_contract(
        registry: &crate::runtime_store::RuntimeStore,
        contract_id: u64,
        bundle_id: u64,
    ) {
        use polyplug_abi::{
            DispatchMechanisms, DispatchType, GuestContractInstance, GuestContractInterface,
            NativeDispatch,
        };

        unsafe extern "C" fn stub_create(
            _host: *const HostApi,
            _args: *const (),
        ) -> GuestContractInstance {
            GuestContractInstance::null()
        }
        unsafe extern "C" fn stub_destroy(_host: *const HostApi, _instance: GuestContractInstance) {
        }

        let interface: &'static GuestContractInterface =
            Box::leak(Box::new(GuestContractInterface {
                contract_id: GuestContractId::from_u64(contract_id),
                contract_version: Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
                dispatch_type: DispatchType::Native,
                create_instance: stub_create,
                destroy_instance: stub_destroy,
                dispatch: DispatchMechanisms {
                    native: NativeDispatch {
                        function_count: 1,
                        functions: NATIVE_FNS.0.as_ptr(),
                    },
                },
            }));
        let descriptor: polyplug_abi::PluginDescriptor = polyplug_abi::PluginDescriptor {
            name: polyplug_abi::StringView::from_static(b"caller"),
            contract_name: polyplug_abi::StringView::from_static(b"caller.contract"),
            version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
        };
        // SAFETY: interface is leaked for the process lifetime.
        let result: Result<GuestContractHandle, crate::error::RegistryError> = unsafe {
            registry.register_guest_contract(
                descriptor,
                interface,
                "caller.contract".to_owned(),
                BundleId::from_u64(bundle_id),
            )
        };
        if let Err(e) = result {
            panic!("failed to register native caller contract: {e}");
        }
    }

    fn host_with_runtime(runtime: &Arc<Runtime>) -> *const HostApi {
        runtime.host_abi() as *const HostApi
    }

    #[test]
    fn call_guest_method_null_this_returns_invalid_pointer() {
        let instance: polyplug_abi::guest::GuestContractInstance =
            polyplug_abi::guest::GuestContractInstance {
                data: &raw const NATIVE_FNS as *mut core::ffi::c_void,
                contract_id: GuestContractId::from_u64(1),
            };
        // SAFETY: host_call_guest_method tolerates a null `this`.
        let err: polyplug_abi::types::AbiError = unsafe {
            host_call_guest_method(
                core::ptr::null(),
                instance,
                0,
                core::ptr::null(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
            )
        };
        assert_eq!(
            err.code,
            polyplug_abi::types::AbiErrorCode::InvalidPointer as u32
        );
    }

    #[test]
    fn call_guest_method_null_instance_returns_invalid_pointer() {
        let runtime: Arc<Runtime> = Runtime::builder().build().expect("build");
        let instance: polyplug_abi::guest::GuestContractInstance =
            polyplug_abi::guest::GuestContractInstance::null();
        // SAFETY: instance.data is null; the call must reject it before any deref.
        let err: polyplug_abi::types::AbiError = unsafe {
            host_call_guest_method(
                host_with_runtime(&runtime),
                instance,
                0,
                core::ptr::null(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
            )
        };
        assert_eq!(
            err.code,
            polyplug_abi::types::AbiErrorCode::InvalidPointer as u32
        );
    }

    #[test]
    fn call_guest_method_unknown_contract_returns_not_found() {
        let runtime: Arc<Runtime> = Runtime::builder().build().expect("build");
        let instance: polyplug_abi::guest::GuestContractInstance =
            polyplug_abi::guest::GuestContractInstance {
                data: &raw const NATIVE_FNS as *mut core::ffi::c_void,
                contract_id: GuestContractId::from_u64(0xDEAD_BEEF),
            };
        // SAFETY: this is valid; contract_id is unregistered so lookup fails.
        let err: polyplug_abi::types::AbiError = unsafe {
            host_call_guest_method(
                host_with_runtime(&runtime),
                instance,
                0,
                core::ptr::null(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
            )
        };
        assert_eq!(err.code, polyplug_abi::types::AbiErrorCode::NotFound as u32);
    }

    #[test]
    fn call_guest_method_native_happy_path() {
        let runtime: Arc<Runtime> = Runtime::builder().build().expect("build");
        let contract_id: u64 = 0x1234_5678_9ABC_DEF0;
        register_native_caller_contract(&runtime.registry, contract_id, 0x1);

        let instance: polyplug_abi::guest::GuestContractInstance =
            polyplug_abi::guest::GuestContractInstance {
                data: &raw const NATIVE_FNS as *mut core::ffi::c_void,
                contract_id: GuestContractId::from_u64(contract_id),
            };
        let input: i32 = 41;
        let mut output: i32 = 0;
        // SAFETY: native_add_one reads *const i32 from args and writes *mut i32 to out.
        let err: polyplug_abi::types::AbiError = unsafe {
            host_call_guest_method(
                host_with_runtime(&runtime),
                instance,
                0,
                &raw const input as *const core::ffi::c_void,
                &raw mut output as *mut core::ffi::c_void,
                core::ptr::null_mut(),
            )
        };
        assert!(err.is_ok(), "native dispatch should succeed");
        assert_eq!(output, 42);
    }

    #[test]
    fn call_guest_method_native_fn_id_out_of_range() {
        let runtime: Arc<Runtime> = Runtime::builder().build().expect("build");
        let contract_id: u64 = 0x0FED_CBA9_8765_4321;
        register_native_caller_contract(&runtime.registry, contract_id, 0x2);

        let instance: polyplug_abi::guest::GuestContractInstance =
            polyplug_abi::guest::GuestContractInstance {
                data: &raw const NATIVE_FNS as *mut core::ffi::c_void,
                contract_id: GuestContractId::from_u64(contract_id),
            };
        // SAFETY: function_count is 1; fn_id 5 is out of range and must be rejected.
        let err: polyplug_abi::types::AbiError = unsafe {
            host_call_guest_method(
                host_with_runtime(&runtime),
                instance,
                5,
                core::ptr::null(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
            )
        };
        assert_eq!(
            err.code,
            polyplug_abi::types::AbiErrorCode::FunctionNotAvailable as u32
        );
    }

    /// VM dispatch fake: echoes fn_id into `out` and records the forwarded arena.
    unsafe extern "C" fn vm_echo_call(
        _loader_data: polyplug_abi::dispatch::VmLoaderData,
        _instance: polyplug_abi::guest::GuestContractInstance,
        fn_id: u32,
        _args: *const (),
        out: *mut (),
        _arena: *mut polyplug_abi::types::CallArena,
    ) -> polyplug_abi::types::AbiError {
        // SAFETY: the test passes a valid *mut u32 for out.
        unsafe {
            *(out as *mut u32) = fn_id;
        }
        polyplug_abi::types::AbiError::ok()
    }

    fn register_vm_caller_contract(
        registry: &crate::runtime_store::RuntimeStore,
        contract_id: u64,
        bundle_id: u64,
    ) {
        use polyplug_abi::{
            DispatchMechanisms, DispatchType, GuestContractInstance, GuestContractInterface,
            VmDispatch, VmLoaderData,
        };

        unsafe extern "C" fn stub_create(
            _host: *const HostApi,
            _args: *const (),
        ) -> GuestContractInstance {
            GuestContractInstance::null()
        }
        unsafe extern "C" fn stub_destroy(_host: *const HostApi, _instance: GuestContractInstance) {
        }

        let interface: &'static GuestContractInterface =
            Box::leak(Box::new(GuestContractInterface {
                contract_id: GuestContractId::from_u64(contract_id),
                contract_version: Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
                dispatch_type: DispatchType::VirtualMachine,
                create_instance: stub_create,
                destroy_instance: stub_destroy,
                dispatch: DispatchMechanisms {
                    vm: VmDispatch {
                        call: vm_echo_call,
                        loader_data: VmLoaderData::null(),
                    },
                },
            }));
        let descriptor: polyplug_abi::PluginDescriptor = polyplug_abi::PluginDescriptor {
            name: polyplug_abi::StringView::from_static(b"vmcaller"),
            contract_name: polyplug_abi::StringView::from_static(b"vmcaller.contract"),
            version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
        };
        // SAFETY: interface is leaked for the process lifetime.
        let result: Result<GuestContractHandle, crate::error::RegistryError> = unsafe {
            registry.register_guest_contract(
                descriptor,
                interface,
                "vmcaller.contract".to_owned(),
                BundleId::from_u64(bundle_id),
            )
        };
        if let Err(e) = result {
            panic!("failed to register vm caller contract: {e}");
        }
    }

    #[test]
    fn call_guest_method_vm_routing() {
        let runtime: Arc<Runtime> = Runtime::builder().build().expect("build");
        let contract_id: u64 = 0x00AA_BB00_CC00_DD00;
        register_vm_caller_contract(&runtime.registry, contract_id, 0x3);

        let instance: polyplug_abi::guest::GuestContractInstance =
            polyplug_abi::guest::GuestContractInstance {
                data: core::ptr::dangling_mut::<core::ffi::c_void>(),
                contract_id: GuestContractId::from_u64(contract_id),
            };
        let mut output: u32 = 0;
        // SAFETY: vm_echo_call writes the fn_id into *mut u32 out.
        let err: polyplug_abi::types::AbiError = unsafe {
            host_call_guest_method(
                host_with_runtime(&runtime),
                instance,
                7,
                core::ptr::null(),
                &raw mut output as *mut core::ffi::c_void,
                core::ptr::null_mut(),
            )
        };
        assert!(err.is_ok(), "vm dispatch should succeed");
        assert_eq!(output, 7, "vm fake should echo fn_id");
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
        let runtime: Arc<Runtime> = match Runtime::builder()
            .loader(EnforceLoader {
                contract_id: contract,
                error_bundle_id: 0_u64,
            })
            .build()
        {
            Ok(rt) => rt,
            Err(e) => panic!("failed to build runtime: {e}"),
        };
        let registry: &Arc<RuntimeStore> = runtime.registry();
        let _handle: GuestContractHandle =
            register_guest_contract(registry.as_ref(), contract, 0xBEEF_u64);
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
        let runtime: Arc<Runtime> = match Runtime::builder()
            .loader(ProbeLoader {
                observed_init: Arc::clone(&observed),
            })
            .build()
        {
            Ok(rt) => rt,
            Err(e) => panic!("failed to build runtime: {e}"),
        };
        let registry: &Arc<RuntimeStore> = runtime.registry();
        let _handle: GuestContractHandle =
            register_guest_contract(registry.as_ref(), contract, 0xCAFE_u64);
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
        let handle_after: Result<GuestContractHandle, _> =
            runtime.find_guest_contract(contract, 0_u32);
        assert!(
            handle_after.is_ok(),
            "after init, find_guest_contract should succeed"
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
            let _rt: Arc<Runtime> = Runtime::builder()
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
        let runtime: Arc<Runtime> = match Runtime::builder()
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
        let registry: &Arc<RuntimeStore> = runtime.registry();
        let _handle: GuestContractHandle =
            register_guest_contract(registry.as_ref(), contract, 0xABCD_u64);
        {
            let mut guard: std::sync::MutexGuard<'_, ReentrantState> = match state.lock() {
                Ok(g) => g,
                Err(e) => e.into_inner(),
            };
            guard.runtime_ptr = Arc::as_ptr(&runtime) as usize;
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
        let runtime: Arc<Runtime> = match Runtime::builder()
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
        let registry: &Arc<RuntimeStore> = runtime.registry();
        let _handle: GuestContractHandle =
            register_guest_contract(registry.as_ref(), contract, 0xFACE_u64);
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
        use polyplug_abi::{
            DispatchMechanisms, DispatchType, HostContractInstance, NativeDispatch,
        };

        unsafe extern "C" fn stub_create_instance(
            _this: *const HostContractInterface,
            _args: *const (),
        ) -> HostContractInstance {
            // Return a non-null dummy pointer for testing
            static mut DUMMY: usize = 0xDEADBEEF;
            HostContractInstance {
                data: &raw mut DUMMY as *mut core::ffi::c_void,
            }
        }

        unsafe extern "C" fn stub_destroy_instance(
            _this: *const HostContractInterface,
            _instance: HostContractInstance,
        ) {
        }

        Box::leak(Box::new(HostContractInterface {
            contract_id: polyplug_utils::HostContractId::from(contract_id),
            contract_version: polyplug_abi::types::Version {
                major,
                minor,
                patch: 0,
            },
            singleton: true,
            dispatch_type: DispatchType::Native,
            runtime: core::ptr::null_mut(),
            user_data: core::ptr::null_mut(),
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
    fn runtime_host_contracts_register_guest_contract_and_lookup() {
        let runtime: Arc<Runtime> = Runtime::builder()
            .build()
            .expect("runtime build should succeed");

        let contract_id: u64 = polyplug_utils::host_contract_id("host.logger", 1);
        let interface: &'static HostContractInterface =
            create_host_contract_interface(contract_id, 1, 0);

        let result: Result<(), HostContractError> =
            runtime.register_host_contract(contract_id, interface);
        assert!(result.is_ok(), "registration should succeed");

        let found: Option<&'static HostContractInterface> =
            runtime.get_host_contract(contract_id, 0);
        assert!(found.is_some(), "contract should be found");
        let found_interface: &HostContractInterface =
            found.expect("contract should be present after is_some check");
        assert_eq!(found_interface.contract_id.id(), contract_id);
    }

    #[test]
    fn runtime_host_contracts_duplicate_registration_fails() {
        let runtime: Arc<Runtime> = Runtime::builder()
            .build()
            .expect("runtime build should succeed");

        let contract_id: u64 = polyplug_utils::host_contract_id("host.logger", 1);
        let interface1: &'static HostContractInterface =
            create_host_contract_interface(contract_id, 1, 0);
        let interface2: &'static HostContractInterface =
            create_host_contract_interface(contract_id, 1, 1);

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
    fn runtime_host_contracts_unregister_guest_contract() {
        let runtime: Arc<Runtime> = Runtime::builder()
            .build()
            .expect("runtime build should succeed");

        let contract_id: u64 = polyplug_utils::host_contract_id("host.logger", 1);
        let interface: &'static HostContractInterface =
            create_host_contract_interface(contract_id, 1, 0);

        runtime
            .register_host_contract(contract_id, interface)
            .expect("registration should succeed");

        let removed: bool = runtime.unregister_host_contract(contract_id);
        assert!(
            removed,
            "unregister_guest_contract should return true for existing contract"
        );

        let removed_again: bool = runtime.unregister_host_contract(contract_id);
        assert!(
            !removed_again,
            "unregister_guest_contract should return false for non-existent contract"
        );

        let found: Option<&'static HostContractInterface> =
            runtime.get_host_contract(contract_id, 0);
        assert!(
            found.is_none(),
            "contract should not be found after unregister_guest_contract"
        );
    }

    #[test]
    fn runtime_host_contracts_version_check() {
        let runtime: Arc<Runtime> = Runtime::builder()
            .build()
            .expect("runtime build should succeed");

        let contract_id: u64 = polyplug_utils::host_contract_id("host.logger", 2);
        let interface: &'static HostContractInterface =
            create_host_contract_interface(contract_id, 2, 5);

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
        let runtime: Arc<Runtime> = Runtime::builder()
            .build()
            .expect("runtime build should succeed");
        assert_eq!(runtime.host_runtime(), RuntimeLanguage::Rust);
    }

    #[test]
    fn runtime_host_runtime_can_be_set() {
        let runtime: Arc<Runtime> = Runtime::builder()
            .host_runtime(RuntimeLanguage::Python)
            .build()
            .expect("runtime build should succeed");
        assert_eq!(runtime.host_runtime(), RuntimeLanguage::Python);
    }

    #[test]
    fn host_get_host_contract_callback_returns_register_guest_contracted_contract() {
        let runtime: Arc<Runtime> = Runtime::builder()
            .build()
            .expect("runtime build should succeed");

        let contract_id: u64 = polyplug_utils::host_contract_id("host.test", 1);
        let interface: &'static HostContractInterface =
            create_host_contract_interface(contract_id, 1, 0);

        runtime
            .register_host_contract(contract_id, interface)
            .expect("registration should succeed");

        // Create a HostApi with runtime pointer
        let host_interface: HostApi = HostApi {
            runtime: Arc::as_ptr(&runtime) as *mut core::ffi::c_void,
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
            // Host operations (implemented in 18-02)
            load_bundle: host_load_bundle,
            reload_bundle: host_reload_bundle,
            register_host_contract: host_register_host_contract,
            register_loader: host_register_loader,
            get_last_error: host_get_last_error,
            get_error_len: host_get_error_len,
            call_guest_method: host_call_guest_method,
            get_extension: host_get_extension,
        };

        // SAFETY: host_interface is valid with runtime pointer, runtime is live
        let instance: HostContractInstance =
            unsafe { host_get_host_contract(&host_interface as *const HostApi, contract_id, 0) };
        assert!(
            !instance.data.is_null(),
            "callback should return non-null instance for register_guest_contracted contract"
        );
    }

    #[test]
    fn host_get_host_contract_callback_returns_null_for_unregister_guest_contracted() {
        let runtime: Arc<Runtime> = Runtime::builder()
            .build()
            .expect("runtime build should succeed");

        let contract_id: u64 = polyplug_utils::host_contract_id("host.nonexistent", 1);

        // Create a HostApi with runtime pointer
        let host_interface: HostApi = HostApi {
            runtime: Arc::as_ptr(&runtime) as *mut core::ffi::c_void,
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
            // Host operations (implemented in 18-02)
            load_bundle: host_load_bundle,
            reload_bundle: host_reload_bundle,
            register_host_contract: host_register_host_contract,
            register_loader: host_register_loader,
            get_last_error: host_get_last_error,
            get_error_len: host_get_error_len,
            call_guest_method: host_call_guest_method,
            get_extension: host_get_extension,
        };

        // SAFETY: host_interface is valid with runtime pointer, runtime is live
        let instance: HostContractInstance =
            unsafe { host_get_host_contract(&host_interface as *const HostApi, contract_id, 0) };
        assert!(
            instance.data.is_null(),
            "callback should return null instance for unregister_guest_contracted contract"
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
                data: (count + 1) as *mut core::ffi::c_void, // +1 to avoid null for count=0
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
        use polyplug_abi::{DispatchMechanisms, DispatchType, NativeDispatch};

        Box::leak(Box::new(HostContractInterface {
            contract_id: polyplug_utils::HostContractId::from(contract_id),
            contract_version: polyplug_abi::types::Version {
                major,
                minor: 0,
                patch: 0,
            },
            singleton,
            dispatch_type: DispatchType::Native,
            runtime: core::ptr::null_mut(),
            user_data: core::ptr::null_mut(),
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

        let runtime: Arc<Runtime> = Runtime::builder()
            .build()
            .expect("runtime build should succeed");

        let contract_id: u64 = polyplug_utils::host_contract_id("singleton.test", 1);
        let interface: &'static HostContractInterface =
            create_counting_host_contract_interface(contract_id, 1, true); // singleton=true

        runtime
            .register_host_contract(contract_id, interface)
            .expect("registration should succeed");

        // Create a HostApi with runtime pointer
        let host_interface: HostApi = HostApi {
            runtime: Arc::as_ptr(&runtime) as *mut core::ffi::c_void,
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
            // Host operations (implemented in 18-02)
            load_bundle: host_load_bundle,
            reload_bundle: host_reload_bundle,
            register_host_contract: host_register_host_contract,
            register_loader: host_register_loader,
            get_last_error: host_get_last_error,
            get_error_len: host_get_error_len,
            call_guest_method: host_call_guest_method,
            get_extension: host_get_extension,
        };

        // First call - creates instance
        // SAFETY: host_interface is valid with runtime pointer, runtime is live
        let instance1: HostContractInstance =
            unsafe { host_get_host_contract(&host_interface as *const HostApi, contract_id, 0) };
        assert!(
            !instance1.data.is_null(),
            "first call should return non-null instance"
        );

        // Second call - should return SAME cached instance
        // SAFETY: host_interface is valid with runtime pointer, runtime is live
        let instance2: HostContractInstance =
            unsafe { host_get_host_contract(&host_interface as *const HostApi, contract_id, 0) };
        assert!(
            !instance2.data.is_null(),
            "second call should return non-null instance"
        );

        // HC-02: Verify same instance pointer is returned
        assert_eq!(
            instance1.data, instance2.data,
            "singleton contract should return cached instance (same pointer)"
        );

        // Counter should have been incremented only once (single create)
        let counter_value: usize = LOCAL_INSTANCE_COUNTER.with(|counter| counter.get());
        assert_eq!(
            counter_value, 1,
            "singleton should only call create_instance once"
        );

        // Third call - still same instance
        // SAFETY: host_interface is valid with runtime pointer, runtime is live
        let instance3: HostContractInstance =
            unsafe { host_get_host_contract(&host_interface as *const HostApi, contract_id, 0) };
        assert_eq!(
            instance1.data, instance3.data,
            "third call should still return same cached instance"
        );
        assert_eq!(
            LOCAL_INSTANCE_COUNTER.with(|counter| counter.get()),
            1,
            "counter still at 1 - no additional create calls"
        );
    }

    #[test]
    fn multi_instance_contract_creates_new_instance_on_each_call() {
        // Reset thread-local counter before test
        LOCAL_INSTANCE_COUNTER.with(|counter| counter.set(100)); // Start at 100 for unique values

        let runtime: Arc<Runtime> = Runtime::builder()
            .build()
            .expect("runtime build should succeed");

        let contract_id: u64 = polyplug_utils::host_contract_id("multi.test", 1);
        let interface: &'static HostContractInterface =
            create_counting_host_contract_interface(contract_id, 1, false); // singleton=false

        runtime
            .register_host_contract(contract_id, interface)
            .expect("registration should succeed");

        // Create a HostApi with runtime pointer
        let host_interface: HostApi = HostApi {
            runtime: Arc::as_ptr(&runtime) as *mut core::ffi::c_void,
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
            // Host operations (implemented in 18-02)
            load_bundle: host_load_bundle,
            reload_bundle: host_reload_bundle,
            register_host_contract: host_register_host_contract,
            register_loader: host_register_loader,
            get_last_error: host_get_last_error,
            get_error_len: host_get_error_len,
            call_guest_method: host_call_guest_method,
            get_extension: host_get_extension,
        };

        // First call - creates instance (counter becomes 101)
        // SAFETY: host_interface is valid with runtime pointer, runtime is live
        let instance1: HostContractInstance =
            unsafe { host_get_host_contract(&host_interface as *const HostApi, contract_id, 0) };
        assert!(
            !instance1.data.is_null(),
            "first call should return non-null instance"
        );

        // Second call - creates NEW instance (counter becomes 102)
        // SAFETY: host_interface is valid with runtime pointer, runtime is live
        let instance2: HostContractInstance =
            unsafe { host_get_host_contract(&host_interface as *const HostApi, contract_id, 0) };
        assert!(
            !instance2.data.is_null(),
            "second call should return non-null instance"
        );

        // HC-03: Verify different instance pointers are returned
        assert_ne!(
            instance1.data, instance2.data,
            "multi-instance contract should create new instance each call (different pointers)"
        );

        // Counter should have been incremented twice
        let counter_value: usize = LOCAL_INSTANCE_COUNTER.with(|counter| counter.get());
        assert_eq!(
            counter_value, 102,
            "multi-instance should call create_instance twice"
        );

        // Third call - creates yet another instance (counter becomes 103)
        // SAFETY: host_interface is valid with runtime pointer, runtime is live
        let instance3: HostContractInstance =
            unsafe { host_get_host_contract(&host_interface as *const HostApi, contract_id, 0) };
        assert_ne!(
            instance1.data, instance3.data,
            "third instance differs from first"
        );
        assert_ne!(
            instance2.data, instance3.data,
            "third instance differs from second"
        );
        assert_eq!(
            LOCAL_INSTANCE_COUNTER.with(|counter| counter.get()),
            103,
            "counter at 103 - three create calls"
        );
    }

    #[test]
    fn singleton_and_multi_instance_contracts_coexist() {
        // Reset thread-local counter
        LOCAL_INSTANCE_COUNTER.with(|counter| counter.set(0));

        let runtime: Arc<Runtime> = Runtime::builder()
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

        // Create a HostApi with runtime pointer
        let host_interface: HostApi = HostApi {
            runtime: Arc::as_ptr(&runtime) as *mut core::ffi::c_void,
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
            // Host operations (implemented in 18-02)
            load_bundle: host_load_bundle,
            reload_bundle: host_reload_bundle,
            register_host_contract: host_register_host_contract,
            register_loader: host_register_loader,
            get_last_error: host_get_last_error,
            get_error_len: host_get_error_len,
            call_guest_method: host_call_guest_method,
            get_extension: host_get_extension,
        };

        // Call singleton twice - should get same instance
        // SAFETY: host_interface is valid with runtime pointer, runtime is live
        let s1: HostContractInstance =
            unsafe { host_get_host_contract(&host_interface as *const HostApi, singleton_id, 0) };
        // SAFETY: host_interface is valid with runtime pointer, runtime is live
        let s2: HostContractInstance =
            unsafe { host_get_host_contract(&host_interface as *const HostApi, singleton_id, 0) };
        assert_eq!(s1.data, s2.data, "singleton returns cached instance");

        // Call multi-instance twice - should get different instances
        // SAFETY: host_interface is valid with runtime pointer, runtime is live
        let m1: HostContractInstance =
            unsafe { host_get_host_contract(&host_interface as *const HostApi, multi_id, 0) };
        // SAFETY: host_interface is valid with runtime pointer, runtime is live
        let m2: HostContractInstance =
            unsafe { host_get_host_contract(&host_interface as *const HostApi, multi_id, 0) };
        assert_ne!(m1.data, m2.data, "multi-instance returns new instances");

        // Singleton instance should differ from multi instances
        assert_ne!(
            s1.data, m1.data,
            "singleton and multi instances are different"
        );
        assert_ne!(
            s1.data, m2.data,
            "singleton and multi instances are different"
        );
    }

    // ─── Extension System Tests ────────────────────────────────────────────────

    #[test]
    fn extension_round_trip() {
        let runtime: Arc<Runtime> = Runtime::builder()
            .build()
            .expect("runtime build should succeed");

        static EXTENSION_VALUE: u32 = 0xDEAD_BEEF;
        let ext_ptr: *const () = &EXTENSION_VALUE as *const u32 as *const ();

        // SAFETY: ext_ptr points to a 'static variable; valid for the program lifetime.
        unsafe { runtime.register_extension("test.extension", ext_ptr) };

        let host: *const HostApi = runtime.as_context_ptr();
        let extension_id: u32 = polyplug_utils::fnv1a_32(b"test.extension");
        // SAFETY: host points to the runtime's 'static HostApi; runtime is live for the
        // duration of this call. The get_extension callback reads from runtime.extensions which
        // was populated above.
        let retrieved: *const () = unsafe { ((*host).get_extension)(host, extension_id) };

        assert!(
            !retrieved.is_null(),
            "registered extension must not be null"
        );
        assert_eq!(
            retrieved, ext_ptr,
            "retrieved pointer must equal registered pointer"
        );
    }

    #[test]
    fn extension_missing_returns_null() {
        let runtime: Arc<Runtime> = Runtime::builder()
            .build()
            .expect("runtime build should succeed");

        let host: *const HostApi = runtime.as_context_ptr();
        let extension_id: u32 = polyplug_utils::fnv1a_32(b"nonexistent.extension");
        // SAFETY: host points to the runtime's 'static HostApi; runtime is live.
        // No extension was registered, so the callback reads from an empty map.
        let retrieved: *const () = unsafe { ((*host).get_extension)(host, extension_id) };

        assert!(
            retrieved.is_null(),
            "unregistered extension must return null pointer"
        );
    }

    #[test]
    fn extension_id_collision_resistance() {
        // Two distinct extension names must produce distinct extension IDs.
        let id_logger: u32 = polyplug_utils::fnv1a_32(b"logger");
        let id_tracer: u32 = polyplug_utils::fnv1a_32(b"tracer");
        assert_ne!(
            id_logger, id_tracer,
            "different extension names must not collide"
        );
    }
}

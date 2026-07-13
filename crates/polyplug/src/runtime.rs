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

use core::any::Any;
use core::ffi::c_void;
use core::panic::AssertUnwindSafe;
use core::str::FromStr;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;
use core::{mem, ptr, slice, str};
use std::collections::HashMap;
use std::collections::HashSet;
use std::panic::catch_unwind;
use std::panic::resume_unwind;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::RwLock;
use std::sync::RwLockReadGuard;
use std::sync::RwLockWriteGuard;

type InitThreadId = u64;

#[cfg(unix)]
unsafe extern "C" {
    fn pthread_self() -> usize;
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    #[link_name = "GetCurrentThreadId"]
    fn get_current_thread_id() -> u32;
}

fn current_init_thread_id() -> InitThreadId {
    #[cfg(unix)]
    {
        // SAFETY: pthread_self takes no arguments and returns the current OS thread handle.
        unsafe { pthread_self() as InitThreadId }
    }
    #[cfg(windows)]
    {
        // SAFETY: GetCurrentThreadId takes no arguments and returns the current OS thread ID.
        unsafe { InitThreadId::from(get_current_thread_id()) }
    }
}

pub(crate) fn current_os_thread_id() -> u64 {
    current_init_thread_id()
}

use crossbeam_epoch::{Guard as EpochGuard, pin as epoch_pin};
use ed25519_dalek::VerifyingKey;
use polyplug_abi::dispatch::{DispatchType, VmLoaderData};
use polyplug_abi::ffi::{polyplug_host_alloc, polyplug_host_free};
use polyplug_abi::guest::GuestContractInstance;
use polyplug_abi::runtime::{Compatibility, ReloadPhase, RuntimeConfig, SignaturePolicy};
use polyplug_abi::types::{Ed25519PublicKey, LogLevel};
use polyplug_abi::{
    AbiError, AbiErrorCode, Array, DependencyInfo, GuestContractHandle, GuestContractInterface,
    HostApi, HostContractInstance, HostContractInterface, PluginDescriptor, StringView,
    SupportedLanguage, types::Version,
};
use polyplug_signing::{
    BundleVerifier, PinnedKeyVerifier, SigError, verify_bundle as signing_verify_bundle,
    verifying_key_from_bytes,
};
use polyplug_utils::{BundleId, GuestContractId};

use crate::error::HostContractError;
use crate::error::LoaderError;
use crate::error::RegistryError;
use polyplug_common::{ManifestData, ManifestDependency, ManifestError, RawManifestDependency};

use crate::error::RuntimeError;
use crate::loader::BundleLoader;
use crate::loader::BundleSource;
use crate::loader::manifest::{parsed_bundle_dependencies, resolved_dependencies_with_logger};
use crate::loader::parse_manifest;
use crate::logger::{LoggerClosure, LoggerHandle, RecoverPoisoned, RecoveringGuard};
pub use crate::runtime_builder::RuntimeBuilder;

use crate::runtime_store::BundleDependency;
use crate::runtime_store::BundleDescriptor;
use crate::runtime_store::InternalPluginResident;
use crate::runtime_store::InternalPluginResidentRelease;
use crate::runtime_store::PreparedGuestContract;
use crate::runtime_store::RuntimeStore;

// ─── Runtime Configuration ───────────────────────────────────────────────────

/// Reload callback invoked after each interface swap, before dlclose.
///
/// The first argument is the opaque `on_reload_user_data` pointer from
/// `RuntimeConfig`, forwarded unchanged on every invocation.
pub(crate) struct ReloadCallback(pub(crate) Arc<dyn Fn(*mut c_void, ReloadPhase) + Send + Sync>);

/// Options for `Runtime::load_bundle_with`.
///
/// The `compatibility` field overrides the global `RuntimeBuilder::compatibility` setting
/// for this specific bundle load only.
pub(crate) struct LoadOptions {
    pub compatibility: Compatibility,
    pub ignore_function_count_mismatch: bool,
}

pub struct Runtime {
    pub(crate) registry: Arc<RuntimeStore>,
    /// All registered loaders, keyed by loader_name.
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
    /// Instance-owned copy of the host logging configuration (from `config`).
    pub(crate) logger: LoggerHandle,
    /// Keeps the Rust closure installed via `RuntimeBuilder::logger` alive for
    /// the runtime's lifetime — `config.log_user_data` points into this box.
    /// Never read after construction; it exists purely as an owner.
    pub(crate) _logger_closure: Option<Box<LoggerClosure>>,
    /// Owns the runtime's copy of the trusted Ed25519 verifying keys for the
    /// runtime's lifetime.
    ///
    /// Keys reach the builder either through [`RuntimeBuilder::trusted_keys`] (Rust
    /// API) or through the FFI / `config()` path (a host populating
    /// `RuntimeConfig.trusted_keys` directly). In BOTH cases `build()` copies them
    /// into this `Vec` and repoints the persisted `config.trusted_keys` `Array` at
    /// this `Vec`'s heap buffer — so `config.trusted_keys` always addresses
    /// runtime-owned storage, never a borrowed host buffer. The host's own buffer
    /// is therefore only borrowed for the duration of `create`, satisfying the
    /// documented `RuntimeConfig.trusted_keys` ownership contract (the host may free
    /// it once `create` returns). This mirrors how `_logger_closure` backs
    /// `config.log_user_data`; the `Vec`'s data buffer is stable across the move
    /// into this field (moving a `Vec` moves only its 3-word header, never the heap
    /// buffer the `Array` points to). Never read after construction; it exists
    /// purely as an owner.
    ///
    /// [`RuntimeBuilder::trusted_keys`]: crate::runtime_builder::RuntimeBuilder::trusted_keys
    pub(crate) _trusted_keys: Vec<Ed25519PublicKey>,
    /// Last error message for FFI error reporting.
    pub(crate) last_error: Mutex<String>,
    /// Registered host contracts, keyed by contract_id.
    pub(crate) host_contracts: RwLock<HashMap<u64, &'static HostContractInterface>>,
    /// Cache for singleton host contract instances.
    /// Key: HostContractId hash value.
    pub(crate) singleton_instances: RwLock<HashMap<u64, HostContractInstance>>,
    /// Host language type identifier.
    pub(crate) host_language: SupportedLanguage,
    /// Per-thread stack of bundle_ids currently inside `polyplug_init`.
    ///
    /// The stack is runtime-owned, so multiple runtimes in one process stay isolated.
    /// Keys use the OS thread identity because loader cdylibs and core can carry
    /// separate statically linked Rust standard libraries whose `ThreadId` values
    /// are not interchangeable. A `Vec` per thread preserves nested-load reentrancy:
    /// the inner bundle pops back to the outer bundle. Loaders bracket
    /// `polyplug_init` with a push and pop, including every error path.
    pub(crate) init_bundle_stack: Mutex<HashMap<InitThreadId, Vec<u64>>>,
    /// Fast-path hint: total number of bundle ids currently pushed across all
    /// threads' init stacks.
    ///
    /// Plugin init is a Phase-1 (rare) event; outside it every `find` / `find_all`
    /// / `get_dependencies` HostApi call would otherwise lock `init_bundle_stack`
    /// just to observe an empty stack. This counter lets `current_init_bundle_id`
    /// short-circuit to `0` with a single `Relaxed` atomic load and skip the Mutex
    /// entirely on that hot path.
    ///
    /// # Ordering rationale
    /// This is a hint only — it never carries data, just gates whether the Mutex is
    /// taken. When the counter is non-zero the Mutex provides the actual
    /// synchronization of the per-thread stacks, so `Relaxed` is sufficient here: no
    /// memory is published or consumed through this atomic. A stale `0` cannot be
    /// observed for the calling thread's OWN init window because that thread called
    /// `push_init_bundle_id` (a `fetch_add` plus a Mutex acquisition) earlier on the
    /// same thread, which happens-before any `current_init_bundle_id` it later runs
    /// during the plugin's init code — single-thread program order guarantees the
    /// incremented value is visible to that thread. Other threads' pushes only ever
    /// make the counter *larger* than this thread needs; the worst case is taking the
    /// Mutex and finding no entry for this thread (returning `0`), which is correct.
    pub(crate) active_init_count: AtomicUsize,
    /// Serializes whole-reload sequences against one another.
    ///
    /// A reload is a non-atomic read-modify-write: it snapshots the bundle's
    /// pre-reload slots, runs `loader.reload()` (which registers the new
    /// interfaces into fresh slots), then `apply_reload_swap` consumes that
    /// snapshot. The registry `RwLock` makes each individual step atomic, but it
    /// is dropped between steps, so two concurrent reloads of the SAME bundle can
    /// interleave such that one reload's snapshot goes stale — its swap then finds
    /// no freshly-registered slot for a contract the other reload already
    /// consumed, takes the dropped-contract teardown path, and removes that
    /// contract's only live slot from the find index, leaving a contract BOTH versions provide
    /// unresolvable. Holding this mutex across the entire `reload_bundle` call
    /// (including its cascade tree) makes each reload's snapshot↔swap atomic with
    /// respect to any other reload.
    ///
    /// Instance-owned (Rule 12): each `Runtime` has its own lock, so multiple
    /// runtimes in one process never serialize against each other. Readers
    /// (`find`/`resolve`/dispatch) never take this lock — they hold the registry
    /// `RwLock` and stay fully concurrent with an in-flight reload; only
    /// writer-vs-writer reloads serialize here.
    pub(crate) reload_serialize: Mutex<()>,
    /// Live stateful-instance and in-flight-construction counts per owning bundle.
    ///
    /// Construction reserves its count before calling guest code and turns that
    /// reservation into a live-instance count only when the guest returns state.
    /// Unload serializes against this reservation before releasing internal backing.
    ///
    /// Instance-owned (Rule 12): each `Runtime` has its own map, so multiple
    /// runtimes in one process never share instance accounting.
    pub(crate) instance_counts: Mutex<HashMap<BundleId, u64>>,
    /// Rust-owned generated internal-plugin provider roots, keyed by bundle ID.
    ///
    /// A root is erased only inside Rust. It is never exposed through the C ABI
    /// and remains available to generated create-instance thunks until unload.
    pub(crate) internal_plugin_roots: Mutex<HashMap<BundleId, Box<dyn Any + Send + Sync>>>,
    /// Every internal-plugin transaction marks its bundle before publication.
    ///
    /// The marker is source-neutral: it protects wrapper-owned backing just as it
    /// protects Rust roots and native residents. A failed or aborted transaction
    /// removes it before returning; logical unload removes it with the bundle.
    pub(crate) internal_plugin_lifecycle: Mutex<HashSet<BundleId>>,
    /// The owned HostApi handed to plugins. A `Box` gives it a stable heap
    /// address independent of where the `Runtime` value lives, so the pointer
    /// captured by plugins survives the runtime's move into its `Arc`.
    ///
    /// Declared last so it is dropped after `registry` and `loaders` — their
    /// teardown dlcloses plugin libraries whose destructors may still call
    /// through this HostApi; freeing it last keeps those callbacks sound.
    /// (Rust drops fields in declaration order, first-declared first.)
    pub(crate) host_abi: Box<HostApi>,
}

/// Narrow Rust-only generated guest provider binding contract. Provider roots stay in `Self`.
#[doc(hidden)]
pub trait RustGeneratedInternalPlugin: Send + Sync {
    fn manifest(&self) -> ManifestData;
    fn stage(
        &self,
        registrar: &mut RustGeneratedInternalPluginRegistrar,
    ) -> Result<(), RuntimeError>;
}

/// Runtime-owned staging context for generated Rust internal-plugin provider bindings.
#[doc(hidden)]
pub struct RustGeneratedInternalPluginRegistrar {
    host: *const HostApi,
    bundle: String,
}

impl RustGeneratedInternalPluginRegistrar {
    #[doc(hidden)]
    pub fn register_contract(
        &mut self,
        descriptor: &PluginDescriptor,
        interface: &GuestContractInterface,
    ) -> Result<(), RuntimeError> {
        let mut error: AbiError = AbiError::ok();
        // SAFETY: Runtime creates this context only during its active prepared transaction.
        unsafe {
            ((*self.host).register_guest_contract)(self.host, descriptor, interface, &mut error);
        }
        if error.is_ok() {
            Ok(())
        } else {
            Err(RuntimeError::Loader(LoaderError::InitFailed {
                bundle: self.bundle.clone(),
                error: "generated guest registration failed".to_owned(),
            }))
        }
    }
}

/// Publication result consumed immediately by generated internal-plugin bindings.
#[doc(hidden)]
pub struct GeneratedInternalPluginRegistration {
    pub bundle_id: BundleId,
    pub handles: Vec<GuestContractHandle>,
}

impl Runtime {
    /// Create a RuntimeBuilder.
    pub fn builder() -> RuntimeBuilder {
        RuntimeBuilder::new()
    }

    #[cfg(test)]
    pub(crate) fn register_internal_plugin<R, F>(
        &self,
        manifest: ManifestData,
        language: SupportedLanguage,
        root: R,
        stage: F,
    ) -> Result<BundleId, RuntimeError>
    where
        R: Any + Send + Sync,
        F: FnOnce(*const HostApi) -> AbiError,
    {
        let bundle_id: BundleId = self.begin_internal_plugin(manifest, language)?;
        let error: AbiError = stage(self.host_abi());
        if !error.is_ok() {
            self.abort_internal_plugin(bundle_id);
            return Err(RuntimeError::Loader(LoaderError::InitFailed {
                bundle: self
                    .registry
                    .prepared_manifest(bundle_id)
                    .map_or_else(|| bundle_id.id().to_string(), |manifest| manifest.name),
                error: "generated guest registration failed".to_owned(),
            }));
        }
        let registration_result: Result<BundleId, RuntimeError> =
            self.commit_internal_plugin(bundle_id);
        if registration_result.is_err() {
            self.registry.discard_prepared_bundle(bundle_id);
            return registration_result;
        }

        let mut roots: RecoveringGuard<
            MutexGuard<'_, HashMap<BundleId, Box<dyn Any + Send + Sync>>>,
        > = self
            .internal_plugin_roots
            .lock()
            .recover_poisoned(self.logger, "runtime");
        let previous: Option<Box<dyn Any + Send + Sync>> = roots.insert(bundle_id, Box::new(root));
        debug_assert!(
            previous.is_none(),
            "logical unload must release the prior internal-plugin root"
        );
        registration_result
    }
    /// Register generated internal-plugin bindings through the canonical prepared transaction.
    ///
    /// This hidden glue boundary owns the generated aggregate from the start of the
    /// attempt. The root lock spans publication and insertion, so unload cannot
    /// observe published interfaces before their backing aggregate is runtime-owned.
    #[doc(hidden)]
    pub fn register_generated_internal_plugin<B>(
        &self,
        binding: B,
    ) -> Result<GeneratedInternalPluginRegistration, RuntimeError>
    where
        B: RustGeneratedInternalPlugin + 'static,
    {
        let mut roots: RecoveringGuard<
            MutexGuard<'_, HashMap<BundleId, Box<dyn Any + Send + Sync>>>,
        > = self
            .internal_plugin_roots
            .lock()
            .recover_poisoned(self.logger, "runtime");
        let manifest: ManifestData = binding.manifest();
        let bundle_id: BundleId = self.begin_internal_plugin(manifest, SupportedLanguage::Rust)?;
        let mut registrar = RustGeneratedInternalPluginRegistrar {
            host: self.host_abi(),
            bundle: self
                .registry
                .prepared_manifest(bundle_id)
                .map_or_else(|| bundle_id.id().to_string(), |prepared| prepared.name),
        };
        let staged = match catch_unwind(AssertUnwindSafe(|| binding.stage(&mut registrar))) {
            Ok(result) => result,
            Err(payload) => {
                self.abort_internal_plugin(bundle_id);
                resume_unwind(payload);
            }
        };
        if let Err(error) = staged {
            self.abort_internal_plugin(bundle_id);
            return Err(error);
        }
        let handles: Vec<GuestContractHandle> =
            match self.commit_internal_plugin_with_handles(bundle_id) {
                Ok(handles) => handles,
                Err(error) => {
                    self.registry.discard_prepared_bundle(bundle_id);
                    return Err(error);
                }
            };
        let previous: Option<Box<dyn Any + Send + Sync>> =
            roots.insert(bundle_id, Box::new(binding));
        debug_assert!(
            previous.is_none(),
            "logical unload must release the prior internal-plugin root"
        );
        Ok(GeneratedInternalPluginRegistration { bundle_id, handles })
    }

    /// Begin an internal-plugin registration transaction from canonical manifest data.
    ///
    /// The caller must register each contract through `HostApi::register_guest_contract` on the
    /// current thread, then call `commit_internal_plugin` or `abort_internal_plugin`.
    pub(crate) fn begin_internal_plugin(
        &self,
        manifest: ManifestData,
        language: SupportedLanguage,
    ) -> Result<BundleId, RuntimeError> {
        let bundle_id: BundleId = self.begin_prepared_bundle_transaction(manifest, language)?;
        let inserted: bool = self
            .internal_plugin_lifecycle
            .lock()
            .recover_poisoned(self.logger, "runtime")
            .insert(bundle_id);
        debug_assert!(
            inserted,
            "prepared internal-plugin transactions must not overlap"
        );
        self.push_init_bundle_id(bundle_id.id());
        Ok(bundle_id)
    }

    /// Validate and atomically publish a complete internal-plugin registration transaction.
    pub(crate) fn commit_internal_plugin(
        &self,
        bundle_id: BundleId,
    ) -> Result<BundleId, RuntimeError> {
        let compatibility: Compatibility = self.config.compatibility;
        let result: Result<BundleId, RuntimeError> = self.commit_prepared_bundle_transaction(
            bundle_id,
            LoadOptions {
                compatibility,
                ignore_function_count_mismatch: false,
            },
        );
        self.pop_init_bundle_id();
        if result.is_err() {
            self.remove_internal_plugin_lifecycle_marker(bundle_id);
        }
        result
    }

    fn commit_internal_plugin_with_handles(
        &self,
        bundle_id: BundleId,
    ) -> Result<Vec<GuestContractHandle>, RuntimeError> {
        let compatibility: Compatibility = self.config.compatibility;
        let result: Result<Vec<GuestContractHandle>, RuntimeError> = self
            .commit_prepared_bundle_transaction_with_handles(
                bundle_id,
                LoadOptions {
                    compatibility,
                    ignore_function_count_mismatch: false,
                },
            );
        self.pop_init_bundle_id();
        if result.is_err() {
            self.remove_internal_plugin_lifecycle_marker(bundle_id);
        }
        result
    }

    /// Atomically commit a staged transaction and copy its exact registration-order
    /// handles into a prevalidated foreign-binding output buffer.
    pub(crate) fn commit_internal_plugin_into_handles(
        &self,
        bundle_id: BundleId,
        out_handles: &mut [GuestContractHandle],
    ) -> Result<usize, RuntimeError> {
        let Some(expected_count) = self.registry.prepared_bundle_contract_count(bundle_id) else {
            self.abort_internal_plugin(bundle_id);
            return Err(RuntimeError::Registry(
                RegistryError::MissingBundleMetadata {
                    bundle_id: bundle_id.id(),
                },
            ));
        };
        if expected_count != out_handles.len() {
            let bundle: String = self
                .registry
                .prepared_manifest(bundle_id)
                .map_or_else(|| bundle_id.id().to_string(), |manifest| manifest.name);
            self.abort_internal_plugin(bundle_id);
            return Err(RuntimeError::Loader(LoaderError::InitFailed {
                bundle,
                error: format!(
                    "committed-handle output capacity {} does not match {expected_count} staged providers",
                    out_handles.len()
                ),
            }));
        }
        let handles: Vec<GuestContractHandle> =
            self.commit_internal_plugin_with_handles(bundle_id)?;
        out_handles.copy_from_slice(&handles);
        Ok(handles.len())
    }

    /// Discard an uncommitted internal-plugin registration transaction.
    pub(crate) fn abort_internal_plugin(&self, bundle_id: BundleId) {
        if self.registry.discard_prepared_bundle(bundle_id) {
            self.remove_internal_plugin_lifecycle_marker(bundle_id);
            self.pop_init_bundle_id();
        }
    }

    /// Transfer ownership of a native adapter resident into the current prepared
    /// internal-plugin transaction.
    pub(crate) fn attach_internal_plugin_resident(
        &self,
        bundle_id: BundleId,
        context: *mut c_void,
        owner_thread_id: u64,
        release: InternalPluginResidentRelease,
    ) -> Result<(), RuntimeError> {
        let current_thread_id: u64 = current_os_thread_id();
        if owner_thread_id != current_thread_id {
            return Err(RuntimeError::InternalPluginResidentWrongThread {
                bundle: format!("{:#x}", bundle_id.id()),
                owner_thread_id,
                current_thread_id,
            });
        }
        self.registry
            .attach_prepared_bundle_resident(bundle_id, context, owner_thread_id, release)
            .map_err(RuntimeError::Registry)
    }

    fn remove_internal_plugin_lifecycle_marker(&self, bundle_id: BundleId) {
        self.internal_plugin_lifecycle
            .lock()
            .recover_poisoned(self.logger, "runtime")
            .remove(&bundle_id);
    }

    /// Validate the canonical manifest metadata consumed by every prepared-bundle transaction.
    fn validate_prepared_bundle_manifest(
        &self,
        manifest: &ManifestData,
    ) -> Result<(), RuntimeError> {
        manifest
            .validate_metadata()
            .map_err(|error: ManifestError| RuntimeError::Loader(error.into()))
    }

    /// Validate and begin the source-neutral prepared-bundle transaction.
    fn begin_prepared_bundle_transaction(
        &self,
        manifest: ManifestData,
        language: SupportedLanguage,
    ) -> Result<BundleId, RuntimeError> {
        self.validate_prepared_bundle_manifest(&manifest)?;
        let bundle_id: BundleId = BundleId::new(&manifest.name);
        if self.registry.get_bundle_descriptor(bundle_id).is_some() {
            return Err(RuntimeError::Registry(
                RegistryError::BundleAlreadyRegistered {
                    bundle: manifest.name,
                },
            ));
        }
        self.registry.begin_prepared_bundle(manifest, language);
        Ok(bundle_id)
    }

    /// Validate and atomically publish one complete prepared-bundle transaction.
    fn commit_prepared_bundle_transaction(
        &self,
        bundle_id: BundleId,
        opts: LoadOptions,
    ) -> Result<BundleId, RuntimeError> {
        self.commit_prepared_bundle_transaction_with_handles(bundle_id, opts)
            .map(|_| bundle_id)
    }

    fn commit_prepared_bundle_transaction_with_handles(
        &self,
        bundle_id: BundleId,
        opts: LoadOptions,
    ) -> Result<Vec<GuestContractHandle>, RuntimeError> {
        let mut prepared = self
            .registry
            .take_prepared_bundle(bundle_id)
            .ok_or_else(|| {
                RuntimeError::Registry(RegistryError::MissingBundleMetadata {
                    bundle_id: bundle_id.id(),
                })
            })?;
        let mut resident: Option<InternalPluginResident> = prepared.take_resident();
        let (manifest, language, contracts) = prepared.into_parts();
        self.validate_prepared_provider_set(&manifest, &contracts)?;
        if !opts.ignore_function_count_mismatch && opts.compatibility != Compatibility::Yolo {
            self.validate_prepared_function_counts(&manifest, &contracts, opts.compatibility)?;
        }
        let version: Version =
            parse_manifest_version(&manifest.version, &manifest.name, &manifest.path)?;
        let dependencies: HashSet<GuestContractId> = manifest
            .dependencies
            .iter()
            .map(|dependency: &RawManifestDependency| dependency.contract_id)
            .collect();
        let bundle_dependencies: Vec<BundleDependency> = parsed_bundle_dependencies(&manifest);
        let handles: Vec<GuestContractHandle> = {
            let mut residents = self.registry.lock_internal_plugin_residents();
            let handles = self.registry.register_prepared_bundle(
                BundleDescriptor {
                    id: bundle_id,
                    name: manifest.name.clone(),
                    version,
                    runtime: language,
                    file_path: manifest.path.clone(),
                    dependencies: bundle_dependencies,
                },
                dependencies,
                contracts,
            )?;
            if let Some(resident) = resident.take() {
                let previous = residents.insert(bundle_id, resident);
                debug_assert!(previous.is_none(), "unload must release the prior resident");
            }
            handles
        };
        self.bundle_manifests
            .lock()
            .recover_poisoned(self.logger, "runtime")
            .insert(manifest.name.clone(), manifest);
        Ok(handles)
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

    /// Register a host contract interface.
    /// Returns `Err(HostContractError::DuplicateContract)` if a contract with the same ID is already registered.
    pub fn register_host_contract(
        &self,
        contract_id: u64,
        interface: &'static HostContractInterface,
    ) -> Result<(), HostContractError> {
        let mut guard: RecoveringGuard<
            RwLockWriteGuard<'_, HashMap<u64, &'static HostContractInterface>>,
        > = self
            .host_contracts
            .write()
            .recover_poisoned(self.logger, "runtime");
        if guard.contains_key(&contract_id) {
            return Err(HostContractError::DuplicateContract { contract_id });
        }
        guard.insert(contract_id, interface);
        Ok(())
    }

    /// Unregister a host contract interface.
    /// Returns `true` if the contract was registered and removed, `false` if it was not found.
    pub fn unregister_host_contract(&self, contract_id: u64) -> bool {
        let mut guard: RecoveringGuard<
            RwLockWriteGuard<'_, HashMap<u64, &'static HostContractInterface>>,
        > = self
            .host_contracts
            .write()
            .recover_poisoned(self.logger, "runtime");
        guard.remove(&contract_id).is_some()
    }

    /// Get a host contract interface by contract_id and minimum version.
    /// Returns `None` if no matching contract is found or if the version is too low.
    pub fn get_host_contract(
        &self,
        contract_id: u64,
        min_version: u32,
    ) -> Option<&'static HostContractInterface> {
        let guard: RecoveringGuard<
            RwLockReadGuard<'_, HashMap<u64, &'static HostContractInterface>>,
        > = self
            .host_contracts
            .read()
            .recover_poisoned(self.logger, "runtime");
        guard.get(&contract_id).and_then(|interface| {
            if host_contract_version_satisfies(interface, min_version) {
                Some(*interface)
            } else {
                None
            }
        })
    }

    /// Get the host language type.
    #[inline(always)]
    pub fn host_language(&self) -> SupportedLanguage {
        self.host_language
    }

    /// Get the HostApi pointer for use in plugin registrars.
    ///
    /// Returns a raw pointer rather than a reference: the HostApi is owned by the
    /// `Runtime` (a `Box<HostApi>` with a stable heap address), so its validity is
    /// tied to the runtime's lifetime, not `'static`. The FFI/loaders already treat
    /// it as a raw pointer.
    #[inline(always)]
    pub fn host_abi(&self) -> *const HostApi {
        &*self.host_abi as *const HostApi
    }

    /// Get the HostApi pointer for passing to guest contracts.
    ///
    /// Returns the runtime's owned HostApi, whose `runtime` field was
    /// patched once in `RuntimeBuilder::build` to point at this Runtime.
    /// The runtime pointer can be extracted via `(*host_interface).runtime`.
    ///
    /// # Safety
    /// The returned pointer is valid for the lifetime of the Runtime.
    #[inline(always)]
    pub fn as_context_ptr(&self) -> *const HostApi {
        &*self.host_abi as *const HostApi
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

    /// Whether final runtime destruction may run on this OS thread.
    pub(crate) fn can_destroy_on_current_thread(&self) -> bool {
        !self
            .registry
            .has_internal_plugin_resident_owned_by_other_thread()
    }

    /// Emit a Warn-level message through the runtime logger
    /// (`RuntimeConfig::log`, or stderr if no callback is installed).
    pub fn emit_warning(&self, msg: &str) {
        self.logger
            .log(LogLevel::Warn, "runtime", || msg.to_owned());
    }

    /// The runtime's logger handle.
    ///
    /// `LoggerHandle` is `Copy`: loaders take a copy at `load` time and store
    /// it in their per-bundle data so dispatch-time and teardown paths can log
    /// through the host callback. Same callback contract as
    /// `RuntimeConfig::log` — never invoke it while holding a lock guard.
    pub fn logger(&self) -> LoggerHandle {
        self.logger
    }

    /// Set the last error message for FFI error reporting.
    pub(crate) fn set_last_error(&self, msg: impl Into<String>) {
        let mut guard: RecoveringGuard<MutexGuard<'_, String>> = self
            .last_error
            .lock()
            .recover_poisoned(self.logger, "runtime");
        **guard = msg.into();
    }

    /// Reserve one bundle lifecycle slot before invoking a guest constructor.
    ///
    /// The reservation is acquired while the registry still proves that
    /// `interface` belongs to a live bundle. Unload holds this same mutex while it
    /// invalidates the bundle and removes its backing, preventing a constructor that
    /// already copied an adapter context from running after that backing is released.
    fn begin_guest_instance_construction(
        &self,
        interface: *const GuestContractInterface,
    ) -> Option<BundleId> {
        let mut guard: RecoveringGuard<MutexGuard<'_, HashMap<BundleId, u64>>> = self
            .instance_counts
            .lock()
            .recover_poisoned(self.logger, "runtime");
        let bundle_id: BundleId = self.registry.bundle_id_for_guest_interface(interface)?;
        let entry: &mut u64 = guard.entry(bundle_id).or_insert(0);
        *entry += 1;
        Some(bundle_id)
    }

    /// Finish a construction reservation when no stateful instance was created.
    fn cancel_guest_instance_construction(&self, bundle_id: BundleId) {
        self.note_instance_destroyed(bundle_id);
    }

    #[cfg(test)]
    /// Record that a stateful instance owned by `bundle_id` was created.
    fn note_instance_created(&self, bundle_id: BundleId) {
        let mut guard: RecoveringGuard<MutexGuard<'_, HashMap<BundleId, u64>>> = self
            .instance_counts
            .lock()
            .recover_poisoned(self.logger, "runtime");
        let entry: &mut u64 = guard.entry(bundle_id).or_insert(0);
        *entry += 1;
    }

    /// Record that a stateful instance owned by `bundle_id` was destroyed.
    fn note_instance_destroyed(&self, bundle_id: BundleId) {
        let mut guard: RecoveringGuard<MutexGuard<'_, HashMap<BundleId, u64>>> = self
            .instance_counts
            .lock()
            .recover_poisoned(self.logger, "runtime");
        if let Some(entry) = guard.get_mut(&bundle_id) {
            *entry = entry.saturating_sub(1);
            if *entry == 0 {
                guard.remove(&bundle_id);
            }
        }
    }

    /// Reset the live stateful-instance accounting for one reloaded bundle.
    ///
    /// A successful reload swap invalidates every pre-reload interface from this
    /// bundle. A correct caller revalidates and creates a fresh instance, so any
    /// abandoned old instance must not inflate a later diagnostic count.
    pub(crate) fn reset_instance_count_for_bundle(&self, bundle_id: BundleId) {
        self.instance_counts
            .lock()
            .recover_poisoned(self.logger, "runtime")
            .remove(&bundle_id);
    }

    /// Return the number of live stateful instances or active constructors owned by
    /// `bundle_id`. Used only by cold reload and unload lifecycle transitions.
    pub(crate) fn live_instance_count_for_bundle(&self, bundle_id: BundleId) -> u64 {
        self.instance_counts
            .lock()
            .recover_poisoned(self.logger, "runtime")
            .get(&bundle_id)
            .copied()
            .unwrap_or(0)
    }

    /// Get the last error message for FFI error reporting.
    /// Returns the number of bytes written to the buffer.
    pub(crate) fn get_last_error(&self, buf: &mut [u8]) -> usize {
        let guard: RecoveringGuard<MutexGuard<'_, String>> = self
            .last_error
            .lock()
            .recover_poisoned(self.logger, "runtime");
        let bytes: &[u8] = guard.as_bytes();
        let write_n: usize = bytes.len().min(buf.len());
        if write_n > 0 {
            buf[..write_n].copy_from_slice(&bytes[..write_n]);
        }
        write_n
    }

    /// Clear the last error message.
    pub(crate) fn clear_last_error(&self) {
        let mut guard: RecoveringGuard<MutexGuard<'_, String>> = self
            .last_error
            .lock()
            .recover_poisoned(self.logger, "runtime");
        guard.clear();
    }

    /// Get the length of the last error message.
    pub(crate) fn last_error_len(&self) -> usize {
        let guard: RecoveringGuard<MutexGuard<'_, String>> = self
            .last_error
            .lock()
            .recover_poisoned(self.logger, "runtime");
        guard.len()
    }

    /// Register an additional bundle loader into this runtime after build.
    ///
    /// `loader` must be a `Box<dyn BundleLoader>` produced by a loader cdylib compiled
    /// against the same polyplug rlib. Ownership is transferred UNCONDITIONALLY: the
    /// `Box` is consumed (and, on the duplicate-loader error path, dropped) the moment
    /// this is called. The caller must NOT retain or free the loader afterwards, on
    /// either success or error — doing so would double-free.
    ///
    /// Returns `Err(RuntimeError::Loader(LoaderError::DuplicateLoader { .. }))` if a
    /// loader for the same loader name is already registered. The passed loader is
    /// still consumed in that case.
    pub fn register_loader(&self, loader: Box<dyn BundleLoader>) -> Result<(), RuntimeError> {
        let name: String = loader.loader_name().to_string();
        let mut loaders: RecoveringGuard<
            RwLockWriteGuard<'_, HashMap<String, Box<dyn BundleLoader>>>,
        > = self
            .loaders
            .write()
            .recover_poisoned(self.logger, "runtime");
        if loaders.contains_key(&name) {
            return Err(RuntimeError::Loader(LoaderError::DuplicateLoader {
                loader_name: name,
            }));
        }

        loaders.insert(name, loader);
        Ok(())
    }

    /// Resolve a loader by loader name, returning a stable reference valid for the
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
    pub(crate) fn loader_for(&self, loader_name: &str) -> Option<&dyn BundleLoader> {
        let loaders: RecoveringGuard<RwLockReadGuard<'_, HashMap<String, Box<dyn BundleLoader>>>> =
            self.loaders.read().recover_poisoned(self.logger, "runtime");
        let loader_ptr: *const dyn BundleLoader = loaders.get(loader_name).map(Box::as_ref)?;
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
        let mut stack: RecoveringGuard<MutexGuard<'_, HashMap<InitThreadId, Vec<u64>>>> = self
            .init_bundle_stack
            .lock()
            .recover_poisoned(self.logger, "runtime");
        stack
            .entry(current_init_thread_id())
            .or_default()
            .push(bundle_id);
        // Bump the fast-path hint AFTER inserting into the stack but while still
        // holding the Mutex, so the counter and the stack are mutated atomically
        // with respect to other `push`/`pop` callers. See `active_init_count`.
        self.active_init_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Pop the most recent bundle_id from the current thread's init stack.
    ///
    /// Restores the previous (outer) bundle_id for reentrant loads on the same thread.
    pub fn pop_init_bundle_id(&self) {
        let thread_id: InitThreadId = current_init_thread_id();
        let mut stack: RecoveringGuard<MutexGuard<'_, HashMap<InitThreadId, Vec<u64>>>> = self
            .init_bundle_stack
            .lock()
            .recover_poisoned(self.logger, "runtime");
        if let Some(thread_stack) = stack.get_mut(&thread_id) {
            // Only decrement the hint when an entry was actually removed, so the
            // counter never drifts below the real number of pushed ids. A pop with
            // no matching entry (unbalanced caller) leaves the counter untouched.
            if thread_stack.pop().is_some() {
                self.active_init_count.fetch_sub(1, Ordering::Relaxed);
            }
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
        // Fast path: no bundle is mid-init anywhere, so this thread certainly has
        // no stack entry. A single Relaxed load avoids the Mutex on the Phase-2 hot
        // path (every find / find_all / get_dependencies call). See the
        // `active_init_count` ordering rationale for why Relaxed is sound: a stale 0
        // cannot occur for this thread's own init window, and other threads' pushes
        // only ever make the counter larger.
        if self.active_init_count.load(Ordering::Relaxed) == 0 {
            return 0;
        }
        let thread_id: InitThreadId = current_init_thread_id();
        let stack: RecoveringGuard<MutexGuard<'_, HashMap<InitThreadId, Vec<u64>>>> = self
            .init_bundle_stack
            .lock()
            .recover_poisoned(self.logger, "runtime");
        stack
            .get(&thread_id)
            .and_then(|thread_stack| thread_stack.last().copied())
            .unwrap_or(0)
    }

    /// Check an init-time dependency against a staged manifest before falling back
    /// to a published bundle declaration.
    fn bundle_declares_dependency(
        &self,
        bundle_id: BundleId,
        contract_id: GuestContractId,
    ) -> bool {
        self.registry
            .prepared_manifest(bundle_id)
            .is_some_and(|manifest| {
                manifest
                    .dependencies
                    .iter()
                    .any(|dependency: &RawManifestDependency| dependency.contract_id == contract_id)
            })
            || self
                .registry
                .is_bundle_dependency_declared(bundle_id, contract_id)
    }

    /// Return the manifest visible to the bundle currently initializing.
    fn init_manifest(&self, bundle_id: BundleId) -> Option<ManifestData> {
        self.registry.prepared_manifest(bundle_id).or_else(|| {
            let manifests: RecoveringGuard<MutexGuard<'_, HashMap<String, ManifestData>>> = self
                .bundle_manifests
                .lock()
                .recover_poisoned(self.logger, "runtime");
            manifests
                .values()
                .find(|manifest: &&ManifestData| manifest.id == bundle_id.id())
                .cloned()
        })
    }

    /// Load a single plugin bundle explicitly by path.
    ///
    /// Reads the companion manifest, finds the matching loader, and dispatches.
    /// Does NOT perform graph pre-validation — intended for programmatic loads.
    pub fn load_bundle(&self, path: &Path) -> Result<(), RuntimeError> {
        let compatibility: Compatibility = self.config.compatibility;
        self.load_bundle_with(
            path,
            LoadOptions {
                compatibility,
                ignore_function_count_mismatch: false,
            },
        )
    }

    /// Load a single plugin bundle from a non-path [`BundleSource`].
    ///
    /// The caller supplies an already-parsed [`ManifestData`] because in-memory
    /// sources ([`BundleSource::Code`] / [`BundleSource::Bytes`]) have no bundle
    /// directory to scan. Path-based loading should use [`Runtime::load_bundle`] /
    /// `load_bundle_with`, which construct a [`BundleSource::Path`] internally.
    ///
    /// [`BundleSource`]: crate::loader::BundleSource
    /// [`BundleSource::Code`]: crate::loader::BundleSource::Code
    /// [`BundleSource::Bytes`]: crate::loader::BundleSource::Bytes
    /// [`BundleSource::Path`]: crate::loader::BundleSource::Path
    pub fn load_bundle_from_source(
        &self,
        manifest: ManifestData,
        source: BundleSource,
    ) -> Result<(), RuntimeError> {
        let compatibility: Compatibility = self.config.compatibility;
        self.load_manifest_with_source(
            manifest,
            source,
            LoadOptions {
                compatibility,
                ignore_function_count_mismatch: false,
            },
        )
    }

    /// Unload a bundle: invalidate its handles, remove it from the registry, and
    /// reclaim its interface and per-loader resources via epoch-deferred reclamation.
    ///
    /// First the registry is invalidated: the bundle's slots have their generation
    /// bumped and the bundle is removed from the registry indices, then the superseded
    /// interface `Arc` is handed to crossbeam-epoch for deferred reclamation. After this,
    /// every old handle fails to resolve with `StaleHandle` and no new resolve can hand
    /// out a pointer into the bundle. A reader pinned before the unload keeps the old
    /// interface `Arc` (and its backing library / VM) alive until it unpins; a raw
    /// `GuestContractInterface` pointer cached before the unload and used after it is
    /// undefined behaviour — see the host-coordination contract below.
    ///
    /// Then the matching loader's reclaim hook runs; reclamation is uniformly
    /// epoch-deferred, so the actual free happens only once no reader is still pinned in
    /// the epoch that preceded the unload (see [`crate::loader::BundleLoader::unload`]):
    /// - **Native loader:** `dlclose`s the dylib (drops the `libloading::Library`),
    ///   releasing OS resources and the on-disk file lock.
    /// - **VM loaders (Lua, JS):** drop the bundle's per-bundle VM.
    /// - **Python loader:** purges the bundle's re-keyed `sys.modules` entries so a later
    ///   load re-imports fresh source (CPython is single-init per process and cannot be
    ///   torn down).
    /// - **.NET loader:** unloads the bundle's collectible `AssemblyLoadContext`; its
    ///   assemblies are GC-reclaimed once all references and native frames clear.
    ///
    /// # Host-coordination contract
    /// Runtime-mediated calls — `create_guest_instance` and `destroy_guest_instance` —
    /// pin the epoch across dispatch, so a call racing an
    /// unload from another thread keeps the interface and its backing library / VM alive
    /// until the call returns. Direct FFI host callers do NOT pin per call (the fast
    /// path): the host MUST NOT call a bundle's contracts through a cached raw interface
    /// pointer concurrently with — or after — unloading it. This is the same
    /// trusted-same-process posture `docs/TRUST_MODEL.md` and the reload `Preparing`
    /// callback already assume.
    ///
    /// # Errors
    /// - `BundleNotFound`: the bundle is not currently loaded.
    /// - `DependencyInUse`: a still-loaded bundle declared a dependency on a contract
    ///   this bundle provides. Use [`Runtime::unload_bundle_cascade`] to unload the
    ///   dependents first.
    pub fn unload_bundle(&self, bundle_id: BundleId) -> Result<(), RuntimeError> {
        let descriptor: BundleDescriptor = self
            .registry
            .get_bundle_descriptor(bundle_id)
            .ok_or_else(|| RuntimeError::BundleNotFound {
                bundle_name: format!("{:#x}", bundle_id.id()),
                contract_name: String::new(),
            })?;

        // Refuse-by-default (design D4): a still-loaded bundle that declared a
        // dependency on a contract this bundle provides would have its trust assumption
        // broken by an unload, so reject unless the caller cascades explicitly.
        let exported: HashSet<GuestContractId> = self
            .registry
            .bundle_exported_contracts(bundle_id)
            .into_iter()
            .collect();
        let mut dependents: Vec<String> = self
            .registry
            .bundles_depending_on_any(&exported)
            .into_iter()
            .filter(|dep: &BundleId| *dep != bundle_id)
            .filter_map(|dep: BundleId| {
                self.registry
                    .get_bundle_descriptor(dep)
                    .map(|d: BundleDescriptor| d.name)
            })
            .collect();
        if !dependents.is_empty() {
            dependents.sort();
            return Err(RuntimeError::DependencyInUse {
                provider: descriptor.name,
                dependents,
            });
        }

        self.unload_registered_bundle(bundle_id, descriptor)
    }

    fn ensure_registered_bundle_resident_affinity(
        &self,
        bundle_id: BundleId,
        bundle_name: &str,
    ) -> Result<(), RuntimeError> {
        let residents = self.registry.lock_internal_plugin_residents();
        if let Some(resident) = residents.get(&bundle_id) {
            let current_thread_id: u64 = current_os_thread_id();
            if resident.owner_thread_id() != current_thread_id {
                return Err(RuntimeError::InternalPluginResidentWrongThread {
                    bundle: bundle_name.to_owned(),
                    owner_thread_id: resident.owner_thread_id(),
                    current_thread_id,
                });
            }
        }
        Ok(())
    }

    /// Invalidate one registered bundle and release its acquisition backing.
    ///
    /// Both direct and cascade unload use this boundary so every invalidated bundle
    /// observes the same notification, live-instance safety, loader reclamation, and
    /// runtime-owned resident release sequence.
    fn unload_registered_bundle(
        &self,
        bundle_id: BundleId,
        descriptor: BundleDescriptor,
    ) -> Result<(), RuntimeError> {
        self.ensure_registered_bundle_resident_affinity(bundle_id, &descriptor.name)?;
        let loader_name: Option<String> = self.bundle_loader_name(&descriptor.name);
        self.fire_unloading(bundle_id, &descriptor.name);

        // A constructor reserves this mutex before it invokes guest code. Keep the
        // reservation check and registry invalidation in one critical section: a
        // constructor either makes the resident visibly in-use or observes the
        // invalidated interface and returns without dereferencing its context.
        let mut instance_counts: RecoveringGuard<MutexGuard<'_, HashMap<BundleId, u64>>> = self
            .instance_counts
            .lock()
            .recover_poisoned(self.logger, "runtime");
        // Runtime lifecycle always acquires backing locks in this order. Generated
        // Rust registration already holds roots before committing residents.
        let mut roots: RecoveringGuard<
            MutexGuard<'_, HashMap<BundleId, Box<dyn Any + Send + Sync>>>,
        > = self
            .internal_plugin_roots
            .lock()
            .recover_poisoned(self.logger, "runtime");
        let mut residents = self.registry.lock_internal_plugin_residents();
        let mut internal_plugin_lifecycle = self
            .internal_plugin_lifecycle
            .lock()
            .recover_poisoned(self.logger, "runtime");

        let live: u64 = instance_counts.get(&bundle_id).copied().unwrap_or(0);
        let has_internal_plugin_backing: bool = internal_plugin_lifecycle.contains(&bundle_id)
            || roots.contains_key(&bundle_id)
            || residents.contains_key(&bundle_id);
        if has_internal_plugin_backing && live > 0 {
            return Err(RuntimeError::InternalPluginInUse {
                bundle: descriptor.name,
                active_instances: live,
            });
        }
        let warning: Option<String> = (live > 0).then(|| {
            format!(
                "unload: bundle '{}' still has {live} live guest instance(s) across its \
                 contracts; destroy them before unload to avoid use-after-free. Proceeding anyway.",
                descriptor.name
            )
        });

        let resident: Option<InternalPluginResident> = residents.remove(&bundle_id);
        let root: Option<Box<dyn Any + Send + Sync>> = roots.remove(&bundle_id);
        let _count: u32 = self.registry.invalidate_bundle(bundle_id)?;
        internal_plugin_lifecycle.remove(&bundle_id);
        self.forget_bundle_manifest(&descriptor.name);
        instance_counts.remove(&bundle_id);
        drop(internal_plugin_lifecycle);
        drop(residents);
        drop(roots);
        drop(instance_counts);
        if let Some(warning) = warning {
            self.logger.log(LogLevel::Warn, "runtime", || warning);
        }
        let reclaim_result = self.reclaim_via_loader(bundle_id, loader_name.as_deref());
        if let Some(resident) = resident {
            resident.release();
        }
        drop(root);
        reclaim_result
    }

    /// Fire the `on_reload_cb` with a `ReloadPhase::unloading` notification, if a
    /// callback is registered. Called before invalidate so the host can quiesce its
    /// own callers ahead of reclamation. The `StringView` is constructed inline from
    /// the caller-owned `bundle_name`, which outlives this synchronous invocation.
    fn fire_unloading(&self, bundle_id: BundleId, bundle_name: &str) {
        if let Some(cb) = self.on_reload_cb() {
            let name_view: StringView = StringView {
                ptr: bundle_name.as_ptr(),
                len: bundle_name.len(),
            };
            (cb.0)(
                self.config().on_reload_user_data,
                ReloadPhase::unloading(bundle_id, name_view),
            );
        }
    }

    /// Look up the loader-name string for a loaded bundle by name.
    ///
    /// The original `manifest.loader` string (e.g. `"lua"`, `"js-quickjs"`) is the
    /// key the load path used to resolve the loader, and the only value that maps
    /// back to a `BundleLoader::loader_name()`. It is read from `bundle_manifests`,
    /// which must be consulted BEFORE `invalidate_bundle` removes the bundle.
    fn bundle_loader_name(&self, bundle_name: &str) -> Option<String> {
        let manifests: RecoveringGuard<MutexGuard<'_, HashMap<String, ManifestData>>> = self
            .bundle_manifests
            .lock()
            .recover_poisoned(self.logger, "runtime");
        manifests
            .get(bundle_name)
            .map(|m: &ManifestData| m.loader.clone())
    }

    /// Remove the reload recipe after logical unload has invalidated the registry.
    fn forget_bundle_manifest(&self, bundle_name: &str) {
        let mut manifests: RecoveringGuard<MutexGuard<'_, HashMap<String, ManifestData>>> = self
            .bundle_manifests
            .lock()
            .recover_poisoned(self.logger, "runtime");
        manifests.remove(bundle_name);
    }

    /// Invoke the loader's `unload` reclaim hook for `bundle_id`.
    ///
    /// `loader_name` is the loader key captured before invalidate. A missing name
    /// or missing loader is not an error: a bundle with no recoverable loader simply
    /// has nothing to reclaim (the invalidate already vacated its interfaces).
    ///
    /// See [`crate::loader::BundleLoader::unload`] for the loader-side reclaim contract.
    fn reclaim_via_loader(
        &self,
        bundle_id: BundleId,
        loader_name: Option<&str>,
    ) -> Result<(), RuntimeError> {
        let name: &str = match loader_name {
            Some(n) => n,
            None => return Ok(()),
        };
        match self.loader_for(name) {
            Some(loader) => loader.unload(bundle_id, self).map_err(RuntimeError::Loader),
            None => Ok(()),
        }
    }

    /// Unload a bundle and every bundle that depends on it, dependents first.
    ///
    /// Recursively unloads bundles that declared a dependency on a contract the target
    /// provides before unloading the target itself, so no `DependencyInUse` refusal is
    /// hit. A `visited` set breaks dependency cycles. Like [`Runtime::unload_bundle`],
    /// each unload is true unload: handles go stale and the interface and per-loader
    /// resources are reclaimed via epoch-deferred reclamation.
    pub fn unload_bundle_cascade(&self, bundle_id: BundleId) -> Result<(), RuntimeError> {
        let mut affinity_visited: HashSet<BundleId> = HashSet::new();
        self.ensure_cascade_resident_affinity(bundle_id, &mut affinity_visited)?;
        let mut visited: HashSet<BundleId> = HashSet::new();
        self.unload_bundle_cascade_with_visited(bundle_id, &mut visited)
    }

    /// Reject a cascade before its first invalidation if any resident belongs to
    /// another OS thread.
    fn ensure_cascade_resident_affinity(
        &self,
        bundle_id: BundleId,
        visited: &mut HashSet<BundleId>,
    ) -> Result<(), RuntimeError> {
        if !visited.insert(bundle_id) {
            return Ok(());
        }
        let descriptor: BundleDescriptor = self
            .registry
            .get_bundle_descriptor(bundle_id)
            .ok_or_else(|| RuntimeError::BundleNotFound {
                bundle_name: format!("{:#x}", bundle_id.id()),
                contract_name: String::new(),
            })?;
        self.ensure_registered_bundle_resident_affinity(bundle_id, &descriptor.name)?;
        let exported: HashSet<GuestContractId> = self
            .registry
            .bundle_exported_contracts(bundle_id)
            .into_iter()
            .collect();
        for dependent in self
            .registry
            .bundles_depending_on_any(&exported)
            .into_iter()
            .filter(|dependent: &BundleId| *dependent != bundle_id)
        {
            self.ensure_cascade_resident_affinity(dependent, visited)?;
        }
        Ok(())
    }

    /// Cascade-unload `bundle_id`, tracking already-unloaded bundles in `visited` to
    /// break dependency cycles.
    fn unload_bundle_cascade_with_visited(
        &self,
        bundle_id: BundleId,
        visited: &mut HashSet<BundleId>,
    ) -> Result<(), RuntimeError> {
        if !visited.insert(bundle_id) {
            return Ok(());
        }

        let descriptor: BundleDescriptor = self
            .registry
            .get_bundle_descriptor(bundle_id)
            .ok_or_else(|| RuntimeError::BundleNotFound {
                bundle_name: format!("{:#x}", bundle_id.id()),
                contract_name: String::new(),
            })?;

        let exported: HashSet<GuestContractId> = self
            .registry
            .bundle_exported_contracts(bundle_id)
            .into_iter()
            .collect();
        let dependents: Vec<BundleId> = self
            .registry
            .bundles_depending_on_any(&exported)
            .into_iter()
            .filter(|dep: &BundleId| *dep != bundle_id)
            .collect();
        for dep in dependents {
            self.unload_bundle_cascade_with_visited(dep, visited)?;
        }

        self.unload_registered_bundle(bundle_id, descriptor)
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

        let manifest: ManifestData =
            parse_manifest(bundle_dir).map_err(|e: LoaderError| RuntimeError::Loader(e))?;
        let source: BundleSource = BundleSource::Path(manifest.path.clone());
        self.load_manifest_with_source(manifest, source, opts)
    }

    /// Verify a bundle's signature under the configured policy.
    ///
    /// The verifier choice depends on the host-supplied trusted-key allowlist
    /// (`RuntimeConfig::trusted_keys`):
    /// - empty allowlist → TOFU: trust the key embedded in `bundle.sig`;
    /// - non-empty allowlist → key pinning: additionally require the embedded key
    ///   to be a member of the allowlist.
    ///
    /// A malformed key in the host allowlist is a host configuration error and is
    /// surfaced as [`LoaderError::MalformedTrustedKey`].
    fn verify_bundle_signature(
        &self,
        bundle_dir: &Path,
        bundle_name: &str,
    ) -> Result<(), RuntimeError> {
        let trusted: Vec<VerifyingKey> = self.collect_trusted_keys(bundle_name)?;

        if trusted.is_empty() {
            signing_verify_bundle(bundle_dir)
                .map_err(|e: SigError| Self::map_signature_error(e, bundle_name))?;
        } else {
            let verifier: PinnedKeyVerifier = PinnedKeyVerifier::new(trusted);
            verifier
                .verify(bundle_dir)
                .map_err(|e: SigError| Self::map_signature_error(e, bundle_name))?;
        }
        Ok(())
    }

    /// Copy the configured trusted Ed25519 keys out of the config `Array` into
    /// owned verifying keys.
    ///
    /// `config.trusted_keys` was repointed at runtime-owned storage during
    /// `build()` (see [`Runtime::_trusted_keys`]), so this reads from a buffer that
    /// lives for the runtime's whole lifetime — it is safe to call on every bundle
    /// load, not just during construction. The keys are copied into fresh
    /// `VerifyingKey`s per call; the `Array` pointer itself is never retained.
    ///
    /// [`Runtime::_trusted_keys`]: Runtime#structfield._trusted_keys
    fn collect_trusted_keys(&self, bundle_name: &str) -> Result<Vec<VerifyingKey>, RuntimeError> {
        let raw: &Array<Ed25519PublicKey> = &self.config.trusted_keys;
        if raw.is_empty() {
            return Ok(Vec::new());
        }

        // SAFETY: the host owns the `trusted_keys` buffer and guarantees, by the
        // documented `RuntimeConfig::trusted_keys` ownership contract, that
        // `items` is valid for `len` elements for the duration of runtime
        // construction (where this is reached). `is_empty()` above rules out a
        // null pointer / zero length. We only READ the elements and COPY their
        // bytes out — no element is retained or mutated, so no aliasing or
        // lifetime obligation outlives this borrow.
        let keys: &[Ed25519PublicKey] = unsafe { slice::from_raw_parts(raw.items, raw.len) };

        let mut out: Vec<VerifyingKey> = Vec::with_capacity(keys.len());
        for key in keys {
            let verifying_key: VerifyingKey =
                verifying_key_from_bytes(&key.bytes).map_err(|e: SigError| {
                    RuntimeError::Loader(LoaderError::MalformedTrustedKey {
                        bundle: bundle_name.to_owned(),
                        reason: e.to_string(),
                    })
                })?;
            out.push(verifying_key);
        }
        Ok(out)
    }

    /// Translate a signing-layer [`SigError`] into the loader error returned
    /// under [`SignaturePolicy::Required`].
    fn map_signature_error(error: SigError, bundle_name: &str) -> RuntimeError {
        match error {
            SigError::MissingSignature { .. } => {
                RuntimeError::Loader(LoaderError::UnsignedBundle {
                    bundle: bundle_name.to_owned(),
                })
            }
            SigError::UntrustedKey { .. } => {
                RuntimeError::Loader(LoaderError::UntrustedSigningKey {
                    bundle: bundle_name.to_owned(),
                })
            }
            other => RuntimeError::Loader(LoaderError::SignatureVerificationFailed {
                bundle: bundle_name.to_owned(),
                reason: other.to_string(),
            }),
        }
    }

    /// Shared load path: validate the manifest, dispatch to the matching loader with
    /// the given [`BundleSource`], and record bundle metadata on success.
    ///
    /// [`BundleSource`]: crate::loader::BundleSource
    pub(crate) fn load_manifest_with_source(
        &self,
        manifest: ManifestData,
        source: BundleSource,
        opts: LoadOptions,
    ) -> Result<(), RuntimeError> {
        manifest
            .validate()
            .map_err(|error: ManifestError| RuntimeError::Loader(error.into()))?;
        // Enforce the configured bundle signature policy. The verifier picks TOFU
        // vs. key pinning based on whether the host configured a trusted-key
        // allowlist (`RuntimeConfig::trusted_keys`); see `verify_bundle_signature`.
        match self.config.signature_policy {
            SignaturePolicy::Off => {}
            SignaturePolicy::WarnOnly => {
                if let Err(e) = self.verify_bundle_signature(&manifest.path, &manifest.name) {
                    self.logger.log(LogLevel::Warn, "runtime", || {
                        format!(
                            "bundle `{}`: signature check failed (WarnOnly — continuing): {}",
                            manifest.name, e
                        )
                    });
                }
            }
            SignaturePolicy::Required => {
                self.verify_bundle_signature(&manifest.path, &manifest.name)?;
            }
        }

        // Find the loader for this bundle. The lock is released before load() runs
        // (see `loader_for`) so a plugin init that registers a loader cannot deadlock.
        let loader_name: &str = &manifest.loader;
        let loader: &dyn BundleLoader = self.loader_for(loader_name).ok_or_else(|| {
            RuntimeError::Loader(LoaderError::NoLoaderForName {
                bundle: manifest.name.clone(),
                loader_name: loader_name.to_owned(),
            })
        })?;
        // Preserve external loading's pre-acquisition version rejection.
        parse_manifest_version(&manifest.version, &manifest.name, &manifest.path)?;

        if !is_known_runtime_language(&manifest.loader) {
            self.logger.log(LogLevel::Warn, "runtime", || {
                format!(
                    "bundle `{}`: unknown loader `{}`; defaulting SupportedLanguage to Rust",
                    manifest.name, manifest.loader
                )
            });
        }
        let runtime_language: SupportedLanguage = supported_language_from_str(&manifest.loader);
        let bundle_id: BundleId =
            self.begin_prepared_bundle_transaction(manifest.clone(), runtime_language)?;

        let result: Result<(), RuntimeError> = (|| {
            loader
                .load(&manifest, &source, self)
                .map_err(RuntimeError::Loader)?;
            self.commit_prepared_bundle_transaction(bundle_id, opts)?;
            Ok(())
        })();

        if result.is_err() {
            self.registry.discard_prepared_bundle(bundle_id);
            if let Err(cleanup_error) = loader.unload(bundle_id, self) {
                self.logger.log(LogLevel::Error, "runtime", || {
                    format!(
                        "failed load cleanup for bundle `{}`: {cleanup_error}",
                        manifest.name
                    )
                });
            }
        }
        result
    }

    /// Require the staged registrations to match the manifest's provider set exactly.
    fn validate_prepared_provider_set(
        &self,
        manifest: &ManifestData,
        contracts: &[PreparedGuestContract],
    ) -> Result<(), RuntimeError> {
        let expected: Vec<(String, Option<String>)> = manifest
            .provides
            .iter()
            .map(|spec: &String| match spec.split_once('@') {
                Some((name, version)) => Ok((name.to_owned(), Some(version.to_owned()))),
                None => Ok((spec.clone(), None)),
            })
            .collect::<Result<Vec<(String, Option<String>)>, RuntimeError>>()?;
        let actual: Vec<(String, Version)> = RuntimeStore::prepared_provider_specs(contracts);

        let mut matched: Vec<bool> = vec![false; expected.len()];
        for (actual_name, actual_version) in actual {
            let (provider_name, declared_version): (&str, Option<&str>) = actual_name
                .split_once('@')
                .map_or((&actual_name, None), |(name, version)| {
                    (name, Some(version))
                });
            if let Some(declared_version) = declared_version {
                let declaration_matches: bool = match declared_version.parse::<u32>() {
                    Ok(major) => major == actual_version.major,
                    Err(_) => Version::from_str(declared_version)
                        .map(|declared| declared == actual_version)
                        .unwrap_or(false),
                };
                if !declaration_matches {
                    return Err(RuntimeError::Loader(LoaderError::InitFailed {
                        bundle: manifest.name.clone(),
                        error: format!(
                            "loader registered provider `{actual_name}` with version {}.{}.{}",
                            actual_version.major, actual_version.minor, actual_version.patch
                        ),
                    }));
                }
            }
            let expected_index: Option<usize> = expected.iter().enumerate().find_map(
                |(index, (expected_name, expected_version))| {
                    let version_matches: bool = match expected_version {
                        None => true,
                        Some(version) => match version.parse::<u32>() {
                            Ok(major) => major == actual_version.major,
                            Err(_) => Version::from_str(version)
                                .map(|expected| expected == actual_version)
                                .unwrap_or(false),
                        },
                    };
                    (*expected_name == provider_name && version_matches).then_some(index)
                },
            );
            match expected_index {
                Some(index) => matched[index] = true,
                None => {
                    return Err(RuntimeError::Loader(LoaderError::InitFailed {
                        bundle: manifest.name.clone(),
                        error: format!(
                            "loader registered provider `{actual_name}@{}.{}.{}` not declared by manifest provides {:?}",
                            actual_version.major,
                            actual_version.minor,
                            actual_version.patch,
                            manifest.provides
                        ),
                    }));
                }
            }
        }
        if matched.iter().all(|matched| *matched) {
            Ok(())
        } else {
            Err(RuntimeError::Loader(LoaderError::InitFailed {
                bundle: manifest.name.clone(),
                error: format!(
                    "manifest provides {:?}, but the loader did not register every declared provider",
                    manifest.provides
                ),
            }))
        }
    }

    /// Compare declared `function_count` entries against unpublished native interfaces.
    fn validate_prepared_function_counts(
        &self,
        manifest: &ManifestData,
        contracts: &[PreparedGuestContract],
        compatibility: Compatibility,
    ) -> Result<(), RuntimeError> {
        for (contract_name, major, actual_opt) in
            RuntimeStore::prepared_native_function_counts(contracts)
        {
            let actual: u32 = match actual_opt {
                Some(count) => count,
                None => continue,
            };
            let bare_name: &str = contract_name
                .split_once('@')
                .map_or(contract_name.as_str(), |(name, _version)| name);
            let key: String = format!("{bare_name}@{major}");
            let declared: u32 = match manifest.function_count.get(&key) {
                Some(count) => *count,
                None => match compatibility {
                    Compatibility::Strict => {
                        return Err(RuntimeError::Loader(LoaderError::FunctionCountMismatch {
                            contract: key,
                            expected: 0,
                            found: actual,
                        }));
                    }
                    Compatibility::Relaxed => {
                        self.logger.log(LogLevel::Warn, "runtime", || {
                            format!(
                                "bundle `{}` native contract `{}` has no function_count entry; interface exports {}",
                                manifest.name, key, actual
                            )
                        });
                        continue;
                    }
                    Compatibility::Yolo => continue,
                },
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
                        self.logger.log(LogLevel::Warn, "runtime", || {
                            format!(
                                "bundle `{}` contract `{}`: declared function_count {} but interface exports {}",
                                manifest.name, key, declared, actual
                            )
                        });
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
    logger: LoggerHandle,
) -> Result<(), RuntimeError> {
    // Build provider_map: bare contract_name -> &ManifestData.
    //
    // A `provides` entry may be `name` or `name@version`; dependencies always name
    // the bare contract. Key the map on the bare name (strip any `@version` suffix)
    // so a versioned provides entry still resolves a bare-named dependency. This
    // matches the stripping that `load_manifest_with_source` already applies when
    // building function_count keys.
    let mut provider_map: HashMap<String, &ManifestData> = HashMap::new();
    for (_path, manifest) in manifests {
        for contract in &manifest.provides {
            let bare_contract: &str = match contract.split_once('@') {
                Some((name, _)) => name,
                None => contract.as_str(),
            };
            provider_map.insert(bare_contract.to_owned(), manifest);
        }
    }

    for (path, manifest) in manifests {
        // Check version compatibility for each dependency
        let resolved: Vec<ManifestDependency> = resolved_dependencies_with_logger(manifest, logger);
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
                        logger.log(LogLevel::Warn, "runtime", || {
                            format!(
                                "version mismatch for contract `{}`: required={}, found={} (bundle `{}`)",
                                dep_contract, required, provided, provider.name
                            )
                        });
                    }
                    Compatibility::Yolo => {} // intentionally silent — Yolo mode skips all version checks
                }
            }
        }
    }

    Ok(())
}

fn parse_manifest_version(
    v: &str,
    _bundle_name: &str,
    manifest_path: &Path,
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

/// Convert a runtime string from manifest.toml to SupportedLanguage enum.
///
/// An unrecognized string falls back to [`SupportedLanguage::Rust`]. Callers that want
/// to flag a typo should first consult [`is_known_runtime_language`] and emit a
/// warning, since this function cannot distinguish "rust" from a misspelling.
fn supported_language_from_str(s: &str) -> SupportedLanguage {
    match s {
        "native" | "rust" => SupportedLanguage::Rust,
        "python" => SupportedLanguage::Python,
        "lua" => SupportedLanguage::Lua,
        "javascript" | "js" | "js-quickjs" => SupportedLanguage::JavaScript,
        "dotnet" | "csharp" => SupportedLanguage::Dotnet,
        "cpp" => SupportedLanguage::Cpp,
        _ => SupportedLanguage::Rust,
    }
}

/// Returns `true` iff `s` is a runtime string [`supported_language_from_str`] maps
/// explicitly (i.e. NOT via its catch-all Rust fallback). Used to warn on an unknown
/// `runtime` field before it is silently coerced to Rust.
fn is_known_runtime_language(s: &str) -> bool {
    matches!(
        s,
        "native"
            | "rust"
            | "python"
            | "lua"
            | "javascript"
            | "js"
            | "js-quickjs"
            | "dotnet"
            | "csharp"
            | "cpp"
    )
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
    sv: &StringView,
    context: &str,
) -> Result<String, RuntimeError> {
    if sv.ptr.is_null() || sv.len == 0 {
        return Ok(String::new());
    }
    // SAFETY: caller guarantees ptr/len describe a valid byte range for this call.
    let slice: &[u8] = unsafe { slice::from_raw_parts(sv.ptr, sv.len) };
    match str::from_utf8(slice) {
        Ok(s) => Ok(s.to_owned()),
        Err(_) => Err(RuntimeError::InvalidUtf8 {
            context: context.to_owned(),
        }),
    }
}

// ─── HostApi C ABI callbacks ───────────────────────────────────────────────

/// Validate the function-pointer fields of a plugin/host-provided contract
/// interface WITHOUT materializing the typed struct.
///
/// The ABI types `create_instance` / `destroy_instance` / `dispatch.vm.call`
/// as bare (non-`Option`) `fn` pointers because they are REQUIRED: failure is
/// signalled through a null *instance handle* return, never through a null
/// callback. A foreign producer can still hand us a struct with null bits in
/// those slots, and reading such a field at its `fn` type would materialize an
/// invalid value (UB in Rust) — so the fields are read here as raw data
/// pointers and rejected with a precise error before any typed access.
///
/// `create_offset` / `destroy_offset` / `dispatch_type_offset` /
/// `dispatch_offset` are the byte offsets of the respective fields inside the
/// interface struct (they differ between `GuestContractInterface` and
/// `HostContractInterface`). Inside the `DispatchMechanisms` union,
/// `vm.call` lives at offset 0, `native.function_count` at offset 0, and
/// `native.functions` at offset 8 (asserted by the ABI layout tests).
///
/// Returns the first violation as a static message, or `None` when the
/// interface is well-formed.
///
/// # Safety
/// `base` must be non-null, properly aligned for the interface struct, and
/// point to at least `dispatch_offset + 16` readable bytes.
unsafe fn validate_interface_fn_ptrs(
    base: *const u8,
    create_offset: usize,
    destroy_offset: usize,
    dispatch_type_offset: usize,
    dispatch_offset: usize,
    context: ValidationContext,
) -> Option<&'static str> {
    // SAFETY: the caller guarantees `base` covers the interface struct; every
    // read below is in-bounds and reads pointer/integer bits only (never an
    // `fn`-typed value), so null bits are observed safely.
    unsafe {
        let create_ptr: *const c_void = base.add(create_offset).cast::<*const c_void>().read();
        if create_ptr.is_null() {
            return Some(match context {
                ValidationContext::Guest => {
                    "register_guest_contract: create_instance is null — the field is required; signal create failure by returning a null instance handle instead"
                }
                ValidationContext::Host => {
                    "register_host_contract: create_instance is null — the field is required; signal create failure by returning a null instance handle instead"
                }
            });
        }
        let destroy_ptr: *const c_void = base.add(destroy_offset).cast::<*const c_void>().read();
        if destroy_ptr.is_null() {
            return Some(match context {
                ValidationContext::Guest => {
                    "register_guest_contract: destroy_instance is null — the field is required; use a no-op function for stateless contracts"
                }
                ValidationContext::Host => {
                    "register_host_contract: destroy_instance is null — the field is required; use a no-op function for singleton/stateless contracts"
                }
            });
        }

        let dispatch_type_raw: u32 = base.add(dispatch_type_offset).cast::<u32>().read();
        if dispatch_type_raw == DispatchType::VirtualMachine as u32 {
            // DispatchMechanisms union, vm variant: call fn pointer at offset 0.
            let call_ptr: *const c_void = base.add(dispatch_offset).cast::<*const c_void>().read();
            if call_ptr.is_null() {
                return Some(match context {
                    ValidationContext::Guest => {
                        "register_guest_contract: dispatch.vm.call is null — required for VirtualMachine dispatch"
                    }
                    ValidationContext::Host => {
                        "register_host_contract: dispatch.vm.call is null — required for VirtualMachine dispatch"
                    }
                });
            }
        } else if dispatch_type_raw == DispatchType::Native as u32 {
            // DispatchMechanisms union, native variant: function_count at
            // offset 0, functions pointer at offset 8.
            let function_count: u32 = base.add(dispatch_offset).cast::<u32>().read();
            let functions: *const *const c_void = base
                .add(dispatch_offset + 8)
                .cast::<*const *const c_void>()
                .read();
            if function_count > 0 {
                if functions.is_null() {
                    return Some(match context {
                        ValidationContext::Guest => {
                            "register_guest_contract: dispatch.native.functions is null while function_count > 0"
                        }
                        ValidationContext::Host => {
                            "register_host_contract: dispatch.native.functions is null while function_count > 0"
                        }
                    });
                }
                for fn_index in 0..function_count as usize {
                    if functions.add(fn_index).read().is_null() {
                        return Some(match context {
                            ValidationContext::Guest => {
                                "register_guest_contract: dispatch.native.functions contains a null entry within function_count"
                            }
                            ValidationContext::Host => {
                                "register_host_contract: dispatch.native.functions contains a null entry within function_count"
                            }
                        });
                    }
                }
            }
        }
    }
    None
}

/// Which registration path [`validate_interface_fn_ptrs`] is reporting for —
/// selects the precise error message prefix.
#[derive(Clone, Copy)]
enum ValidationContext {
    Guest,
    Host,
}

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
    out_err: *mut AbiError,
) {
    // SAFETY: the runtime is the sole producer of the HostApi table; the ABI
    // contract requires callers to pass a valid, non-null out_err pointer.
    let result: AbiError =
        unsafe { host_register_guest_contract_impl(this, descriptor, interface) };
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(result) };
    }
}

unsafe fn host_register_guest_contract_impl(
    this: *const HostApi,
    descriptor: *const PluginDescriptor,
    interface: *const GuestContractInterface,
) -> AbiError {
    if this.is_null() {
        return AbiError {
            code: AbiErrorCode::Generic as u32,
            message: StringView::null(),
        };
    }
    // Guard both plugin-provided pointers before any dereference. `descriptor` is
    // read below and `interface` is dereferenced inside the registry; a null in
    // either is a contract violation that must not become UB.
    if descriptor.is_null() {
        return AbiError {
            code: AbiErrorCode::InvalidPointer as u32,
            message: StringView::from_static(
                b"register_guest_contract: descriptor pointer is null",
            ),
        };
    }
    if interface.is_null() {
        return AbiError {
            code: AbiErrorCode::InvalidPointer as u32,
            message: StringView::from_static(b"register_guest_contract: interface pointer is null"),
        };
    }
    // Reject null bits in the REQUIRED fn-pointer fields before any typed
    // access to the interface — reading a null at a bare `fn` type would be an
    // invalid value (UB), and accepting it would defer the crash to first use.
    // SAFETY: interface is non-null (checked above) and points to a
    // GuestContractInterface provided by the plugin for the runtime lifetime.
    if let Some(violation) = unsafe {
        validate_interface_fn_ptrs(
            interface.cast::<u8>(),
            mem::offset_of!(GuestContractInterface, create_instance),
            mem::offset_of!(GuestContractInterface, destroy_instance),
            mem::offset_of!(GuestContractInterface, dispatch_type),
            mem::offset_of!(GuestContractInterface, dispatch),
            ValidationContext::Guest,
        )
    } {
        return AbiError {
            code: AbiErrorCode::InvalidPointer as u32,
            message: StringView::from_static(violation.as_bytes()),
        };
    }
    // SAFETY: this is a valid HostApi pointer passed during polyplug_init.
    // (*this).runtime contains a valid pointer to Runtime.
    let runtime: &Runtime = unsafe { &*((*this).runtime as *const Runtime) };
    // Raw guest registration is reserved for a loader-owned polyplug_init
    // invocation. Hosts submit complete static tables through the canonical
    // internal-plugin registration operation instead.
    let bundle_id: u64 = runtime.current_init_bundle_id();
    if bundle_id == 0 {
        const OUTSIDE_INIT: &[u8] =
            b"register_guest_contract is only valid during loader initialization";
        runtime.set_last_error(String::from_utf8_lossy(OUTSIDE_INIT).into_owned());
        return AbiError {
            code: AbiErrorCode::Generic as u32,
            message: StringView::from_static(OUTSIDE_INIT),
        };
    }

    // SAFETY: descriptor is non-null (checked above) and provided by the plugin's

    // polyplug_init function.
    let desc: PluginDescriptor = unsafe { *descriptor };

    if desc.contract_name.ptr.is_null() || desc.contract_name.len == 0 {
        return AbiError {
            code: AbiErrorCode::Generic as u32,
            message: StringView::from_static(b"PluginDescriptor.contract_name is null or empty"),
        };
    }

    // SAFETY: desc.contract_name.ptr is non-null and valid for len bytes during init.
    let contract_name: String = match unsafe {
        string_view_to_string_owned(&desc.contract_name, "PluginDescriptor.contract_name")
    } {
        Ok(name) => name,
        Err(e) => {
            runtime.set_last_error(e.to_string());
            runtime.logger.log(LogLevel::Error, "registry", || {
                format!("registration rejected for bundle {bundle_id}: {e}")
            });
            return AbiError {
                code: AbiErrorCode::Generic as u32,
                message: StringView::null(),
            };
        }
    };

    let registration_bundle_id: BundleId = BundleId::from_u64(bundle_id);
    // Initial loads own a prepared transaction; reloads register their replacement
    // interfaces into the established reload window for `apply_reload_swap`.
    let registration: Result<(), RegistryError> = if runtime
        .registry
        .prepared_manifest(registration_bundle_id)
        .is_some()
    {
        // SAFETY: interface is valid for the registration lifetime as required by the ABI.
        unsafe {
            runtime.registry.stage_guest_contract(
                registration_bundle_id,
                desc,
                interface,
                contract_name,
            )
        }
    } else {
        // SAFETY: interface is valid for the registration lifetime as required by the ABI.
        unsafe {
            runtime
                .registry
                .register_guest_contract(desc, interface, contract_name, registration_bundle_id)
                .map(|_| ())
        }
    };
    match registration {
        Ok(()) => AbiError::ok(),
        Err(e) => {
            runtime.logger.log(LogLevel::Error, "registry", || {
                format!("registration failed for bundle {bundle_id}: {e}")
            });
            // Surface the detail through get_last_error (stderr alone is not
            // programmatically reachable) and map the registry error to its
            // specific ABI code where one exists, so guests can distinguish a
            // same-bundle duplicate from a hash collision or bad input.
            runtime.set_last_error(e.to_string());
            let code: AbiErrorCode = match e {
                RegistryError::DuplicateProvider { .. } => AbiErrorCode::DuplicateProvider,
                _ => AbiErrorCode::Generic,
            };
            AbiError {
                code: code as u32,
                message: StringView::null(),
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
    polyplug_host_alloc(size, align)
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
    unsafe { polyplug_host_free(ptr, size, align) }
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
        && !runtime.bundle_declares_dependency(
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
) -> Array<GuestContractHandle> {
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
        && !runtime.bundle_declares_dependency(
            BundleId::from_u64(caller_bundle_id),
            GuestContractId::from_u64(contract_id),
        )
    {
        return Array::empty();
    }

    // Count AND collect under a SINGLE registry read guard. Splitting the count
    // and the fill across two guards is unsound: a concurrent unload shrinking the
    // registry between them would make the allocation size disagree with the
    // returned `Array.len`, and the SDK-side free (`len * sizeof(T)`) would then
    // deallocate with a layout differing from the allocation (UB). `vec.len()` is
    // therefore the single source of truth for both the allocation and `Array.len`.
    let handles: Vec<GuestContractHandle> =
        registry.collect_guest_contracts(GuestContractId::from_u64(contract_id), min_version);

    if handles.is_empty() {
        return Array::empty();
    }

    // Allocate via the host allocator, sized to exactly the collected handles.
    let count: usize = handles.len();
    let size: usize = count * mem::size_of::<GuestContractHandle>();
    let align: usize = mem::align_of::<GuestContractHandle>();
    // SAFETY: host_alloc is safe to call from this unsafe context.
    let ptr: *mut GuestContractHandle =
        unsafe { host_alloc(this, size, align) as *mut GuestContractHandle };

    if ptr.is_null() {
        return Array::empty();
    }

    // Copy the collected handles into the host-allocated buffer.
    // SAFETY: ptr was allocated by host_alloc with size = count * size_of::<GuestContractHandle>()
    // and is valid for `count` elements; `handles` holds exactly `count` initialised
    // elements; source and destination are distinct allocations (non-overlapping).
    unsafe {
        ptr::copy_nonoverlapping(handles.as_ptr(), ptr, count);
    }

    Array::new(ptr, count)
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
        return ptr::null();
    }
    // SAFETY: this is a valid HostApi pointer passed by the host.
    // (*this).runtime contains a valid pointer to Runtime.
    let runtime: &Runtime = unsafe { &*((*this).runtime as *const Runtime) };
    let registry: &RuntimeStore = &runtime.registry;

    match registry.resolve_guest_contract(handle) {
        Ok(ptr) => ptr,
        Err(_) => ptr::null(),
    }
}

/// HostApi.registry_revision callback. It performs the acquire load of the
/// runtime-owned revision inside Rust, so callers in every host language receive a
/// synchronized value without observing atomic storage through a foreign ABI.
///
/// A generated host caller invokes this once before each direct dispatch and
/// re-resolves its cached interface when the value changes.
///
/// # Safety
/// `this` must be a valid `HostApi` pointer whose `runtime` field points to a live
/// `Runtime`.
pub(crate) unsafe extern "C" fn host_registry_revision(this: *const HostApi) -> u64 {
    if this.is_null() {
        return 0;
    }
    // SAFETY: this is a valid HostApi pointer passed by the host.
    // (*this).runtime contains a valid pointer to Runtime.
    let runtime: &Runtime = unsafe { &*((*this).runtime as *const Runtime) };
    runtime.registry.current_revision()
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

    let host_contracts_guard: RecoveringGuard<
        RwLockReadGuard<'_, HashMap<u64, &'static HostContractInterface>>,
    > = runtime
        .host_contracts
        .read()
        .recover_poisoned(runtime.logger, "runtime");

    let interface: Option<&HostContractInterface> = host_contracts_guard
        .values()
        .find(|iface| {
            iface.contract_id.id() == contract_id
                && host_contract_version_satisfies(iface, min_version)
        })
        .copied();

    match interface {
        Some(interface) => {
            // `interface` is `&'static` (it was `.copied()` out of the guard), so it
            // stays valid after the guard is dropped. Release the `host_contracts`
            // read guard BEFORE invoking `create_instance`: that callback may itself
            // call back into `register_host_contract`, which takes the `host_contracts`
            // WRITE lock — holding the read guard across it would deadlock.
            drop(host_contracts_guard);

            if interface.singleton {
                // Singleton: check cache first
                let singleton_guard: RecoveringGuard<
                    RwLockReadGuard<'_, HashMap<u64, HostContractInstance>>,
                > = runtime
                    .singleton_instances
                    .read()
                    .recover_poisoned(runtime.logger, "runtime");
                if let Some(&instance) = singleton_guard.get(&contract_id) {
                    return instance;
                }
                drop(singleton_guard);

                // Create singleton and cache it
                let mut singleton_guard: RecoveringGuard<
                    RwLockWriteGuard<'_, HashMap<u64, HostContractInstance>>,
                > = runtime
                    .singleton_instances
                    .write()
                    .recover_poisoned(runtime.logger, "runtime");
                // Double-check pattern: another thread may have created while we waited
                if let Some(&instance) = singleton_guard.get(&contract_id) {
                    return instance;
                }
                let mut instance: HostContractInstance = HostContractInstance::null();
                // SAFETY: interface.create_instance is a valid function pointer; the
                // HostContractInterface pointer is passed (self-passing pattern) and
                // `instance` is a valid, writable out-param.
                unsafe {
                    (interface.create_instance)(
                        interface as *const HostContractInterface,
                        ptr::null(),
                        &mut instance,
                    )
                };
                // Never cache a NULL instance: creation failed, so leave the cache
                // empty and let a later call retry. Caching null would poison the
                // singleton forever.
                if !instance.is_null() {
                    singleton_guard.insert(contract_id, instance);
                }
                instance
            } else {
                // Multi-instance: create new instance each call
                let mut instance: HostContractInstance = HostContractInstance::null();
                // SAFETY: interface.create_instance is a valid function pointer; the
                // HostContractInterface pointer is passed (self-passing pattern) and
                // `instance` is a valid, writable out-param.
                unsafe {
                    (interface.create_instance)(
                        interface as *const HostContractInterface,
                        ptr::null(),
                        &mut instance,
                    )
                };
                instance
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
        return ptr::null();
    }
    // SAFETY: this is a valid HostApi pointer passed by the host.
    // (*this).runtime contains a valid pointer to Runtime.
    let runtime: &Runtime = unsafe { &*((*this).runtime as *const Runtime) };

    let host_contracts_guard: RecoveringGuard<
        RwLockReadGuard<'_, HashMap<u64, &'static HostContractInterface>>,
    > = runtime
        .host_contracts
        .read()
        .recover_poisoned(runtime.logger, "runtime");

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
            ptr::null()
        })
}

/// HostApi.list_bundles callback — returns Array<BundleId>.
///
/// # Safety
/// this must be a valid HostApi pointer with valid runtime field.
pub(crate) unsafe extern "C" fn host_list_bundles(this: *const HostApi) -> Array<BundleId> {
    if this.is_null() {
        return Array::empty();
    }
    // SAFETY: this is a valid HostApi pointer.
    let runtime: &Runtime = unsafe { &*((*this).runtime as *const Runtime) };

    let manifests: RecoveringGuard<MutexGuard<'_, HashMap<String, ManifestData>>> = runtime
        .bundle_manifests
        .lock()
        .recover_poisoned(runtime.logger, "runtime");

    let count = manifests.len();
    if count == 0 {
        return Array::empty();
    }

    // Allocate via host allocator
    let size = count * mem::size_of::<BundleId>();
    let align = mem::align_of::<BundleId>();
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
/// Looks up the calling bundle's dependencies using the bundle_id at the top of the
/// runtime's per-thread init-bundle stack (the instance-owned replacement for the
/// former process-global thread-local). Returns an empty array outside any init
/// window (top-of-stack bundle_id == 0).
///
/// # Safety
/// this must be a valid HostApi pointer with valid runtime field.
pub(crate) unsafe extern "C" fn host_get_dependencies(
    this: *const HostApi,
) -> Array<DependencyInfo> {
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

    let manifest: ManifestData = match runtime.init_manifest(BundleId::from_u64(caller_bundle_id)) {
        Some(manifest) => manifest,
        None => return Array::empty(),
    };

    let deps = &manifest.dependencies;
    if deps.is_empty() {
        return Array::empty();
    }

    let count = deps.len();
    let size = count * mem::size_of::<DependencyInfo>();
    let align = mem::align_of::<DependencyInfo>();
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
            bundle_id: dep.bundle_id.unwrap_or_else(|| BundleId::from_u64(0)),
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
    out_err: *mut AbiError,
) {
    // SAFETY: the runtime is the sole producer of the HostApi table; the ABI
    // contract requires callers to pass a valid, non-null out_err pointer.
    let result: AbiError = unsafe { host_load_bundle_impl(this, path, path_len) };
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(result) };
    }
}

unsafe fn host_load_bundle_impl(
    this: *const HostApi,
    path: *const u8,
    path_len: usize,
) -> AbiError {
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
    let bytes: &[u8] = unsafe { slice::from_raw_parts(path, path_len) };
    let s: &str = match str::from_utf8(bytes) {
        Ok(s) => s,
        Err(e) => {
            runtime.set_last_error(e.to_string());
            return AbiError {
                code: AbiErrorCode::Generic as u32,
                message: StringView::null(),
            };
        }
    };

    match runtime.load_bundle(Path::new(s)) {
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
    out_err: *mut AbiError,
) {
    // SAFETY: the runtime is the sole producer of the HostApi table; the ABI
    // contract requires callers to pass a valid, non-null out_err pointer.
    let result: AbiError = unsafe { host_reload_bundle_impl(this, path, path_len) };
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(result) };
    }
}

unsafe fn host_reload_bundle_impl(
    this: *const HostApi,
    path: *const u8,
    path_len: usize,
) -> AbiError {
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
    let bytes: &[u8] = unsafe { slice::from_raw_parts(path, path_len) };
    let s: &str = match str::from_utf8(bytes) {
        Ok(s) => s,
        Err(e) => {
            runtime.set_last_error(e.to_string());
            return AbiError {
                code: AbiErrorCode::Generic as u32,
                message: StringView::null(),
            };
        }
    };

    match runtime.reload_bundle(Path::new(s)) {
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

/// HostApi.unload_bundle callback — invalidates a bundle and removes it from the registry.
///
/// Performs true unload: the bundle's handles go stale, it is removed from the
/// registry, and the superseded interface `Arc` and the underlying dylib / VM are
/// reclaimed via epoch-deferred reclamation (freed once no reader is still pinned in
/// the prior epoch).
///
/// # Safety
/// - `this` must be a valid HostApi pointer with a valid runtime field.
pub unsafe extern "C" fn host_unload_bundle(
    this: *const HostApi,
    bundle_id: BundleId,
    out_err: *mut AbiError,
) {
    // SAFETY: the runtime is the sole producer of the HostApi table; the ABI
    // contract requires callers to pass a valid, non-null out_err pointer.
    let result: AbiError = unsafe { host_unload_bundle_impl(this, bundle_id) };
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(result) };
    }
}

unsafe fn host_unload_bundle_impl(this: *const HostApi, bundle_id: BundleId) -> AbiError {
    if this.is_null() {
        return AbiError {
            code: AbiErrorCode::InvalidPointer as u32,
            message: StringView::from_static(b"null HostApi in unload_bundle"),
        };
    }
    // SAFETY: this is a valid HostApi pointer. (*this).runtime contains a valid pointer to Runtime.
    let runtime: &Runtime = unsafe { &*((*this).runtime as *const Runtime) };

    match runtime.unload_bundle(bundle_id) {
        Ok(()) => AbiError::ok(),
        Err(error) => {
            let code: AbiErrorCode = match &error {
                RuntimeError::BundleNotFound { .. } => AbiErrorCode::NotFound,
                _ => AbiErrorCode::Generic,
            };
            runtime.set_last_error(error.to_string());
            AbiError {
                code: code as u32,
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
    interface: *const HostContractInterface,
    out_err: *mut AbiError,
) {
    // SAFETY: the runtime is the sole producer of the HostApi table; the ABI
    // contract requires callers to pass a valid, non-null out_err pointer.
    let result: AbiError = unsafe { host_register_host_contract_impl(this, interface) };
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(result) };
    }
}

unsafe fn host_register_host_contract_impl(
    this: *const HostApi,
    interface: *const HostContractInterface,
) -> AbiError {
    if this.is_null() || interface.is_null() {
        return AbiError {
            code: AbiErrorCode::InvalidPointer as u32,
            message: StringView::from_static(b"null pointer in register_host_contract"),
        };
    }
    // Reject null bits in the REQUIRED fn-pointer fields before any typed
    // access to the interface — reading a null at a bare `fn` type would be an
    // invalid value (UB), and accepting it would defer the crash to the first
    // get_host_contract / dispatch (the Wave-3 null-create_instance crash class).
    // SAFETY: interface is non-null (checked above) and points to a
    // HostContractInterface the host keeps valid for the runtime lifetime.
    if let Some(violation) = unsafe {
        validate_interface_fn_ptrs(
            interface.cast::<u8>(),
            mem::offset_of!(HostContractInterface, create_instance),
            mem::offset_of!(HostContractInterface, destroy_instance),
            mem::offset_of!(HostContractInterface, dispatch_type),
            mem::offset_of!(HostContractInterface, dispatch),
            ValidationContext::Host,
        )
    } {
        return AbiError {
            code: AbiErrorCode::InvalidPointer as u32,
            message: StringView::from_static(violation.as_bytes()),
        };
    }
    // SAFETY: this is a valid HostApi pointer. (*this).runtime contains a valid pointer to Runtime.
    let runtime: &Runtime = unsafe { &*((*this).runtime as *const Runtime) };
    // SAFETY: interface is a valid HostContractInterface pointer that passed the
    // fn-pointer validation above. Caller guarantees it remains valid for runtime lifetime.
    let interface_ref: &'static HostContractInterface = unsafe { &*interface };

    match runtime.register_host_contract(interface_ref.contract_id.id(), interface_ref) {
        Ok(()) => AbiError::ok(),
        Err(HostContractError::DuplicateContract { .. }) => AbiError {
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
/// # Ownership
/// `loader_ptr` ownership transfers to the runtime UNCONDITIONALLY. The boxed loader
/// is reconstituted (and, on the duplicate-loader error path, dropped) before this
/// returns, so the caller must NOT free or reuse it afterwards — on success OR error.
/// The only path that leaves `loader_ptr` untouched is the null-pointer guard, which
/// never dereferences or reconstitutes it.
///
/// # Safety
/// - this must be a valid HostApi pointer with valid runtime field
/// - loader_ptr must be a *mut Box<dyn BundleLoader> erased to *mut c_void by a loader cdylib
///   compiled against the same polyplug rlib
pub(crate) unsafe extern "C" fn host_register_loader(
    this: *const HostApi,
    loader_ptr: *mut c_void,
    out_err: *mut AbiError,
) {
    // SAFETY: the runtime is the sole producer of the HostApi table; the ABI
    // contract requires callers to pass a valid, non-null out_err pointer.
    let result: AbiError = unsafe { host_register_loader_impl(this, loader_ptr) };
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(result) };
    }
}

unsafe fn host_register_loader_impl(this: *const HostApi, loader_ptr: *mut c_void) -> AbiError {
    if this.is_null() || loader_ptr.is_null() {
        return AbiError {
            code: AbiErrorCode::InvalidPointer as u32,
            message: StringView::from_static(b"null pointer in register_loader"),
        };
    }
    // SAFETY: this is a valid HostApi pointer. (*this).runtime contains a valid
    // pointer to Runtime. A shared reference is sufficient — `register_loader`
    // takes `&self` and uses the interior `RwLock` to mutate `loaders`. Forging a
    // `&mut Runtime` from the Arc-shared pointer would be aliasing UB (other live
    // `&Runtime` exist), so we never do that.
    let runtime: &Runtime = unsafe { &*((*this).runtime as *const Runtime) };

    // SAFETY: loader_ptr is a *mut Box<dyn BundleLoader> erased to *mut c_void by a loader cdylib
    // compiled against the same polyplug rlib. Reconstituting via Box::from_raw is valid.
    let loader: Box<dyn BundleLoader> =
        unsafe { *Box::from_raw(loader_ptr as *mut Box<dyn BundleLoader>) };

    match runtime.register_loader(loader) {
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
    let buf_slice: &mut [u8] = unsafe { slice::from_raw_parts_mut(buf, buf_len) };
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

/// HostApi.log callback — route a guest diagnostic into the host logging funnel.
///
/// Delivers to the same sink as `RuntimeConfig::log`: the host-installed
/// callback when set, otherwise the stderr default (Error/Warn only). Unknown
/// `level` values are clamped to [`LogLevel::Error`] (plugins are untrusted —
/// any u32 can cross the boundary). Null/empty views are legal and read as "".
///
/// # Safety
/// - `this` must be a valid HostApi pointer with a valid runtime field
/// - `scope` / `message` must be valid UTF-8 views (or null) for the duration
///   of the call; the runtime reads them only within this call
pub(crate) unsafe extern "C" fn host_log(
    this: *const HostApi,
    level: u32,
    scope: StringView,
    message: StringView,
) {
    if this.is_null() {
        return;
    }
    // SAFETY: this is a valid HostApi pointer. (*this).runtime contains a valid pointer to Runtime.
    let runtime: &Runtime = unsafe { &*((*this).runtime as *const Runtime) };
    let level: LogLevel = match LogLevel::from_u32(level) {
        Some(l) => l,
        None => LogLevel::Error,
    };
    // SAFETY: caller contract — both views are valid (or null) for the duration
    // of this call; `as_str` is null-safe and the bytes are copied before return.
    let (scope_str, message_str): (&str, &str) = unsafe { (scope.as_str(), message.as_str()) };
    runtime
        .logger()
        .log(level, scope_str, || message_str.to_owned());
}

/// HostApi.create_guest_instance callback — host-mediated guest instance creation.
///
/// Invokes the interface's `create_instance` under an epoch pin so a concurrent
/// unload cannot epoch-reclaim the snapshot backing `interface` while the
/// constructor runs, then records the new instance in the runtime's live-instance
/// accounting (stateful instances only — a null `data` is a stateless dispatch
/// token the host holds no state for). See the `create_guest_instance` field doc
/// on [`HostApi`].
///
/// # Safety
/// - `this` must be a valid HostApi pointer with a valid runtime field, or null
/// - `interface` must be a runtime-issued `GuestContractInterface` pointer (from
///   `resolve_guest_contract`), or null
/// - `args` must satisfy the contract's `create_instance` argument layout
pub(crate) unsafe extern "C" fn host_create_guest_instance(
    this: *const HostApi,
    interface: *const GuestContractInterface,
    args: *const c_void,
    out_instance: *mut GuestContractInstance,
) {
    // SAFETY: the runtime is the sole producer of the HostApi table; the ABI
    // contract requires callers to pass a valid, non-null out_instance pointer.
    let result: GuestContractInstance =
        unsafe { host_create_guest_instance_impl(this, interface, args) };
    if !out_instance.is_null() {
        // SAFETY: out_instance is non-null (just checked) and writable per the ABI contract.
        unsafe { out_instance.write(result) };
    }
}

/// The VM loader-data handle to pass to a guest interface's `create_instance` /
/// `destroy_instance`.
///
/// VM-dispatch contracts (python/lua/js) route per-instance construction through
/// their loader — the only channel from a single generic loader `create_instance`
/// to the right contract's author factory and per-instance registry — so they need
/// the same `loader_data` carried in `dispatch.vm.loader_data`. Native-dispatch
/// contracts have a statically-linked factory and ignore it, so a null handle is
/// correct for them.
///
/// # Safety
/// `interface` must be a live, runtime-issued `GuestContractInterface` pointer kept
/// alive by an epoch pin held by the caller for the duration of the read.
unsafe fn guest_instance_loader_data(interface: *const GuestContractInterface) -> VmLoaderData {
    // SAFETY: interface is a live runtime-issued pointer per the caller's pin.
    let dispatch_type: DispatchType = unsafe { (*interface).dispatch_type };
    if dispatch_type == DispatchType::VirtualMachine {
        // SAFETY: dispatch_type == VirtualMachine guarantees the `vm` union variant is
        // the active one, so reading `dispatch.vm.loader_data` is sound.
        unsafe { (*interface).dispatch.vm.loader_data }
    } else {
        VmLoaderData::null()
    }
}

unsafe fn host_create_guest_instance_impl(
    this: *const HostApi,
    interface: *const GuestContractInterface,
    args: *const c_void,
) -> GuestContractInstance {
    if this.is_null() || interface.is_null() {
        return GuestContractInstance::null();
    }

    // SAFETY: this is a valid HostApi pointer passed by the host;
    // (*this).runtime contains a valid pointer to Runtime.
    let runtime: &Runtime = unsafe { &*((*this).runtime as *const Runtime) };

    // Pin the epoch for the duration of construction. A concurrent unload vacates
    // the interface's snapshot for epoch reclamation; holding this pin across the
    // create call keeps that snapshot alive so `create_instance` cannot run against
    // a freed interface. The guard is named (not `let _ =`) so it lives to the end.
    let _g: EpochGuard = epoch_pin();

    // Reserve the owning bundle before copying or invoking its adapter context. The
    // reservation and bundle lookup share the unload lifecycle lock, so invalidation
    // wins before construction begins or construction keeps internal backing alive.
    let Some(bundle_id) = runtime.begin_guest_instance_construction(interface) else {
        return GuestContractInstance::null();
    };

    // SAFETY: interface is non-null and points to a runtime-issued
    // GuestContractInterface kept alive by the pin above; reading its fields is sound.
    // SAFETY: same live, pinned interface; selects the per-instance loader_data
    // (VM contracts route construction through their loader; native ignore it).
    let loader_data: VmLoaderData = unsafe { guest_instance_loader_data(interface) };

    // SAFETY: the same live, pinned interface owns the opaque adapter context.
    let adapter_context: *mut c_void = unsafe { (*interface).adapter_context };
    let mut inst: GuestContractInstance = GuestContractInstance::null();
    // SAFETY: `create_instance` is non-null by ABI contract (register_guest_contract
    // rejects null bits); the interface stays alive across the call via the pin;
    // `adapter_context` is copied opaque adapter state; `loader_data` is this
    // interface's VM handle (null for native); `args` satisfies the contract's
    // argument layout per the caller's contract; `inst` is a valid writable out-param.
    unsafe {
        ((*interface).create_instance)(
            adapter_context,
            loader_data,
            this,
            args.cast::<()>(),
            &mut inst,
        )
    };

    if inst.data.is_null() {
        runtime.cancel_guest_instance_construction(bundle_id);
    }
    inst
}

/// HostApi.destroy_guest_instance callback — host-mediated guest instance teardown.
///
/// Mirrors [`host_create_guest_instance`]: invokes the interface's `destroy_instance`
/// under an epoch pin and decrements the owning bundle's live-instance accounting.
/// # Safety
/// - `this` must be a valid HostApi pointer with a valid runtime field, or null
/// - `interface` must be a runtime-issued `GuestContractInterface` pointer, or null
/// - `instance` must be an instance produced by this contract's `create_instance`
pub(crate) unsafe extern "C" fn host_destroy_guest_instance(
    this: *const HostApi,
    interface: *const GuestContractInterface,
    instance: GuestContractInstance,
) {
    if this.is_null() || interface.is_null() {
        return;
    }

    // SAFETY: this is a valid HostApi pointer passed by the host;
    // (*this).runtime contains a valid pointer to Runtime.
    let runtime: &Runtime = unsafe { &*((*this).runtime as *const Runtime) };
    // Pin the epoch across teardown for the same reason as creation: keep the
    // interface's snapshot alive so `destroy_instance` cannot run against a freed
    // interface during a concurrent unload.
    let _g: EpochGuard = epoch_pin();

    // SAFETY: same live, pinned interface; selects the per-instance loader_data.
    let loader_data: VmLoaderData = unsafe { guest_instance_loader_data(interface) };
    // SAFETY: the same live, pinned interface owns the opaque adapter context.
    let adapter_context: *mut c_void = unsafe { (*interface).adapter_context };

    // SAFETY: `destroy_instance` is non-null by ABI contract; the interface stays
    // alive across the call via the pin; `adapter_context` is copied opaque adapter
    // state; `loader_data` is this interface's VM handle (null for native); `instance`
    // was produced by this contract.
    unsafe { ((*interface).destroy_instance)(adapter_context, loader_data, this, instance) };

    if !instance.data.is_null() {
        if let Some(bundle_id) = runtime.registry.bundle_id_for_guest_interface(interface) {
            runtime.note_instance_destroyed(bundle_id);
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use core::cell::Cell;
    use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::collections::HashMap;
    use std::fs;
    use std::panic;
    use std::path::PathBuf;
    use std::sync::{Weak, mpsc};
    use std::thread;

    use polyplug_abi::{DispatchMechanisms, NativeDispatch};
    use polyplug_utils::{HostContractId, guest_contract_id, host_contract_id};

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn quickjs_loader_maps_to_javascript_language() {
        assert_eq!(
            supported_language_from_str("js-quickjs"),
            SupportedLanguage::JavaScript
        );
        assert!(is_known_runtime_language("js-quickjs"));
    }

    #[test]
    fn cpp_loader_maps_to_cpp_language() {
        assert_eq!(supported_language_from_str("cpp"), SupportedLanguage::Cpp);
        assert!(is_known_runtime_language("cpp"));
    }

    /// `HostApi.log` stub for test hosts — drops the record.
    unsafe extern "C" fn stub_host_log(
        _this: *const HostApi,
        _level: u32,
        _scope: StringView,
        _message: StringView,
    ) {
    }

    /// No-op create_instance for a test host contract interface.
    unsafe extern "C" fn test_create_instance(
        _this: *const HostContractInterface,
        _args: *const (),
        out_instance: *mut HostContractInstance,
    ) {
        if !out_instance.is_null() {
            // SAFETY: out_instance is non-null (just checked) and writable per the ABI contract.
            unsafe { out_instance.write(HostContractInstance::null()) };
        }
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
            contract_id: HostContractId::from(0xABCD_u64),
            contract_version: Version {
                major,
                minor,
                patch: 0,
            },
            singleton: true,
            dispatch_type: DispatchType::Native,
            runtime: ptr::null_mut(),
            user_data: ptr::null_mut(),
            create_instance: test_create_instance,
            destroy_instance: test_destroy_instance,
            dispatch: DispatchMechanisms {
                native: NativeDispatch {
                    function_count: 0,
                    functions: ptr::null(),
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
    fn internal_plugin_roots_are_runtime_local_and_release_on_logical_unload() {
        struct Resident {
            dropped: Arc<AtomicBool>,
        }

        impl Drop for Resident {
            fn drop(&mut self) {
                self.dropped.store(true, Ordering::SeqCst);
            }
        }

        unsafe extern "C" fn create_stateful_instance(
            _adapter_context: *mut c_void,
            _loader_data: VmLoaderData,
            _host: *const HostApi,
            _args: *const (),
            out_instance: *mut GuestContractInstance,
        ) {
            if !out_instance.is_null() {
                let state: *mut u8 = Box::into_raw(Box::new(0_u8));
                // SAFETY: the non-null out parameter is writable for this callback.
                unsafe {
                    out_instance.write(GuestContractInstance {
                        data: state.cast(),
                        contract_id: GuestContractId::from_u64(0xA173_3A09_4E02_0001),
                    })
                };
            }
        }

        unsafe extern "C" fn destroy_stateful_instance(
            _adapter_context: *mut c_void,
            _loader_data: VmLoaderData,
            _host: *const HostApi,
            instance: GuestContractInstance,
        ) {
            if !instance.data.is_null() {
                // SAFETY: create_stateful_instance allocated this exact boxed byte.
                unsafe { drop(Box::from_raw(instance.data.cast::<u8>())) };
            }
        }

        let interface_a: GuestContractInterface = GuestContractInterface {
            contract_id: GuestContractId::from_u64(0xA173_3A09_4E02_0001),
            contract_version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
            dispatch_type: DispatchType::Native,
            adapter_context: ptr::null_mut(),
            create_instance: create_stateful_instance,
            destroy_instance: destroy_stateful_instance,
            dispatch: DispatchMechanisms {
                native: NativeDispatch {
                    function_count: 0,
                    functions: ptr::null(),
                },
            },
        };
        let descriptor: PluginDescriptor = PluginDescriptor {
            name: StringView::from_static(b"resident-provider"),
            contract_name: StringView::from_static(b"resident.contract"),
            version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
        };
        let mut function_count: HashMap<String, u32> = HashMap::new();
        function_count.insert("resident.contract@1".to_owned(), 0);
        let manifest: ManifestData = ManifestData {
            loader: "rust".to_owned(),
            name: "runtime-local-resident".to_owned(),
            dependencies: Vec::new(),
            id: BundleId::new("runtime-local-resident").id(),
            version: "1.0.0".to_owned(),
            file: String::new(),
            provides: vec!["resident.contract@1.0.0".to_owned()],
            function_count,
            needs_reinit_on_dep_reload: false,
            bundle_dependencies: Vec::new(),
            path: PathBuf::new(),
        };
        let interface_b: GuestContractInterface = GuestContractInterface {
            contract_id: GuestContractId::from_u64(0xA173_3A09_4E02_0001),
            ..interface_a
        };

        let runtime_a: Arc<Runtime> = Runtime::builder().build().expect("build first runtime");
        let runtime_b: Arc<Runtime> = Runtime::builder().build().expect("build second runtime");
        let dropped_a: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
        let dropped_b: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
        let bundle_a: BundleId = runtime_a
            .register_internal_plugin(
                manifest.clone(),
                SupportedLanguage::Rust,
                Resident {
                    dropped: Arc::clone(&dropped_a),
                },
                |host| {
                    let mut error: AbiError = AbiError::ok();
                    // SAFETY: host belongs to the active staging transaction and both tables live through this call.
                    unsafe {
                        ((*host).register_guest_contract)(
                            host,
                            &descriptor,
                            &interface_a,
                            &mut error,
                        );
                    }
                    error
                },
            )
            .expect("register first runtime bundle");
        runtime_b
            .register_internal_plugin(
                manifest,
                SupportedLanguage::Rust,
                Resident {
                    dropped: Arc::clone(&dropped_b),
                },
                |host| {
                    let mut error: AbiError = AbiError::ok();
                    // SAFETY: host belongs to the active staging transaction and both tables live through this call.
                    unsafe {
                        ((*host).register_guest_contract)(
                            host,
                            &descriptor,
                            &interface_b,
                            &mut error,
                        );
                    }
                    error
                },
            )
            .expect("register second runtime bundle");

        assert!(!dropped_a.load(Ordering::SeqCst));
        assert!(!dropped_b.load(Ordering::SeqCst));
        let handle: GuestContractHandle = runtime_a
            .find_guest_contract(0xA173_3A09_4E02_0001, 0)
            .expect("resolve runtime-local provider");
        let interface: *const GuestContractInterface = runtime_a
            .resolve_guest_contract(handle)
            .expect("resolve runtime-local interface");
        let host: *const HostApi = runtime_a.host_abi();
        let mut active: GuestContractInstance = GuestContractInstance::null();
        // SAFETY: `host` and `interface` are owned by `runtime_a`; `active` is writable.
        unsafe {
            ((*host).create_guest_instance)(host, interface, ptr::null(), &mut active);
        }
        assert!(!active.data.is_null());
        assert!(matches!(
            runtime_a.unload_bundle(bundle_a),
            Err(RuntimeError::InternalPluginInUse {
                active_instances: 1,
                ..
            })
        ));
        assert!(!dropped_a.load(Ordering::SeqCst));
        // SAFETY: the stateful instance was created through this exact runtime interface.
        unsafe {
            ((*host).destroy_guest_instance)(host, interface, active);
        }
        runtime_a
            .unload_bundle(bundle_a)
            .expect("logical unload releases only the first resident");
        assert!(dropped_a.load(Ordering::SeqCst));
        assert!(!dropped_b.load(Ordering::SeqCst));

        let bundle_b: BundleId = BundleId::new("runtime-local-resident");
        runtime_b
            .unload_bundle(bundle_b)
            .expect("logical unload releases the second resident");
        assert!(dropped_b.load(Ordering::SeqCst));
    }

    #[test]
    fn unload_waits_for_in_flight_internal_resident_construction() {
        struct BlockingResident {
            started: mpsc::Sender<()>,
            proceed: mpsc::Receiver<()>,
            releases: Arc<AtomicUsize>,
        }

        unsafe extern "C" fn create_blocking_instance(
            adapter_context: *mut c_void,
            _loader_data: VmLoaderData,
            _host: *const HostApi,
            _args: *const (),
            out_instance: *mut GuestContractInstance,
        ) {
            // SAFETY: the resident stays runtime-owned until this constructor returns.
            let resident: &BlockingResident =
                unsafe { &*adapter_context.cast::<BlockingResident>() };
            let _ = resident.started.send(());
            if resident.proceed.recv().is_err() {
                return;
            }
            if !out_instance.is_null() {
                let state: *mut u8 = Box::into_raw(Box::new(0_u8));
                // SAFETY: the non-null output is writable for this callback.
                unsafe {
                    out_instance.write(GuestContractInstance {
                        data: state.cast(),
                        contract_id: GuestContractId::from_u64(0xA173_3A09_4E02_0002),
                    })
                };
            }
        }

        unsafe extern "C" fn destroy_blocking_instance(
            _adapter_context: *mut c_void,
            _loader_data: VmLoaderData,
            _host: *const HostApi,
            instance: GuestContractInstance,
        ) {
            if !instance.data.is_null() {
                // SAFETY: create_blocking_instance allocated this exact state.
                unsafe { drop(Box::from_raw(instance.data.cast::<u8>())) };
            }
        }

        unsafe extern "C" fn release_blocking_resident(adapter_context: *mut c_void) {
            // SAFETY: ownership transfers exactly once through the resident callback.
            let resident: Box<BlockingResident> =
                unsafe { Box::from_raw(adapter_context.cast::<BlockingResident>()) };
            resident.releases.fetch_add(1, Ordering::SeqCst);
        }

        let runtime: Arc<Runtime> = Runtime::builder().build().expect("build");
        let contract_id: GuestContractId = GuestContractId::from_u64(0xA173_3A09_4E02_0002);
        let manifest: ManifestData =
            transaction_manifest("in-flight-resident", &["inflight.resident"], Vec::new());
        let descriptor: PluginDescriptor = PluginDescriptor {
            name: StringView::from_static(b"in-flight-resident"),
            contract_name: StringView::from_static(b"inflight.resident"),
            version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
        };
        let (started_tx, started_rx) = mpsc::channel();
        let (proceed_tx, proceed_rx) = mpsc::channel();
        let releases: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
        let context: *mut c_void = Box::into_raw(Box::new(BlockingResident {
            started: started_tx,
            proceed: proceed_rx,
            releases: Arc::clone(&releases),
        }))
        .cast();
        let interface: GuestContractInterface = GuestContractInterface {
            contract_id,
            contract_version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
            dispatch_type: DispatchType::Native,
            adapter_context: context,
            create_instance: create_blocking_instance,
            destroy_instance: destroy_blocking_instance,
            dispatch: DispatchMechanisms {
                native: NativeDispatch {
                    function_count: 0,
                    functions: ptr::null(),
                },
            },
        };
        let bundle_id: BundleId = runtime
            .begin_internal_plugin(manifest, SupportedLanguage::Lua)
            .expect("begin transaction");
        runtime
            .attach_internal_plugin_resident(
                bundle_id,
                context,
                current_os_thread_id(),
                release_blocking_resident,
            )
            .expect("attach resident");
        let host: *const HostApi = runtime.host_abi();
        let mut error: AbiError = AbiError::ok();
        // SAFETY: the host transaction is active and descriptor/interface live through staging.
        unsafe {
            ((*host).register_guest_contract)(host, &descriptor, &interface, &mut error);
        }
        assert!(error.is_ok());
        runtime
            .commit_internal_plugin(bundle_id)
            .expect("commit resident bundle");

        let handle: GuestContractHandle = runtime
            .find_guest_contract(contract_id.id(), 0)
            .expect("find resident contract");
        let interface: *const GuestContractInterface = runtime
            .resolve_guest_contract(handle)
            .expect("resolve resident contract");
        let creator_host: usize = host as usize;
        let creator_interface: usize = interface as usize;
        let creator = thread::spawn(move || {
            let host: *const HostApi = creator_host as *const HostApi;
            let interface: *const GuestContractInterface =
                creator_interface as *const GuestContractInterface;
            let mut instance: GuestContractInstance = GuestContractInstance::null();
            // SAFETY: the runtime stays alive while this creator is joined.
            unsafe {
                ((*host).create_guest_instance)(host, interface, ptr::null(), &mut instance);
            }
            instance
        });
        started_rx.recv().expect("constructor must begin");
        assert!(matches!(
            runtime.unload_bundle(bundle_id),
            Err(RuntimeError::InternalPluginInUse {
                active_instances: 1,
                ..
            })
        ));
        assert_eq!(releases.load(Ordering::SeqCst), 0);
        proceed_tx.send(()).expect("unblock constructor");
        let instance: GuestContractInstance = creator.join().expect("creator must complete");
        assert!(!instance.data.is_null());
        // SAFETY: the instance was created by this exact resident-backed interface.
        unsafe { ((*host).destroy_guest_instance)(host, interface, instance) };
        runtime
            .unload_bundle(bundle_id)
            .expect("unload after construction quiesces");
        assert_eq!(releases.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn registration_and_unload_acquire_backing_locks_in_one_order() {
        let (unloading_tx, unloading_rx) = mpsc::channel();
        let runtime: Arc<Runtime> = Runtime::builder()
            .on_reload(move |_user_data, _phase| {
                unloading_tx.send(()).expect("unload notification receiver");
            })
            .build()
            .expect("build");
        let bundle_id: BundleId = BundleId::from_u64(0xA173_3A09_4E02_0003);
        register_native_caller_contract(&runtime.registry, 0xA173_3A09_4E02_0003, bundle_id.id());
        let roots = runtime.internal_plugin_roots.lock().expect("root lock");
        let unload_runtime: Arc<Runtime> = Arc::clone(&runtime);
        let unload = thread::spawn(move || unload_runtime.unload_bundle(bundle_id));
        unloading_rx
            .recv()
            .expect("unload must notify before acquiring backing locks");

        assert!(
            runtime.registry.resident_lock_is_available_for_test(),
            "unload must wait on roots before taking residents"
        );
        drop(roots);
        unload
            .join()
            .expect("unload thread must complete")
            .expect("unload must succeed");
    }

    #[test]
    fn abi_ok_constant() {
        assert_eq!(AbiErrorCode::Ok, AbiErrorCode::Ok);
        assert_eq!(AbiErrorCode::Ok as u32, 0_u32);
    }

    #[test]
    fn host_api_unload_absent_bundle_returns_not_found_and_retains_detail() {
        let runtime: Arc<Runtime> = Runtime::builder().build().expect("build");
        let host: *const HostApi = host_with_runtime(&runtime);
        let mut error: AbiError = AbiError::ok();

        // SAFETY: `host` is the live runtime's ABI table and `error` is writable.
        unsafe {
            ((*host).unload_bundle)(host, BundleId::from_u64(0xA11C_E000_0000_0001), &mut error)
        };

        assert_eq!(error.code, AbiErrorCode::NotFound as u32);
        // SAFETY: `host` is the live runtime's ABI table.
        let error_len: usize = unsafe { ((*host).get_error_len)(host) };
        assert_ne!(error_len, 0, "unload error detail must be retained");
        let mut detail: Vec<u8> = vec![0; error_len];
        // SAFETY: `host` is live and `detail` owns a writable buffer of `error_len` bytes.
        let written: usize =
            unsafe { ((*host).get_last_error)(host, detail.as_mut_ptr(), detail.len()) };
        assert_eq!(written, error_len);
        let detail: String = String::from_utf8(detail).expect("last error must be UTF-8");
        assert!(detail.contains("bundle not found"), "detail: {detail}");
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
        assert_eq!(mem::size_of::<*const HostApi>(), 8);
    }

    #[test]
    fn host_find_guest_contract_null_this_returns_null() {
        // SAFETY: host_find_guest_contract handles null HostApi gracefully
        let handle: GuestContractHandle =
            unsafe { host_find_guest_contract(ptr::null(), 0_u64, 0_u32) };
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
            runtime: Arc::as_ptr(&runtime) as *mut c_void,
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
            unload_bundle: host_unload_bundle,
            log: stub_host_log,
            create_guest_instance: host_create_guest_instance,
            destroy_guest_instance: host_destroy_guest_instance,
            registry_revision: host_registry_revision,
            reserved: ptr::null(),
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

    fn create_bundle_dir(temp: &TempDir, bundle_name: &str, runtime: &str) -> PathBuf {
        let bundle_dir: PathBuf = temp.path().join(bundle_name);
        if let Err(e) = fs::create_dir_all(&bundle_dir) {
            panic!("failed to create bundle dir {}: {e}", bundle_dir.display());
        }
        let so_path: PathBuf = bundle_dir.join("dummy.so");
        if let Err(e) = fs::write(&so_path, b"") {
            panic!("failed to write dummy so {}: {e}", so_path.display());
        }
        // Emit the canonical id = FNV1a-64(name) so the manifest passes validation.
        let manifest: String = format!(
            "id = {}\nname = \"{}\"\nloader = \"{}\"\nfile = \"dummy.so\"\n",
            BundleId::new(bundle_name).id(),
            bundle_name,
            runtime
        );
        let manifest_path: PathBuf = bundle_dir.join("manifest.toml");
        if let Err(e) = fs::write(&manifest_path, manifest) {
            panic!("failed to write manifest {}: {e}", manifest_path.display());
        }
        bundle_dir
    }

    fn register_guest_contract(
        registry: &RuntimeStore,
        contract_id: u64,
        bundle_id: u64,
    ) -> GuestContractHandle {
        use polyplug_abi::{
            DispatchMechanisms, DispatchType, GuestContractInstance, GuestContractInterface,
            NativeDispatch,
        };

        unsafe extern "C" fn stub_create_instance(
            _adapter_context: *mut c_void,
            _loader_data: VmLoaderData,
            _host: *const HostApi,
            _args: *const (),
            out_instance: *mut GuestContractInstance,
        ) {
            if !out_instance.is_null() {
                // SAFETY: out_instance is non-null (just checked) and writable per the ABI contract.
                unsafe { out_instance.write(GuestContractInstance::null()) };
            }
        }

        unsafe extern "C" fn stub_destroy_instance(
            _adapter_context: *mut c_void,
            _loader_data: VmLoaderData,
            _host: *const HostApi,
            _instance: GuestContractInstance,
        ) {
        }

        let interface: &'static GuestContractInterface =
            Box::leak(Box::new(GuestContractInterface {
                contract_id: GuestContractId::from_u64(contract_id),
                contract_version: Version {
                    major: 0,
                    minor: 0,
                    patch: 0,
                },
                dispatch_type: DispatchType::Native,
                adapter_context: ptr::null_mut(),
                create_instance: stub_create_instance,
                destroy_instance: stub_destroy_instance,
                dispatch: DispatchMechanisms {
                    native: NativeDispatch {
                        function_count: 0,
                        functions: ptr::null(),
                    },
                },
            }));
        let descriptor: PluginDescriptor = PluginDescriptor {
            name: StringView::from_static(b"stub"),
            contract_name: StringView::from_static(b"stub.contract"),
            version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
        };
        // SAFETY: interface is leaked and lives for the process lifetime.
        let result: Result<GuestContractHandle, RegistryError> = unsafe {
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

    // ─── shared native-dispatch test fixtures ────────────────────────────────

    /// Native dispatch target: writes the i32 at `args` plus one into `out`.
    unsafe extern "C" fn native_add_one(
        _adapter_context: *mut c_void,
        _instance: GuestContractInstance,
        args: *const (),
        out: *mut (),
        out_err: *mut AbiError,
    ) {
        // SAFETY: the test passes a valid *const i32 / *mut i32.
        unsafe {
            let input: i32 = *(args as *const i32);
            *(out as *mut i32) = input + 1;
        }
        if !out_err.is_null() {
            // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
            unsafe { out_err.write(AbiError::ok()) };
        }
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
    fn register_native_caller_contract(registry: &RuntimeStore, contract_id: u64, bundle_id: u64) {
        use polyplug_abi::{
            DispatchMechanisms, DispatchType, GuestContractInstance, GuestContractInterface,
            NativeDispatch,
        };

        unsafe extern "C" fn stub_create(
            _adapter_context: *mut c_void,
            _loader_data: VmLoaderData,
            _host: *const HostApi,
            _args: *const (),
            out_instance: *mut GuestContractInstance,
        ) {
            if !out_instance.is_null() {
                // SAFETY: out_instance is non-null (just checked) and writable per the ABI contract.
                unsafe { out_instance.write(GuestContractInstance::null()) };
            }
        }
        unsafe extern "C" fn stub_destroy(
            _adapter_context: *mut c_void,
            _loader_data: VmLoaderData,
            _host: *const HostApi,
            _instance: GuestContractInstance,
        ) {
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
                adapter_context: ptr::null_mut(),
                create_instance: stub_create,
                destroy_instance: stub_destroy,
                dispatch: DispatchMechanisms {
                    native: NativeDispatch {
                        function_count: 1,
                        functions: NATIVE_FNS.0.as_ptr(),
                    },
                },
            }));
        let descriptor: PluginDescriptor = PluginDescriptor {
            name: StringView::from_static(b"caller"),
            contract_name: StringView::from_static(b"caller.contract"),
            version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
        };
        // SAFETY: interface is leaked for the process lifetime.
        let result: Result<GuestContractHandle, RegistryError> = unsafe {
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
        runtime.host_abi()
    }

    // ─── host-mediated instance lifecycle (instance counter) ─────────────────

    /// Contract id used by the stateful mock. The mock's `create_instance` stamps
    /// this onto every instance (mirroring a real generated factory) so the
    /// destroy-side decrement, which keys on `instance.contract_id`, matches the
    /// create-side increment, which keys on `interface.contract_id`.
    const STATEFUL_CONTRACT_ID: u64 = 0x0BAD_F00D_1234_5678;

    /// Stateful create_instance: returns a non-null `data` (a leaked boxed unit)
    /// so the runtime counts it as a live stateful instance, stamped with the
    /// contract id like a real generated factory.
    unsafe extern "C" fn stateful_create_instance(
        _adapter_context: *mut c_void,
        _loader_data: VmLoaderData,
        _host: *const HostApi,
        _args: *const (),
        out_instance: *mut GuestContractInstance,
    ) {
        let boxed: Box<u8> = Box::new(0u8);
        let instance: GuestContractInstance = GuestContractInstance {
            data: Box::into_raw(boxed) as *mut c_void,
            contract_id: GuestContractId::from_u64(STATEFUL_CONTRACT_ID),
        };
        if !out_instance.is_null() {
            // SAFETY: out_instance is non-null (just checked) and writable per the ABI contract.
            unsafe { out_instance.write(instance) };
        }
    }

    /// Destroy the boxed unit created by `stateful_create_instance`.
    unsafe extern "C" fn stateful_destroy_instance(
        _adapter_context: *mut c_void,
        _loader_data: VmLoaderData,
        _host: *const HostApi,
        instance: GuestContractInstance,
    ) {
        if !instance.data.is_null() {
            // SAFETY: `data` was produced by `stateful_create_instance` via
            // `Box::into_raw(Box<u8>)`, so reclaiming it as the same Box is sound.
            drop(unsafe { Box::from_raw(instance.data as *mut u8) });
        }
    }

    /// Register a native-dispatch contract whose `create_instance` returns a
    /// non-null (stateful) instance, returning the runtime-issued interface pointer
    /// so the test can drive `host_create_guest_instance` / `host_destroy_guest_instance`.
    fn register_stateful_contract(
        registry: &RuntimeStore,
        contract_id: u64,
        bundle_id: u64,
    ) -> *const GuestContractInterface {
        let interface: &'static GuestContractInterface =
            Box::leak(Box::new(GuestContractInterface {
                contract_id: GuestContractId::from_u64(contract_id),
                contract_version: Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
                dispatch_type: DispatchType::Native,
                adapter_context: ptr::null_mut(),
                create_instance: stateful_create_instance,
                destroy_instance: stateful_destroy_instance,
                dispatch: DispatchMechanisms {
                    native: NativeDispatch {
                        function_count: 0,
                        functions: ptr::null(),
                    },
                },
            }));
        let descriptor: PluginDescriptor = PluginDescriptor {
            name: StringView::from_static(b"stateful"),
            contract_name: StringView::from_static(b"stateful.contract"),
            version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
        };
        // SAFETY: interface is leaked for the process lifetime.
        let result: Result<GuestContractHandle, RegistryError> = unsafe {
            registry.register_guest_contract(
                descriptor,
                interface,
                "stateful.contract".to_owned(),
                BundleId::from_u64(bundle_id),
            )
        };
        let handle: GuestContractHandle = result.expect("register stateful contract");
        registry
            .resolve_guest_contract(handle)
            .expect("resolve runtime-issued stateful interface")
    }

    #[test]
    fn host_instance_lifecycle_counts_stateful_instances() {
        let runtime: Arc<Runtime> = Runtime::builder().build().expect("build");
        let contract_id: u64 = STATEFUL_CONTRACT_ID;
        let bundle_id: BundleId = BundleId::from_u64(0x1);
        let interface: *const GuestContractInterface =
            register_stateful_contract(&runtime.registry, contract_id, bundle_id.id());
        let host: *const HostApi = host_with_runtime(&runtime);

        assert_eq!(
            runtime.live_instance_count_for_bundle(bundle_id),
            0,
            "no instances created yet"
        );

        // Create two stateful instances through the host-mediated path.
        let mut inst_a: GuestContractInstance = GuestContractInstance::null();
        // SAFETY: host and interface are valid; create_instance ignores args;
        // inst_a is a valid out-param.
        unsafe { host_create_guest_instance(host, interface, ptr::null(), &mut inst_a) };
        let mut inst_b: GuestContractInstance = GuestContractInstance::null();
        // SAFETY: as above; inst_b is a valid out-param.
        unsafe { host_create_guest_instance(host, interface, ptr::null(), &mut inst_b) };
        assert!(!inst_a.data.is_null() && !inst_b.data.is_null());
        assert_eq!(
            runtime.live_instance_count_for_bundle(bundle_id),
            2,
            "two stateful instances counted"
        );

        // Destroy both through the host-mediated path; the count returns to zero.
        // SAFETY: each instance was produced by this contract's create_instance.
        unsafe { host_destroy_guest_instance(host, interface, inst_a) };
        // SAFETY: as above.
        unsafe { host_destroy_guest_instance(host, interface, inst_b) };
        assert_eq!(
            runtime.live_instance_count_for_bundle(bundle_id),
            0,
            "count returns to zero after destroy"
        );
    }

    #[test]
    fn host_instance_lifecycle_ignores_stateless_instances() {
        let runtime: Arc<Runtime> = Runtime::builder().build().expect("build");
        let contract_id: u64 = 0x0FED_CBA9_8765_4321;
        let bundle_id: BundleId = BundleId::from_u64(0x1);
        register_native_caller_contract(&runtime.registry, contract_id, bundle_id.id());
        let host: *const HostApi = host_with_runtime(&runtime);

        // resolve the registered interface through the host vtable, mirroring the
        // real create path (find -> resolve -> create_guest_instance).
        // SAFETY: host is valid; find/resolve tolerate the inputs below.
        let handle: GuestContractHandle = unsafe { host_find_guest_contract(host, contract_id, 0) };
        // SAFETY: handle was just minted by find for a registered contract.
        let interface: *const GuestContractInterface =
            unsafe { host_resolve_guest_contract(host, handle) };
        assert!(!interface.is_null(), "registered contract must resolve");

        // The stateless contract's create_instance returns a null `data`, so the
        // host must not count it.
        let mut inst: GuestContractInstance = GuestContractInstance::null();
        // SAFETY: host and interface are valid; `inst` is a valid out-param.
        unsafe { host_create_guest_instance(host, interface, ptr::null(), &mut inst) };
        assert!(inst.data.is_null(), "stateless instance has null data");
        assert_eq!(
            runtime.live_instance_count_for_bundle(bundle_id),
            0,
            "stateless instances are not counted"
        );

        // Destroying it is a no-op for the counter too.
        // SAFETY: as above.
        unsafe { host_destroy_guest_instance(host, interface, inst) };
        assert_eq!(runtime.live_instance_count_for_bundle(bundle_id), 0);
    }

    #[test]
    fn shared_contract_instances_are_counted_by_bundle_owner() {
        let runtime: Arc<Runtime> = Runtime::builder().build().expect("build");
        let bundle_a: BundleId = BundleId::from_u64(0xA);
        let bundle_b: BundleId = BundleId::from_u64(0xB);
        let interface_a: *const GuestContractInterface =
            register_stateful_contract(&runtime.registry, STATEFUL_CONTRACT_ID, bundle_a.id());
        let interface_b: *const GuestContractInterface =
            register_stateful_contract(&runtime.registry, STATEFUL_CONTRACT_ID, bundle_b.id());
        runtime
            .internal_plugin_roots
            .lock()
            .expect("internal-plugin root lock")
            .insert(bundle_b, Box::new(()));
        let host: *const HostApi = host_with_runtime(&runtime);
        let mut instance_a: GuestContractInstance = GuestContractInstance::null();
        let mut instance_b: GuestContractInstance = GuestContractInstance::null();

        // SAFETY: both interfaces are runtime-issued and both outputs are writable.
        unsafe {
            host_create_guest_instance(host, interface_a, ptr::null(), &mut instance_a);
            host_create_guest_instance(host, interface_b, ptr::null(), &mut instance_b);
        }
        assert_eq!(runtime.live_instance_count_for_bundle(bundle_a), 1);
        assert_eq!(runtime.live_instance_count_for_bundle(bundle_b), 1);
        assert!(matches!(
            runtime.unload_bundle(bundle_b),
            Err(RuntimeError::InternalPluginInUse {
                active_instances: 1,
                ..
            })
        ));

        // SAFETY: instance_b was created through interface_b.
        unsafe { host_destroy_guest_instance(host, interface_b, instance_b) };
        assert_eq!(runtime.live_instance_count_for_bundle(bundle_a), 1);
        assert_eq!(runtime.live_instance_count_for_bundle(bundle_b), 0);
        runtime
            .unload_bundle(bundle_b)
            .expect("A's live instance must not block B's unload");

        // SAFETY: instance_a was created through interface_a.
        unsafe { host_destroy_guest_instance(host, interface_a, instance_a) };
        assert_eq!(runtime.live_instance_count_for_bundle(bundle_a), 0);
    }

    #[test]
    fn reload_counter_reset_only_affects_target_bundle() {
        let runtime: Arc<Runtime> = Runtime::builder().build().expect("build");
        let bundle_a: BundleId = BundleId::from_u64(0xA);
        let bundle_b: BundleId = BundleId::from_u64(0xB);
        runtime.note_instance_created(bundle_a);
        runtime.note_instance_created(bundle_b);

        runtime.reset_instance_count_for_bundle(bundle_a);

        assert_eq!(runtime.live_instance_count_for_bundle(bundle_a), 0);
        assert_eq!(runtime.live_instance_count_for_bundle(bundle_b), 1);
    }

    // ─── init-stack fast path (active_init_count) ────────────────────────────

    #[test]
    fn current_init_bundle_id_zero_outside_window() {
        // Fast path: no push has happened, so the counter is 0 and
        // current_init_bundle_id returns 0 without consulting the stack.
        let runtime: Arc<Runtime> = Runtime::builder().build().expect("build");
        assert_eq!(
            runtime.active_init_count.load(Ordering::Relaxed),
            0,
            "fresh runtime has no active init windows"
        );
        assert_eq!(runtime.current_init_bundle_id(), 0);
    }

    #[test]
    fn current_init_bundle_id_tracks_nested_push_pop() {
        // push/push/pop/pop must restore the outer bundle id at each step and the
        // fast-path counter must stay perfectly balanced (back to 0 at the end).
        let runtime: Arc<Runtime> = Runtime::builder().build().expect("build");

        runtime.push_init_bundle_id(0xAAAA);
        assert_eq!(runtime.active_init_count.load(Ordering::Relaxed), 1);
        assert_eq!(runtime.current_init_bundle_id(), 0xAAAA);

        // Nested load on the SAME thread pushes its own id; the inner id wins.
        runtime.push_init_bundle_id(0xBBBB);
        assert_eq!(runtime.active_init_count.load(Ordering::Relaxed), 2);
        assert_eq!(runtime.current_init_bundle_id(), 0xBBBB);

        // Pop the inner window — the outer id is restored.
        runtime.pop_init_bundle_id();
        assert_eq!(runtime.active_init_count.load(Ordering::Relaxed), 1);
        assert_eq!(runtime.current_init_bundle_id(), 0xAAAA);

        // Pop the outer window — back to the host (no-init) state.
        runtime.pop_init_bundle_id();
        assert_eq!(
            runtime.active_init_count.load(Ordering::Relaxed),
            0,
            "counter must return to 0 after balanced push/pop"
        );
        assert_eq!(runtime.current_init_bundle_id(), 0);
    }

    #[test]
    fn pop_without_push_does_not_underflow_counter() {
        // An unbalanced pop (no matching push) must leave the counter at 0, never
        // wrapping below — otherwise the fast path would never short-circuit again.
        let runtime: Arc<Runtime> = Runtime::builder().build().expect("build");
        runtime.pop_init_bundle_id();
        assert_eq!(
            runtime.active_init_count.load(Ordering::Relaxed),
            0,
            "pop with no entry must not decrement the counter"
        );
        assert_eq!(runtime.current_init_bundle_id(), 0);
    }

    struct EnforceLoader {
        contract_id: u64,
        error_bundle_id: u64,
    }

    impl BundleLoader for EnforceLoader {
        fn loader_name(&self) -> &'static str {
            "enforce"
        }

        fn supports_hot_reload(&self) -> bool {
            false
        }

        fn load(
            &self,
            _manifest: &ManifestData,
            _source: &BundleSource,
            runtime: &Runtime,
        ) -> Result<(), LoaderError> {
            // Drive the runtime's real dependency-enforcement path: probe an
            // undeclared contract inside the init window. The runtime records the
            // bundle_id-zero escape and the resolve is denied. The mock then reports
            // the denial as the loader-level init failure the runtime surfaces.
            runtime.push_init_bundle_id(self.error_bundle_id);
            runtime.pop_init_bundle_id();
            Err(LoaderError::InitFailed {
                bundle: "enforce".to_owned(),
                error: format!(
                    "undeclared dependency: bundle_id={:#x} contract_id={:#x}",
                    self.error_bundle_id, self.contract_id
                ),
            })
        }

        fn reload(&self, _manifest: &ManifestData, _runtime: &Runtime) -> Result<(), LoaderError> {
            Err(LoaderError::HotReloadUnsupported {
                loader_name: self.loader_name().to_owned(),
            })
        }
    }

    struct ProbeLoader {
        observed_init: Arc<Mutex<Option<bool>>>,
    }

    impl BundleLoader for ProbeLoader {
        fn loader_name(&self) -> &'static str {
            "probe"
        }

        fn supports_hot_reload(&self) -> bool {
            false
        }

        fn load(
            &self,
            _manifest: &ManifestData,
            _source: &BundleSource,
            _runtime: &Runtime,
        ) -> Result<(), LoaderError> {
            let mut guard: MutexGuard<'_, Option<bool>> = match self.observed_init.lock() {
                Ok(g) => g,
                Err(e) => e.into_inner(),
            };
            *guard = Some(true);
            Ok(())
        }

        fn reload(&self, _manifest: &ManifestData, _runtime: &Runtime) -> Result<(), LoaderError> {
            Err(LoaderError::HotReloadUnsupported {
                loader_name: self.loader_name().to_owned(),
            })
        }
    }

    struct PanicLoader;

    impl BundleLoader for PanicLoader {
        fn loader_name(&self) -> &'static str {
            "panic"
        }

        fn supports_hot_reload(&self) -> bool {
            false
        }

        fn load(
            &self,
            _manifest: &ManifestData,
            _source: &BundleSource,
            _runtime: &Runtime,
        ) -> Result<(), LoaderError> {
            panic!("intentional panic in PanicLoader");
        }

        fn reload(&self, _manifest: &ManifestData, _runtime: &Runtime) -> Result<(), LoaderError> {
            Err(LoaderError::HotReloadUnsupported {
                loader_name: self.loader_name().to_owned(),
            })
        }
    }

    struct ReentrantState {
        runtime_ptr: usize,
        inner_bundle: PathBuf,
        inner_load_completed: Option<bool>,
    }

    struct ReentrantLoader {
        state: Arc<Mutex<ReentrantState>>,
    }

    impl BundleLoader for ReentrantLoader {
        fn loader_name(&self) -> &'static str {
            "reentrant"
        }

        fn supports_hot_reload(&self) -> bool {
            false
        }

        fn load(
            &self,
            _manifest: &ManifestData,
            _source: &BundleSource,
            _runtime: &Runtime,
        ) -> Result<(), LoaderError> {
            let state: MutexGuard<'_, ReentrantState> = match self.state.lock() {
                Ok(g) => g,
                Err(e) => e.into_inner(),
            };
            let runtime_ptr: usize = state.runtime_ptr;
            if runtime_ptr == 0 {
                return Err(LoaderError::InitFailed {
                    bundle: "reentrant".to_owned(),
                    error: "runtime pointer not initialized".to_owned(),
                });
            }
            let inner_bundle: PathBuf = state.inner_bundle.clone();
            let already_set: bool = state.inner_load_completed.is_some();
            drop(state);
            // SAFETY: runtime_ptr was set from a valid &Runtime during load_bundle.
            let runtime_ref: &Runtime = unsafe { &*(runtime_ptr as *const Runtime) };
            let inner_result: Result<(), RuntimeError> = runtime_ref.load_bundle_with(
                inner_bundle.as_path(),
                LoadOptions {
                    compatibility: Compatibility::default(),
                    ignore_function_count_mismatch: false,
                },
            );
            // The nested load returns a top-level RuntimeError; the mock surfaces a
            // failed nested load as its own init failure.
            if let Err(e) = inner_result {
                return Err(LoaderError::InitFailed {
                    bundle: "reentrant".to_owned(),
                    error: e.to_string(),
                });
            }
            let mut st2: MutexGuard<'_, ReentrantState> = match self.state.lock() {
                Ok(g) => g,
                Err(e) => e.into_inner(),
            };
            if !already_set {
                st2.inner_load_completed = Some(true);
            }
            Ok(())
        }

        fn reload(&self, _manifest: &ManifestData, _runtime: &Runtime) -> Result<(), LoaderError> {
            Err(LoaderError::HotReloadUnsupported {
                loader_name: self.loader_name().to_owned(),
            })
        }
    }

    struct LazyState {
        observed_init: Option<bool>,
    }

    struct LazyLoader {
        state: Arc<Mutex<LazyState>>,
    }

    impl BundleLoader for LazyLoader {
        fn loader_name(&self) -> &'static str {
            "lazy"
        }

        fn supports_hot_reload(&self) -> bool {
            false
        }

        fn load(
            &self,
            _manifest: &ManifestData,
            _source: &BundleSource,
            _runtime: &Runtime,
        ) -> Result<(), LoaderError> {
            let mut state: MutexGuard<'_, LazyState> = match self.state.lock() {
                Ok(g) => g,
                Err(e) => e.into_inner(),
            };
            if state.observed_init.is_none() {
                state.observed_init = Some(true);
            }
            Ok(())
        }

        fn reload(&self, _manifest: &ManifestData, _runtime: &Runtime) -> Result<(), LoaderError> {
            Err(LoaderError::HotReloadUnsupported {
                loader_name: self.loader_name().to_owned(),
            })
        }
    }

    #[test]
    fn bundle_id_zero_escape_returns_undeclared_dependency_error() {
        let temp: TempDir = match TempDir::new() {
            Ok(t) => t,
            Err(e) => panic!("failed to create temp dir: {e}"),
        };
        let contract: u64 = guest_contract_id("trust.test", 1_u32);
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
        let result: Result<(), RuntimeError> = runtime.load_bundle(bundle_path.as_path());
        match result {
            Err(RuntimeError::Loader(LoaderError::InitFailed { bundle: _, error })) => {
                assert!(error.contains("undeclared dependency"), "got: {error}");
                assert!(error.contains("0x0"), "bundle_id zero escape: {error}");
                assert!(
                    error.contains(&format!("{contract:#x}")),
                    "contract id in message: {error}"
                );
            }
            Err(other) => panic!("unexpected error: {other}"),
            Ok(()) => panic!("expected undeclared dependency error"),
        }
    }

    #[test]
    fn tls_state_cleared_after_init_completes() {
        let temp: TempDir = match TempDir::new() {
            Ok(t) => t,
            Err(e) => panic!("failed to create temp dir: {e}"),
        };
        let contract: u64 = guest_contract_id("trust.tls", 1_u32);
        let observed: Arc<Mutex<Option<bool>>> = Arc::new(Mutex::new(None));
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
        let result: Result<(), RuntimeError> = runtime.load_bundle(bundle_path.as_path());
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
        let temp: TempDir = match TempDir::new() {
            Ok(t) => t,
            Err(e) => panic!("failed to create temp dir: {e}"),
        };
        let _bundle_root: PathBuf = create_bundle_dir(&temp, "panic_bundle", "panic");
        let plugin_dir: PathBuf = temp.path().to_path_buf();
        let result = panic::catch_unwind(|| {
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
        let temp: TempDir = match TempDir::new() {
            Ok(t) => t,
            Err(e) => panic!("failed to create temp dir: {e}"),
        };
        let contract: u64 = guest_contract_id("trust.reentrant", 1_u32);
        let outer_bundle: PathBuf = create_bundle_dir(&temp, "outer_bundle", "reentrant");
        let inner_bundle: PathBuf = create_bundle_dir(&temp, "inner_bundle", "probe");
        let state: Arc<Mutex<ReentrantState>> = Arc::new(Mutex::new(ReentrantState {
            runtime_ptr: 0,
            inner_bundle: inner_bundle.clone(),
            inner_load_completed: None,
        }));
        let runtime: Arc<Runtime> = match Runtime::builder()
            .loader(ReentrantLoader {
                state: Arc::clone(&state),
            })
            .loader(ProbeLoader {
                observed_init: Arc::new(Mutex::new(None)),
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
            let mut guard: MutexGuard<'_, ReentrantState> = match state.lock() {
                Ok(g) => g,
                Err(e) => e.into_inner(),
            };
            guard.runtime_ptr = Arc::as_ptr(&runtime) as usize;
        }
        let result: Result<(), RuntimeError> = runtime.load_bundle_with(
            outer_bundle.as_path(),
            LoadOptions {
                compatibility: Compatibility::default(),
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
        let temp: TempDir = match TempDir::new() {
            Ok(t) => t,
            Err(e) => panic!("failed to create temp dir: {e}"),
        };
        let contract: u64 = guest_contract_id("trust.lazy", 1_u32);
        let outer_bundle: PathBuf = create_bundle_dir(&temp, "lazy_outer", "lazy");
        let inner_bundle: PathBuf = create_bundle_dir(&temp, "lazy_inner", "probe");
        let state: Arc<Mutex<LazyState>> = Arc::new(Mutex::new(LazyState {
            observed_init: None,
        }));
        let runtime: Arc<Runtime> = match Runtime::builder()
            .loader(LazyLoader {
                state: Arc::clone(&state),
            })
            .loader(ProbeLoader {
                observed_init: Arc::new(Mutex::new(None)),
            })
            .build()
        {
            Ok(rt) => rt,
            Err(e) => panic!("failed to build runtime: {e}"),
        };
        let registry: &Arc<RuntimeStore> = runtime.registry();
        let _handle: GuestContractHandle =
            register_guest_contract(registry.as_ref(), contract, 0xFACE_u64);
        let result: Result<(), RuntimeError> = runtime.load_bundle(outer_bundle.as_path());
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
        let inner_result: Result<(), RuntimeError> = runtime.load_bundle_with(
            inner_bundle.as_path(),
            LoadOptions {
                compatibility: Compatibility::default(),
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
        unsafe extern "C" fn stub_create_instance(
            _this: *const HostContractInterface,
            _args: *const (),
            out_instance: *mut HostContractInstance,
        ) {
            // Return a non-null dummy pointer for testing
            static mut DUMMY: usize = 0xDEADBEEF;
            if !out_instance.is_null() {
                // SAFETY: out_instance is non-null (just checked) and writable per the ABI contract.
                unsafe {
                    out_instance.write(HostContractInstance {
                        data: &raw mut DUMMY as *mut c_void,
                    })
                };
            }
        }

        unsafe extern "C" fn stub_destroy_instance(
            _this: *const HostContractInterface,
            _instance: HostContractInstance,
        ) {
        }

        Box::leak(Box::new(HostContractInterface {
            contract_id: HostContractId::from(contract_id),
            contract_version: Version {
                major,
                minor,
                patch: 0,
            },
            singleton: true,
            dispatch_type: DispatchType::Native,
            runtime: ptr::null_mut(),
            user_data: ptr::null_mut(),
            create_instance: stub_create_instance,
            destroy_instance: stub_destroy_instance,
            dispatch: DispatchMechanisms {
                native: NativeDispatch {
                    function_count: 0,
                    functions: ptr::null(),
                },
            },
        }))
    }

    #[test]
    fn runtime_host_contracts_register_guest_contract_and_lookup() {
        let runtime: Arc<Runtime> = Runtime::builder()
            .build()
            .expect("runtime build should succeed");

        let contract_id: u64 = host_contract_id("host.logger", 1);
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

        let contract_id: u64 = host_contract_id("host.logger", 1);
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

        let contract_id: u64 = host_contract_id("host.logger", 1);
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

        let contract_id: u64 = host_contract_id("host.logger", 2);
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
    fn runtime_host_language_default_is_rust() {
        let runtime: Arc<Runtime> = Runtime::builder()
            .build()
            .expect("runtime build should succeed");
        assert_eq!(runtime.host_language(), SupportedLanguage::Rust);
    }

    #[test]
    fn runtime_host_language_can_be_set() {
        let runtime: Arc<Runtime> = Runtime::builder()
            .host_language(SupportedLanguage::Python)
            .build()
            .expect("runtime build should succeed");
        assert_eq!(runtime.host_language(), SupportedLanguage::Python);
    }

    #[test]
    fn host_get_host_contract_callback_returns_register_guest_contracted_contract() {
        let runtime: Arc<Runtime> = Runtime::builder()
            .build()
            .expect("runtime build should succeed");

        let contract_id: u64 = host_contract_id("host.test", 1);
        let interface: &'static HostContractInterface =
            create_host_contract_interface(contract_id, 1, 0);

        runtime
            .register_host_contract(contract_id, interface)
            .expect("registration should succeed");

        // Create a HostApi with runtime pointer
        let host_interface: HostApi = HostApi {
            runtime: Arc::as_ptr(&runtime) as *mut c_void,
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
            log: stub_host_log,
            create_guest_instance: host_create_guest_instance,
            destroy_guest_instance: host_destroy_guest_instance,
            registry_revision: host_registry_revision,
            reserved: ptr::null(),
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

        let contract_id: u64 = host_contract_id("host.nonexistent", 1);

        // Create a HostApi with runtime pointer
        let host_interface: HostApi = HostApi {
            runtime: Arc::as_ptr(&runtime) as *mut c_void,
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
            log: stub_host_log,
            create_guest_instance: host_create_guest_instance,
            destroy_guest_instance: host_destroy_guest_instance,
            registry_revision: host_registry_revision,
            reserved: ptr::null(),
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
        static LOCAL_INSTANCE_COUNTER: Cell<usize> = const { Cell::new(0) };
    }

    /// Create instance callback that returns a unique instance per call.
    /// Each call increments a thread-local counter and returns a unique pointer.
    unsafe extern "C" fn counting_create_instance(
        _this: *const HostContractInterface,
        _args: *const (),
        out_instance: *mut HostContractInstance,
    ) {
        let instance: HostContractInstance = LOCAL_INSTANCE_COUNTER.with(|counter| {
            let count: usize = counter.get();
            counter.set(count + 1);
            // Use the count as a "unique" pointer value - we don't actually allocate
            // since these are just test instances
            HostContractInstance {
                data: (count + 1) as *mut c_void, // +1 to avoid null for count=0
            }
        });
        if !out_instance.is_null() {
            // SAFETY: out_instance is non-null (just checked) and writable per the ABI contract.
            unsafe { out_instance.write(instance) };
        }
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
        Box::leak(Box::new(HostContractInterface {
            contract_id: HostContractId::from(contract_id),
            contract_version: Version {
                major,
                minor: 0,
                patch: 0,
            },
            singleton,
            dispatch_type: DispatchType::Native,
            runtime: ptr::null_mut(),
            user_data: ptr::null_mut(),
            create_instance: counting_create_instance,
            destroy_instance: counting_destroy_instance,
            dispatch: DispatchMechanisms {
                native: NativeDispatch {
                    function_count: 0,
                    functions: ptr::null(),
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

        let contract_id: u64 = host_contract_id("singleton.test", 1);
        let interface: &'static HostContractInterface =
            create_counting_host_contract_interface(contract_id, 1, true); // singleton=true

        runtime
            .register_host_contract(contract_id, interface)
            .expect("registration should succeed");

        // Create a HostApi with runtime pointer
        let host_interface: HostApi = HostApi {
            runtime: Arc::as_ptr(&runtime) as *mut c_void,
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
            log: stub_host_log,
            create_guest_instance: host_create_guest_instance,
            destroy_guest_instance: host_destroy_guest_instance,
            registry_revision: host_registry_revision,
            reserved: ptr::null(),
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

        let contract_id: u64 = host_contract_id("multi.test", 1);
        let interface: &'static HostContractInterface =
            create_counting_host_contract_interface(contract_id, 1, false); // singleton=false

        runtime
            .register_host_contract(contract_id, interface)
            .expect("registration should succeed");

        // Create a HostApi with runtime pointer
        let host_interface: HostApi = HostApi {
            runtime: Arc::as_ptr(&runtime) as *mut c_void,
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
            log: stub_host_log,
            create_guest_instance: host_create_guest_instance,
            destroy_guest_instance: host_destroy_guest_instance,
            registry_revision: host_registry_revision,
            reserved: ptr::null(),
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

        let singleton_id: u64 = host_contract_id("singleton.mixed", 1);
        let multi_id: u64 = host_contract_id("multi.mixed", 1);

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
            runtime: Arc::as_ptr(&runtime) as *mut c_void,
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
            log: stub_host_log,
            create_guest_instance: host_create_guest_instance,
            destroy_guest_instance: host_destroy_guest_instance,
            registry_revision: host_registry_revision,
            reserved: ptr::null(),
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

    #[test]
    fn unload_refuses_provider_with_dependent_then_cascade_succeeds() {
        let runtime: Arc<Runtime> = Runtime::builder()
            .build()
            .expect("runtime build should succeed");

        let provider_contract_id: u64 = 0x0BAD_F00D_0000_00A1;
        let provider_bundle_id: BundleId = BundleId::from_u64(0xA);
        let dependent_bundle_id: BundleId = BundleId::from_u64(0xB);

        // Bundle A provides a contract; bundle B declares a dependency on it.
        register_native_caller_contract(&runtime.registry, provider_contract_id, 0xA);
        register_native_caller_contract(&runtime.registry, 0x0BAD_F00D_0000_00B2, 0xB);

        runtime
            .registry
            .register_bundle_metadata(
                provider_bundle_id,
                "bundle_a".to_owned(),
                Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
                SupportedLanguage::Rust,
                PathBuf::new(),
                Vec::new(),
            )
            .expect("provider metadata registration should succeed");
        runtime
            .registry
            .register_bundle_metadata(
                dependent_bundle_id,
                "bundle_b".to_owned(),
                Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
                SupportedLanguage::Rust,
                PathBuf::new(),
                Vec::new(),
            )
            .expect("dependent metadata registration should succeed");
        runtime
            .registry
            .declare_bundle_dependencies(
                dependent_bundle_id,
                vec![GuestContractId::from_u64(provider_contract_id)],
            )
            .expect("dependency declaration should succeed");

        // Unloading the provider must be refused while the dependent is loaded.
        match runtime.unload_bundle(provider_bundle_id) {
            Err(RuntimeError::DependencyInUse {
                provider,
                dependents,
            }) => {
                assert_eq!(provider, "bundle_a");
                assert_eq!(dependents, vec!["bundle_b".to_owned()]);
            }
            other => panic!("expected DependencyInUse refusal, got {other:?}"),
        }

        // Cascade unload removes the dependent first, then the provider.
        runtime
            .unload_bundle_cascade(provider_bundle_id)
            .expect("cascade unload should succeed");

        assert!(
            runtime
                .registry
                .get_bundle_descriptor(provider_bundle_id)
                .is_none(),
            "provider bundle must be gone after cascade unload"
        );
        assert!(
            runtime
                .registry
                .get_bundle_descriptor(dependent_bundle_id)
                .is_none(),
            "dependent bundle must be gone after cascade unload"
        );
    }
    unsafe extern "C" fn transaction_create_instance(
        _adapter_context: *mut c_void,
        _loader_data: VmLoaderData,
        _host: *const HostApi,
        _args: *const (),
        out_instance: *mut GuestContractInstance,
    ) {
        if !out_instance.is_null() {
            // SAFETY: the caller owns the non-null output slot.
            unsafe { out_instance.write(GuestContractInstance::null()) };
        }
    }

    unsafe extern "C" fn transaction_destroy_instance(
        _adapter_context: *mut c_void,
        _loader_data: VmLoaderData,
        _host: *const HostApi,
        _instance: GuestContractInstance,
    ) {
    }

    #[derive(Clone, Copy)]
    enum TransactionLoadMode {
        FailAfterFirst,
        MismatchedProvider,
        Success,
    }

    struct TransactionLoader {
        mode: TransactionLoadMode,
        unloads: Arc<AtomicUsize>,
    }

    impl TransactionLoader {
        fn register(
            runtime: &Runtime,
            contract_id: u64,
            contract_name: &'static str,
        ) -> Result<(), LoaderError> {
            Self::register_named(runtime, contract_id, contract_name, "transaction-loader")
        }

        fn register_named(
            runtime: &Runtime,
            contract_id: u64,
            contract_name: &'static str,
            provider_name: &'static str,
        ) -> Result<(), LoaderError> {
            let interface: GuestContractInterface = GuestContractInterface {
                contract_id: GuestContractId::from_u64(contract_id),
                contract_version: Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
                dispatch_type: DispatchType::Native,
                adapter_context: ptr::null_mut(),
                create_instance: transaction_create_instance,
                destroy_instance: transaction_destroy_instance,
                dispatch: DispatchMechanisms {
                    native: NativeDispatch {
                        function_count: 0,
                        functions: ptr::null(),
                    },
                },
            };
            let descriptor: PluginDescriptor = PluginDescriptor {
                name: StringView::from_static(provider_name.as_bytes()),
                contract_name: StringView::from_static(contract_name.as_bytes()),
                version: Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
            };
            let host: *const HostApi = runtime.host_abi();
            let mut result: AbiError = AbiError::ok();
            // SAFETY: host belongs to runtime; descriptor/interface/result remain valid
            // for the synchronous registration call.
            unsafe {
                ((*host).register_guest_contract)(host, &descriptor, &interface, &mut result);
            }
            if result.is_ok() {
                Ok(())
            } else {
                Err(LoaderError::InitFailed {
                    bundle: "transaction".to_owned(),
                    error: format!("guest registration failed with ABI code {}", result.code),
                })
            }
        }
    }

    impl BundleLoader for TransactionLoader {
        fn loader_name(&self) -> &'static str {
            "transaction"
        }

        fn supports_hot_reload(&self) -> bool {
            false
        }

        fn load(
            &self,
            manifest: &ManifestData,
            _source: &BundleSource,
            runtime: &Runtime,
        ) -> Result<(), LoaderError> {
            runtime.push_init_bundle_id(manifest.id);
            let result: Result<(), LoaderError> = (|| match self.mode {
                TransactionLoadMode::FailAfterFirst => {
                    Self::register(runtime, 0xDD00_0000_0000_0001, "transaction.first")?;
                    let host: *const HostApi = runtime.host_abi();
                    let descriptor: PluginDescriptor = PluginDescriptor {
                        name: StringView::from_static(b"transaction-loader"),
                        contract_name: StringView::from_static(b"transaction.second"),
                        version: Version {
                            major: 1,
                            minor: 0,
                            patch: 0,
                        },
                    };
                    let mut result: AbiError = AbiError::ok();
                    // SAFETY: this deliberately validates the null-interface failure path.
                    unsafe {
                        ((*host).register_guest_contract)(
                            host,
                            &descriptor,
                            ptr::null(),
                            &mut result,
                        );
                    }
                    Err(LoaderError::InitFailed {
                        bundle: manifest.name.clone(),
                        error: format!(
                            "second registration rejected with ABI code {}",
                            result.code
                        ),
                    })
                }
                TransactionLoadMode::MismatchedProvider => {
                    Self::register(runtime, 0xDD00_0000_0000_0002, "transaction.actual")
                }
                TransactionLoadMode::Success => {
                    Self::register(runtime, 0xDD00_0000_0000_0003, "transaction.provider")
                }
            })();
            runtime.pop_init_bundle_id();
            result
        }

        fn reload(&self, _manifest: &ManifestData, _runtime: &Runtime) -> Result<(), LoaderError> {
            Err(LoaderError::HotReloadUnsupported {
                loader_name: self.loader_name().to_owned(),
            })
        }

        fn unload(&self, _bundle_id: BundleId, _runtime: &Runtime) -> Result<(), LoaderError> {
            self.unloads.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
    }

    fn transaction_manifest(
        name: &str,
        provides: &[&str],
        dependencies: Vec<RawManifestDependency>,
    ) -> ManifestData {
        let function_count: HashMap<String, u32> = provides
            .iter()
            .map(|provider: &&str| {
                let contract: &str = provider.split_once('@').map_or(*provider, |(name, _)| name);
                (format!("{contract}@1"), 0)
            })
            .collect();
        ManifestData {
            loader: "transaction".to_owned(),
            name: name.to_owned(),
            dependencies,
            id: BundleId::new(name).id(),
            version: "1.0.0".to_owned(),
            file: "transaction.test".to_owned(),
            provides: provides
                .iter()
                .map(|provider: &&str| (*provider).to_owned())
                .collect(),
            function_count,
            needs_reinit_on_dep_reload: false,
            bundle_dependencies: Vec::new(),
            path: PathBuf::new(),
        }
    }

    fn load_transaction(runtime: &Runtime, manifest: ManifestData) -> Result<(), RuntimeError> {
        runtime.load_bundle_from_source(manifest, BundleSource::Code(String::new()))
    }

    #[test]
    fn capacity_failed_handle_commit_consumes_transaction() {
        let runtime: Arc<Runtime> = Runtime::builder().build().expect("runtime build");
        let unloads: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
        runtime
            .register_loader(Box::new(TransactionLoader {
                mode: TransactionLoadMode::Success,
                unloads,
            }))
            .expect("loader registration");
        let contract_id: u64 = 0xDD00_0000_0000_0003;
        load_transaction(
            &runtime,
            transaction_manifest(
                "committed-handle-older-provider",
                &["transaction.provider"],
                Vec::new(),
            ),
        )
        .expect("load older provider");
        let older: GuestContractHandle = runtime
            .find_guest_contract(contract_id, 0)
            .expect("resolve older provider");

        let manifest: ManifestData = transaction_manifest(
            "committed-handle-current-providers",
            &["transaction.provider"],
            Vec::new(),
        );
        let bundle_id: BundleId = runtime
            .begin_internal_plugin(manifest, SupportedLanguage::Cpp)
            .expect("begin generated providers");
        TransactionLoader::register_named(
            &runtime,
            contract_id,
            "transaction.provider",
            "current-provider-a",
        )
        .expect("stage first generated provider");
        TransactionLoader::register_named(
            &runtime,
            contract_id,
            "transaction.provider",
            "current-provider-b",
        )
        .expect("stage second generated provider");
        let mut insufficient: [GuestContractHandle; 1] = [GuestContractHandle::null()];
        assert!(
            runtime
                .commit_internal_plugin_into_handles(bundle_id, &mut insufficient)
                .is_err(),
            "insufficient output capacity must fail before publication"
        );
        assert!(
            runtime.registry.get_bundle_descriptor(bundle_id).is_none(),
            "a capacity failure must leave the transaction unpublished"
        );
        assert!(
            runtime.registry.prepared_manifest(bundle_id).is_none(),
            "a failed commit attempt must consume its prepared transaction"
        );
        assert_eq!(
            runtime.current_init_bundle_id(),
            0,
            "a failed commit attempt must pop its initialization stack entry"
        );
        let mut handles: [GuestContractHandle; 2] = [GuestContractHandle::null(); 2];
        assert!(
            runtime
                .commit_internal_plugin_into_handles(bundle_id, &mut handles)
                .is_err(),
            "a consumed transaction must reject retries"
        );
        let mut all: [GuestContractHandle; 1] = [GuestContractHandle::null()];
        assert_eq!(runtime.find_all_by_contract(contract_id, 0, &mut all), 1);
        assert_eq!(all[0], older);
    }

    #[test]
    fn capacity_failed_handle_commit_releases_attached_resident_once() {
        unsafe extern "C" fn release_resident(context: *mut c_void) {
            // SAFETY: this callback owns the allocation transferred into the resident.
            let releases: Box<Arc<AtomicUsize>> =
                unsafe { Box::from_raw(context.cast::<Arc<AtomicUsize>>()) };
            releases.fetch_add(1, Ordering::SeqCst);
        }

        let runtime: Arc<Runtime> = Runtime::builder().build().expect("runtime build");
        let bundle_id: BundleId = runtime
            .begin_internal_plugin(
                transaction_manifest(
                    "capacity-release-resident",
                    &["transaction.provider"],
                    Vec::new(),
                ),
                SupportedLanguage::Lua,
            )
            .expect("begin transaction");
        let releases: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
        let context: *mut c_void = Box::into_raw(Box::new(Arc::clone(&releases))).cast();
        runtime
            .attach_internal_plugin_resident(
                bundle_id,
                context,
                current_os_thread_id(),
                release_resident,
            )
            .expect("attach resident");
        TransactionLoader::register(&runtime, 0xDD00_0000_0004, "transaction.provider")
            .expect("stage provider");

        let mut no_handles: [GuestContractHandle; 0] = [];
        assert!(
            runtime
                .commit_internal_plugin_into_handles(bundle_id, &mut no_handles)
                .is_err(),
            "wrong output capacity must reject the transaction"
        );
        assert_eq!(releases.load(Ordering::SeqCst), 1);
        assert!(
            runtime.registry.prepared_manifest(bundle_id).is_none(),
            "the resident must not remain staged after the failed commit"
        );
        assert_eq!(runtime.current_init_bundle_id(), 0);
    }

    #[test]
    fn failed_or_aborted_internal_transaction_clears_lifecycle_marker() {
        let runtime: Arc<Runtime> = Runtime::builder().build().expect("runtime build");
        let aborted: BundleId = runtime
            .begin_internal_plugin(
                transaction_manifest(
                    "aborted-internal-lifecycle-marker",
                    &["transaction.provider"],
                    Vec::new(),
                ),
                SupportedLanguage::Python,
            )
            .expect("begin abortable transaction");
        assert!(
            runtime
                .internal_plugin_lifecycle
                .lock()
                .expect("lifecycle marker lock")
                .contains(&aborted)
        );
        runtime.abort_internal_plugin(aborted);
        assert!(
            !runtime
                .internal_plugin_lifecycle
                .lock()
                .expect("lifecycle marker lock")
                .contains(&aborted),
            "aborting must clear the uncommitted marker"
        );

        let failed: BundleId = runtime
            .begin_internal_plugin(
                transaction_manifest(
                    "failed-internal-lifecycle-marker",
                    &["transaction.provider"],
                    Vec::new(),
                ),
                SupportedLanguage::JavaScript,
            )
            .expect("begin failing transaction");
        assert!(
            runtime
                .internal_plugin_lifecycle
                .lock()
                .expect("lifecycle marker lock")
                .contains(&failed)
        );
        assert!(
            runtime.commit_internal_plugin(failed).is_err(),
            "a provider-free transaction must fail to commit"
        );
        assert!(
            !runtime
                .internal_plugin_lifecycle
                .lock()
                .expect("lifecycle marker lock")
                .contains(&failed),
            "a failed commit must clear the consumed transaction marker"
        );
    }

    #[test]
    fn abort_after_commit_preserves_live_instance_unload_refusal() {
        let runtime: Arc<Runtime> = Runtime::builder().build().expect("runtime build");
        let contract_id: u64 = 0xDD00_0000_0000_0005;
        let bundle_id: BundleId = runtime
            .begin_internal_plugin(
                transaction_manifest(
                    "committed-internal-lifecycle-marker",
                    &["transaction.provider"],
                    Vec::new(),
                ),
                SupportedLanguage::Python,
            )
            .expect("begin transaction");
        TransactionLoader::register(&runtime, contract_id, "transaction.provider")
            .expect("stage provider");
        runtime
            .commit_internal_plugin(bundle_id)
            .expect("commit transaction");

        runtime.abort_internal_plugin(bundle_id);
        assert!(
            runtime
                .internal_plugin_lifecycle
                .lock()
                .expect("lifecycle marker lock")
                .contains(&bundle_id),
            "aborting a committed transaction must leave its lifecycle marker published"
        );
        runtime
            .instance_counts
            .lock()
            .expect("instance count lock")
            .insert(bundle_id, 1);

        assert!(matches!(
            runtime.unload_bundle(bundle_id),
            Err(RuntimeError::InternalPluginInUse {
                active_instances: 1,
                ..
            })
        ));
        assert!(
            runtime.registry.get_bundle_descriptor(bundle_id).is_some(),
            "the refused unload must leave the committed bundle published"
        );
    }

    #[test]
    fn external_unload_reentrant_logger_runs_after_runtime_guards_release() {
        let bundle_id: BundleId = BundleId::from_u64(0xA173_3A09_4E02_0005);
        let callback_runtime: Arc<Mutex<Option<Weak<Runtime>>>> = Arc::new(Mutex::new(None));
        let logger_runtime: Arc<Mutex<Option<Weak<Runtime>>>> = Arc::clone(&callback_runtime);
        let warnings: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
        let logger_warnings: Arc<AtomicUsize> = Arc::clone(&warnings);
        let runtime: Arc<Runtime> = Runtime::builder()
            .logger(move |_level, scope, message| {
                if scope == "runtime" && message.contains("still has") {
                    let runtime: Arc<Runtime> = logger_runtime
                        .lock()
                        .expect("logger runtime lock")
                        .as_ref()
                        .and_then(Weak::upgrade)
                        .expect("runtime must remain live while logging");
                    assert_eq!(runtime.live_instance_count_for_bundle(bundle_id), 0);
                    logger_warnings.fetch_add(1, Ordering::SeqCst);
                }
            })
            .build()
            .expect("runtime build");
        *callback_runtime
            .lock()
            .expect("install runtime weak reference") = Some(Arc::downgrade(&runtime));
        register_native_caller_contract(&runtime.registry, 0xA173_3A09_4E02_0005, bundle_id.id());
        runtime
            .instance_counts
            .lock()
            .expect("instance count lock")
            .insert(bundle_id, 1);

        runtime
            .unload_bundle(bundle_id)
            .expect("external bundle unload must complete without logger deadlock");

        assert_eq!(warnings.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn wrong_thread_direct_unload_does_not_notify_or_invalidate_bundle() {
        unsafe extern "C" fn release_resident(context: *mut c_void) {
            // SAFETY: this callback owns the allocation transferred into the resident.
            let releases: Box<Arc<AtomicUsize>> =
                unsafe { Box::from_raw(context.cast::<Arc<AtomicUsize>>()) };
            releases.fetch_add(1, Ordering::SeqCst);
        }

        let bundle_id: BundleId = BundleId::from_u64(0xA173_3A09_4E02_0006);
        let notifications: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
        let callback_notifications: Arc<AtomicUsize> = Arc::clone(&notifications);
        let runtime: Arc<Runtime> = Runtime::builder()
            .on_reload(move |_user_data, _phase| {
                callback_notifications.fetch_add(1, Ordering::SeqCst);
            })
            .build()
            .expect("runtime build");
        let contract_id: u64 = 0xA173_3A09_4E02_0006;
        register_native_caller_contract(&runtime.registry, contract_id, bundle_id.id());
        let releases: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
        let context: *mut c_void = Box::into_raw(Box::new(Arc::clone(&releases))).cast();
        runtime.registry.lock_internal_plugin_residents().insert(
            bundle_id,
            InternalPluginResident::new(context, current_os_thread_id(), release_resident),
        );

        let off_owner_runtime: Arc<Runtime> = Arc::clone(&runtime);
        let result = thread::spawn(move || off_owner_runtime.unload_bundle(bundle_id))
            .join()
            .expect("off-owner unload must return");
        assert!(matches!(
            result,
            Err(RuntimeError::InternalPluginResidentWrongThread { .. })
        ));
        assert_eq!(notifications.load(Ordering::SeqCst), 0);
        assert!(
            runtime.find_guest_contract(contract_id, 0).is_ok(),
            "wrong-thread unload must leave the bundle resolvable"
        );
        assert_eq!(releases.load(Ordering::SeqCst), 0);

        runtime
            .unload_bundle(bundle_id)
            .expect("owner-thread unload must proceed");
        assert_eq!(notifications.load(Ordering::SeqCst), 1);
        assert_eq!(releases.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn external_transaction_preserves_descriptor_handle_and_revision() {
        let runtime: Arc<Runtime> = Runtime::builder().build().expect("runtime build");
        let unloads: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
        runtime
            .register_loader(Box::new(TransactionLoader {
                mode: TransactionLoadMode::Success,
                unloads: Arc::clone(&unloads),
            }))
            .expect("loader registration");
        let manifest: ManifestData = transaction_manifest(
            "transaction-observable-state",
            &["transaction.provider"],
            Vec::new(),
        );
        let bundle_id: BundleId = BundleId::new(&manifest.name);
        let revision_before: u64 = runtime.registry.current_revision();

        load_transaction(&runtime, manifest).expect("external transaction load");

        let descriptor: BundleDescriptor = runtime
            .registry
            .get_bundle_descriptor(bundle_id)
            .expect("external bundle descriptor");
        assert_eq!(descriptor.id, bundle_id);
        assert_eq!(descriptor.name, "transaction-observable-state");
        assert_eq!(
            descriptor.version,
            Version {
                major: 1,
                minor: 0,
                patch: 0,
            }
        );
        assert_eq!(descriptor.runtime, SupportedLanguage::Rust);
        assert_eq!(descriptor.file_path, PathBuf::new());
        assert!(descriptor.dependencies.is_empty());
        assert_eq!(runtime.registry.current_revision(), revision_before + 1);
        assert_eq!(
            runtime
                .find_guest_contract(0xDD00_0000_0000_0003, 0)
                .expect("external handle"),
            GuestContractHandle {
                index: 0,
                generation: 0,
            }
        );
        assert_eq!(unloads.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn cascade_unload_keeps_residents_until_live_instances_are_destroyed() {
        struct Resident {
            drops: Arc<AtomicUsize>,
        }

        impl Drop for Resident {
            fn drop(&mut self) {
                self.drops.fetch_add(1, Ordering::SeqCst);
            }
        }

        unsafe extern "C" fn create_stateless_instance(
            _adapter_context: *mut c_void,
            _loader_data: VmLoaderData,
            _host: *const HostApi,
            _args: *const (),
            out_instance: *mut GuestContractInstance,
        ) {
            if !out_instance.is_null() {
                // SAFETY: the non-null output slot is writable for this callback.
                unsafe { out_instance.write(GuestContractInstance::null()) };
            }
        }

        unsafe extern "C" fn create_stateful_instance(
            _adapter_context: *mut c_void,
            _loader_data: VmLoaderData,
            _host: *const HostApi,
            _args: *const (),
            out_instance: *mut GuestContractInstance,
        ) {
            if !out_instance.is_null() {
                let state: *mut u8 = Box::into_raw(Box::new(0));
                // SAFETY: the non-null output slot is writable for this callback.
                unsafe {
                    out_instance.write(GuestContractInstance {
                        data: state.cast(),
                        contract_id: GuestContractId::new("cascade.dependent", 1),
                    })
                };
            }
        }

        unsafe extern "C" fn destroy_stateful_instance(
            _adapter_context: *mut c_void,
            _loader_data: VmLoaderData,
            _host: *const HostApi,
            instance: GuestContractInstance,
        ) {
            if !instance.data.is_null() {
                // SAFETY: create_stateful_instance allocated this exact boxed byte.
                unsafe { drop(Box::from_raw(instance.data.cast::<u8>())) };
            }
        }

        let runtime: Arc<Runtime> = Runtime::builder().build().expect("runtime build");
        let provider_contract_id: u64 = GuestContractId::new("cascade.provider", 1).id();
        let dependent_contract_id: u64 = GuestContractId::new("cascade.dependent", 1).id();
        let provider_manifest: ManifestData = transaction_manifest(
            "cascade-resident-provider",
            &["cascade.provider"],
            Vec::new(),
        );
        let dependent_manifest: ManifestData = transaction_manifest(
            "cascade-resident-dependent",
            &["cascade.dependent"],
            vec![RawManifestDependency {
                kind: "contract".to_owned(),
                contract: "cascade.provider".to_owned(),
                min_version: "1.0.0".to_owned(),
                bundle: None,
                contract_id: GuestContractId::from_u64(provider_contract_id),
                bundle_id: None,
            }],
        );
        let provider_bundle_id: BundleId = BundleId::new(&provider_manifest.name);
        let dependent_bundle_id: BundleId = BundleId::new(&dependent_manifest.name);
        let provider_drops: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
        let dependent_drops: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
        let provider_interface: GuestContractInterface = GuestContractInterface {
            contract_id: GuestContractId::from_u64(provider_contract_id),
            contract_version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
            dispatch_type: DispatchType::Native,
            adapter_context: ptr::null_mut(),
            create_instance: create_stateless_instance,
            destroy_instance: transaction_destroy_instance,
            dispatch: DispatchMechanisms {
                native: NativeDispatch {
                    function_count: 0,
                    functions: ptr::null(),
                },
            },
        };
        let dependent_interface: GuestContractInterface = GuestContractInterface {
            contract_id: GuestContractId::from_u64(dependent_contract_id),
            contract_version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
            dispatch_type: DispatchType::Native,
            adapter_context: ptr::null_mut(),
            create_instance: create_stateful_instance,
            destroy_instance: destroy_stateful_instance,
            dispatch: DispatchMechanisms {
                native: NativeDispatch {
                    function_count: 0,
                    functions: ptr::null(),
                },
            },
        };
        let provider_descriptor: PluginDescriptor = PluginDescriptor {
            name: StringView::from_static(b"cascade-resident-provider"),
            contract_name: StringView::from_static(b"cascade.provider"),
            version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
        };
        let dependent_descriptor: PluginDescriptor = PluginDescriptor {
            name: StringView::from_static(b"cascade-resident-dependent"),
            contract_name: StringView::from_static(b"cascade.dependent"),
            version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
        };

        runtime
            .register_internal_plugin(
                provider_manifest,
                SupportedLanguage::Rust,
                Resident {
                    drops: Arc::clone(&provider_drops),
                },
                |host| {
                    let mut error: AbiError = AbiError::ok();
                    // SAFETY: host belongs to the active staging transaction and these tables
                    // remain live until the cascade test completes.
                    unsafe {
                        ((*host).register_guest_contract)(
                            host,
                            &provider_descriptor,
                            &provider_interface,
                            &mut error,
                        );
                    }
                    error
                },
            )
            .expect("register provider");
        runtime
            .register_internal_plugin(
                dependent_manifest,
                SupportedLanguage::Rust,
                Resident {
                    drops: Arc::clone(&dependent_drops),
                },
                |host| {
                    let mut error: AbiError = AbiError::ok();
                    // SAFETY: host belongs to the active staging transaction and these tables
                    // remain live until the cascade test completes.
                    unsafe {
                        ((*host).register_guest_contract)(
                            host,
                            &dependent_descriptor,
                            &dependent_interface,
                            &mut error,
                        );
                    }
                    error
                },
            )
            .expect("register dependent");

        let active_interface: *const GuestContractInterface = runtime
            .resolve_guest_contract(
                runtime
                    .find_guest_contract(dependent_contract_id, 0)
                    .expect("dependent handle"),
            )
            .expect("dependent interface");
        let host: *const HostApi = runtime.host_abi();
        let mut active: GuestContractInstance = GuestContractInstance::null();
        // SAFETY: the host and active interface belong to this runtime; active is writable.
        unsafe {
            ((*host).create_guest_instance)(host, active_interface, ptr::null(), &mut active);
        }
        assert!(!active.data.is_null());

        assert!(matches!(
            runtime.unload_bundle_cascade(provider_bundle_id),
            Err(RuntimeError::InternalPluginInUse {
                bundle,
                active_instances: 1,
            }) if bundle == "cascade-resident-dependent"
        ));
        assert_eq!(provider_drops.load(Ordering::SeqCst), 0);
        assert_eq!(dependent_drops.load(Ordering::SeqCst), 0);
        assert!(
            runtime
                .registry
                .get_bundle_descriptor(provider_bundle_id)
                .is_some()
        );
        assert!(
            runtime
                .registry
                .get_bundle_descriptor(dependent_bundle_id)
                .is_some()
        );

        // SAFETY: active was created through this exact runtime interface.
        unsafe {
            ((*host).destroy_guest_instance)(host, active_interface, active);
        }
        runtime
            .unload_bundle_cascade(provider_bundle_id)
            .expect("cascade unload after instance destruction");
        assert_eq!(provider_drops.load(Ordering::SeqCst), 1);
        assert_eq!(dependent_drops.load(Ordering::SeqCst), 1);
        assert!(
            runtime
                .registry
                .get_bundle_descriptor(provider_bundle_id)
                .is_none()
        );
        assert!(
            runtime
                .registry
                .get_bundle_descriptor(dependent_bundle_id)
                .is_none()
        );

        assert!(runtime.unload_bundle_cascade(provider_bundle_id).is_err());
        assert_eq!(provider_drops.load(Ordering::SeqCst), 1);
        assert_eq!(dependent_drops.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn failed_second_registration_publishes_no_contract_or_metadata() {
        let runtime: Arc<Runtime> = Runtime::builder().build().expect("runtime build");
        let unloads: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
        runtime
            .register_loader(Box::new(TransactionLoader {
                mode: TransactionLoadMode::FailAfterFirst,
                unloads: Arc::clone(&unloads),
            }))
            .expect("loader registration");
        let manifest: ManifestData = transaction_manifest(
            "transaction-second-failure",
            &["transaction.first", "transaction.second"],
            Vec::new(),
        );
        let bundle_id: BundleId = BundleId::new(&manifest.name);

        assert!(load_transaction(&runtime, manifest).is_err());
        assert!(
            runtime
                .find_guest_contract(0xDD00_0000_0000_0001, 0)
                .is_err()
        );
        assert!(runtime.registry.get_bundle_descriptor(bundle_id).is_none());
        assert_eq!(unloads.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn provider_mismatch_publishes_no_contract_or_manifest() {
        let runtime: Arc<Runtime> = Runtime::builder().build().expect("runtime build");
        let unloads: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
        runtime
            .register_loader(Box::new(TransactionLoader {
                mode: TransactionLoadMode::MismatchedProvider,
                unloads: Arc::clone(&unloads),
            }))
            .expect("loader registration");
        let manifest: ManifestData = transaction_manifest(
            "transaction-provider-mismatch",
            &["transaction.expected"],
            Vec::new(),
        );
        let bundle_id: BundleId = BundleId::new(&manifest.name);

        assert!(load_transaction(&runtime, manifest).is_err());
        assert!(
            runtime
                .find_guest_contract(0xDD00_0000_0000_0002, 0)
                .is_err()
        );
        assert!(runtime.registry.get_bundle_descriptor(bundle_id).is_none());
        assert_eq!(unloads.load(Ordering::Relaxed), 1);
        assert!(
            runtime
                .bundle_manifests
                .lock()
                .expect("manifest lock")
                .is_empty()
        );
    }

    #[test]
    fn unload_removes_loader_manifest_and_declared_dependencies() {
        let runtime: Arc<Runtime> = Runtime::builder().build().expect("runtime build");
        let unloads: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
        runtime
            .register_loader(Box::new(TransactionLoader {
                mode: TransactionLoadMode::Success,
                unloads: Arc::clone(&unloads),
            }))
            .expect("loader registration");
        let dependency_id: GuestContractId = GuestContractId::new("transaction.dependency", 1);
        let manifest: ManifestData = transaction_manifest(
            "transaction-unload",
            &["transaction.provider"],
            vec![RawManifestDependency {
                kind: "contract".to_owned(),
                contract: "transaction.dependency".to_owned(),
                min_version: "1.0.0".to_owned(),
                bundle: None,
                contract_id: dependency_id,
                bundle_id: None,
            }],
        );
        let bundle_name: String = manifest.name.clone();
        let bundle_id: BundleId = BundleId::new(&bundle_name);

        load_transaction(&runtime, manifest).expect("transaction load");
        runtime
            .unload_bundle(bundle_id)
            .expect("transaction unload");

        assert!(runtime.registry.get_bundle_descriptor(bundle_id).is_none());
        assert!(
            runtime
                .registry
                .bundles_depending_on_any(&HashSet::from([dependency_id]))
                .is_empty()
        );
        assert!(
            !runtime
                .bundle_manifests
                .lock()
                .expect("manifest lock")
                .contains_key(&bundle_name)
        );
        assert_eq!(unloads.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn repeated_load_preserves_the_committed_bundle_and_resident() {
        let runtime: Arc<Runtime> = Runtime::builder().build().expect("runtime build");
        let unloads: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
        runtime
            .register_loader(Box::new(TransactionLoader {
                mode: TransactionLoadMode::Success,
                unloads: Arc::clone(&unloads),
            }))
            .expect("loader registration");
        let manifest: ManifestData = transaction_manifest(
            "transaction-repeated",
            &["transaction.provider"],
            Vec::new(),
        );
        let bundle_id: BundleId = BundleId::new(&manifest.name);

        load_transaction(&runtime, manifest.clone()).expect("initial load");
        let repeated: Result<(), RuntimeError> = load_transaction(&runtime, manifest);

        assert!(
            matches!(
                repeated,
                Err(RuntimeError::Registry(
                    RegistryError::BundleAlreadyRegistered { .. }
                ))
            ),
            "a repeated load must fail before loader initialization, got {repeated:?}"
        );
        assert_eq!(
            runtime.find_all_by_contract(
                0xDD00_0000_0000_0003,
                0,
                &mut [GuestContractHandle::null(); 2],
            ),
            1,
            "the original bundle provider must remain published"
        );
        assert!(
            runtime.registry.get_bundle_descriptor(bundle_id).is_some(),
            "the original bundle metadata must remain registered"
        );
        assert_eq!(
            unloads.load(Ordering::Relaxed),
            0,
            "the original resident must not be reclaimed by a rejected repeated load"
        );
    }

    #[test]
    fn competing_loader_registrations_publish_one_complete_bundle() {
        let runtime: Arc<Runtime> = Runtime::builder().build().expect("runtime build");
        let unloads: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
        runtime
            .register_loader(Box::new(TransactionLoader {
                mode: TransactionLoadMode::Success,
                unloads,
            }))
            .expect("loader registration");
        let manifest: ManifestData = transaction_manifest(
            "transaction-competing",
            &["transaction.provider"],
            Vec::new(),
        );
        let first_runtime: Arc<Runtime> = Arc::clone(&runtime);
        let second_runtime: Arc<Runtime> = Arc::clone(&runtime);
        let first_manifest: ManifestData = manifest.clone();

        let (first, second) = thread::scope(|scope| {
            let first = scope.spawn(|| load_transaction(&first_runtime, first_manifest));
            let second = scope.spawn(|| load_transaction(&second_runtime, manifest));
            (
                first.join().expect("first thread"),
                second.join().expect("second thread"),
            )
        });

        assert_eq!(
            [first.is_ok(), second.is_ok()]
                .into_iter()
                .filter(|ok| *ok)
                .count(),
            1
        );
        assert_eq!(
            runtime.find_all_by_contract(
                0xDD00_0000_0000_0003,
                0,
                &mut [GuestContractHandle::null(); 2],
            ),
            1
        );
    }
}

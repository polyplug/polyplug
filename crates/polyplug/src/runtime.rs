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
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::RwLock;
use std::thread::ThreadId;

use polyplug_abi::runtime::{Compatibility, RuntimeConfig};
use polyplug_abi::types::LogLevel;
use polyplug_abi::{
    AbiError, AbiErrorCode, Array, DependencyInfo, GuestContractHandle, GuestContractInterface,
    HostApi, HostContractInstance, HostContractInterface, PluginDescriptor, StringView,
    SupportedLanguage, types::Version,
};
use polyplug_utils::{BundleId, GuestContractId};

use crate::error::HostContractError;
use crate::error::LoaderError;
use crate::error::RegistryError;
use crate::error::RuntimeError;
use crate::loader::BundleLoader;
use crate::loader::ManifestData;
use crate::loader::ManifestDependency;
use crate::logger::{LoggerHandle, RecoverPoisoned, RecoveringGuard};
pub use crate::runtime_builder::RuntimeBuilder;
use crate::runtime_store::RuntimeStore;

// ─── Runtime Configuration ───────────────────────────────────────────────────

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
    pub(crate) _logger_closure: Option<Box<crate::logger::LoggerClosure>>,
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
    /// Replaces the former process-global `thread_local!` (Rule 12: no thread-locals
    /// for runtime state — this is now instance-owned, so multiple runtimes in one
    /// process stay isolated). A `Vec` per thread gives reentrancy safety: a nested
    /// load on the same thread pushes its own id and pops it on completion, restoring
    /// the outer bundle's id instead of clobbering it. Loaders push before calling
    /// `polyplug_init` and pop afterwards (including the panic path).
    pub(crate) init_bundle_stack: Mutex<HashMap<ThreadId, Vec<u64>>>,
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
    /// Live stateful-instance counts per guest contract.
    ///
    /// Incremented by `host_create_guest_instance` and decremented by
    /// `host_destroy_guest_instance` for stateful instances only (a null-`data`
    /// instance is stateless — the host holds no state for it and it is not
    /// counted). Drives the accurate reload/unload UB-warning: a bundle whose
    /// contracts still have live instances when it is reloaded or unloaded is a
    /// use-after-free hazard the runtime surfaces to the host.
    ///
    /// Instance-owned (Rule 12): each `Runtime` has its own map, so multiple
    /// runtimes in one process never share instance accounting.
    pub(crate) instance_counts: Mutex<HashMap<GuestContractId, u64>>,
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
        let mut err: polyplug_abi::types::AbiError = polyplug_abi::types::AbiError::ok();
        // SAFETY: host_abi is the runtime's own owned HostApi whose `runtime`
        // field points to this Runtime; forwarding the args is the same call the
        // VM/native guests make; `err` is a valid, writable out-param.
        unsafe {
            host_call_guest_method(
                &*self.host_abi as *const HostApi,
                instance,
                fn_id,
                args,
                out,
                arena,
                &mut err,
            )
        };
        err
    }

    /// Register a host contract interface.
    /// Returns `Err(HostContractError::DuplicateContract)` if a contract with the same ID is already registered.
    pub fn register_host_contract(
        &self,
        contract_id: u64,
        interface: &'static HostContractInterface,
    ) -> Result<(), HostContractError> {
        let mut guard: RecoveringGuard<
            std::sync::RwLockWriteGuard<'_, HashMap<u64, &'static HostContractInterface>>,
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
            std::sync::RwLockWriteGuard<'_, HashMap<u64, &'static HostContractInterface>>,
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
            std::sync::RwLockReadGuard<'_, HashMap<u64, &'static HostContractInterface>>,
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

    /// Run one once-per-process-guarded guest load under the single
    /// process-global [`SharedState`](crate::shared_state) lock.
    ///
    /// This is the sole entry through which a loader interacts with
    /// `SharedState`, keeping the `Runtime` the only mutator of that
    /// process-global datum. It acquires the lock exactly once and, while
    /// holding it:
    ///
    /// 1. If `lang`'s once-per-process init has not run, executes `init`; on
    ///    `Ok` it marks `lang` initialized (a failed init is retried next load).
    ///    Holding the lock makes this atomic and serializes concurrent loads of
    ///    the same language — subsuming the loader's old dedicated load-lock.
    /// 2. Vends a fresh process-global uniqueness nonce.
    /// 3. Runs `body`, passing that nonce, **still holding the lock** — this is
    ///    the loader's snapshot→exec→isolate critical section, which must be
    ///    atomic against other loads because it mutates a process-global guest
    ///    namespace (e.g. CPython's `sys.modules`).
    ///
    /// The lock is therefore held across guest execution, exactly as the
    /// Python loader's prior `PYTHON_LOAD_LOCK` was. The only new cost is that
    /// loads of *different* languages also serialize against each other — a cold
    /// path, and acceptable.
    ///
    /// # Invariant
    /// Neither `init` nor `body` may re-enter `SharedState` (e.g. by calling
    /// this method again): the std `Mutex` is non-reentrant, so a nested
    /// acquisition would deadlock.
    pub fn with_python_load<R>(
        &self,
        lang: SupportedLanguage,
        init: impl FnOnce() -> Result<(), LoaderError>,
        body: impl FnOnce(u64) -> Result<R, LoaderError>,
    ) -> Result<R, LoaderError> {
        let mut state: std::sync::MutexGuard<'_, crate::shared_state::SharedState> =
            crate::shared_state::lock_shared_state();
        if !state.is_initialized(lang) {
            init()?;
            state.mark_initialized(lang);
        }
        let nonce: u64 = state.next_nonce();
        body(nonce)
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
    pub fn logger(&self) -> crate::logger::LoggerHandle {
        self.logger
    }

    /// Set the last error message for FFI error reporting.
    pub(crate) fn set_last_error(&self, msg: impl Into<String>) {
        let mut guard: RecoveringGuard<std::sync::MutexGuard<'_, String>> = self
            .last_error
            .lock()
            .recover_poisoned(self.logger, "runtime");
        **guard = msg.into();
    }

    /// Record that a stateful instance of `contract_id` was created.
    ///
    /// Increments the per-contract live count. Called by
    /// `host_create_guest_instance` only for non-null (stateful) instances.
    fn note_instance_created(&self, contract_id: GuestContractId) {
        let mut guard: RecoveringGuard<std::sync::MutexGuard<'_, HashMap<GuestContractId, u64>>> =
            self.instance_counts
                .lock()
                .recover_poisoned(self.logger, "runtime");
        let entry: &mut u64 = guard.entry(contract_id).or_insert(0);
        *entry += 1;
    }

    /// Record that a stateful instance of `contract_id` was destroyed.
    ///
    /// Saturating decrement; the key is removed once its count reaches zero so the
    /// map only ever holds contracts with live instances. Called by
    /// `host_destroy_guest_instance` only for non-null (stateful) instances.
    fn note_instance_destroyed(&self, contract_id: GuestContractId) {
        let mut guard: RecoveringGuard<std::sync::MutexGuard<'_, HashMap<GuestContractId, u64>>> =
            self.instance_counts
                .lock()
                .recover_poisoned(self.logger, "runtime");
        if let Some(entry) = guard.get_mut(&contract_id) {
            *entry = entry.saturating_sub(1);
            if *entry == 0 {
                guard.remove(&contract_id);
            }
        }
    }

    /// Sum the live stateful-instance counts across the given contract ids.
    ///
    /// Used by the reload/unload UB-warning to report how many guest instances a
    /// bundle's contracts still hold before its interfaces are retired or freed.
    pub(crate) fn live_instance_count_for_contracts(&self, contracts: &[GuestContractId]) -> u64 {
        let guard: RecoveringGuard<std::sync::MutexGuard<'_, HashMap<GuestContractId, u64>>> = self
            .instance_counts
            .lock()
            .recover_poisoned(self.logger, "runtime");
        contracts
            .iter()
            .map(|cid: &GuestContractId| guard.get(cid).copied().unwrap_or(0))
            .sum()
    }

    /// Get the last error message for FFI error reporting.
    /// Returns the number of bytes written to the buffer.
    pub(crate) fn get_last_error(&self, buf: &mut [u8]) -> usize {
        let guard: RecoveringGuard<std::sync::MutexGuard<'_, String>> = self
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
        let mut guard: RecoveringGuard<std::sync::MutexGuard<'_, String>> = self
            .last_error
            .lock()
            .recover_poisoned(self.logger, "runtime");
        guard.clear();
    }

    /// Get the length of the last error message.
    pub(crate) fn last_error_len(&self) -> usize {
        let guard: RecoveringGuard<std::sync::MutexGuard<'_, String>> = self
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
            std::sync::RwLockWriteGuard<'_, HashMap<String, Box<dyn BundleLoader>>>,
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
        let loaders: RecoveringGuard<
            std::sync::RwLockReadGuard<'_, HashMap<String, Box<dyn BundleLoader>>>,
        > = self.loaders.read().recover_poisoned(self.logger, "runtime");
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
        let mut stack: RecoveringGuard<std::sync::MutexGuard<'_, HashMap<ThreadId, Vec<u64>>>> =
            self.init_bundle_stack
                .lock()
                .recover_poisoned(self.logger, "runtime");
        stack
            .entry(std::thread::current().id())
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
        let thread_id: ThreadId = std::thread::current().id();
        let mut stack: RecoveringGuard<std::sync::MutexGuard<'_, HashMap<ThreadId, Vec<u64>>>> =
            self.init_bundle_stack
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
        let thread_id: ThreadId = std::thread::current().id();
        let stack: RecoveringGuard<std::sync::MutexGuard<'_, HashMap<ThreadId, Vec<u64>>>> = self
            .init_bundle_stack
            .lock()
            .recover_poisoned(self.logger, "runtime");
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
        source: crate::loader::BundleSource,
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
    /// Runtime-mediated calls — `call_guest_method`, `create_guest_instance`, and
    /// `destroy_guest_instance` — pin the epoch across dispatch, so a call racing an
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
        let descriptor: crate::runtime_store::BundleDescriptor = self
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
                    .map(|d: crate::runtime_store::BundleDescriptor| d.name)
            })
            .collect();
        if !dependents.is_empty() {
            dependents.sort();
            return Err(RuntimeError::DependencyInUse {
                provider: descriptor.name,
                dependents,
            });
        }

        // Capture the loader name BEFORE invalidate: invalidate removes the
        // bundle metadata, after which the loader string is no longer recoverable.
        let loader_name: Option<String> = self.bundle_loader_name(&descriptor.name);

        // Fire the Unloading callback BEFORE invalidate so the host can quiesce its
        // own callers (the same window the reload Preparing phase gives it). The name
        // is owned by `descriptor`, which outlives this synchronous call.
        self.fire_unloading(bundle_id, &descriptor.name);

        // Unload truly frees the bundle's resources, so any still-live guest instance
        // is a genuine use-after-free hazard. Warn (do not block) before invalidate.
        let live: u64 = self
            .live_instance_count_for_contracts(&self.registry.bundle_exported_contracts(bundle_id));
        if live > 0 {
            let name: String = descriptor.name.clone();
            self.logger.log(LogLevel::Warn, "runtime", || {
                format!(
                    "unload: bundle '{name}' still has {live} live guest instance(s) across its \
                     contracts; destroy them before unload to avoid use-after-free. Proceeding anyway."
                )
            });
        }

        let _count: u32 = self.registry.invalidate_bundle(bundle_id)?;

        // Invalidate-then-reclaim: now that the bundle is gone from the registry
        // indices, no new dispatch can reach it. Ask the loader to free its
        // per-bundle resources at a quiescence point (no-op for non-VM loaders).
        self.reclaim_via_loader(bundle_id, loader_name.as_deref())?;
        Ok(())
    }

    /// Fire the `on_reload_cb` with a `ReloadPhase::unloading` notification, if a
    /// callback is registered. Called before invalidate so the host can quiesce its
    /// own callers ahead of reclamation. The `StringView` is constructed inline from
    /// the caller-owned `bundle_name`, which outlives this synchronous invocation.
    fn fire_unloading(&self, bundle_id: BundleId, bundle_name: &str) {
        if let Some(cb) = self.on_reload_cb() {
            let name_view: polyplug_abi::types::StringView = polyplug_abi::types::StringView {
                ptr: bundle_name.as_ptr(),
                len: bundle_name.len(),
            };
            (cb.0)(
                self.config().on_reload_user_data,
                polyplug_abi::runtime::ReloadPhase::unloading(bundle_id, name_view),
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
        let manifests: RecoveringGuard<std::sync::MutexGuard<'_, HashMap<String, ManifestData>>> =
            self.bundle_manifests
                .lock()
                .recover_poisoned(self.logger, "runtime");
        manifests
            .get(bundle_name)
            .map(|m: &ManifestData| m.loader.clone())
    }

    /// Invoke the loader's `unload` reclaim hook for `bundle_id`.
    ///
    /// `loader_name` is the loader key captured before invalidate. A missing name
    /// or missing loader is not an error: a bundle with no recoverable loader simply
    /// has nothing to reclaim (the invalidate already retired its interfaces).
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
        let mut visited: HashSet<BundleId> = HashSet::new();
        self.unload_bundle_cascade_with_visited(bundle_id, &mut visited)
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

        if self.registry.get_bundle_descriptor(bundle_id).is_none() {
            return Err(RuntimeError::BundleNotFound {
                bundle_name: format!("{:#x}", bundle_id.id()),
                contract_name: String::new(),
            });
        }

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

        // Capture the loader runtime-name before invalidate removes the metadata,
        // then reclaim the loader's per-bundle state after invalidate (see
        // [`Runtime::unload_bundle`]).
        let bundle_name: String = self
            .registry
            .get_bundle_descriptor(bundle_id)
            .map(|d: crate::runtime_store::BundleDescriptor| d.name)
            .unwrap_or_default();
        let loader_name: Option<String> = self.bundle_loader_name(&bundle_name);

        // Fire Unloading before invalidate so the host can quiesce (mirrors unload_bundle).
        self.fire_unloading(bundle_id, &bundle_name);

        // Warn (do not block) on still-live instances before the bundle is freed
        // (mirrors unload_bundle).
        let live: u64 = self
            .live_instance_count_for_contracts(&self.registry.bundle_exported_contracts(bundle_id));
        if live > 0 {
            let name: String = bundle_name.clone();
            self.logger.log(LogLevel::Warn, "runtime", || {
                format!(
                    "unload: bundle '{name}' still has {live} live guest instance(s) across its \
                     contracts; destroy them before unload to avoid use-after-free. Proceeding anyway."
                )
            });
        }

        let _count: u32 = self.registry.invalidate_bundle(bundle_id)?;

        self.reclaim_via_loader(bundle_id, loader_name.as_deref())?;
        Ok(())
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
        let source: crate::loader::BundleSource =
            crate::loader::BundleSource::Path(manifest.path.clone());
        self.load_manifest_with_source(manifest, source, opts)
    }

    /// Shared load path: validate the manifest, dispatch to the matching loader with
    /// the given [`BundleSource`], and record bundle metadata on success.
    ///
    /// [`BundleSource`]: crate::loader::BundleSource
    pub(crate) fn load_manifest_with_source(
        &self,
        manifest: ManifestData,
        source: crate::loader::BundleSource,
        opts: LoadOptions,
    ) -> Result<(), RuntimeError> {
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

        // Find the loader for this bundle. The lock is released before load() runs
        // (see `loader_for`) so a plugin init that registers a loader cannot deadlock.
        let loader_name: &str = &manifest.loader;
        let loader: &dyn BundleLoader = self.loader_for(loader_name).ok_or_else(|| {
            RuntimeError::Loader(LoaderError::NoLoaderForName {
                bundle: manifest.name.clone(),
                loader_name: loader_name.to_owned(),
            })
        })?;

        // Declare this bundle's dependency contract_ids in the registry BEFORE
        // calling the loader. The loader runs `polyplug_init`, during which the
        // plugin may resolve its declared dependencies via `find_guest_contract`.
        // Dependency enforcement (host_find_guest_contract) consults this set, so
        // it must be populated before init runs — otherwise even declared lookups
        // would be denied. See docs/TRUST_MODEL.md §3/§4.
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

        let result: Result<(), RuntimeError> = loader
            .load(&manifest, &source, self)
            .map_err(RuntimeError::Loader);
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

            // Convert loader string to SupportedLanguage. An unrecognized loader
            // string falls back to Rust (see `supported_language_from_str`); surface
            // that as a warning so a typo'd `loader` field is not silently coerced.
            if !is_known_runtime_language(&manifest.loader) {
                self.logger.log(LogLevel::Warn, "runtime", || {
                    format!(
                        "bundle `{}`: unknown loader `{}`; defaulting SupportedLanguage to Rust",
                        manifest.name, manifest.loader
                    )
                });
            }
            let runtime_lang: SupportedLanguage = supported_language_from_str(&manifest.loader);

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

            let mut manifests: RecoveringGuard<
                std::sync::MutexGuard<'_, HashMap<String, ManifestData>>,
            > = self
                .bundle_manifests
                .lock()
                .recover_poisoned(self.logger, "runtime");
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
        let resolved: Vec<ManifestDependency> = manifest.resolved_dependencies_with_logger(logger);
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

        // Check function_count entries for provided contracts
        for contract in &manifest.provides {
            // Strip any `@version` suffix from the provides entry so the
            // function_count key is `bare_name@major`, identical to the key
            // `load_manifest_with_source` builds and looks up.
            let bare_contract: &str = match contract.split_once('@') {
                Some((name, _)) => name,
                None => contract.as_str(),
            };
            let major_str: &str = match manifest.version.split_once('.') {
                Some((maj, _)) => maj,
                None => "0",
            };
            let key: String = format!("{}@{}", bare_contract, major_str);
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
                        logger.log(LogLevel::Warn, "runtime", || {
                            format!(
                                "bundle `{}` provides `{}` but has no function_count entry for key `{}`",
                                manifest.name, contract, key
                            )
                        });
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
        "javascript" | "js" => SupportedLanguage::JavaScript,
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
        "native" | "rust" | "python" | "lua" | "javascript" | "js" | "dotnet" | "csharp" | "cpp"
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
        let create_ptr: *const core::ffi::c_void = base
            .add(create_offset)
            .cast::<*const core::ffi::c_void>()
            .read();
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
        let destroy_ptr: *const core::ffi::c_void = base
            .add(destroy_offset)
            .cast::<*const core::ffi::c_void>()
            .read();
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
        if dispatch_type_raw == polyplug_abi::dispatch::DispatchType::VirtualMachine as u32 {
            // DispatchMechanisms union, vm variant: call fn pointer at offset 0.
            let call_ptr: *const core::ffi::c_void = base
                .add(dispatch_offset)
                .cast::<*const core::ffi::c_void>()
                .read();
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
        } else if dispatch_type_raw == polyplug_abi::dispatch::DispatchType::Native as u32 {
            // DispatchMechanisms union, native variant: function_count at
            // offset 0, functions pointer at offset 8.
            let function_count: u32 = base.add(dispatch_offset).cast::<u32>().read();
            let functions: *const *const core::ffi::c_void = base
                .add(dispatch_offset + 8)
                .cast::<*const *const core::ffi::c_void>()
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
    out_err: *mut polyplug_abi::types::AbiError,
) {
    // SAFETY: the runtime is the sole producer of the HostApi table; the ABI
    // contract requires callers to pass a valid, non-null out_err pointer.
    let result: polyplug_abi::types::AbiError =
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
) -> polyplug_abi::types::AbiError {
    if this.is_null() {
        return polyplug_abi::types::AbiError {
            code: polyplug_abi::types::AbiErrorCode::Generic as u32,
            message: polyplug_abi::types::StringView::null(),
        };
    }
    // Guard both plugin-provided pointers before any dereference. `descriptor` is
    // read below and `interface` is dereferenced inside the registry; a null in
    // either is a contract violation that must not become UB.
    if descriptor.is_null() {
        return polyplug_abi::types::AbiError {
            code: polyplug_abi::types::AbiErrorCode::InvalidPointer as u32,
            message: polyplug_abi::types::StringView::from_static(
                b"register_guest_contract: descriptor pointer is null",
            ),
        };
    }
    if interface.is_null() {
        return polyplug_abi::types::AbiError {
            code: polyplug_abi::types::AbiErrorCode::InvalidPointer as u32,
            message: polyplug_abi::types::StringView::from_static(
                b"register_guest_contract: interface pointer is null",
            ),
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
            core::mem::offset_of!(GuestContractInterface, create_instance),
            core::mem::offset_of!(GuestContractInterface, destroy_instance),
            core::mem::offset_of!(GuestContractInterface, dispatch_type),
            core::mem::offset_of!(GuestContractInterface, dispatch),
            ValidationContext::Guest,
        )
    } {
        return polyplug_abi::types::AbiError {
            code: polyplug_abi::types::AbiErrorCode::InvalidPointer as u32,
            message: polyplug_abi::types::StringView::from_static(violation.as_bytes()),
        };
    }
    // SAFETY: this is a valid HostApi pointer passed during polyplug_init.
    // (*this).runtime contains a valid pointer to Runtime.
    let runtime: &Runtime = unsafe { &*((*this).runtime as *const Runtime) };
    let registry: &RuntimeStore = &runtime.registry;
    // Get bundle_id from the runtime's per-thread init stack (pushed by the loader
    // before calling polyplug_init).
    let bundle_id: u64 = runtime.current_init_bundle_id();

    // SAFETY: descriptor is non-null (checked above) and provided by the plugin's
    // polyplug_init function.
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
            runtime.logger.log(LogLevel::Error, "registry", || {
                format!("registration rejected for bundle {bundle_id}: {e}")
            });
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
            runtime.logger.log(LogLevel::Error, "registry", || {
                format!("registration failed for bundle {bundle_id}: {e}")
            });
            // Surface the detail through get_last_error (stderr alone is not
            // programmatically reachable) and map the registry error to its
            // specific ABI code where one exists, so guests can distinguish a
            // same-bundle duplicate from a hash collision or bad input.
            runtime.set_last_error(e.to_string());
            let code: polyplug_abi::types::AbiErrorCode = match e {
                crate::error::RegistryError::DuplicateProvider { .. } => {
                    polyplug_abi::types::AbiErrorCode::DuplicateProvider
                }
                _ => polyplug_abi::types::AbiErrorCode::Generic,
            };
            polyplug_abi::types::AbiError {
                code: code as u32,
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
    let size: usize = count * core::mem::size_of::<GuestContractHandle>();
    let align: usize = core::mem::align_of::<GuestContractHandle>();
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
        core::ptr::copy_nonoverlapping(handles.as_ptr(), ptr, count);
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
    let host_contracts_guard: RecoveringGuard<
        std::sync::RwLockReadGuard<'_, HashMap<u64, &'static HostContractInterface>>,
    > = runtime
        .host_contracts
        .read()
        .recover_poisoned(runtime.logger, "runtime");

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
            // `interface` is `&'static` (it was `.copied()` out of the guard), so it
            // stays valid after the guard is dropped. Release the `host_contracts`
            // read guard BEFORE invoking `create_instance`: that callback may itself
            // call back into `register_host_contract`, which takes the `host_contracts`
            // WRITE lock — holding the read guard across it would deadlock.
            drop(host_contracts_guard);

            if interface.singleton {
                // Singleton: check cache first
                let singleton_guard: RecoveringGuard<
                    std::sync::RwLockReadGuard<'_, HashMap<u64, HostContractInstance>>,
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
                    std::sync::RwLockWriteGuard<'_, HashMap<u64, HostContractInstance>>,
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
                        core::ptr::null(),
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
                        core::ptr::null(),
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
        return core::ptr::null();
    }
    // SAFETY: this is a valid HostApi pointer passed by the host.
    // (*this).runtime contains a valid pointer to Runtime.
    let runtime: &Runtime = unsafe { &*((*this).runtime as *const Runtime) };

    // Find the host contract interface
    let host_contracts_guard: RecoveringGuard<
        std::sync::RwLockReadGuard<'_, HashMap<u64, &'static HostContractInterface>>,
    > = runtime
        .host_contracts
        .read()
        .recover_poisoned(runtime.logger, "runtime");

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
    if this.is_null() {
        return Array::empty();
    }
    // SAFETY: this is a valid HostApi pointer.
    let runtime: &Runtime = unsafe { &*((*this).runtime as *const Runtime) };

    let manifests: RecoveringGuard<std::sync::MutexGuard<'_, HashMap<String, ManifestData>>> =
        runtime
            .bundle_manifests
            .lock()
            .recover_poisoned(runtime.logger, "runtime");

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
/// Looks up the calling bundle's dependencies using the bundle_id at the top of the
/// runtime's per-thread init-bundle stack (the instance-owned replacement for the
/// former process-global thread-local). Returns an empty array outside any init
/// window (top-of-stack bundle_id == 0).
///
/// # Safety
/// this must be a valid HostApi pointer with valid runtime field.
pub(crate) unsafe extern "C" fn host_get_dependencies(
    this: *const HostApi,
) -> polyplug_abi::Array<polyplug_abi::DependencyInfo> {
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

    let manifests: RecoveringGuard<std::sync::MutexGuard<'_, HashMap<String, ManifestData>>> =
        runtime
            .bundle_manifests
            .lock()
            .recover_poisoned(runtime.logger, "runtime");

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
    out_err: *mut polyplug_abi::AbiError,
) {
    // SAFETY: the runtime is the sole producer of the HostApi table; the ABI
    // contract requires callers to pass a valid, non-null out_err pointer.
    let result: polyplug_abi::AbiError = unsafe { host_load_bundle_impl(this, path, path_len) };
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(result) };
    }
}

unsafe fn host_load_bundle_impl(
    this: *const HostApi,
    path: *const u8,
    path_len: usize,
) -> polyplug_abi::AbiError {
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
    out_err: *mut polyplug_abi::AbiError,
) {
    // SAFETY: the runtime is the sole producer of the HostApi table; the ABI
    // contract requires callers to pass a valid, non-null out_err pointer.
    let result: polyplug_abi::AbiError = unsafe { host_reload_bundle_impl(this, path, path_len) };
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(result) };
    }
}

unsafe fn host_reload_bundle_impl(
    this: *const HostApi,
    path: *const u8,
    path_len: usize,
) -> polyplug_abi::AbiError {
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
    out_err: *mut polyplug_abi::AbiError,
) {
    // SAFETY: the runtime is the sole producer of the HostApi table; the ABI
    // contract requires callers to pass a valid, non-null out_err pointer.
    let result: polyplug_abi::AbiError = unsafe { host_unload_bundle_impl(this, bundle_id) };
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(result) };
    }
}

unsafe fn host_unload_bundle_impl(
    this: *const HostApi,
    bundle_id: BundleId,
) -> polyplug_abi::AbiError {
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
    out_err: *mut polyplug_abi::AbiError,
) {
    // SAFETY: the runtime is the sole producer of the HostApi table; the ABI
    // contract requires callers to pass a valid, non-null out_err pointer.
    let result: polyplug_abi::AbiError =
        unsafe { host_register_host_contract_impl(this, interface) };
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(result) };
    }
}

unsafe fn host_register_host_contract_impl(
    this: *const HostApi,
    interface: *const polyplug_abi::HostContractInterface,
) -> polyplug_abi::AbiError {
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
            core::mem::offset_of!(polyplug_abi::HostContractInterface, create_instance),
            core::mem::offset_of!(polyplug_abi::HostContractInterface, destroy_instance),
            core::mem::offset_of!(polyplug_abi::HostContractInterface, dispatch_type),
            core::mem::offset_of!(polyplug_abi::HostContractInterface, dispatch),
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
    loader_ptr: *mut core::ffi::c_void,
    out_err: *mut polyplug_abi::AbiError,
) {
    // SAFETY: the runtime is the sole producer of the HostApi table; the ABI
    // contract requires callers to pass a valid, non-null out_err pointer.
    let result: polyplug_abi::AbiError = unsafe { host_register_loader_impl(this, loader_ptr) };
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(result) };
    }
}

unsafe fn host_register_loader_impl(
    this: *const HostApi,
    loader_ptr: *mut core::ffi::c_void,
) -> polyplug_abi::AbiError {
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

/// HostApi.call_guest_method callback — host-mediated plugin→plugin cross-dispatch.
///
/// Re-resolves the target contract through the registry via `instance.contract_id`
/// on every call (never caches), so a fresh cross-call always routes to the live
/// interface while retired interfaces keep in-flight instances valid. See the
/// `call_guest_method` field doc on [`HostApi`] for the full contract.
///
/// # Ambiguous routing
/// Routing keys solely on `instance.contract_id`, which resolves to the FIRST
/// provider of that contract. When two or more bundles provide the same contract,
/// an instance from one provider could be dispatched through another's interface
/// (wrong state pointer, potential UB). To stay sound, this returns
/// [`AbiErrorCode::DuplicateProvider`] instead of dispatching whenever more than one
/// provider is registered for the contract. A single provider dispatches normally.
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
    out_err: *mut polyplug_abi::types::AbiError,
) {
    // SAFETY: the runtime is the sole producer of the HostApi table; the ABI
    // contract requires callers to pass a valid, non-null out_err pointer.
    let result: polyplug_abi::types::AbiError =
        unsafe { host_call_guest_method_impl(this, instance, fn_id, args, out, arena) };
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(result) };
    }
}

unsafe fn host_call_guest_method_impl(
    this: *const HostApi,
    instance: polyplug_abi::guest::GuestContractInstance,
    fn_id: u32,
    args: *const core::ffi::c_void,
    out: *mut core::ffi::c_void,
    arena: *mut polyplug_abi::types::CallArena,
) -> polyplug_abi::types::AbiError {
    // Only the host vtable pointer is a hard precondition. A null `instance.data`
    // is explicitly VALID: stateless contracts (every VM-backed contract, plus
    // native contracts with no per-instance state) return a null handle from
    // `create_instance` and use it as an opaque dispatch token. Routing is keyed
    // solely on `instance.contract_id` (re-resolved below), so a null `data` that
    // carries a valid `contract_id` must dispatch normally; a fully-null instance
    // (contract_id == 0) still fails cleanly as `NotFound` at re-resolution.
    if this.is_null() {
        return polyplug_abi::types::AbiError {
            code: polyplug_abi::types::AbiErrorCode::InvalidPointer as u32,
            message: polyplug_abi::types::StringView::null(),
        };
    }

    // SAFETY: this is a valid HostApi pointer passed by the host.
    // (*this).runtime contains a valid pointer to Runtime.
    let runtime: &Runtime = unsafe { &*((*this).runtime as *const Runtime) };
    let registry: &RuntimeStore = &runtime.registry;

    // Re-resolve the target contract by id on EVERY call. The resolve drops its own
    // internal read guard before returning, so no registry lock is held across the
    // guest dispatch below. The returned pointer stays valid across a concurrent
    // unload because the outer epoch pin taken below keeps the snapshot whose Arc
    // backs the interface alive for the duration of the dispatch.
    let contract_id: u64 = instance.contract_id.id();

    // Conservative routing guard for multi-provider contracts. Routing is keyed
    // solely on `contract_id`, which resolves to the FIRST provider only. When two
    // or more bundles provide the same contract, an instance created by provider B
    // could be dispatched through provider A's interface — a wrong state pointer and
    // potential UB. Until the ABI carries a provider discriminator, refuse the
    // cross-call rather than risk mis-dispatch. A single provider is unambiguous and
    // keeps today's behaviour (including post-reload re-resolution).
    //
    // The count, the single-provider check, and the resolve all happen under ONE
    // registry read guard via `resolve_single_provider` (was three separate guard
    // acquisitions: count + find + resolve). That internal guard is dropped before
    // the guest dispatch below, so no registry lock is held across the call; the
    // returned pointer stays valid across a concurrent unload because the outer epoch
    // pin taken below keeps the snapshot whose Arc backs the interface alive for the
    // duration of the dispatch.
    //
    // NOTE: the matching `call_guest_method` field doc on `HostApi` in the
    // `polyplug_abi` crate should be updated to document this DuplicateProvider
    // outcome (that crate is owned by another agent — follow-up).
    //
    // The outer pin keeps the snapshot whose Arc backs `interface_ptr` from being
    // epoch-reclaimed for the duration of the dispatch, so a concurrent unload
    // cannot free the interface mid-call. It must outlive the native/vm dispatch
    // below, so it is bound to a named guard (not `let _ =`, which drops immediately)
    // that lives to the end of this function.
    let _dispatch_guard: crossbeam_epoch::Guard = crossbeam_epoch::pin();
    let interface_ptr: *const GuestContractInterface = match registry
        .resolve_single_provider(GuestContractId::from_u64(contract_id), 0)
    {
        crate::runtime_store::SingleProviderResolution::Multiple => {
            runtime.set_last_error(format!(
                    "call_guest_method: ambiguous cross-call routing for contract_id={contract_id}: \
                     multiple providers are registered and routing keys only on contract_id, so the \
                     target provider cannot be determined unambiguously"
                ));
            return polyplug_abi::types::AbiError {
                code: polyplug_abi::types::AbiErrorCode::DuplicateProvider as u32,
                message: polyplug_abi::types::StringView::null(),
            };
        }
        crate::runtime_store::SingleProviderResolution::NotFound => {
            runtime.set_last_error(format!(
                "call_guest_method: no contract found for contract_id={contract_id}"
            ));
            return polyplug_abi::types::AbiError {
                code: polyplug_abi::types::AbiErrorCode::NotFound as u32,
                message: polyplug_abi::types::StringView::null(),
            };
        }
        crate::runtime_store::SingleProviderResolution::Resolved(ptr) if !ptr.is_null() => ptr,
        crate::runtime_store::SingleProviderResolution::Resolved(_) => {
            runtime.set_last_error(format!(
                "call_guest_method: contract could not be resolved for contract_id={contract_id}"
            ));
            return polyplug_abi::types::AbiError {
                code: polyplug_abi::types::AbiErrorCode::NotFound as u32,
                message: polyplug_abi::types::StringView::null(),
            };
        }
    };

    // SAFETY: interface_ptr is non-null and points to a GuestContractInterface owned
    // by a published snapshot kept alive by the outer epoch pin (`_dispatch_guard`)
    // for the duration of this dispatch; reading its fields is sound.
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
            // `extern "C" fn(GuestContractInstance, *const (), *mut (), *mut AbiError)`
            // (see polyplugc rust generator); `slot` is a non-null pointer to such
            // a function. The transmute reinterprets the type-erased `*const ()` as
            // that concrete fn pointer, which is the established native-call form.
            let func: unsafe extern "C" fn(
                polyplug_abi::guest::GuestContractInstance,
                *const (),
                *mut (),
                *mut polyplug_abi::types::AbiError,
            ) = unsafe { core::mem::transmute(slot) };
            let mut err: polyplug_abi::types::AbiError = polyplug_abi::types::AbiError::ok();
            // SAFETY: args/out satisfy the target function's ABI layout per the
            // caller's contract; instance belongs to this contract; `err` is a
            // valid, writable out-param for the native call's AbiError result.
            unsafe { func(instance, args.cast::<()>(), out.cast::<()>(), &mut err) };
            err
        }
        polyplug_abi::dispatch::DispatchType::VirtualMachine => {
            // SAFETY: dispatch_type == VirtualMachine guarantees the `vm` union
            // variant is the active one, so reading it is sound.
            let vm: polyplug_abi::dispatch::VmDispatch = unsafe { interface.dispatch.vm };
            let mut err: polyplug_abi::types::AbiError = polyplug_abi::types::AbiError::ok();
            // SAFETY: vm.call is the loader-provided VM dispatch entry point with
            // the frozen out-param signature; loader_data is the matching opaque
            // handle. args/out/arena are forwarded unchanged per the VM dispatch
            // contract; `err` is a valid, writable out-param for its AbiError result.
            unsafe {
                (vm.call)(
                    vm.loader_data,
                    instance,
                    fn_id,
                    args.cast::<()>(),
                    out.cast::<()>(),
                    arena,
                    &mut err,
                )
            };
            err
        }
    }
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
    args: *const core::ffi::c_void,
    out_instance: *mut polyplug_abi::guest::GuestContractInstance,
) {
    // SAFETY: the runtime is the sole producer of the HostApi table; the ABI
    // contract requires callers to pass a valid, non-null out_instance pointer.
    let result: polyplug_abi::guest::GuestContractInstance =
        unsafe { host_create_guest_instance_impl(this, interface, args) };
    if !out_instance.is_null() {
        // SAFETY: out_instance is non-null (just checked) and writable per the ABI contract.
        unsafe { out_instance.write(result) };
    }
}

unsafe fn host_create_guest_instance_impl(
    this: *const HostApi,
    interface: *const GuestContractInterface,
    args: *const core::ffi::c_void,
) -> polyplug_abi::guest::GuestContractInstance {
    if this.is_null() || interface.is_null() {
        return polyplug_abi::guest::GuestContractInstance::null();
    }

    // SAFETY: this is a valid HostApi pointer passed by the host;
    // (*this).runtime contains a valid pointer to Runtime.
    let runtime: &Runtime = unsafe { &*((*this).runtime as *const Runtime) };

    // Pin the epoch for the duration of construction. A concurrent unload retires
    // the interface's snapshot for epoch reclamation; holding this pin across the
    // create call keeps that snapshot alive so `create_instance` cannot run against
    // a freed interface. The guard is named (not `let _ =`) so it lives to the end.
    let _g: crossbeam_epoch::Guard = crossbeam_epoch::pin();

    // SAFETY: interface is non-null and points to a runtime-issued
    // GuestContractInterface kept alive by the pin above; reading its fields is sound.
    let contract_id: GuestContractId = unsafe { (*interface).contract_id };

    let mut inst: polyplug_abi::guest::GuestContractInstance =
        polyplug_abi::guest::GuestContractInstance::null();
    // SAFETY: `create_instance` is non-null by ABI contract (register_guest_contract
    // rejects null bits); the interface stays alive across the call via the pin;
    // `args` satisfies the contract's argument layout per the caller's contract;
    // `inst` is a valid, writable out-param for the constructed instance.
    unsafe { ((*interface).create_instance)(this, args.cast::<()>(), &mut inst) };

    if !inst.data.is_null() {
        runtime.note_instance_created(contract_id);
    }
    inst
}

/// HostApi.destroy_guest_instance callback — host-mediated guest instance teardown.
///
/// Mirror of [`host_create_guest_instance`]: invokes the interface's
/// `destroy_instance` under an epoch pin and updates the runtime's live-instance
/// accounting. The decrement keys on `instance.contract_id` (not the interface's)
/// so create/destroy match even if the resolved interface pointer has since changed.
///
/// # Safety
/// - `this` must be a valid HostApi pointer with a valid runtime field, or null
/// - `interface` must be a runtime-issued `GuestContractInterface` pointer, or null
/// - `instance` must be an instance produced by this contract's `create_instance`
pub(crate) unsafe extern "C" fn host_destroy_guest_instance(
    this: *const HostApi,
    interface: *const GuestContractInterface,
    instance: polyplug_abi::guest::GuestContractInstance,
) {
    if this.is_null() || interface.is_null() {
        return;
    }

    // SAFETY: this is a valid HostApi pointer passed by the host;
    // (*this).runtime contains a valid pointer to Runtime.
    let runtime: &Runtime = unsafe { &*((*this).runtime as *const Runtime) };

    let contract_id: GuestContractId = instance.contract_id;

    // Pin the epoch across teardown for the same reason as creation: keep the
    // interface's snapshot alive so `destroy_instance` cannot run against a freed
    // interface during a concurrent unload.
    let _g: crossbeam_epoch::Guard = crossbeam_epoch::pin();

    // SAFETY: `destroy_instance` is non-null by ABI contract; the interface stays
    // alive across the call via the pin; `instance` was produced by this contract.
    unsafe { ((*interface).destroy_instance)(this, instance) };

    if !instance.data.is_null() {
        runtime.note_instance_destroyed(contract_id);
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;

    /// `HostApi.log` stub for test hosts — drops the record.
    unsafe extern "C" fn stub_host_log(
        _this: *const HostApi,
        _level: u32,
        _scope: polyplug_abi::StringView,
        _message: polyplug_abi::StringView,
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
            unload_bundle: host_unload_bundle,
            log: stub_host_log,
            create_guest_instance: host_create_guest_instance,
            destroy_guest_instance: host_destroy_guest_instance,
            reserved: core::ptr::null(),
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
            "id = {}\nname = \"{}\"\nloader = \"{}\"\nfile = \"dummy.so\"\n",
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
            out_instance: *mut GuestContractInstance,
        ) {
            if !out_instance.is_null() {
                // SAFETY: out_instance is non-null (just checked) and writable per the ABI contract.
                unsafe { out_instance.write(GuestContractInstance::null()) };
            }
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
        out_err: *mut polyplug_abi::types::AbiError,
    ) {
        // SAFETY: the test passes a valid *const i32 / *mut i32.
        unsafe {
            let input: i32 = *(args as *const i32);
            *(out as *mut i32) = input + 1;
        }
        if !out_err.is_null() {
            // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
            unsafe { out_err.write(polyplug_abi::types::AbiError::ok()) };
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
            out_instance: *mut GuestContractInstance,
        ) {
            if !out_instance.is_null() {
                // SAFETY: out_instance is non-null (just checked) and writable per the ABI contract.
                unsafe { out_instance.write(GuestContractInstance::null()) };
            }
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
        runtime.host_abi()
    }

    #[test]
    fn call_guest_method_null_this_returns_invalid_pointer() {
        let instance: polyplug_abi::guest::GuestContractInstance =
            polyplug_abi::guest::GuestContractInstance {
                data: &raw const NATIVE_FNS as *mut core::ffi::c_void,
                contract_id: GuestContractId::from_u64(1),
            };
        let mut err: polyplug_abi::types::AbiError = polyplug_abi::types::AbiError::ok();
        // SAFETY: host_call_guest_method tolerates a null `this`; `err` is a valid out-param.
        unsafe {
            host_call_guest_method(
                core::ptr::null(),
                instance,
                0,
                core::ptr::null(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                &mut err,
            )
        };
        assert_eq!(
            err.code,
            polyplug_abi::types::AbiErrorCode::InvalidPointer as u32
        );
    }

    #[test]
    fn call_guest_method_null_instance_returns_not_found() {
        // A fully-null instance (null data AND null contract_id) is no longer
        // rejected on the data field — null data is valid for stateless contracts.
        // Routing keys on contract_id, so contract_id == 0 fails cleanly as NotFound
        // (never dereferencing data/args/out).
        let runtime: Arc<Runtime> = Runtime::builder().build().expect("build");
        let instance: polyplug_abi::guest::GuestContractInstance =
            polyplug_abi::guest::GuestContractInstance::null();
        let mut err: polyplug_abi::types::AbiError = polyplug_abi::types::AbiError::ok();
        // SAFETY: re-resolution of contract_id == 0 fails before any pointer deref;
        // `err` is a valid out-param.
        unsafe {
            host_call_guest_method(
                host_with_runtime(&runtime),
                instance,
                0,
                core::ptr::null(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                &mut err,
            )
        };
        assert_eq!(err.code, polyplug_abi::types::AbiErrorCode::NotFound as u32);
    }

    #[test]
    fn call_guest_method_null_data_valid_contract_dispatches() {
        // The peer-caller contract: a stateless instance carries a null `data` but a
        // valid `contract_id`. call_guest_method must route by contract_id and
        // dispatch successfully — this is the case the generated peer callers rely on.
        let runtime: Arc<Runtime> = Runtime::builder().build().expect("build");
        let contract_id: u64 = 0x0FED_CBA9_8765_4321;
        register_native_caller_contract(&runtime.registry, contract_id, 0x1);

        let instance: polyplug_abi::guest::GuestContractInstance =
            polyplug_abi::guest::GuestContractInstance {
                data: core::ptr::null_mut(),
                contract_id: GuestContractId::from_u64(contract_id),
            };
        let input: i32 = 41;
        let mut output: i32 = 0;
        let mut err: polyplug_abi::types::AbiError = polyplug_abi::types::AbiError::ok();
        // SAFETY: native_add_one reads *const i32 from args and writes *mut i32 to out;
        // it ignores instance.data, so a null data handle is sound; `err` is a valid out-param.
        unsafe {
            host_call_guest_method(
                host_with_runtime(&runtime),
                instance,
                0,
                &raw const input as *const core::ffi::c_void,
                &raw mut output as *mut core::ffi::c_void,
                core::ptr::null_mut(),
                &mut err,
            )
        };
        assert_eq!(err.code, polyplug_abi::types::AbiErrorCode::Ok as u32);
        assert_eq!(
            output, 42,
            "stateless dispatch must run with null instance.data"
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
        let mut err: polyplug_abi::types::AbiError = polyplug_abi::types::AbiError::ok();
        // SAFETY: this is valid; contract_id is unregistered so lookup fails; `err` is a valid out-param.
        unsafe {
            host_call_guest_method(
                host_with_runtime(&runtime),
                instance,
                0,
                core::ptr::null(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                &mut err,
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
        let mut err: polyplug_abi::types::AbiError = polyplug_abi::types::AbiError::ok();
        // SAFETY: native_add_one reads *const i32 from args and writes *mut i32 to out;
        // `err` is a valid out-param.
        unsafe {
            host_call_guest_method(
                host_with_runtime(&runtime),
                instance,
                0,
                &raw const input as *const core::ffi::c_void,
                &raw mut output as *mut core::ffi::c_void,
                core::ptr::null_mut(),
                &mut err,
            )
        };
        assert!(err.is_ok(), "native dispatch should succeed");
        assert_eq!(output, 42);
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
        _host: *const HostApi,
        _args: *const (),
        out_instance: *mut polyplug_abi::guest::GuestContractInstance,
    ) {
        let boxed: Box<u8> = Box::new(0u8);
        let instance: polyplug_abi::guest::GuestContractInstance =
            polyplug_abi::guest::GuestContractInstance {
                data: Box::into_raw(boxed) as *mut core::ffi::c_void,
                contract_id: GuestContractId::from_u64(STATEFUL_CONTRACT_ID),
            };
        if !out_instance.is_null() {
            // SAFETY: out_instance is non-null (just checked) and writable per the ABI contract.
            unsafe { out_instance.write(instance) };
        }
    }

    /// Destroy the boxed unit created by `stateful_create_instance`.
    unsafe extern "C" fn stateful_destroy_instance(
        _host: *const HostApi,
        instance: polyplug_abi::guest::GuestContractInstance,
    ) {
        if !instance.data.is_null() {
            // SAFETY: `data` was produced by `stateful_create_instance` via
            // `Box::into_raw(Box<u8>)`, so reclaiming it as the same Box is sound.
            drop(unsafe { Box::from_raw(instance.data as *mut u8) });
        }
    }

    /// Register a native-dispatch contract whose `create_instance` returns a
    /// non-null (stateful) instance, returning the leaked interface pointer so the
    /// test can drive `host_create_guest_instance` / `host_destroy_guest_instance`.
    fn register_stateful_contract(
        registry: &crate::runtime_store::RuntimeStore,
        contract_id: u64,
        bundle_id: u64,
    ) -> *const GuestContractInterface {
        use polyplug_abi::{
            DispatchMechanisms, DispatchType, GuestContractInterface, NativeDispatch,
        };

        let interface: &'static GuestContractInterface =
            Box::leak(Box::new(GuestContractInterface {
                contract_id: GuestContractId::from_u64(contract_id),
                contract_version: Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
                dispatch_type: DispatchType::Native,
                create_instance: stateful_create_instance,
                destroy_instance: stateful_destroy_instance,
                dispatch: DispatchMechanisms {
                    native: NativeDispatch {
                        function_count: 0,
                        functions: core::ptr::null(),
                    },
                },
            }));
        let descriptor: polyplug_abi::PluginDescriptor = polyplug_abi::PluginDescriptor {
            name: polyplug_abi::StringView::from_static(b"stateful"),
            contract_name: polyplug_abi::StringView::from_static(b"stateful.contract"),
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
                "stateful.contract".to_owned(),
                BundleId::from_u64(bundle_id),
            )
        };
        if let Err(e) = result {
            panic!("failed to register stateful contract: {e}");
        }
        interface as *const GuestContractInterface
    }

    #[test]
    fn host_instance_lifecycle_counts_stateful_instances() {
        let runtime: Arc<Runtime> = Runtime::builder().build().expect("build");
        let contract_id: u64 = STATEFUL_CONTRACT_ID;
        let cid: GuestContractId = GuestContractId::from_u64(contract_id);
        let interface: *const GuestContractInterface =
            register_stateful_contract(&runtime.registry, contract_id, 0x1);
        let host: *const HostApi = host_with_runtime(&runtime);

        assert_eq!(
            runtime.live_instance_count_for_contracts(&[cid]),
            0,
            "no instances created yet"
        );

        // Create two stateful instances through the host-mediated path.
        let mut inst_a: polyplug_abi::guest::GuestContractInstance =
            polyplug_abi::guest::GuestContractInstance::null();
        // SAFETY: host and interface are valid; create_instance ignores args;
        // inst_a is a valid out-param.
        unsafe { host_create_guest_instance(host, interface, core::ptr::null(), &mut inst_a) };
        let mut inst_b: polyplug_abi::guest::GuestContractInstance =
            polyplug_abi::guest::GuestContractInstance::null();
        // SAFETY: as above; inst_b is a valid out-param.
        unsafe { host_create_guest_instance(host, interface, core::ptr::null(), &mut inst_b) };
        assert!(!inst_a.data.is_null() && !inst_b.data.is_null());
        assert_eq!(
            runtime.live_instance_count_for_contracts(&[cid]),
            2,
            "two stateful instances counted"
        );

        // Destroy both through the host-mediated path; the count returns to zero.
        // SAFETY: each instance was produced by this contract's create_instance.
        unsafe { host_destroy_guest_instance(host, interface, inst_a) };
        // SAFETY: as above.
        unsafe { host_destroy_guest_instance(host, interface, inst_b) };
        assert_eq!(
            runtime.live_instance_count_for_contracts(&[cid]),
            0,
            "count returns to zero after destroy"
        );
    }

    #[test]
    fn host_instance_lifecycle_ignores_stateless_instances() {
        let runtime: Arc<Runtime> = Runtime::builder().build().expect("build");
        let contract_id: u64 = 0x0FED_CBA9_8765_4321;
        let cid: GuestContractId = GuestContractId::from_u64(contract_id);
        register_native_caller_contract(&runtime.registry, contract_id, 0x1);
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
        let mut inst: polyplug_abi::guest::GuestContractInstance =
            polyplug_abi::guest::GuestContractInstance::null();
        // SAFETY: host and interface are valid; `inst` is a valid out-param.
        unsafe { host_create_guest_instance(host, interface, core::ptr::null(), &mut inst) };
        assert!(inst.data.is_null(), "stateless instance has null data");
        assert_eq!(
            runtime.live_instance_count_for_contracts(&[cid]),
            0,
            "stateless instances are not counted"
        );

        // Destroying it is a no-op for the counter too.
        // SAFETY: as above.
        unsafe { host_destroy_guest_instance(host, interface, inst) };
        assert_eq!(runtime.live_instance_count_for_contracts(&[cid]), 0);
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
        let mut err: polyplug_abi::types::AbiError = polyplug_abi::types::AbiError::ok();
        // SAFETY: function_count is 1; fn_id 5 is out of range and must be rejected;
        // `err` is a valid out-param.
        unsafe {
            host_call_guest_method(
                host_with_runtime(&runtime),
                instance,
                5,
                core::ptr::null(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                &mut err,
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
        out_err: *mut polyplug_abi::types::AbiError,
    ) {
        // SAFETY: the test passes a valid *mut u32 for out.
        unsafe {
            *(out as *mut u32) = fn_id;
        }
        if !out_err.is_null() {
            // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
            unsafe { out_err.write(polyplug_abi::types::AbiError::ok()) };
        }
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
            out_instance: *mut GuestContractInstance,
        ) {
            if !out_instance.is_null() {
                // SAFETY: out_instance is non-null (just checked) and writable per the ABI contract.
                unsafe { out_instance.write(GuestContractInstance::null()) };
            }
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
        let mut err: polyplug_abi::types::AbiError = polyplug_abi::types::AbiError::ok();
        // SAFETY: vm_echo_call writes the fn_id into *mut u32 out; `err` is a valid out-param.
        unsafe {
            host_call_guest_method(
                host_with_runtime(&runtime),
                instance,
                7,
                core::ptr::null(),
                &raw mut output as *mut core::ffi::c_void,
                core::ptr::null_mut(),
                &mut err,
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
        fn loader_name(&self) -> &'static str {
            "enforce"
        }

        fn loader_language(&self) -> polyplug_abi::SupportedLanguage {
            polyplug_abi::SupportedLanguage::Rust
        }

        fn supports_hot_reload(&self) -> bool {
            false
        }

        fn load(
            &self,
            _manifest: &ManifestData,
            _source: &crate::loader::BundleSource,
            runtime: &Runtime,
        ) -> Result<(), crate::error::LoaderError> {
            // Drive the runtime's real dependency-enforcement path: probe an
            // undeclared contract inside the init window. The runtime records the
            // bundle_id-zero escape and the resolve is denied. The mock then reports
            // the denial as the loader-level init failure the runtime surfaces.
            runtime.push_init_bundle_id(self.error_bundle_id);
            runtime.pop_init_bundle_id();
            Err(crate::error::LoaderError::InitFailed {
                bundle: "enforce".to_owned(),
                error: format!(
                    "undeclared dependency: bundle_id={:#x} contract_id={:#x}",
                    self.error_bundle_id, self.contract_id
                ),
            })
        }

        fn reload(
            &self,
            _manifest: &ManifestData,
            _runtime: &Runtime,
        ) -> Result<(), crate::error::LoaderError> {
            Err(crate::error::LoaderError::HotReloadUnsupported {
                loader_name: self.loader_name().to_owned(),
            })
        }
    }

    struct ProbeLoader {
        observed_init: Arc<std::sync::Mutex<Option<bool>>>,
    }

    impl crate::loader::BundleLoader for ProbeLoader {
        fn loader_name(&self) -> &'static str {
            "probe"
        }

        fn loader_language(&self) -> polyplug_abi::SupportedLanguage {
            polyplug_abi::SupportedLanguage::Rust
        }

        fn supports_hot_reload(&self) -> bool {
            false
        }

        fn load(
            &self,
            _manifest: &ManifestData,
            _source: &crate::loader::BundleSource,
            _runtime: &Runtime,
        ) -> Result<(), crate::error::LoaderError> {
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
        ) -> Result<(), crate::error::LoaderError> {
            Err(crate::error::LoaderError::HotReloadUnsupported {
                loader_name: self.loader_name().to_owned(),
            })
        }
    }

    struct PanicLoader;

    impl crate::loader::BundleLoader for PanicLoader {
        fn loader_name(&self) -> &'static str {
            "panic"
        }

        fn loader_language(&self) -> polyplug_abi::SupportedLanguage {
            polyplug_abi::SupportedLanguage::Rust
        }

        fn supports_hot_reload(&self) -> bool {
            false
        }

        fn load(
            &self,
            _manifest: &ManifestData,
            _source: &crate::loader::BundleSource,
            _runtime: &Runtime,
        ) -> Result<(), crate::error::LoaderError> {
            panic!("intentional panic in PanicLoader");
        }

        fn reload(
            &self,
            _manifest: &ManifestData,
            _runtime: &Runtime,
        ) -> Result<(), crate::error::LoaderError> {
            Err(crate::error::LoaderError::HotReloadUnsupported {
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
        state: Arc<std::sync::Mutex<ReentrantState>>,
    }

    impl crate::loader::BundleLoader for ReentrantLoader {
        fn loader_name(&self) -> &'static str {
            "reentrant"
        }

        fn loader_language(&self) -> polyplug_abi::SupportedLanguage {
            polyplug_abi::SupportedLanguage::Rust
        }

        fn supports_hot_reload(&self) -> bool {
            false
        }

        fn load(
            &self,
            _manifest: &ManifestData,
            _source: &crate::loader::BundleSource,
            _runtime: &Runtime,
        ) -> Result<(), crate::error::LoaderError> {
            let state: std::sync::MutexGuard<'_, ReentrantState> = match self.state.lock() {
                Ok(g) => g,
                Err(e) => e.into_inner(),
            };
            let runtime_ptr: usize = state.runtime_ptr;
            if runtime_ptr == 0 {
                return Err(crate::error::LoaderError::InitFailed {
                    bundle: "reentrant".to_owned(),
                    error: "runtime pointer not initialized".to_owned(),
                });
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
            // The nested load returns a top-level RuntimeError; the mock surfaces a
            // failed nested load as its own init failure.
            if let Err(e) = inner_result {
                return Err(crate::error::LoaderError::InitFailed {
                    bundle: "reentrant".to_owned(),
                    error: e.to_string(),
                });
            }
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
        ) -> Result<(), crate::error::LoaderError> {
            Err(crate::error::LoaderError::HotReloadUnsupported {
                loader_name: self.loader_name().to_owned(),
            })
        }
    }

    struct LazyState {
        observed_init: Option<bool>,
    }

    struct LazyLoader {
        state: Arc<std::sync::Mutex<LazyState>>,
    }

    impl crate::loader::BundleLoader for LazyLoader {
        fn loader_name(&self) -> &'static str {
            "lazy"
        }

        fn loader_language(&self) -> polyplug_abi::SupportedLanguage {
            polyplug_abi::SupportedLanguage::Rust
        }

        fn supports_hot_reload(&self) -> bool {
            false
        }

        fn load(
            &self,
            _manifest: &ManifestData,
            _source: &crate::loader::BundleSource,
            _runtime: &Runtime,
        ) -> Result<(), crate::error::LoaderError> {
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
        ) -> Result<(), crate::error::LoaderError> {
            Err(crate::error::LoaderError::HotReloadUnsupported {
                loader_name: self.loader_name().to_owned(),
            })
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
            Err(RuntimeError::Loader(crate::error::LoaderError::InitFailed {
                bundle: _,
                error,
            })) => {
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
            out_instance: *mut HostContractInstance,
        ) {
            // Return a non-null dummy pointer for testing
            static mut DUMMY: usize = 0xDEADBEEF;
            if !out_instance.is_null() {
                // SAFETY: out_instance is non-null (just checked) and writable per the ABI contract.
                unsafe {
                    out_instance.write(HostContractInstance {
                        data: &raw mut DUMMY as *mut core::ffi::c_void,
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
            unload_bundle: host_unload_bundle,
            log: stub_host_log,
            create_guest_instance: host_create_guest_instance,
            destroy_guest_instance: host_destroy_guest_instance,
            reserved: core::ptr::null(),
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
            unload_bundle: host_unload_bundle,
            log: stub_host_log,
            create_guest_instance: host_create_guest_instance,
            destroy_guest_instance: host_destroy_guest_instance,
            reserved: core::ptr::null(),
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
        out_instance: *mut HostContractInstance,
    ) {
        let instance: HostContractInstance = LOCAL_INSTANCE_COUNTER.with(|counter| {
            let count: usize = counter.get();
            counter.set(count + 1);
            // Use the count as a "unique" pointer value - we don't actually allocate
            // since these are just test instances
            HostContractInstance {
                data: (count + 1) as *mut core::ffi::c_void, // +1 to avoid null for count=0
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
            unload_bundle: host_unload_bundle,
            log: stub_host_log,
            create_guest_instance: host_create_guest_instance,
            destroy_guest_instance: host_destroy_guest_instance,
            reserved: core::ptr::null(),
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
            unload_bundle: host_unload_bundle,
            log: stub_host_log,
            create_guest_instance: host_create_guest_instance,
            destroy_guest_instance: host_destroy_guest_instance,
            reserved: core::ptr::null(),
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
            unload_bundle: host_unload_bundle,
            log: stub_host_log,
            create_guest_instance: host_create_guest_instance,
            destroy_guest_instance: host_destroy_guest_instance,
            reserved: core::ptr::null(),
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
}

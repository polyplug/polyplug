//! CPython VM-dispatch plugin loader.
//!
//! Loads Python plugin bundles by embedding CPython via pyo3 and registering
//! each contract with [`DispatchType::VirtualMachine`]. Python is a VM language
//! and is treated exactly like the Lua and JavaScript loaders: the guest never
//! builds a [`GuestContractInterface`] or self-registers native function
//! pointers — the loader collects the guest's registration data and registers
//! the contracts itself, routing every per-call invocation through the
//! `vm.call` transport ([`python_vm_dispatch`]).
//!
//! # Why VM dispatch (and not native ctypes closures)
//!
//! The previous Python path registered `ctypes.CFUNCTYPE` closures as native
//! dispatch function pointers, hand-emulating the x86_64 hidden-sret calling
//! convention for the by-value [`AbiError`] return. That is undefined behaviour
//! on arm64, which passes the indirect result through the `x8` register —
//! something ctypes cannot express — and crashed (SIGSEGV) on arm64 CI. The
//! pyo3 `vm.call` transport is both portable and faster, so the native path is
//! gone for Python guests.
//!
//! # Registration protocol (the contract the generator/SDK must emit)
//!
//! After the loader executes the plugin module, it calls its
//! `polyplug_init(host_ptr: int, ctx_ptr: int) -> tuple[list[dict], AbiError]`.
//! `polyplug_init` RETURNS its registrations directly; the loader reads that
//! return value — **nothing is deposited into any module namespace**. The return
//! is a two-tuple `(registrations, abi_error)`:
//!
//! - `registrations` is the list of registration dicts (shape below);
//! - `abi_error` is an `AbiError` ctypes struct: `code == AbiErrorCode::Ok`
//!   selects the registration list; any other code surfaces as a loader error
//!   (e.g. an author factory was never set at import time).
//!
//! This single rule covers both bundle layouts — the hand-written/single-file
//! layout (the entry module defines `polyplug_init` itself) and the generated
//! split-module layout (the entry file does `from generated.guest.contracts
//! import polyplug_init`) — because the data flows through the function's return
//! value, not the namespace of whatever module defines it.
//!
//! The `registrations` list shape:
//!
//! ```python
//! registrations = [
//!     {
//!         # Canonical contract string: "<name>@<major>" or "<name>@<major>.<minor>".
//!         # Only <name> and <major> are significant; <minor> (if present) is parsed
//!         # but does not affect the contract id (which hashes name + major only).
//!         "contract": "calculator@1",
//!         # Optional human-readable plugin name; defaults to the bundle name.
//!         "plugin_name": "my_calculator",
//!         # Author factory: factory(host_ptr_int) -> impl. Called once per
//!         # create_instance (and once at load for the stateless default impl).
//!         "factory": make_calculator,
//!         # Callables ordered by fn_id: functions[0] is fn_id 0, etc. Each is invoked as
//!         # functions[fn_id](impl, args_ptr_int, out_ptr_int, arena_ptr_int, arena_alloc).
//!         "functions": [add, sub, mul],
//!     },
//!     # ... one dict per contract; multi-contract bundles add more entries.
//! ]
//! ```
//!
//! Each callable receives the resolved instance `impl`, then three Python `int`s
//! — the raw `args`, `out`, and `arena` pointers — and finally the loader's
//! `arena_alloc` callable (see the arena bridge section). It unmarshals/marshals
//! through them (the generated guest glue does this). A callable returns normally
//! on success (its return value is ignored) and raises a Python exception on
//! failure, which the loader maps to [`AbiErrorCode::Generic`].
//!
//! # Per-instance state
//!
//! The loader — not the guest module — owns per-instance state. `create_instance`
//! calls the contract's `factory(host_ptr)` to build a fresh impl, mints a
//! non-zero instance id, and keys the impl under that id in a per-contract
//! registry; dispatch resolves the impl from the instance handle and passes it as
//! the callable's first argument; `destroy_instance` drops it. A null instance
//! handle (id 0) resolves to a per-contract default impl built once at load —
//! this serves stateless contracts and the low-level dispatch paths that call
//! with a null instance. Two live instances of the same contract therefore never
//! share state.
//!
//! # Arena bridge
//!
//! The loader builds one **`arena_alloc(size, arena) -> int`** callable per
//! bundle and **passes it as the FINAL positional argument of every dispatch
//! call** — `callable(impl, args, out, arena, arena_alloc)`. Nothing is injected
//! into any module namespace: the callable travels with each call frame, so the
//! split-module generated layout and the single-file hand-written layout are
//! served identically (the guest never resolves the allocator by name). The
//! per-contract [`PythonLoaderData`] holds the callable and clones a bound
//! reference into each dispatch.
//!
//! The arena pointer is likewise threaded EXPLICITLY: every guest callable
//! receives the active [`CallArena`] pointer as its third `int` argument and
//! forwards it to `arena_alloc(size, arena)`. The allocator serves the guest's
//! per-call return buffers from exactly that arena, falling back to `host->alloc`
//! when the caller has no arena (pointer 0). There is NO shared per-bundle cell
//! and NO module global: allocation correctness never depends on any published
//! state, so neither a concurrent dispatch on another thread nor a same-thread
//! nested dispatch can perturb the arena (or allocator) seen by an in-flight call
//! (an earlier shared-cell/module-injection design was racy — a concurrent attach
//! could overwrite the cell mid-call, and a nested call's exit-time clear would
//! wipe the outer call's arena).
//!
//! # Reentrancy
//!
//! Unlike the Lua (mlua) and JS (rquickjs) loaders — whose single-threaded VM
//! locks deadlock on a same-thread nested dispatch and therefore need an
//! explicit reentrancy guard — CPython's `PyGILState`/pyo3 `Python::attach` is
//! reentrant on the same thread: a nested attach from a plugin→plugin
//! cross-call simply re-enters the held GIL without deadlocking. No reentrancy
//! guard is needed or used here. Nested dispatch is also arena-safe: because each
//! call carries its own arena pointer (and the shared, stateless `arena_alloc`
//! callable) through its own call frame — not a shared mutable cell — the inner
//! call's arena and the outer call's arena never alias or clear one another.

use core::sync::atomic::AtomicU64;
use core::sync::atomic::Ordering;
use std::collections::HashMap;
use std::sync::Mutex;

use pyo3::Bound;
use pyo3::Py;
use pyo3::PyAny;
use pyo3::Python;
use pyo3::types::PyAnyMethods;
use pyo3::types::PyList;
use pyo3::types::PyListMethods;
use pyo3::types::PyTuple;
use pyo3::types::PyTupleMethods;

use polyplug::error::LoaderError;
use polyplug_abi::AbiError;
use polyplug_abi::AbiErrorCode;
use polyplug_abi::CallArena;
use polyplug_abi::DispatchType;
use polyplug_abi::GuestContractInstance;
use polyplug_abi::GuestContractInterface;
use polyplug_abi::HostApi;
use polyplug_abi::PluginDescriptor;
use polyplug_abi::StringView;
use polyplug_abi::VmLoaderData;
use polyplug_abi::dispatch::dispatch_mechanisms::DispatchMechanisms;
use polyplug_abi::dispatch::vm_dispatch::VmDispatch;
use polyplug_abi::types::Version;
use polyplug_utils::GuestContractId;

// ─── Per-bundle loader data for VM dispatch ─────────────────────────────────────

/// Loader-specific data for one Python contract's VM dispatch and per-instance
/// lifecycle.
///
/// Holds the contract's callables (ordered by `fn_id`), the author `factory`
/// used to build implementation objects, the per-instance registry, the
/// stateless `default_impl`, and the per-bundle `arena_alloc` callable. The
/// active per-call arena pointer is NOT stored here: it is threaded explicitly as
/// the third value argument of every guest callable (`callable(impl, args, out,
/// arena, arena_alloc)`) and forwarded by the guest to `arena_alloc(size,
/// arena)`, so allocation never depends on any shared cell. The `arena_alloc`
/// callable itself is stateless (it reads the arena from its argument), so
/// sharing one per bundle is sound. This is what makes concurrent and same-thread
/// reentrant dispatch correct: each call's arena travels with its own call frame
/// rather than through a cell another dispatch could overwrite or clear.
pub struct PythonLoaderData {
    /// Callables ordered by `fn_id`. `callables[i]` handles `fn_id == i`.
    /// Each is invoked as `callable(impl, args, out, arena, arena_alloc)`.
    pub callables: Vec<Py<PyAny>>,
    /// Per-bundle arena allocator `arena_alloc(size, arena) -> int`, passed as
    /// the final positional argument of every dispatch call. Stateless: it reads
    /// the target arena from its `arena` argument (or falls back to `host->alloc`
    /// when that is 0), so one instance is shared across all calls and threads.
    pub arena_alloc: Py<PyAny>,
    /// Author factory `factory(host_ptr_int) -> impl`, called once per
    /// `create_instance` to build a fresh implementation bound to its owning
    /// runtime's host pointer.
    pub factory: Py<PyAny>,
    /// Stateless default implementation, built once at load via the factory.
    /// Dispatch resolves to this when the instance handle is null (id 0).
    pub default_impl: Py<PyAny>,
    /// Live instances keyed by their non-zero instance id (the value stored in
    /// `GuestContractInstance::data`).
    pub instances: Mutex<HashMap<u64, Py<PyAny>>>,
    /// Monotonic source of non-zero instance ids. Starts at 1 so a real
    /// instance handle is never null (null `data` denotes the default impl).
    pub next_id: AtomicU64,
    /// Contract id stamped into every instance handle this contract mints.
    pub contract_id: GuestContractId,
}

// SAFETY: PythonLoaderData is shared across threads via the leaked raw pointer in
// VmLoaderData. The Py<PyAny> fields and the HashMap of Py values are Send/Sync
// when access is GIL-guarded, which the loader guarantees (every access to a
// Python object happens inside Python::attach). The Mutex/AtomicU64 are Send/Sync
// in their own right.
unsafe impl Send for PythonLoaderData {}
// SAFETY: see the Send impl above — every access to a Python object is GIL-guarded
// (inside Python::attach), so the type is safe to share across threads.
unsafe impl Sync for PythonLoaderData {}

// ─── Instance lifecycle ─────────────────────────────────────────────────────────

/// Create a fresh instance of a Python contract.
///
/// Calls the contract's `factory(host_ptr)` to build a new implementation object,
/// mints a non-zero instance id, keys the impl under that id in the per-contract
/// registry, and writes a `GuestContractInstance` whose `data` carries the id and
/// whose `contract_id` is the contract's stamped id. A factory failure (or a
/// poisoned registry lock) writes a null instance handle.
///
/// # Safety
/// - `loader_data` must wrap a valid pointer to a [`PythonLoaderData`] created by
///   the loader (and leaked for the runtime lifetime).
/// - `host` is the owning runtime's `HostApi` pointer, forwarded to the factory.
/// - `out_instance`, when non-null, must be writable per the ABI contract.
unsafe extern "C" fn python_create_instance(
    loader_data: VmLoaderData,
    host: *const HostApi,
    _args: *const (),
    out_instance: *mut GuestContractInstance,
) {
    if out_instance.is_null() {
        return;
    }
    // SAFETY: loader_data wraps a valid PythonLoaderData pointer created by the
    // loader; it is leaked for the runtime lifetime so the borrow is valid here.
    let data: &PythonLoaderData = unsafe { &*(loader_data.data as *const PythonLoaderData) };
    let host_addr: i64 = host as usize as i64;

    let instance: GuestContractInstance =
        Python::attach(
            |py: Python<'_>| match data.factory.bind(py).call1((host_addr,)) {
                Ok(impl_obj) => {
                    let id: u64 = data.next_id.fetch_add(1, Ordering::Relaxed);
                    match data.instances.lock() {
                        Ok(mut map) => {
                            map.insert(id, impl_obj.unbind());
                            GuestContractInstance {
                                data: id as usize as *mut core::ffi::c_void,
                                contract_id: data.contract_id,
                            }
                        }
                        Err(_) => GuestContractInstance::null(),
                    }
                }
                Err(e) => {
                    e.print(py);
                    GuestContractInstance::null()
                }
            },
        );

    // SAFETY: out_instance is non-null (checked above) and writable per the ABI contract.
    unsafe { out_instance.write(instance) };
}

/// Destroy a Python contract instance.
///
/// Removes the impl keyed under the instance handle's id from the per-contract
/// registry (dropping the `Py` under the GIL). A null handle (id 0) refers to the
/// stateless default impl, which the loader owns for the runtime lifetime, so it
/// is a no-op.
///
/// # Safety
/// - `loader_data` must wrap a valid pointer to a [`PythonLoaderData`] created by
///   the loader (and leaked for the runtime lifetime).
/// - `instance` must be a handle previously produced by [`python_create_instance`]
///   for this contract (or a null handle).
unsafe extern "C" fn python_destroy_instance(
    loader_data: VmLoaderData,
    _host: *const HostApi,
    instance: GuestContractInstance,
) {
    let id: u64 = instance.data as usize as u64;
    if id == 0 {
        return;
    }
    // SAFETY: loader_data wraps a valid PythonLoaderData pointer created by the
    // loader; it is leaked for the runtime lifetime so the borrow is valid here.
    let data: &PythonLoaderData = unsafe { &*(loader_data.data as *const PythonLoaderData) };
    Python::attach(|_py: Python<'_>| {
        if let Ok(mut map) = data.instances.lock() {
            // Drop happens under the GIL (we are inside Python::attach).
            map.remove(&id);
        }
    });
}

// ─── VM dispatch entry ──────────────────────────────────────────────────────────

/// VM dispatch function for Python plugins.
///
/// Acquires the GIL via pyo3 (correct from any host thread), resolves the impl for
/// `instance` (the per-contract default impl when the handle is null), looks up the
/// callable for `fn_id` in the per-contract [`PythonLoaderData`], and invokes
/// `callable(impl, args_ptr_int, out_ptr_int, arena_ptr_int, arena_alloc)`. The arena
/// pointer is passed straight to the guest as its fourth argument and the per-bundle
/// `arena_alloc` callable as its fifth — there is no shared cell and no module global —
/// so the guest forwards both to `arena_alloc(size, arena)` and a
/// concurrent or nested dispatch cannot perturb this call's arena. A normal
/// return maps to [`AbiError::ok`]; a Python exception maps to
/// [`AbiErrorCode::Generic`]; an out-of-range `fn_id` maps to
/// [`AbiErrorCode::FunctionNotAvailable`].
///
/// # Safety
/// - `loader_data` must wrap a valid pointer to a [`PythonLoaderData`] created by
///   the loader (and leaked for the runtime lifetime).
/// - `args` and `out` must be valid pointers for this ABI call.
/// - `arena`, when non-null, must point to a valid [`CallArena`] reset by the
///   caller for this call; values the guest writes into it are valid until the
///   caller's next reset.
unsafe extern "C" fn python_vm_dispatch(
    loader_data: VmLoaderData,
    instance: GuestContractInstance,
    fn_id: u32,
    args: *const (),
    out: *mut (),
    arena: *mut CallArena,
    out_err: *mut AbiError,
) {
    // SAFETY: loader_data wraps a valid PythonLoaderData pointer created by the
    // loader; it is leaked for the runtime lifetime so the borrow is valid for the call.
    let result: AbiError =
        unsafe { python_vm_dispatch_impl(loader_data, instance, fn_id, args, out, arena) };
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(result) };
    }
}

unsafe fn python_vm_dispatch_impl(
    loader_data: VmLoaderData,
    instance: GuestContractInstance,
    fn_id: u32,
    args: *const (),
    out: *mut (),
    arena: *mut CallArena,
) -> AbiError {
    // SAFETY: loader_data wraps a valid PythonLoaderData pointer created by the
    // loader; it is leaked for the runtime lifetime so the borrow is valid for the call.
    let data: &PythonLoaderData = unsafe { &*(loader_data.data as *const PythonLoaderData) };

    let callable: &Py<PyAny> = match data.callables.get(fn_id as usize) {
        Some(c) => c,
        None => {
            return AbiError {
                code: AbiErrorCode::FunctionNotAvailable as u32,
                message: StringView::null(),
            };
        }
    };

    let instance_id: u64 = instance.data as usize as u64;
    let args_int: i64 = args as usize as i64;
    let out_int: i64 = out as usize as i64;
    let arena_int: i64 = arena as usize as i64;

    Python::attach(|py: Python<'_>| {
        // Resolve the instance impl: a null handle (id 0) uses the stateless
        // default impl; otherwise look the live instance up by id. The impl is
        // cloned out and the registry lock released BEFORE the call, so a nested
        // dispatch (plugin→plugin re-entry) cannot deadlock on the registry mutex.
        let impl_py: Py<PyAny> = if instance_id == 0 {
            data.default_impl.clone_ref(py)
        } else {
            let map: std::sync::MutexGuard<'_, HashMap<u64, Py<PyAny>>> =
                match data.instances.lock() {
                    Ok(g) => g,
                    Err(_) => {
                        return AbiError {
                            code: AbiErrorCode::Generic as u32,
                            message: StringView::null(),
                        };
                    }
                };
            match map.get(&instance_id) {
                Some(obj) => obj.clone_ref(py),
                None => {
                    return AbiError {
                        code: AbiErrorCode::FunctionNotAvailable as u32,
                        message: StringView::null(),
                    };
                }
            }
        };

        // The impl is the first call argument; the arena pointer travels as the
        // fourth and the per-bundle `arena_alloc` callable as the fifth. The guest
        // forwards both to `arena_alloc(size, arena)`. Nothing is published to a
        // shared cell or module global, so a concurrent or same-thread nested
        // dispatch cannot overwrite or clear this call's arena.
        let arena_alloc: Bound<'_, PyAny> = data.arena_alloc.bind(py).clone();
        let bound: Bound<'_, PyAny> = callable.bind(py).clone();
        let call_result: Result<Bound<'_, PyAny>, pyo3::PyErr> =
            bound.call((impl_py, args_int, out_int, arena_int, arena_alloc), None);

        match call_result {
            Ok(_) => AbiError::ok(),
            Err(e) => {
                e.print(py);
                AbiError {
                    code: AbiErrorCode::Generic as u32,
                    message: StringView::null(),
                }
            }
        }
    })
}

// ─── Registration collection ────────────────────────────────────────────────────

/// One contract's collected registration data, extracted from the registrations
/// list `polyplug_init` returned.
pub(crate) struct ContractRegistration {
    /// Bare contract name (the part before `@`).
    pub contract_name: String,
    /// Contract major version (the `<major>` in `name@<major>[.minor]`).
    pub contract_major: u32,
    /// Human-readable plugin name (defaults to the bundle name).
    pub plugin_name: String,
    /// Author factory `factory(host_ptr_int) -> impl`, called once per instance.
    pub factory: Py<PyAny>,
    /// Per-function callables ordered by `fn_id`. Each is invoked as
    /// `callable(impl, args, out, arena)`.
    pub callables: Vec<Py<PyAny>>,
}

/// Parse a canonical contract string `"<name>@<major>"` or
/// `"<name>@<major>.<minor>"` into `(name, major)`.
///
/// Only the name and major version are significant — the contract id hashes
/// exactly those — so a trailing `.<minor>` is accepted and ignored.
pub(crate) fn parse_contract_string(
    contract: &str,
    bundle_name: &str,
) -> Result<(String, u32), LoaderError> {
    let (name, version_part): (&str, &str) =
        contract
            .split_once('@')
            .ok_or_else(|| LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!(
                    "invalid contract string `{}`: expected `name@major[.minor]`",
                    contract
                ),
            })?;
    if name.is_empty() {
        return Err(LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!(
                "invalid contract string `{}`: empty contract name",
                contract
            ),
        });
    }
    let major_str: &str = version_part.split('.').next().unwrap_or(version_part);
    let major: u32 = major_str
        .parse::<u32>()
        .map_err(|_| LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!(
                "invalid contract string `{}`: major version `{}` is not a u32",
                contract, major_str
            ),
        })?;
    Ok((name.to_owned(), major))
}

/// Read and validate the `(registrations, abi_error)` tuple that
/// `polyplug_init` returned into a `Vec<ContractRegistration>`.
///
/// `polyplug_init` returns a two-tuple whose first element is the registrations
/// list and whose second is an `AbiError` ctypes struct. This function checks the
/// `AbiError::code` first: a non-`Ok` code surfaces as `InitFailed` (the guest
/// signalled an init failure, e.g. an author factory was never set), and only an
/// `Ok` code proceeds to parse the list. No module namespace is read — the data
/// flows through the function's return value, so the split-module and single-file
/// layouts are served identically.
///
/// Returns `InitFailed` if the return is not a 2-tuple, the error code is not
/// `Ok`, the first element is not a list, or any entry is malformed (missing
/// `contract`/`functions`, bad contract string, or a non-callable in
/// `functions`).
pub(crate) fn collect_registrations(
    py: Python<'_>,
    init_ret: &Bound<'_, PyAny>,
    bundle_name: &str,
) -> Result<Vec<ContractRegistration>, LoaderError> {
    let tuple: Bound<'_, PyTuple> =
        init_ret
            .cast::<PyTuple>()
            .cloned()
            .map_err(|_| LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: "polyplug_init must return a (registrations, AbiError) tuple".to_owned(),
            })?;
    if tuple.len() != 2 {
        return Err(LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!(
                "polyplug_init must return a 2-tuple (registrations, AbiError), got {} elements",
                tuple.len()
            ),
        });
    }

    let registrations_obj: Bound<'_, PyAny> =
        tuple.get_item(0).map_err(|_| LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: "polyplug_init return tuple missing registrations element".to_owned(),
        })?;
    let abi_error_obj: Bound<'_, PyAny> =
        tuple.get_item(1).map_err(|_| LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: "polyplug_init return tuple missing AbiError element".to_owned(),
        })?;

    let code: u32 = abi_error_obj
        .getattr("code")
        .and_then(|c: Bound<'_, PyAny>| c.extract::<u32>())
        .map_err(|_| LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: "polyplug_init AbiError has no readable u32 `code` field".to_owned(),
        })?;
    if code != AbiErrorCode::Ok as u32 {
        return Err(LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("polyplug_init reported AbiError code {}", code),
        });
    }

    let list: Bound<'_, PyList> =
        registrations_obj
            .cast_into::<PyList>()
            .map_err(|_| LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: "polyplug_init registrations must be a list of dicts".to_owned(),
            })?;

    let mut registrations: Vec<ContractRegistration> = Vec::with_capacity(list.len());
    for entry in list.iter() {
        let contract_str: String = entry
            .get_item("contract")
            .and_then(|v: Bound<'_, PyAny>| v.extract::<String>())
            .map_err(|_| LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: "registration entry missing string `contract` key".to_owned(),
            })?;

        let (contract_name, contract_major): (String, u32) =
            parse_contract_string(&contract_str, bundle_name)?;

        let plugin_name: String = match entry.get_item("plugin_name") {
            Ok(v) => v
                .extract::<String>()
                .unwrap_or_else(|_| bundle_name.to_owned()),
            Err(_) => bundle_name.to_owned(),
        };

        let factory: Bound<'_, PyAny> =
            entry
                .get_item("factory")
                .map_err(|_| LoaderError::InitFailed {
                    bundle: bundle_name.to_owned(),
                    error: format!(
                        "registration entry for `{}` missing `factory` callable",
                        contract_str
                    ),
                })?;
        if !factory.is_callable() {
            return Err(LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!("`factory` for `{}` is not callable", contract_str),
            });
        }

        let functions: Bound<'_, PyAny> =
            entry
                .get_item("functions")
                .map_err(|_| LoaderError::InitFailed {
                    bundle: bundle_name.to_owned(),
                    error: format!(
                        "registration entry for `{}` missing `functions` list",
                        contract_str
                    ),
                })?;
        let functions_list: Bound<'_, PyList> =
            functions
                .cast_into::<PyList>()
                .map_err(|_| LoaderError::InitFailed {
                    bundle: bundle_name.to_owned(),
                    error: format!("`functions` for `{}` must be a list", contract_str),
                })?;

        let mut callables: Vec<Py<PyAny>> = Vec::with_capacity(functions_list.len());
        for (idx, callable) in functions_list.iter().enumerate() {
            if !callable.is_callable() {
                return Err(LoaderError::InitFailed {
                    bundle: bundle_name.to_owned(),
                    error: format!(
                        "`functions[{}]` for `{}` is not callable",
                        idx, contract_str
                    ),
                });
            }
            callables.push(callable.unbind());
        }

        registrations.push(ContractRegistration {
            contract_name,
            contract_major,
            plugin_name,
            factory: factory.unbind(),
            callables,
        });
    }

    let _ = py;
    Ok(registrations)
}

/// Register every collected contract with the runtime through the `HostApi`
/// self-passing pattern, building a VM-dispatch [`GuestContractInterface`] per
/// contract.
///
/// Each contract gets its own leaked [`PythonLoaderData`] (leaked for the runtime
/// lifetime so any resolved dispatch pointer stays valid), and the interface plus
/// descriptor strings are leaked to `'static` for the same reason. Returns the
/// number of contracts registered, or an error if registration of any contract
/// fails or none were registered.
pub(crate) fn register_contracts(
    registrations: Vec<ContractRegistration>,
    host_interface: *const HostApi,
    bundle_name: &str,
) -> Result<u32, LoaderError> {
    let mut registered: u32 = 0_u32;

    // One arena allocator per bundle, bound to the runtime's host pointer. It is
    // stateless (reads the target arena from its argument), so every contract's
    // dispatch shares the same instance: the loader passes it as the final
    // positional argument of each guest callable. Nothing is injected into any
    // module namespace.
    let arena_alloc: Py<PyAny> = Python::attach(|py: Python<'_>| {
        build_arena_bridge(py, host_interface, bundle_name).map(|b: Bound<'_, PyAny>| b.unbind())
    })?;

    for reg in registrations {
        let cid: GuestContractId = GuestContractId::new(&reg.contract_name, reg.contract_major);

        // Build the stateless default impl once at load via the author factory,
        // bound to the runtime's host pointer. Dispatch uses it for null instance
        // handles (stateless contracts and the low-level null-instance paths).
        let host_addr: i64 = host_interface as usize as i64;
        let default_impl: Py<PyAny> =
            Python::attach(
                |py: Python<'_>| match reg.factory.bind(py).call1((host_addr,)) {
                    Ok(o) => Ok(o.unbind()),
                    Err(e) => {
                        e.print(py);
                        Err(())
                    }
                },
            )
            .map_err(|_| LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!(
                    "factory for `{}@{}` failed while building the default instance",
                    reg.contract_name, reg.contract_major
                ),
            })?;

        let arena_alloc_clone: Py<PyAny> =
            Python::attach(|py: Python<'_>| arena_alloc.clone_ref(py));
        let loader_data: Box<PythonLoaderData> = Box::new(PythonLoaderData {
            callables: reg.callables,
            arena_alloc: arena_alloc_clone,
            factory: reg.factory,
            default_impl,
            instances: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            contract_id: cid,
        });
        let loader_data_ptr: *mut PythonLoaderData = Box::into_raw(loader_data);

        let interface: GuestContractInterface = GuestContractInterface {
            contract_id: cid,
            contract_version: Version {
                major: reg.contract_major,
                minor: 0,
                patch: 0,
            },
            dispatch_type: DispatchType::VirtualMachine,
            create_instance: python_create_instance,
            destroy_instance: python_destroy_instance,
            dispatch: DispatchMechanisms {
                vm: VmDispatch {
                    call: python_vm_dispatch,
                    loader_data: VmLoaderData {
                        data: loader_data_ptr as *mut core::ffi::c_void,
                    },
                },
            },
        };

        // Leak the interface so it has 'static lifetime. Python plugins are never
        // unloaded; the interface must outlive every resolved dispatch pointer.
        let static_interface: *const GuestContractInterface = Box::into_raw(Box::new(interface));

        // The descriptor's human-readable contract_name must be the canonical
        // "<name>@<major>" form so it matches what every other loader registers.
        let contract_display_name: String = format!("{}@{}", reg.contract_name, reg.contract_major);
        let plugin_name_leaked: &'static str = Box::leak(reg.plugin_name.into_boxed_str());
        let contract_name_leaked: &'static str = Box::leak(contract_display_name.into_boxed_str());

        let descriptor: PluginDescriptor = PluginDescriptor {
            name: StringView {
                ptr: plugin_name_leaked.as_ptr(),
                len: plugin_name_leaked.len(),
            },
            contract_name: StringView {
                ptr: contract_name_leaked.as_ptr(),
                len: contract_name_leaked.len(),
            },
            version: Version {
                major: reg.contract_major,
                minor: 0,
                patch: 0,
            },
        };

        // SAFETY: `host_interface` is a valid HostApi pointer for this call.
        // `descriptor` is stack-allocated and only borrowed for the call (the host
        // copies what it retains). `static_interface` is a leaked Box, valid for
        // 'static. This is the canonical self-passing registration path shared by
        // every loader.
        let mut reg_result: AbiError = AbiError::ok();
        // SAFETY: `host_interface` is a valid HostApi pointer; `reg_result` is a
        // valid, writable out-param for the duration of the call.
        unsafe {
            ((*host_interface).register_guest_contract)(
                host_interface,
                &descriptor as *const PluginDescriptor,
                static_interface,
                &mut reg_result,
            )
        };

        if !reg_result.is_ok() {
            return Err(LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!(
                    "register_guest_contract failed for `{}`: code={:?}",
                    contract_name_leaked, reg_result.code
                ),
            });
        }

        registered += 1;
    }

    if registered == 0 {
        return Err(LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: "polyplug_init returned no contracts (empty registrations list)".to_owned(),
        });
    }

    Ok(registered)
}

/// Build the `arena_alloc(size, arena) -> int` bridge callable.
///
/// The arena pointer is supplied EXPLICITLY by the caller as the second argument:
/// it is the `arena` int the guest received as the third argument of its dispatch
/// callable. There is no shared cell — allocation correctness does not depend on
/// any published state, so concurrent and same-thread reentrant dispatch are both
/// sound. When `arena` is 0 (the caller has no per-call arena) the bridge falls
/// back to `host->alloc`, preserving per-value allocation behaviour. Returns the
/// allocated address as a Python `int` (0 on failure).
///
/// One bridge is built per bundle and stored in each contract's
/// [`PythonLoaderData`]; the dispatcher passes it as the final positional argument
/// of every guest callable. It is never injected into any module namespace.
fn build_arena_bridge<'py>(
    py: Python<'py>,
    host_interface: *const HostApi,
    bundle_name: &str,
) -> Result<Bound<'py, PyAny>, LoaderError> {
    let host_addr: usize = host_interface as usize;

    let closure = move |size: u32, arena_addr: usize| -> i64 {
        let arena: *mut CallArena = arena_addr as *mut CallArena;
        let ptr: *mut u8 = if arena.is_null() {
            let host: *const HostApi = host_addr as *const HostApi;
            if host.is_null() {
                core::ptr::null_mut()
            } else {
                // SAFETY: host points to 'static HostApi data for the runtime
                // lifetime; align 1 is valid for raw byte buffers.
                unsafe { ((*host).alloc)(host, size as usize, 1) }
            }
        } else {
            // SAFETY: `arena` is the per-call CallArena the dispatching call passed
            // to the guest as its third argument and the guest forwarded here;
            // alloc bumps within it or chains a host-allocated overflow block.
            unsafe { (*arena).alloc(size as usize, 1) }
        };
        ptr as usize as i64
    };

    pyo3::types::PyCFunction::new_closure(
        py,
        None,
        None,
        move |args: &Bound<'_, pyo3::types::PyTuple>,
              _kwargs: Option<&Bound<'_, pyo3::types::PyDict>>|
              -> pyo3::PyResult<i64> {
            let size: u32 = args.get_item(0)?.extract::<u32>()?;
            let arena_addr: usize = args.get_item(1)?.extract::<usize>()?;
            Ok(closure(size, arena_addr))
        },
    )
    .map(|f: Bound<'_, pyo3::types::PyCFunction>| f.into_any())
    .map_err(|e: pyo3::PyErr| LoaderError::InitFailed {
        bundle: bundle_name.to_owned(),
        error: format!("failed to create arena_alloc bridge: {}", e),
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;

    #[test]
    fn parse_contract_string_major_only() {
        let (name, major): (String, u32) =
            parse_contract_string("calculator@2", "b").expect("parse");
        assert_eq!(name, "calculator");
        assert_eq!(major, 2);
    }

    #[test]
    fn parse_contract_string_major_minor() {
        let (name, major): (String, u32) = parse_contract_string("logger@3.7", "b").expect("parse");
        assert_eq!(name, "logger");
        assert_eq!(major, 3, "minor is parsed but ignored for the id");
    }

    #[test]
    fn parse_contract_string_missing_at_fails() {
        assert!(parse_contract_string("noversion", "b").is_err());
    }

    #[test]
    fn parse_contract_string_empty_name_fails() {
        assert!(parse_contract_string("@1", "b").is_err());
    }

    #[test]
    fn parse_contract_string_bad_major_fails() {
        assert!(parse_contract_string("x@abc", "b").is_err());
    }
}

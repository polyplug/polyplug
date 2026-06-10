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
//! After the loader executes the plugin module and calls its
//! `polyplug_init(host_ptr: int, ctx_ptr: int) -> None`, the loader reads a
//! module-level attribute named **`_polyplug_registrations`** from the namespace
//! of the module that **defines** `polyplug_init` (its `__globals__`), not from
//! the entry module that was loaded. This is the single rule that covers both
//! bundle layouts with one semantic:
//!
//! - the hand-written/single-file layout, where the entry module defines
//!   `polyplug_init` itself (so `__globals__` *is* the entry module's namespace),
//!   and
//! - the generated layout, where the entry file does
//!   `from generated.guest.contracts import polyplug_init` and the registrations
//!   are deposited into the *contracts* module that defines `polyplug_init`.
//!
//! If `polyplug_init` is not a plain Python function (it has no `__globals__`,
//! e.g. a C callable), the loader falls back to reading `_polyplug_registrations`
//! from the entry module's namespace.
//!
//! Its shape:
//!
//! ```python
//! _polyplug_registrations = [
//!     {
//!         # Canonical contract string: "<name>@<major>" or "<name>@<major>.<minor>".
//!         # Only <name> and <major> are significant; <minor> (if present) is parsed
//!         # but does not affect the contract id (which hashes name + major only).
//!         "contract": "calculator@1",
//!         # Optional human-readable plugin name; defaults to the bundle name.
//!         "plugin_name": "my_calculator",
//!         # Callables ordered by fn_id: functions[0] is fn_id 0, etc.
//!         # Each is invoked as functions[fn_id](args_ptr_int, out_ptr_int, arena_ptr_int).
//!         "functions": [add, sub, mul],
//!     },
//!     # ... one dict per contract; multi-contract bundles add more entries.
//! ]
//! ```
//!
//! Each callable receives three Python `int`s — the raw `args`, `out`, and
//! `arena` pointers — and unmarshals/marshals through them (the generated guest
//! glue does this). A callable returns normally on success (its return value is
//! ignored) and raises a Python exception on failure, which the loader maps to
//! [`AbiErrorCode::Generic`].
//!
//! # Arena bridge
//!
//! The loader injects a module-level callable **`_polyplug_arena_alloc(size,
//! arena) -> int`** before `polyplug_init` runs — into the namespace of the
//! module that *defines* `polyplug_init` (its `__globals__`) AND the entry
//! module. This is the same single rule that governs registrations collection:
//! the guest's ABI functions that call `_polyplug_arena_alloc` live in the module
//! defining `polyplug_init`, so the bridge must be reachable from that module's
//! globals. In the split-module generated layout the entry file only
//! `from … import polyplug_init`, so its own namespace is not where the ABI
//! functions resolve names — `__globals__` is. The dual injection is
//! belt-and-braces: in the single-file hand-written layout the two targets are
//! the same dict; if `polyplug_init` is not a plain Python function (no
//! `__globals__`, e.g. a C callable), only the entry-module injection applies —
//! the same fallback as registrations collection. Injection must happen after
//! `polyplug_init` is located but before it (and any dispatch) runs.
//!
//! The arena pointer is threaded EXPLICITLY: every guest callable receives the
//! active [`CallArena`] pointer as its third `int` argument and forwards it to
//! `_polyplug_arena_alloc(size, arena)`. The bridge serves the guest's per-call
//! return buffers from exactly that arena, falling back to `host->alloc` when the
//! caller has no arena (pointer 0). There is NO shared per-bundle cell: allocation
//! correctness never depends on any published state, so neither a concurrent
//! dispatch on another thread nor a same-thread nested dispatch can perturb the
//! arena seen by an in-flight call (an earlier shared-cell design was racy — a
//! concurrent attach could overwrite the cell mid-call, and a nested call's
//! exit-time clear would wipe the outer call's arena).
//!
//! # Reentrancy
//!
//! Unlike the Lua (mlua) and JS (rquickjs) loaders — whose single-threaded VM
//! locks deadlock on a same-thread nested dispatch and therefore need an
//! explicit reentrancy guard — CPython's `PyGILState`/pyo3 `Python::attach` is
//! reentrant on the same thread: a nested attach from a plugin→plugin
//! cross-call simply re-enters the held GIL without deadlocking. No reentrancy
//! guard is needed or used here. Nested dispatch is also arena-safe: because each
//! call carries its own arena pointer through its own call frame (not a shared
//! cell), the inner call's arena and the outer call's arena never alias or clear
//! one another.

use pyo3::Bound;
use pyo3::Py;
use pyo3::PyAny;
use pyo3::Python;
use pyo3::types::PyAnyMethods;
use pyo3::types::PyDict;
use pyo3::types::PyDictMethods;
use pyo3::types::PyList;
use pyo3::types::PyListMethods;

use polyplug::error::LoaderError;
use polyplug::error::RuntimeError;
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

/// The module-level attribute the guest populates with its contract
/// registrations. See the module docs for the exact shape.
pub(crate) const REGISTRATIONS_ATTR: &str = "_polyplug_registrations";

/// The arena-allocator bridge injected into the plugin module namespace.
pub(crate) const ARENA_ALLOC_ATTR: &str = "_polyplug_arena_alloc";

// ─── Per-bundle loader data for VM dispatch ─────────────────────────────────────

/// Loader-specific data for one Python contract's VM dispatch.
///
/// Holds the contract's callables (ordered by `fn_id`). The active per-call
/// arena pointer is NOT stored here: it is threaded explicitly as the third
/// argument of every guest callable (`callable(args, out, arena)`) and forwarded
/// by the guest to `_polyplug_arena_alloc(size, arena)`, so allocation never
/// depends on any shared cell. This is what makes concurrent and same-thread
/// reentrant dispatch correct: each call's arena travels with its own call frame
/// rather than through a cell another dispatch could overwrite or clear.
pub struct PythonLoaderData {
    /// Callables ordered by `fn_id`. `callables[i]` handles `fn_id == i`.
    pub callables: Vec<Py<PyAny>>,
}

// SAFETY: PythonLoaderData is shared across threads via the leaked raw pointer in
// VmLoaderData. `callables` (Py<PyAny>) is Send/Sync when access is GIL-guarded,
// which python_vm_dispatch guarantees (every access is inside Python::attach).
unsafe impl Send for PythonLoaderData {}
// SAFETY: see the Send impl above — every access to `callables` is GIL-guarded
// (inside Python::attach), so the type is safe to share across threads.
unsafe impl Sync for PythonLoaderData {}

// ─── Instance lifecycle stubs ──────────────────────────────────────────────────

/// Stub `create_instance` for Python plugins — returns a null instance.
///
/// # Safety
/// Python plugins use VM dispatch; per-contract state lives in the interpreter,
/// so no opaque instance handle is produced.
unsafe extern "C" fn python_create_instance(
    _host: *const HostApi,
    _args: *const (),
) -> GuestContractInstance {
    GuestContractInstance::null()
}

/// Stub `destroy_instance` for Python plugins — no instance state to free.
///
/// # Safety
/// Python plugins do not own instance data; this is a no-op.
unsafe extern "C" fn python_destroy_instance(
    _host: *const HostApi,
    _instance: GuestContractInstance,
) {
}

// ─── VM dispatch entry ──────────────────────────────────────────────────────────

/// VM dispatch function for Python plugins.
///
/// Acquires the GIL via pyo3 (correct from any host thread), looks up the
/// callable for `fn_id` in the per-contract [`PythonLoaderData`], and invokes
/// `callable(args_ptr_int, out_ptr_int, arena_ptr_int)`. The arena pointer is
/// passed straight to the guest as the third argument — there is no shared cell —
/// so the guest forwards it to `_polyplug_arena_alloc(size, arena)` and a
/// concurrent or nested dispatch cannot perturb this call's arena. A normal
/// return maps to [`AbiError::ok`]; a Python exception maps to
/// [`AbiErrorCode::Generic`]; an out-of-range `fn_id` maps to
/// [`AbiErrorCode::FunctionNotAvailable`].
///
/// # Safety
/// - `loader_data` must wrap a valid pointer to a [`PythonLoaderData`] created by
///   the loader (and leaked for the runtime lifetime — retire-not-drop).
/// - `args` and `out` must be valid pointers for this ABI call.
/// - `arena`, when non-null, must point to a valid [`CallArena`] reset by the
///   caller for this call; values the guest writes into it are valid until the
///   caller's next reset.
unsafe extern "C" fn python_vm_dispatch(
    loader_data: VmLoaderData,
    _instance: GuestContractInstance,
    fn_id: u32,
    args: *const (),
    out: *mut (),
    arena: *mut CallArena,
) -> AbiError {
    // SAFETY: loader_data wraps a valid PythonLoaderData pointer created by the
    // loader; it is leaked (retire-not-drop) so the borrow is valid for the call.
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

    let args_int: i64 = args as usize as i64;
    let out_int: i64 = out as usize as i64;
    let arena_int: i64 = arena as usize as i64;

    Python::attach(|py: Python<'_>| {
        // The arena pointer travels as the third call argument; the guest forwards
        // it to `_polyplug_arena_alloc(size, arena)`. No shared cell is published,
        // so a concurrent or same-thread nested dispatch cannot overwrite or clear
        // this call's arena.
        let bound: Bound<'_, PyAny> = callable.bind(py).clone();
        let call_result: Result<Bound<'_, PyAny>, pyo3::PyErr> =
            bound.call((args_int, out_int, arena_int), None);

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

/// One contract's collected registration data, extracted from the guest's
/// `_polyplug_registrations` list.
pub(crate) struct ContractRegistration {
    /// Bare contract name (the part before `@`).
    pub contract_name: String,
    /// Contract major version (the `<major>` in `name@<major>[.minor]`).
    pub contract_major: u32,
    /// Human-readable plugin name (defaults to the bundle name).
    pub plugin_name: String,
    /// Callables ordered by `fn_id`.
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
) -> Result<(String, u32), RuntimeError> {
    let (name, version_part): (&str, &str) = contract.split_once('@').ok_or_else(|| {
        RuntimeError::Loader(LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!(
                "invalid contract string `{}`: expected `name@major[.minor]`",
                contract
            ),
        })
    })?;
    if name.is_empty() {
        return Err(RuntimeError::Loader(LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!(
                "invalid contract string `{}`: empty contract name",
                contract
            ),
        }));
    }
    let major_str: &str = version_part.split('.').next().unwrap_or(version_part);
    let major: u32 = major_str.parse::<u32>().map_err(|_| {
        RuntimeError::Loader(LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!(
                "invalid contract string `{}`: major version `{}` is not a u32",
                contract, major_str
            ),
        })
    })?;
    Ok((name.to_owned(), major))
}

/// Resolve `_polyplug_registrations` from the namespace of the module that
/// *defines* `polyplug_init`.
///
/// The registrations live in the global namespace of the module that defines
/// `polyplug_init`, which is exactly that function's `__globals__` dict. This
/// makes the split-module generated layout (entry file imports `polyplug_init`
/// from a `contracts` module that deposits the registrations) and the single-file
/// hand-written layout (entry module defines `polyplug_init` itself) collapse to
/// one rule.
///
/// `__globals__` is a plain `dict`, so the attribute is read by *item* lookup,
/// not attribute lookup. If `init_fn` is not a plain Python function (no
/// `__globals__`, e.g. a C callable), fall back to the entry module's namespace,
/// where attribute lookup reaches the same `__dict__` keys.
///
/// Returns `Ok(None)` when the namespace exists but the attribute is absent so
/// the caller can emit a precise "missing" error.
fn resolve_registrations<'py>(
    init_fn: &Bound<'py, PyAny>,
    entry_module: &Bound<'py, PyAny>,
) -> Result<Option<Bound<'py, PyAny>>, pyo3::PyErr> {
    match init_fn.getattr("__globals__") {
        Ok(globals) => match globals.cast_into::<PyDict>() {
            Ok(dict) => dict.get_item(REGISTRATIONS_ATTR),
            // `__globals__` is always a dict for a real Python function; if it is
            // somehow not, treat registrations as absent rather than erroring.
            Err(_) => Ok(None),
        },
        Err(_) => match entry_module.getattr(REGISTRATIONS_ATTR) {
            Ok(attr) => Ok(Some(attr)),
            Err(_) => Ok(None),
        },
    }
}

/// Read and validate the guest's `_polyplug_registrations` attribute into a
/// `Vec<ContractRegistration>`.
///
/// The attribute is read from the namespace of the module that *defines*
/// `polyplug_init` (its `__globals__` dict), falling back to the entry module's
/// namespace when `init_fn` is not a plain Python function (see
/// [`resolve_registrations`]).
///
/// Returns `InitFailed` if the attribute is missing, is not a list, or any entry
/// is malformed (missing `contract`/`functions`, bad contract string, or a
/// non-callable in `functions`).
pub(crate) fn collect_registrations(
    py: Python<'_>,
    init_fn: &Bound<'_, PyAny>,
    entry_module: &Bound<'_, PyAny>,
    bundle_name: &str,
) -> Result<Vec<ContractRegistration>, RuntimeError> {
    let attr: Bound<'_, PyAny> = resolve_registrations(init_fn, entry_module)
        .map_err(|_| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!(
                    "failed to read `{}` from the module defining polyplug_init",
                    REGISTRATIONS_ATTR
                ),
            })
        })?
        .ok_or_else(|| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!(
                    "Python plugin missing `{}` in the module defining polyplug_init",
                    REGISTRATIONS_ATTR
                ),
            })
        })?;

    let list: Bound<'_, PyList> = attr.cast_into::<PyList>().map_err(|_| {
        RuntimeError::Loader(LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("`{}` must be a list of dicts", REGISTRATIONS_ATTR),
        })
    })?;

    let mut registrations: Vec<ContractRegistration> = Vec::with_capacity(list.len());
    for entry in list.iter() {
        let contract_str: String = entry
            .get_item("contract")
            .and_then(|v: Bound<'_, PyAny>| v.extract::<String>())
            .map_err(|_| {
                RuntimeError::Loader(LoaderError::InitFailed {
                    bundle: bundle_name.to_owned(),
                    error: "registration entry missing string `contract` key".to_owned(),
                })
            })?;

        let (contract_name, contract_major): (String, u32) =
            parse_contract_string(&contract_str, bundle_name)?;

        let plugin_name: String = match entry.get_item("plugin_name") {
            Ok(v) => v
                .extract::<String>()
                .unwrap_or_else(|_| bundle_name.to_owned()),
            Err(_) => bundle_name.to_owned(),
        };

        let functions: Bound<'_, PyAny> = entry.get_item("functions").map_err(|_| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!(
                    "registration entry for `{}` missing `functions` list",
                    contract_str
                ),
            })
        })?;
        let functions_list: Bound<'_, PyList> = functions.cast_into::<PyList>().map_err(|_| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!("`functions` for `{}` must be a list", contract_str),
            })
        })?;

        let mut callables: Vec<Py<PyAny>> = Vec::with_capacity(functions_list.len());
        for (idx, callable) in functions_list.iter().enumerate() {
            if !callable.is_callable() {
                return Err(RuntimeError::Loader(LoaderError::InitFailed {
                    bundle: bundle_name.to_owned(),
                    error: format!(
                        "`functions[{}]` for `{}` is not callable",
                        idx, contract_str
                    ),
                }));
            }
            callables.push(callable.unbind());
        }

        registrations.push(ContractRegistration {
            contract_name,
            contract_major,
            plugin_name,
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
/// Each contract gets its own leaked [`PythonLoaderData`] (retire-not-drop:
/// previously resolved dispatch pointers must stay valid for the runtime
/// lifetime), and the interface plus descriptor strings are leaked to `'static`
/// for the same reason. Returns the number of contracts registered, or an error
/// if registration of any contract fails or none were registered.
pub(crate) fn register_contracts(
    registrations: Vec<ContractRegistration>,
    host_interface: *const HostApi,
    bundle_name: &str,
) -> Result<u32, RuntimeError> {
    let mut registered: u32 = 0_u32;

    for reg in registrations {
        let cid: GuestContractId = GuestContractId::new(&reg.contract_name, reg.contract_major);

        let loader_data: Box<PythonLoaderData> = Box::new(PythonLoaderData {
            callables: reg.callables,
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
        let reg_result: AbiError = unsafe {
            ((*host_interface).register_guest_contract)(
                host_interface,
                &descriptor as *const PluginDescriptor,
                static_interface,
            )
        };

        if !reg_result.is_ok() {
            return Err(RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!(
                    "register_guest_contract failed for `{}`: code={:?}",
                    contract_name_leaked, reg_result.code
                ),
            }));
        }

        registered += 1;
    }

    if registered == 0 {
        return Err(RuntimeError::Loader(LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!(
                "`{}` registered no contracts (empty list)",
                REGISTRATIONS_ATTR
            ),
        }));
    }

    Ok(registered)
}

/// Build the `_polyplug_arena_alloc(size, arena) -> int` bridge callable.
///
/// The arena pointer is supplied EXPLICITLY by the caller as the second argument:
/// it is the `arena` int the guest received as the third argument of its dispatch
/// callable. There is no shared cell — allocation correctness does not depend on
/// any published state, so concurrent and same-thread reentrant dispatch are both
/// sound. When `arena` is 0 (the caller has no per-call arena) the bridge falls
/// back to `host->alloc`, preserving per-value allocation behaviour. Returns the
/// allocated address as a Python `int` (0 on failure).
///
/// One bridge is built per bundle; the caller injects it into the relevant module
/// namespaces via [`inject_arena_bridge`].
fn build_arena_bridge<'py>(
    py: Python<'py>,
    host_interface: *const HostApi,
    bundle_name: &str,
) -> Result<Bound<'py, PyAny>, RuntimeError> {
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
    .map_err(|e: pyo3::PyErr| {
        RuntimeError::Loader(LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("failed to create `{}` bridge: {}", ARENA_ALLOC_ATTR, e),
        })
    })
}

/// Build the arena bridge and inject it into BOTH the entry module's namespace
/// and the namespace of the module that *defines* `polyplug_init` (its
/// `__globals__` dict).
///
/// This is the same single rule that governs registrations collection (see
/// [`resolve_registrations`]): the ABI functions that call
/// `_polyplug_arena_alloc` live in the module that *defines* `polyplug_init`, so
/// the bridge must be reachable from that module's globals. In the split-module
/// generated layout the entry file only `from … import polyplug_init`, so its own
/// namespace is not where the ABI functions resolve names — `__globals__` is.
///
/// Injecting into both namespaces is belt-and-braces: in the single-file
/// hand-written layout the entry module *is* the module defining `polyplug_init`,
/// so both targets are the same dict and the second `setitem` is harmless; in the
/// split-module layout the entry-module injection is harmless and the
/// `__globals__` injection is the one that makes dispatch resolve the bridge.
///
/// `__globals__` is a plain `dict`, so the bridge is set by *item* assignment, not
/// attribute assignment. If `init_fn` is not a plain Python function (no
/// `__globals__`, e.g. a C callable), only the entry-module injection applies —
/// the same fallback as registrations collection.
///
/// Must be called *after* `polyplug_init` is located (so its `__globals__` is the
/// real defining namespace) but *before* it is invoked (so the bridge is present
/// when the guest's ABI functions are first reachable).
pub(crate) fn inject_arena_bridge(
    py: Python<'_>,
    module: &Bound<'_, PyAny>,
    init_fn: &Bound<'_, PyAny>,
    host_interface: *const HostApi,
    bundle_name: &str,
) -> Result<(), RuntimeError> {
    let bridge: Bound<'_, PyAny> = build_arena_bridge(py, host_interface, bundle_name)?;

    // Inject into the entry module's namespace (covers single-file plugins, where
    // the entry module is also the module defining polyplug_init).
    module
        .setattr(ARENA_ALLOC_ATTR, &bridge)
        .map_err(|e: pyo3::PyErr| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!("failed to inject `{}`: {}", ARENA_ALLOC_ATTR, e),
            })
        })?;

    // Inject into the namespace of the module that defines polyplug_init (its
    // __globals__), where the guest's ABI functions resolve the bridge. This is
    // the load-bearing injection for the split-module generated layout. Falls back
    // to nothing extra when init_fn has no __globals__ (non-plain callable): the
    // entry-module injection above already covers that case.
    if let Ok(globals) = init_fn.getattr("__globals__")
        && let Ok(dict) = globals.cast_into::<PyDict>()
    {
        dict.set_item(ARENA_ALLOC_ATTR, &bridge)
            .map_err(|e: pyo3::PyErr| {
                RuntimeError::Loader(LoaderError::InitFailed {
                    bundle: bundle_name.to_owned(),
                    error: format!(
                        "failed to inject `{}` into polyplug_init.__globals__: {}",
                        ARENA_ALLOC_ATTR, e
                    ),
                })
            })?;
    }

    Ok(())
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

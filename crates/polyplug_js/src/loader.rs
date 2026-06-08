//! QuickJS in-process plugin loader implementation.
//!
//! Loads JS plugin bundles via the embedded QuickJS VM (rquickjs).
//! Each bundle gets its own QuickJS Runtime and Context for complete isolation
//! between bundles and between polyplug Runtime instances.
//! Uses VM dispatch to call JS functions through the QuickJS API.

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::thread::ThreadId;

use rquickjs::Array;
use rquickjs::Context;
use rquickjs::Ctx;
use rquickjs::Function;
use rquickjs::Object;
use rquickjs::Persistent;
use rquickjs::Runtime;
use rquickjs::Value;

use polyplug::Runtime as PolyplugRuntime;
use polyplug::error::LoaderError;
use polyplug::error::RuntimeError;
use polyplug::loader::BundleLoader;
use polyplug::loader::BundleSource;
use polyplug::loader::ManifestData;
use polyplug_abi::AbiError;
use polyplug_abi::AbiErrorCode;
use polyplug_abi::BundleInitContext;
use polyplug_abi::CallArena;
use polyplug_abi::DispatchType;
use polyplug_abi::GuestContractHandle;
use polyplug_abi::GuestContractInstance;
use polyplug_abi::GuestContractInterface;
use polyplug_abi::HostApi;
use polyplug_abi::PluginDescriptor;
use polyplug_abi::StringView;
use polyplug_abi::VmLoaderData;
use polyplug_abi::dispatch::dispatch_mechanisms::DispatchMechanisms;
use polyplug_abi::dispatch::vm_dispatch::VmDispatch;
use polyplug_abi::types::Version;
use polyplug_utils::BundleId;
use polyplug_utils::GuestContractId;

use crate::config::JsConfig;

// ─── Registration data stored in QuickJS runtime userdata ──────────────────────

use core::cell::RefCell;
use std::rc::Rc;

use rquickjs::JsLifetime;
use rquickjs::runtime::UserDataError;
use rquickjs::runtime::UserDataGuard;

/// Registration data collected from the JS plugin during polyplug_init.
///
/// This struct is stored in the QuickJS runtime's userdata to avoid thread-local
/// storage, ensuring multiple polyplug runtimes can coexist in the same process.
struct JsRegistrationData {
    contract_id: u64,
    contract_version: u32,
    _fn_count: usize,
    contract_name: String,
    functions: Vec<Persistent<Function<'static>>>,
}

// SAFETY: JsRegistrationData has no lifetime parameters and contains only 'static
// data (Persistent<Function<'static>> is 'static). This implementation allows the
// type to be stored in rquickjs's userdata storage.
unsafe impl<'js> JsLifetime<'js> for JsRegistrationData {
    type Changed<'to> = JsRegistrationData;
}

// ─── JS Loader Data for VM Dispatch ───────────────────────────────────────────

/// Loader-specific data for JS plugin dispatch.
///
/// Each bundle gets its own QuickJS Runtime and Context, ensuring complete
/// isolation between bundles and between polyplug Runtime instances.
/// The Context is cached for fast dispatch without per-call creation overhead.
///
/// # Field drop order
/// Rust drops fields in declaration order, and QuickJS's `JS_FreeRuntime` asserts
/// that every JS object (the `Persistent<Function>`s) and the `Context` are already
/// freed when the `Runtime` is dropped. `functions` and `ctx` are therefore declared
/// BEFORE `_runtime` so they drop first; reordering them would re-introduce a
/// `JS_FreeRuntime: Assertion 'list_empty(&rt->gc_obj_list)' failed` abort when a
/// bundle's VM is reclaimed on unload.
pub struct JsLoaderData {
    pub functions: Vec<Persistent<Function<'static>>>,
    pub ctx: Context,
    pub _runtime: Runtime,
    /// Thread-aware same-VM reentrancy guard for [`js_dispatch`].
    ///
    /// rquickjs is built with the `parallel` feature, so a `Context` is reachable
    /// from any thread and is internally lock-guarded. Two cases must be told
    /// apart, and a plain `AtomicBool` cannot distinguish them:
    ///
    /// 1. SAME-thread nested dispatch — a plugin→plugin cross-call
    ///    (`host->call_guest_method`) that resolves back to a contract in THIS
    ///    same Context while this thread is already mid-dispatch. That would call
    ///    `Context::with` recursively on the same context on the same thread, which
    ///    rquickjs panics/aborts on. This MUST be refused with `ReentrantCall`
    ///    BEFORE `Context::with` is entered.
    /// 2. CROSS-thread concurrent dispatch — a different thread dispatches into
    ///    this Context while a dispatch is in flight on another thread. rquickjs
    ///    serializes this safely by blocking on its internal lock, matching the
    ///    HostApi contract ("safe to call from any thread; the runtime handles
    ///    internal synchronization"). This MUST proceed and be allowed to block.
    ///
    /// The set of thread ids currently inside a dispatch on this Context captures
    /// exactly that distinction: presence of the current thread's id means a
    /// same-thread nested call (refuse); absence means a fresh caller — possibly
    /// from another thread concurrently — which proceeds. It lives on the per-VM
    /// `JsLoaderData`, never globally, so it is Rule-12 compliant. Contention is
    /// trivial: the vec holds 0..N concurrent caller threads and never duplicates.
    pub in_dispatch_threads: Mutex<Vec<ThreadId>>,
}

/// Owning, thread-shareable handle to a bundle's [`JsLoaderData`].
///
/// `JsLoaderData` is `!Send`/`!Sync` because `Persistent<Function>` carries a raw
/// `*mut JSRuntime`. The previous design laundered this by leaking the box behind a
/// raw `*mut c_void` inside `VmLoaderData`; this newtype instead owns the box so it
/// can be dropped on unload, while restoring `Send + Sync` so the loader's `live` /
/// `retired` collections (and therefore `JsLoader`) stay `Send + Sync`.
///
/// The pointer stored in the dispatch `bridge_data` is `self.0`'s stable heap
/// address; it is dereferenced only inside `js_dispatch`, which enters
/// `Context::with`. rquickjs is built with the `parallel` feature, so every VM
/// access serializes on the runtime's internal lock — exactly the invariant the
/// cross-thread dispatch path already relies on.
struct SendVm(Box<JsLoaderData>);

// SAFETY: every access to the wrapped JsLoaderData's VM goes through `Context::with`
// (in js_dispatch) or the bundle's own `in_dispatch_threads` Mutex (in unload).
// rquickjs's `parallel` feature serializes all VM operations on the runtime's
// internal lock, so moving ownership of the box between threads and sharing `&SendVm`
// across threads never produces concurrent unsynchronized VM access. This is the same
// soundness the existing cross-thread `js_dispatch` path depends on.
unsafe impl Send for SendVm {}
// SAFETY: see the Send impl — VM access is serialized by rquickjs's internal lock and
// the per-bundle in_dispatch_threads Mutex, so shared references are sound.
unsafe impl Sync for SendVm {}

impl SendVm {
    /// The stable heap address of the wrapped [`JsLoaderData`], used as the dispatch
    /// `bridge_data`. Stable across moves of the `SendVm`/`Box` while owned.
    fn as_ptr(&self) -> *const JsLoaderData {
        &*self.0 as *const JsLoaderData
    }

    /// Borrow the wrapped [`JsLoaderData`] (e.g. to inspect `in_dispatch_threads`).
    fn data(&self) -> &JsLoaderData {
        &self.0
    }
}

/// RAII guard that removes the current thread's id from
/// [`JsLoaderData::in_dispatch_threads`] on every exit path, including panics
/// that unwind through `js_dispatch`.
struct JsDispatchGuard<'a> {
    threads: &'a Mutex<Vec<ThreadId>>,
}

impl Drop for JsDispatchGuard<'_> {
    fn drop(&mut self) {
        let this: ThreadId = std::thread::current().id();
        // Recover from poisoning: a panic in another dispatch may have poisoned
        // the lock, but the data is a plain Vec<ThreadId> that cannot be left
        // logically corrupt between lock/unlock, so reusing the inner value is
        // sound. This is production code, so we never unwrap.
        let mut guard: std::sync::MutexGuard<'_, Vec<ThreadId>> =
            self.threads.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(pos) = guard.iter().position(|&id| id == this) {
            guard.swap_remove(pos);
        }
    }
}

// ─── JS Dispatch Function ─────────────────────────────────────────────────────

// ─── Instance Lifecycle Stubs ──────────────────────────────────────────────────

/// Stub create_instance for JS plugins - returns null instance.
///
/// # Safety
/// JS plugins use VM dispatch with global state; instances are not used.
unsafe extern "C" fn js_create_instance(
    _host: *const HostApi,
    _args: *const (),
) -> GuestContractInstance {
    GuestContractInstance::null()
}

/// Stub destroy_instance for JS plugins - no cleanup needed.
///
/// # Safety
/// JS plugins don't own instance data.
unsafe extern "C" fn js_destroy_instance(_host: *const HostApi, _instance: GuestContractInstance) {}

// ─── JS Dispatch Function ─────────────────────────────────────────────────────

/// Dispatch function for JS plugins using VM dispatch pattern.
///
/// # Safety
/// - `loader_data` must be a valid VmLoaderData wrapping JsLoaderData
/// - `args` and `out` must be valid pointers for the ABI call
/// - `arena`, when non-null, must point to a valid [`CallArena`] reset by the
///   caller for this call. Values written by the guest into the arena (via
///   `polyplug.arenaAlloc`) are valid until the caller's next reset.
unsafe extern "C" fn js_dispatch(
    loader_data: VmLoaderData,
    _instance: GuestContractInstance,
    fn_id: u32,
    args: *const (),
    out: *mut (),
    arena: *mut CallArena,
) -> AbiError {
    // SAFETY: loader_data wraps a valid pointer to JsLoaderData created by the loader.
    let data: &JsLoaderData = unsafe { &*(loader_data.data as *const JsLoaderData) };

    // Reject ONLY same-thread nested reentrancy BEFORE entering Context::with. If
    // this thread is already inside a dispatch on this Context (a plugin→plugin
    // cross-call resolving back here), a nested Context::with on the same thread
    // would abort, so refuse with ReentrantCall. A different thread dispatching
    // concurrently is NOT reentrancy: it is allowed to proceed and rquickjs's
    // internal lock serializes it safely. The tracking Mutex is held only around
    // the membership check/insert below, never across Context::with.
    let this_thread: ThreadId = std::thread::current().id();
    {
        // Recover from poisoning (a prior dispatch panic): the Vec<ThreadId>
        // cannot be left logically corrupt between lock/unlock, so the inner
        // value is reusable. Production code, so no unwrap.
        let mut threads: std::sync::MutexGuard<'_, Vec<ThreadId>> = data
            .in_dispatch_threads
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if threads.contains(&this_thread) {
            // Drop the guard (unlock) before returning.
            drop(threads);
            return AbiError {
                code: AbiErrorCode::ReentrantCall as u32,
                message: StringView::null(),
            };
        }
        threads.push(this_thread);
    }
    // From here on this thread's id is registered; the guard removes it on every
    // exit path (early return, normal return, or panic unwind).
    let _dispatch_guard: JsDispatchGuard<'_> = JsDispatchGuard {
        threads: &data.in_dispatch_threads,
    };

    let func_persistent: &Persistent<Function<'static>> = match data.functions.get(fn_id as usize) {
        Some(f) => f,
        None => {
            return AbiError {
                code: AbiErrorCode::FunctionNotAvailable as u32,
                message: StringView::null(),
            };
        }
    };

    let args_usize: usize = args as usize;
    let out_usize: usize = out as usize;

    // Pass pointers as f64. User-space addresses fit within 2^48 < 2^53 (float64 mantissa),
    // so the conversion from usize → f64 → usize is exact on all supported platforms.
    // The generated JS wrapper receives plain Number arguments and passes them directly
    // to readU32/writeU32, which also accept f64 — no BigInt conversion needed.
    let args_f64: f64 = args_usize as f64;
    let out_f64: f64 = out_usize as f64;

    let arena_usize: usize = arena as usize;

    let call_result: Result<i32, rquickjs::Error> = data.ctx.with(|ctx| {
        // Publish the per-call arena pointer on the polyplug object so the
        // arenaAlloc bridge can serve allocations from it. The VM lock is held
        // for the whole closure, so single-threaded access is guaranteed. The
        // pointer is cleared after the call so a stale arena is never reachable.
        set_arena_ptr(&ctx, arena_usize);

        let js_fn: Function<'_> = func_persistent.clone().restore(&ctx)?;

        let result: Result<i32, rquickjs::Error> =
            js_fn.call::<(f64, f64), i32>((args_f64, out_f64));

        set_arena_ptr(&ctx, 0);
        result
    });

    match call_result {
        Ok(0) => AbiError::ok(),
        Ok(code) => AbiError {
            // AbiError.code is a raw u32, so the plugin-provided code is stored
            // verbatim — no enum materialization, hence no UB on unknown values.
            code: code as u32,
            message: StringView::null(),
        },
        Err(_) => AbiError {
            code: AbiErrorCode::Generic as u32,
            message: StringView::null(),
        },
    }
}

// ─── Host function registration ───────────────────────────────────────────────

fn pack_handle(h: GuestContractHandle) -> Option<u64> {
    if h.is_null() {
        None
    } else {
        // Carry the full handle identity (generation in the high 32 bits, index in
        // the low 32) so a JS-held token round-trips back to the exact slot+generation
        // and stale handles are detected on resolve. Mirrors GuestContractHandle::pack.
        Some(h.pack())
    }
}

/// Publish the per-call arena pointer on the `polyplug` global as lo/hi f64 halves.
///
/// Stored as f64 for the same reason as the host vtable pointer: rquickjs would
/// sign-extend a u32 above INT32_MAX to a negative tagged int and corrupt the
/// pointer. A value of 0 means "no arena" (arenaAlloc falls back to host->alloc).
fn set_arena_ptr<'js>(ctx: &Ctx<'js>, ptr: usize) {
    let Ok(polyplug_obj) = ctx.globals().get::<&str, Object<'js>>("polyplug") else {
        return;
    };
    let _ = polyplug_obj.set("_arenaLo", (ptr as u32) as f64);
    let _ = polyplug_obj.set("_arenaHi", ((ptr >> 32) as u32) as f64);
}

/// Read the per-call arena pointer from the `polyplug` global, or null if unset.
fn get_arena_ptr<'js>(ctx: &Ctx<'js>) -> *mut CallArena {
    let Ok(polyplug_obj) = ctx.globals().get::<&str, Object<'js>>("polyplug") else {
        return core::ptr::null_mut();
    };
    let lo: f64 = polyplug_obj.get::<&str, f64>("_arenaLo").unwrap_or(0.0);
    let hi: f64 = polyplug_obj.get::<&str, f64>("_arenaHi").unwrap_or(0.0);
    (((hi as u64) << 32) | lo as u64) as usize as *mut CallArena
}

/// Helper to get HostApi pointer from JS globals.
///
/// Lo/hi are stored as f64 to avoid rquickjs sign-extending u32 > INT32_MAX to negative
/// tagged ints, which would cause u32::from_js to fail or return a wrong value.
fn get_host_interface_from_globals<'js>(ctx: &Ctx<'js>) -> Option<*const HostApi> {
    let polyplug_obj: Object<'js> = ctx.globals().get::<&str, Object<'js>>("polyplug").ok()?;

    let vtable_lo: f64 = polyplug_obj.get::<&str, f64>("_hostVtableLo").ok()?;
    let vtable_hi: f64 = polyplug_obj.get::<&str, f64>("_hostVtableHi").ok()?;

    let host_interface_ptr: *const HostApi =
        ((vtable_hi as u64) << 32 | vtable_lo as u64) as usize as *const HostApi;

    if host_interface_ptr.is_null() {
        None
    } else {
        Some(host_interface_ptr)
    }
}

fn register_host_functions<'js>(
    ctx: &Ctx<'js>,
    polyplug_obj: &Object<'js>,
    host_interface: *const HostApi,
    bundle_name: &str,
) -> Result<(), RuntimeError> {
    // Store host interface pointer as JS globals on the polyplug object
    let host_interface_usize: usize = host_interface as usize;

    // Store as f64: u32 > INT32_MAX would be sign-extended by rquickjs to a negative
    // tagged int, causing f64::from_js or u32::from_js to fail on read-back.
    polyplug_obj
        .set("_hostVtableLo", (host_interface_usize as u32) as f64)
        .map_err(|e: rquickjs::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!("JS runtime js-quickjs error: _hostVtableLo set failed: {e}"),
            })
        })?;
    polyplug_obj
        .set(
            "_hostVtableHi",
            ((host_interface_usize >> 32) as u32) as f64,
        )
        .map_err(|e: rquickjs::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!("JS runtime js-quickjs error: _hostVtableHi set failed: {e}"),
            })
        })?;

    let find_by_contract_fn: Function<'js> = Function::new(
        ctx.clone(),
        |ctx: Ctx<'js>, lo: u32, hi: u32, min_ver: u32| -> Option<u64> {
            let contract_id: u64 = (hi as u64) << 32 | lo as u64;
            let hvt: *const HostApi = get_host_interface_from_globals(&ctx)?;
            // SAFETY: hvt points to 'static HostApi data.
            let handle: GuestContractHandle =
                unsafe { ((*hvt).find_guest_contract)(hvt, contract_id, min_ver) };
            pack_handle(handle)
        },
    )
    .map_err(|e: rquickjs::Error| {
        RuntimeError::Loader(LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!(
                "JS runtime js-quickjs error: findByContract function creation failed: {e}"
            ),
        })
    })?;

    polyplug_obj
        .set("findByContract", find_by_contract_fn)
        .map_err(|e: rquickjs::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!("JS runtime js-quickjs error: findByContract set failed: {e}"),
            })
        })?;

    let find_by_bundle_fn: Function<'js> = Function::new(
        ctx.clone(),
        |_ctx: Ctx<'js>,
         _blo: u32,
         _bhi: u32,
         _clo: u32,
         _chi: u32,
         _min_ver: u32|
         -> Option<u64> {
            // Note: find_by_bundle was removed from HostApi in the instance-based model.
            // Use find_guest_contract instead.
            None
        },
    )
    .map_err(|e: rquickjs::Error| {
        RuntimeError::Loader(LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!(
                "JS runtime js-quickjs error: findByBundle function creation failed: {e}"
            ),
        })
    })?;

    polyplug_obj
        .set("findByBundle", find_by_bundle_fn)
        .map_err(|e: rquickjs::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!("JS runtime js-quickjs error: findByBundle set failed: {e}"),
            })
        })?;

    let find_all_by_contract_fn: Function<'js> = Function::new(
        ctx.clone(),
        |ctx: Ctx<'js>, lo: u32, hi: u32, min_ver: u32| -> u32 {
            let contract_id: u64 = (hi as u64) << 32 | lo as u64;
            let hvt: *const HostApi = match get_host_interface_from_globals(&ctx) {
                Some(ptr) => ptr,
                None => return 0_u32,
            };
            // SAFETY: hvt points to 'static HostApi data.
            // find_all_guest_contracts returns Array<GuestContractHandle>.
            let handles: polyplug_abi::types::Array<GuestContractHandle> =
                unsafe { ((*hvt).find_all_guest_contracts)(hvt, contract_id, min_ver) };
            handles.len as u32
        },
    )
    .map_err(|e: rquickjs::Error| {
        RuntimeError::Loader(LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!(
                "JS runtime js-quickjs error: findAllByContract function creation failed: {e}"
            ),
        })
    })?;

    polyplug_obj
        .set("findAllByContract", find_all_by_contract_fn)
        .map_err(|e: rquickjs::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!("JS runtime js-quickjs error: findAllByContract set failed: {e}"),
            })
        })?;

    let resolve_guest_contract_fn: Function<'js> = Function::new(
        ctx.clone(),
        |ctx: Ctx<'js>, packed: u64| -> Option<u64> {
            // Unpack the full handle identity: index in the low 32 bits, generation
            // in the high 32 (matches pack_handle / GuestContractHandle::pack).
            let index: u32 = packed as u32;
            let generation: u32 = (packed >> 32) as u32;
            let handle: GuestContractHandle = GuestContractHandle { index, generation };
            let hvt: *const HostApi = get_host_interface_from_globals(&ctx)?;
            // SAFETY: hvt points to 'static HostApi data.
            let vtable_ptr: *const GuestContractInterface =
                unsafe { ((*hvt).resolve_guest_contract)(hvt, handle) };
            if vtable_ptr.is_null() {
                None
            } else {
                Some(vtable_ptr as usize as u64)
            }
        },
    )
    .map_err(|e: rquickjs::Error| {
        RuntimeError::Loader(LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!(
                "JS runtime js-quickjs error: resolveGuestContract function creation failed: {e}"
            ),
        })
    })?;

    polyplug_obj
        .set("resolveGuestContract", resolve_guest_contract_fn)
        .map_err(|e: rquickjs::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!("JS runtime js-quickjs error: resolveGuestContract set failed: {e}"),
            })
        })?;

    let get_host_contract_fn: Function<'js> = Function::new(
        ctx.clone(),
        |ctx: Ctx<'js>, contract_id: u64, min_version: u32| -> Option<u64> {
            let hvt: *const HostApi = get_host_interface_from_globals(&ctx)?;
            // SAFETY: hvt points to 'static HostApi data.
            let instance: polyplug_abi::HostContractInstance =
                unsafe { ((*hvt).get_host_contract)(hvt, contract_id, min_version) };
            if instance.data.is_null() {
                None
            } else {
                Some(instance.data as usize as u64)
            }
        },
    )
    .map_err(|e: rquickjs::Error| {
        RuntimeError::Loader(LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!(
                "JS runtime js-quickjs error: getHostContract function creation failed: {e}"
            ),
        })
    })?;

    polyplug_obj
        .set("getHostContract", get_host_contract_fn)
        .map_err(|e: rquickjs::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!("JS runtime js-quickjs error: getHostContract set failed: {e}"),
            })
        })?;

    let register_vtable_fn: Function<'js> = Function::new(
        ctx.clone(),
        |ctx: Ctx<'js>,
         contract_lo: u32,
         contract_hi: u32,
         vtable_obj: Object<'js>,
         fn_count: u32,
         contract_name: String,
         contract_version: u32| {
            let contract_id: u64 = (contract_hi as u64) << 32 | contract_lo as u64;
            let fn_count_usize: usize = fn_count as usize;

            let mut functions: Vec<Persistent<Function<'static>>> =
                Vec::with_capacity(fn_count_usize);
            let functions_array: Object<'js> =
                match vtable_obj.get::<&str, Object<'js>>("functions") {
                    Ok(arr) => arr,
                    Err(_) => return,
                };

            for i in 0..fn_count_usize {
                let func: Function<'js> = match functions_array.get::<u32, Function<'js>>(i as u32)
                {
                    Ok(f) => f,
                    Err(_) => return,
                };
                let func_persistent: Persistent<Function<'static>> = Persistent::save(&ctx, func);
                functions.push(func_persistent);
            }

            let data: JsRegistrationData = JsRegistrationData {
                contract_id,
                contract_version,
                _fn_count: fn_count_usize,
                contract_name,
                functions,
            };

            let slot_guard: UserDataGuard<Rc<RefCell<Option<JsRegistrationData>>>> =
                match ctx.userdata::<Rc<RefCell<Option<JsRegistrationData>>>>() {
                    Some(guard) => guard,
                    None => return,
                };
            let mut cell: core::cell::RefMut<Option<JsRegistrationData>> = slot_guard.borrow_mut();
            *cell = Some(data);
        },
    )
    .map_err(|e: rquickjs::Error| {
        RuntimeError::Loader(LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!(
                "JS runtime js-quickjs error: registerVtable function creation failed: {e}"
            ),
        })
    })?;

    polyplug_obj
        .set("registerVtable", register_vtable_fn)
        .map_err(|e: rquickjs::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!("JS runtime js-quickjs error: registerVtable set failed: {e}"),
            })
        })?;

    let alloc_fn: Function<'js> = Function::new(
        ctx.clone(),
        |ctx: Ctx<'js>, size: u32| -> Result<Array<'js>, rquickjs::Error> {
            let hvt: *const HostApi = match get_host_interface_from_globals(&ctx) {
                Some(ptr) => ptr,
                None => {
                    let arr: Array<'js> = Array::new(ctx.clone()).map_err(|_| {
                        rquickjs::Exception::throw_message(&ctx, "array creation failed")
                    })?;
                    let _ = arr.set(0, 0.0_f64);
                    let _ = arr.set(1, 0.0_f64);
                    return Ok(arr);
                }
            };
            // SAFETY: hvt points to 'static HostApi data.
            let ptr: *mut u8 = unsafe { ((*hvt).alloc)(hvt, size as usize, 1) };
            let ptr_usize: usize = ptr as usize;
            let arr: Array<'js> = Array::new(ctx.clone())
                .map_err(|_| rquickjs::Exception::throw_message(&ctx, "array creation failed"))?;
            // Store as f64: rquickjs sign-extends u32 > INT32_MAX to negative JS ints,
            // which breaks BigInt reconstruction. f64 preserves the unsigned value exactly.
            let _ = arr.set(0, (ptr_usize as u32) as f64);
            let _ = arr.set(1, ((ptr_usize >> 32) as u32) as f64);
            Ok(arr)
        },
    )
    .map_err(|e: rquickjs::Error| {
        RuntimeError::Loader(LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("JS runtime js-quickjs error: alloc function creation failed: {e}"),
        })
    })?;

    polyplug_obj
        .set("alloc", alloc_fn)
        .map_err(|e: rquickjs::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!("JS runtime js-quickjs error: alloc set failed: {e}"),
            })
        })?;

    // arenaAlloc serves the guest's per-call return buffers from the current
    // CallArena (published by js_dispatch), falling back to host->alloc when no
    // arena is active. Returns [lo, hi] f64 halves, matching alloc.
    let arena_alloc_fn: Function<'js> = Function::new(
        ctx.clone(),
        |ctx: Ctx<'js>, size: u32| -> Result<Array<'js>, rquickjs::Error> {
            let arena: *mut CallArena = get_arena_ptr(&ctx);
            let ptr: *mut u8 = if arena.is_null() {
                match get_host_interface_from_globals(&ctx) {
                    // SAFETY: hvt points to 'static HostApi data.
                    Some(hvt) => unsafe { ((*hvt).alloc)(hvt, size as usize, 1) },
                    None => core::ptr::null_mut(),
                }
            } else {
                // SAFETY: `arena` is the valid per-call CallArena published by
                // js_dispatch under the VM lock; alloc bumps within it or chains
                // a host-allocated overflow block.
                unsafe { (*arena).alloc(size as usize, 1) }
            };
            let ptr_usize: usize = ptr as usize;
            let arr: Array<'js> = Array::new(ctx.clone())
                .map_err(|_| rquickjs::Exception::throw_message(&ctx, "array creation failed"))?;
            let _ = arr.set(0, (ptr_usize as u32) as f64);
            let _ = arr.set(1, ((ptr_usize >> 32) as u32) as f64);
            Ok(arr)
        },
    )
    .map_err(|e: rquickjs::Error| {
        RuntimeError::Loader(LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("JS runtime js-quickjs error: arenaAlloc function creation failed: {e}"),
        })
    })?;

    polyplug_obj
        .set("arenaAlloc", arena_alloc_fn)
        .map_err(|e: rquickjs::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!("JS runtime js-quickjs error: arenaAlloc set failed: {e}"),
            })
        })?;

    // lo/hi are f64 for the same reason as alloc's return values: u32 > INT32_MAX would be
    // sign-extended by rquickjs to a negative JS int, corrupting the pointer reconstruction.
    // size/align must match the original allocation so the host allocator frees the exact
    // region — passing size=0 makes polyplug_host_free a no-op, which leaks every block.
    let free_fn: Function<'js> = Function::new(
        ctx.clone(),
        |ctx: Ctx<'js>, lo: f64, hi: f64, size: u32, align: u32| {
            let hvt: *const HostApi = match get_host_interface_from_globals(&ctx) {
                Some(ptr) => ptr,
                None => return,
            };
            let ptr: *mut u8 = ((hi as u64) << 32 | lo as u64) as usize as *mut u8;
            if ptr.is_null() {
                return;
            }
            // SAFETY: hvt points to 'static HostApi data.
            unsafe { ((*hvt).free)(hvt, ptr, size as usize, align as usize) };
        },
    )
    .map_err(|e: rquickjs::Error| {
        RuntimeError::Loader(LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("JS runtime js-quickjs error: free function creation failed: {e}"),
        })
    })?;

    polyplug_obj
        .set("free", free_fn)
        .map_err(|e: rquickjs::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!("JS runtime js-quickjs error: free set failed: {e}"),
            })
        })?;

    let read_i32_fn: Function<'js> = Function::new(ctx.clone(), |ptr_num: f64| -> i32 {
        let ptr_u64: u64 = ptr_num as u64;
        let ptr: *const i32 = ptr_u64 as usize as *const i32;
        if ptr.is_null() {
            return 0;
        }
        // SAFETY: ptr is a valid pointer provided by the host for reading.
        unsafe { *ptr }
    })
    .map_err(|e: rquickjs::Error| {
        RuntimeError::Loader(LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("JS runtime js-quickjs error: readI32 function creation failed: {e}"),
        })
    })?;

    polyplug_obj
        .set("readI32", read_i32_fn)
        .map_err(|e: rquickjs::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!("JS runtime js-quickjs error: readI32 set failed: {e}"),
            })
        })?;

    let write_i32_fn: Function<'js> = Function::new(ctx.clone(), |ptr_num: f64, value: i32| {
        let ptr_u64: u64 = ptr_num as u64;
        let ptr: *mut i32 = ptr_u64 as usize as *mut i32;
        if ptr.is_null() {
            return;
        }
        // SAFETY: ptr is a valid pointer provided by the host for writing.
        unsafe {
            *ptr = value;
        }
    })
    .map_err(|e: rquickjs::Error| {
        RuntimeError::Loader(LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("JS runtime js-quickjs error: writeI32 function creation failed: {e}"),
        })
    })?;

    polyplug_obj
        .set("writeI32", write_i32_fn)
        .map_err(|e: rquickjs::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!("JS runtime js-quickjs error: writeI32 set failed: {e}"),
            })
        })?;

    let read_byte_fn: Function<'js> = Function::new(ctx.clone(), |ptr_num: f64| -> u32 {
        let ptr_u64: u64 = ptr_num as u64;
        let ptr: *const u8 = ptr_u64 as usize as *const u8;
        if ptr.is_null() {
            return 0;
        }
        // SAFETY: ptr is a valid pointer provided by the host for reading.
        unsafe { *ptr as u32 }
    })
    .map_err(|e: rquickjs::Error| {
        RuntimeError::Loader(LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("JS runtime js-quickjs error: readByte function creation failed: {e}"),
        })
    })?;

    polyplug_obj
        .set("readByte", read_byte_fn)
        .map_err(|e: rquickjs::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!("JS runtime js-quickjs error: readByte set failed: {e}"),
            })
        })?;

    let write_byte_fn: Function<'js> = Function::new(ctx.clone(), |ptr_num: f64, value: u32| {
        let ptr_u64: u64 = ptr_num as u64;
        let ptr: *mut u8 = ptr_u64 as usize as *mut u8;
        if ptr.is_null() {
            return;
        }
        // SAFETY: ptr is a valid pointer provided by the host for writing.
        unsafe {
            *ptr = value as u8;
        }
    })
    .map_err(|e: rquickjs::Error| {
        RuntimeError::Loader(LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("JS runtime js-quickjs error: writeByte function creation failed: {e}"),
        })
    })?;

    polyplug_obj
        .set("writeByte", write_byte_fn)
        .map_err(|e: rquickjs::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!("JS runtime js-quickjs error: writeByte set failed: {e}"),
            })
        })?;

    let read_memory_fn: Function<'js> = Function::new(
        ctx.clone(),
        // Returns Array<'js> of u8 values instead of ArrayBuffer: a plain Array of integers
        // is unambiguous and Uint8Array(array_of_ints) works correctly in QuickJS.
        |ctx: Ctx<'js>, ptr_num: f64, len: u32| -> Result<Array<'js>, rquickjs::Error> {
            let ptr_u64: u64 = ptr_num as u64;
            let ptr: *const u8 = ptr_u64 as usize as *const u8;
            let len_usize: usize = len as usize;

            let arr: Array<'js> = Array::new(ctx.clone())
                .map_err(|_| rquickjs::Exception::throw_message(&ctx, "Array creation failed"))?;

            if ptr.is_null() || len_usize == 0 {
                return Ok(arr);
            }

            // SAFETY: ptr is a valid pointer provided by the host for reading.
            // The caller guarantees the memory region [ptr, ptr+len) is valid.
            let bytes: &[u8] = unsafe { core::slice::from_raw_parts(ptr, len_usize) };

            for (i, &byte) in bytes.iter().enumerate() {
                // Byte values are 0-255, always fit in 31-bit signed int — no sign-extension issue.
                let _ = arr.set(i, u32::from(byte) as f64);
            }

            Ok(arr)
        },
    )
    .map_err(|e: rquickjs::Error| {
        RuntimeError::Loader(LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("JS runtime js-quickjs error: readMemory function creation failed: {e}"),
        })
    })?;

    polyplug_obj
        .set("readMemory", read_memory_fn)
        .map_err(|e: rquickjs::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!("JS runtime js-quickjs error: readMemory set failed: {e}"),
            })
        })?;

    // Return f64 instead of u32: rquickjs sign-extends u32 > INT32_MAX when converting to
    // JavaScript tagged ints. Returning f64 ensures the full unsigned range [0, 2^32) is
    // represented exactly as a JS Number (all u32 values are < 2^53 = float64 mantissa cap).
    let read_u32_fn: Function<'js> = Function::new(ctx.clone(), |ptr_num: f64| -> f64 {
        let ptr_u64: u64 = ptr_num as u64;
        let ptr: *const u32 = ptr_u64 as usize as *const u32;
        if ptr.is_null() {
            return 0.0;
        }
        // SAFETY: ptr is a valid pointer provided by the host for reading.
        unsafe { *ptr as f64 }
    })
    .map_err(|e: rquickjs::Error| {
        RuntimeError::Loader(LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("JS runtime js-quickjs error: readU32 function creation failed: {e}"),
        })
    })?;

    polyplug_obj
        .set("readU32", read_u32_fn)
        .map_err(|e: rquickjs::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!("JS runtime js-quickjs error: readU32 set failed: {e}"),
            })
        })?;

    // Both arguments are f64: same reasoning as readU32 — u32 values > INT32_MAX would be
    // sign-extended by rquickjs if typed as u32, corrupting large pointer halves or values.
    let write_u32_fn: Function<'js> = Function::new(ctx.clone(), |ptr_num: f64, value: f64| {
        let ptr_u64: u64 = ptr_num as u64;
        let ptr: *mut u32 = ptr_u64 as usize as *mut u32;
        if ptr.is_null() {
            return;
        }
        // SAFETY: ptr is a valid pointer provided by the host for writing.
        unsafe {
            *ptr = value as u64 as u32;
        }
    })
    .map_err(|e: rquickjs::Error| {
        RuntimeError::Loader(LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("JS runtime js-quickjs error: writeU32 function creation failed: {e}"),
        })
    })?;

    polyplug_obj
        .set("writeU32", write_u32_fn)
        .map_err(|e: rquickjs::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!("JS runtime js-quickjs error: writeU32 set failed: {e}"),
            })
        })?;

    Ok(())
}

// ─── JsLoader ────────────────────────────────────────────────────────────────

/// QuickJS in-process JS plugin loader.
pub struct JsLoader {
    _config: JsConfig,
    /// Per-bundle VM state owned by the loader, keyed by [`BundleId`].
    ///
    /// Each loaded bundle contributes one [`JsLoaderData`] (holding its own QuickJS
    /// `Runtime` and `Context`). The boxes are owned here instead of leaked via
    /// `Box::into_raw`, so [`JsLoader::unload`] can drop them and truly reclaim the
    /// VM. The VM dispatch `bridge_data` points at the boxed `JsLoaderData`'s stable
    /// heap address; the box is never moved out of the map while owned, so the
    /// pointer stays valid for as long as the bundle is loaded — exactly the
    /// guarantee the old leak provided. Reload appends rather than replaces so a
    /// superseded VM stays alive for any in-flight dispatch.
    live: Mutex<HashMap<BundleId, Vec<SendVm>>>,
    /// VM state that could not be dropped at unload because a dispatch was still in
    /// flight on the VM (non-quiescent). Held for the loader's lifetime so the raw
    /// `bridge_data` pointer the in-flight dispatch still dereferences stays valid —
    /// dropping it would be a use-after-free. This is the deferred-reclaim fallback.
    retired: Mutex<Vec<SendVm>>,
}

impl JsLoader {
    pub fn new(config: JsConfig) -> JsLoader {
        JsLoader {
            _config: config,
            live: Mutex::new(HashMap::new()),
            retired: Mutex::new(Vec::new()),
        }
    }

    /// Read the plugin's JS source from the on-disk bundle directory.
    ///
    /// Used by the [`BundleSource::Path`] flow. The file is resolved from the
    /// manifest's `file` field, defaulting to `bundle.js`.
    fn read_path_source(manifest: &ManifestData) -> Result<String, RuntimeError> {
        let bundle_path: PathBuf = if !manifest.file.is_empty() {
            manifest.path.join(&manifest.file)
        } else {
            manifest.path.join("bundle.js")
        };
        std::fs::read_to_string(&bundle_path).map_err(|e: std::io::Error| {
            RuntimeError::Loader(LoaderError::ManifestParse {
                path: bundle_path.display().to_string(),
                reason: e.to_string(),
            })
        })
    }

    /// Shared load/reload implementation.
    ///
    /// Both `load` and `reload` produce identical behaviour; `reload` only adds a
    /// hot-reload-enabled guard before delegating here.
    ///
    /// `bundle_js` is the plugin's JS source text — read from disk for
    /// [`BundleSource::Path`], or supplied directly for [`BundleSource::Code`] /
    /// [`BundleSource::Bytes`]. `bundle_dir` is the on-disk bundle directory for
    /// path sources, or `None` for in-memory sources, which are single-file and
    /// self-contained (JS bundles are always one flat `bundle.js`, so there is no
    /// bundle directory to provision and no sibling files to resolve). When `None`,
    /// `globalThis.bundlePath` and `BundleInitContext.bundle_path` are an empty
    /// string, matching the "no bundle directory for non-path sources" contract.
    fn load_inner(
        &self,
        manifest: &ManifestData,
        bundle_js: &str,
        bundle_dir: Option<&Path>,
        runtime: &PolyplugRuntime,
    ) -> Result<(), RuntimeError> {
        let bundle_id: u64 = manifest.id;

        let qjs_runtime: Runtime = Runtime::new().map_err(|e: rquickjs::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: manifest.name.clone(),
                error: format!("JS runtime init failed: QuickJS runtime init failed: {e}"),
            })
        })?;

        let ctx: Context = Context::full(&qjs_runtime).map_err(|e: rquickjs::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: manifest.name.clone(),
                error: format!("JS runtime js-quickjs error: context creation failed: {e}"),
            })
        })?;

        // Get the HostApi pointer from the runtime.
        // This interface already has the runtime pointer set internally.
        let host_interface: *const HostApi = runtime.as_context_ptr();

        // Push bundle_id onto the runtime's per-thread init stack for dependency
        // enforcement during init. The matching pop MUST run on every exit path
        // (success and error) so the stack never leaks an entry.
        runtime.push_init_bundle_id(bundle_id);

        // In-memory sources (Code/Bytes) carry no bundle directory, so bundlePath
        // and BundleInitContext.bundle_path are empty for them.
        let bundle_dir_str: String = match bundle_dir {
            Some(dir) => dir.to_string_lossy().into_owned(),
            None => String::new(),
        };

        let registration_slot: Rc<RefCell<Option<JsRegistrationData>>> =
            Rc::new(RefCell::new(None));

        let init_outcome: Result<(), RuntimeError> = ctx.with(|ctx_ref: Ctx<'_>| {
            ctx_ref
                .store_userdata(Rc::clone(&registration_slot))
                .map_err(
                    |_: UserDataError<Rc<RefCell<Option<JsRegistrationData>>>>| {
                        RuntimeError::Loader(LoaderError::InitFailed {
                            bundle: manifest.name.clone(),
                            error: "JS runtime js-quickjs error: failed to store registration slot in userdata".to_owned(),
                        })
                    },
                )?;

            let globals: Object<'_> = ctx_ref.globals();
            let polyplug_obj: Object<'_> =
                Object::new(ctx_ref.clone()).map_err(|e: rquickjs::Error| {
                    RuntimeError::Loader(LoaderError::InitFailed {
                        bundle: manifest.name.clone(),
                        error: format!("JS runtime js-quickjs error: object creation failed: {e}"),
                    })
                })?;
            register_host_functions(
                &ctx_ref,
                &polyplug_obj,
                host_interface,
                &manifest.name,
            )?;
            globals
                .set("polyplug", polyplug_obj)
                .map_err(|e: rquickjs::Error| {
                    RuntimeError::Loader(LoaderError::InitFailed {
                        bundle: manifest.name.clone(),
                        error: format!("JS runtime js-quickjs error: global set failed: {e}"),
                    })
                })?;

            let set_bundle: String = format!("globalThis.bundlePath = {:?};", bundle_dir_str);
            ctx_ref
                .eval::<Value<'_>, _>(set_bundle.as_str())
                .map_err(|e: rquickjs::Error| {
                    RuntimeError::Loader(LoaderError::InitFailed {
                        bundle: manifest.name.clone(),
                        error: format!("JS runtime js-quickjs error: bundlePath injection failed: {e}"),
                    })
                })?;

            ctx_ref
                .eval::<Value<'_>, _>(bundle_js)
                .map_err(|e: rquickjs::Error| {
                    RuntimeError::Loader(LoaderError::InitFailed {
                        bundle: manifest.name.clone(),
                        error: format!("JS runtime js-quickjs error: bundle eval failed: {e}"),
                    })
                })?;

            let init_fn: Function<'_> = ctx_ref
                .globals()
                .get::<&str, Function<'_>>("polyplug_init")
                .map_err(|_| {
                    RuntimeError::Loader(LoaderError::InitSymbolMissing {
                        bundle: bundle_dir_str.clone(),
                    })
                })?;

            // SAFETY: Intentionally leaked; bundle_path_static outlives this call.
            let bundle_path_static: &'static str =
                Box::leak(bundle_dir_str.clone().into_boxed_str());
            let plugin_ctx: BundleInitContext = BundleInitContext {
                bundle_path: StringView {
                    ptr: bundle_path_static.as_ptr(),
                    len: bundle_path_static.len(),
                },
                bundle_id,
            };

            // Pass HostApi and BundleInitContext pointers as 4 f64 arguments:
            //   (host_lo, host_hi, ctx_lo, ctx_hi)
            // The generated polyplug_init expects this 4-arg lo/hi split convention.
            // f64 is used instead of u32 because rquickjs sign-extends u32 > INT32_MAX to
            // negative tagged ints. f64 represents the full unsigned 32-bit range exactly.
            let host_usize: usize = host_interface as usize;
            let ctx_usize: usize = &plugin_ctx as *const BundleInitContext as usize;
            let host_lo: f64 = (host_usize as u32) as f64;
            let host_hi: f64 = ((host_usize >> 32) as u32) as f64;
            let ctx_lo: f64 = (ctx_usize as u32) as f64;
            let ctx_hi: f64 = ((ctx_usize >> 32) as u32) as f64;

            init_fn
                .call::<(f64, f64, f64, f64), ()>((host_lo, host_hi, ctx_lo, ctx_hi))
                .map_err(|e: rquickjs::Error| {
                    RuntimeError::Loader(LoaderError::InitFailed {
                        bundle: manifest.name.clone(),
                        error: format!("JS runtime js-quickjs error: polyplug_init call failed: {e}"),
                    })
                })?;

            Ok::<(), RuntimeError>(())
        });

        // Pop bundle_id from the init stack after init completes (always, including
        // the error path) so the stack does not leak an entry.
        runtime.pop_init_bundle_id();

        init_outcome?;

        let registration_data: JsRegistrationData =
            registration_slot.borrow_mut().take().ok_or_else(|| {
                RuntimeError::Loader(LoaderError::InitFailed {
                    bundle: manifest.name.clone(),
                    error: "JS runtime js-quickjs error: polyplug_init did not call registerVtable"
                        .to_owned(),
                })
            })?;

        let loader_data: SendVm = SendVm(Box::new(JsLoaderData {
            _runtime: qjs_runtime,
            ctx,
            functions: registration_data.functions,
            in_dispatch_threads: Mutex::new(Vec::new()),
        }));

        // The box's heap address is stable across later moves of the `SendVm`/`Box`
        // (moving them moves the pointer, not the allocation), so it stays valid
        // once the box is owned by the loader's `live` map below.
        let loader_data_ptr: *const JsLoaderData = loader_data.as_ptr();

        let contract_id: GuestContractId = GuestContractId::from_u64(registration_data.contract_id);
        let major_version: u32 = registration_data.contract_version >> 16;

        let plugin_interface: GuestContractInterface = GuestContractInterface {
            contract_id,
            contract_version: Version {
                major: major_version,
                minor: 0,
                patch: 0,
            },
            dispatch_type: DispatchType::VirtualMachine,
            create_instance: js_create_instance,
            destroy_instance: js_destroy_instance,
            dispatch: DispatchMechanisms {
                vm: VmDispatch {
                    call: js_dispatch,
                    loader_data: VmLoaderData {
                        data: loader_data_ptr as *mut JsLoaderData as *mut core::ffi::c_void,
                    },
                },
            },
        };

        // register_guest_contract COPIES every field into the registry's own
        // `Arc<GuestContractInterface>` during the synchronous call (the copy's
        // `dispatch.vm.bridge_data` still points at our owned `JsLoaderData` box).
        // The registry never retains this pointer, so a stack value valid for the
        // call is sufficient — no leak, which keeps a load→unload→load loop bounded.
        let interface_for_reg: GuestContractInterface = plugin_interface;
        let static_interface: *const GuestContractInterface =
            &interface_for_reg as *const GuestContractInterface;

        // The contract name's StringView is copied into an owned String by the
        // registry during the call, so a stack-owned String suffices — no leak.
        let contract_name_owned: String = registration_data.contract_name;
        let descriptor: PluginDescriptor = PluginDescriptor {
            name: StringView::from_static(b"js-quickjs-plugin"),
            contract_name: StringView {
                ptr: contract_name_owned.as_ptr(),
                len: contract_name_owned.len(),
            },
            version: Version {
                major: major_version,
                minor: 0,
                patch: 0,
            },
        };

        // SAFETY: host_interface, descriptor, and static_interface are valid for this call.
        // The register_guest_contract function uses self-passing pattern.
        let abi_result: AbiError = unsafe {
            ((*host_interface).register_guest_contract)(
                host_interface,
                &descriptor,
                static_interface,
            )
        };

        if !abi_result.is_ok() {
            // The registry copy made during register_guest_contract may already point
            // at this box's heap address; retire it (keep it alive) rather than
            // dropping it here, which would dangle the registry's bridge_data.
            self.retire_vm_state(vec![loader_data]);
            return Err(RuntimeError::Loader(LoaderError::InitFailed {
                bundle: manifest.name.clone(),
                error: format!(
                    "JS runtime js-quickjs error: register_guest_contract returned error code {:?}",
                    abi_result.code
                ),
            }));
        }

        // Take ownership of this bundle's VM state. Reload appends to any existing
        // entry instead of replacing it, so a superseded VM stays alive for an
        // in-flight dispatch (retire-not-drop across reload).
        let mut live: std::sync::MutexGuard<'_, HashMap<BundleId, Vec<SendVm>>> =
            self.live.lock().unwrap_or_else(PoisonError::into_inner);
        live.entry(BundleId::from_u64(bundle_id))
            .or_default()
            .push(loader_data);

        Ok(())
    }

    /// Move per-bundle VM state into the loader's `retired` list, keeping it alive
    /// for the loader's lifetime. Used when a box must not be dropped because the
    /// registry (or an in-flight dispatch) still references its heap address.
    fn retire_vm_state(&self, mut state: Vec<SendVm>) {
        if state.is_empty() {
            return;
        }
        let mut retired: std::sync::MutexGuard<'_, Vec<SendVm>> =
            self.retired.lock().unwrap_or_else(PoisonError::into_inner);
        retired.append(&mut state);
    }

    /// Number of live VM-state entries currently owned for `bundle_id`.
    #[cfg(test)]
    fn live_vm_count(&self, bundle_id: BundleId) -> usize {
        let live: std::sync::MutexGuard<'_, HashMap<BundleId, Vec<SendVm>>> =
            self.live.lock().unwrap_or_else(PoisonError::into_inner);
        live.get(&bundle_id).map(Vec::len).unwrap_or(0)
    }

    /// Number of VM-state entries retired (deferred reclaim) by this loader.
    #[cfg(test)]
    fn retired_vm_count(&self) -> usize {
        let retired: std::sync::MutexGuard<'_, Vec<SendVm>> =
            self.retired.lock().unwrap_or_else(PoisonError::into_inner);
        retired.len()
    }
}

impl BundleLoader for JsLoader {
    fn runtime_name(&self) -> &'static str {
        "js-quickjs"
    }

    fn load(
        &self,
        manifest: &ManifestData,
        source: &BundleSource,
        runtime: &PolyplugRuntime,
    ) -> Result<(), RuntimeError> {
        match source {
            // On-disk source: read bundle.js from the bundle directory and eval it,
            // provisioning bundlePath/bundle_path from that directory.
            BundleSource::Path(_) => {
                let bundle_js: String = JsLoader::read_path_source(manifest)?;
                self.load_inner(manifest, &bundle_js, Some(&manifest.path), runtime)
            }
            // In-memory JS source text: eval it directly in the bundle's fresh
            // QuickJS Context, exactly as the Path flow evals the entry file's
            // contents. There is no bundle directory — JS bundles are always one
            // flat, self-contained bundle.js, so this is a natural fit.
            BundleSource::Code(code) => self.load_inner(manifest, code, None, runtime),
            // Raw bytes: validate UTF-8, then take the same path as Code. JS source
            // must be valid UTF-8 text; invalid bytes are a structured error.
            BundleSource::Bytes(bytes) => {
                let code: &str = core::str::from_utf8(bytes).map_err(|_| {
                    RuntimeError::Loader(LoaderError::InvalidSourceEncoding {
                        loader: "js-quickjs",
                        source_kind: source.kind(),
                        bundle: manifest.name.clone(),
                    })
                })?;
                self.load_inner(manifest, code, None, runtime)
            }
        }
    }

    fn reload(
        &self,
        manifest: &ManifestData,
        runtime: &PolyplugRuntime,
    ) -> Result<(), RuntimeError> {
        if !runtime.config().hot_reload_enabled {
            return Err(RuntimeError::HotReloadDisabled);
        }
        // reload is path-based (the watcher only tracks on-disk bundles).
        let bundle_js: String = JsLoader::read_path_source(manifest)?;
        self.load_inner(manifest, &bundle_js, Some(&manifest.path), runtime)
    }

    /// Reclaim the bundle's QuickJS VM at a quiescence point.
    ///
    /// Called by the runtime AFTER `invalidate_bundle` has removed the bundle from
    /// the registry, so no dispatch can *resolve* this contract anew.
    ///
    /// # Host-coordination contract (best-effort quiescence)
    /// `call_guest_method` deliberately releases the registry lock between resolving
    /// an interface and the moment this VM registers the call in
    /// `in_dispatch_threads` (runtime.rs — no lock is held across guest dispatch, for
    /// concurrency/reentrancy). A call that resolved *just before* `invalidate_bundle`
    /// is therefore not guaranteed to be visible in `in_dispatch_threads` here. So,
    /// exactly like hot-reload, the host MUST NOT call a bundle's contracts
    /// concurrently with unloading it (see `Runtime::unload_bundle` and the
    /// trusted-same-process model in TRUST_MODEL.md). `in_dispatch_threads` is a
    /// best-effort defense-in-depth, not a complete guarantee.
    ///
    /// For each `JsLoaderData` owned by the bundle:
    /// - if its `in_dispatch_threads` is EMPTY (the expected case when the host has
    ///   honored the contract), the box is dropped here, dropping its QuickJS
    ///   `Context` and `Runtime` — true reclaim;
    /// - if it is NON-EMPTY (a dispatch is visibly in flight on another thread),
    ///   dropping the box would free the VM out from under that dispatch (a UAF), so
    ///   the box is moved into the loader-owned `retired` list instead and a single
    ///   line is logged. Reclaim is deferred; the VM stays alive for the loader's
    ///   lifetime.
    ///
    /// Spin-waiting is deliberately NOT used: a same-thread re-entrant unload would
    /// deadlock against its own in-flight dispatch.
    fn unload(&self, bundle_id: BundleId, _runtime: &PolyplugRuntime) -> Result<(), RuntimeError> {
        let state: Vec<SendVm> = {
            let mut live: std::sync::MutexGuard<'_, HashMap<BundleId, Vec<SendVm>>> =
                self.live.lock().unwrap_or_else(PoisonError::into_inner);
            match live.remove(&bundle_id) {
                Some(v) => v,
                None => return Ok(()),
            }
        };

        for data in state {
            let in_flight: bool = {
                let threads: std::sync::MutexGuard<'_, Vec<ThreadId>> = data
                    .data()
                    .in_dispatch_threads
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                !threads.is_empty()
            };

            if in_flight {
                eprintln!(
                    "[polyplug_js] unload of bundle {:#x} deferred: a dispatch is in flight on this VM; retiring its state to avoid a use-after-free",
                    bundle_id.id()
                );
                self.retire_vm_state(vec![data]);
            } else {
                // Quiescent: dropping `data` drops the QuickJS Context and Runtime —
                // true reclaim.
                drop(data);
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use core::sync::atomic::AtomicUsize;
    use core::sync::atomic::Ordering;
    use std::sync::Arc;
    use std::sync::Barrier;

    use super::*;

    #[test]
    fn js_quickjs_runtime_name() {
        let loader: JsLoader = JsLoader::new(JsConfig {});
        assert_eq!(loader.runtime_name(), "js-quickjs");
    }

    /// Minimal JS bundle registering one contract with a single no-op function.
    fn unload_bundle_js(contract_id: u64, contract_name: &str) -> String {
        let contract_lo: u32 = contract_id as u32;
        let contract_hi: u32 = (contract_id >> 32) as u32;
        format!(
            r#"
function polyplug_init(host_lo, host_hi, ctx_lo, ctx_hi) {{
    var vtable = {{ functions: [ function(args, out) {{ return 0; }} ] }};
    polyplug.registerVtable({contract_lo}, {contract_hi}, vtable, 1, "{contract_name}", 0x00010000);
}}
"#
        )
    }

    /// Build a temp JS bundle directory + ManifestData for the `test.unload@1`
    /// contract and return both (dir kept alive).
    fn write_unload_bundle(name: &str) -> (tempfile::TempDir, ManifestData) {
        let contract_id: u64 = polyplug_utils::guest_contract_id("test.unload", 1);
        let dir: tempfile::TempDir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("bundle.js"),
            unload_bundle_js(contract_id, "test.unload@1"),
        )
        .expect("write bundle.js");
        let manifest: ManifestData = ManifestData {
            id: polyplug_utils::bundle_id(name),
            name: name.to_owned(),
            runtime: "js-quickjs".to_owned(),
            file: "bundle.js".to_owned(),
            path: dir.path().to_path_buf(),
            version: String::new(),
            provides: Vec::new(),
            function_count: HashMap::new(),
            dependencies: Vec::new(),
            needs_reinit_on_dep_reload: false,
            bundle_dependencies: Vec::new(),
        };
        (dir, manifest)
    }

    /// A quiescent unload (no in-flight dispatch) drops the bundle's VM state from
    /// the loader's owned map — true reclaim — and does not retire anything.
    #[test]
    fn unload_quiescent_reclaims_vm_state() {
        let loader: JsLoader = JsLoader::new(JsConfig {});
        let runtime: Arc<PolyplugRuntime> = polyplug::runtime::RuntimeBuilder::new()
            .loader(JsLoader::new(JsConfig {}))
            .build()
            .expect("runtime build must succeed");
        let (_dir, manifest): (tempfile::TempDir, ManifestData) =
            write_unload_bundle("js_unload_quiescent");
        let bundle_id: BundleId = BundleId::from_u64(manifest.id);

        loader
            .load(
                &manifest,
                &BundleSource::Path(manifest.path.clone()),
                &runtime,
            )
            .expect("load must succeed");
        assert_eq!(
            loader.live_vm_count(bundle_id),
            1,
            "the bundle's VM state must be owned after load"
        );

        loader
            .unload(bundle_id, &runtime)
            .expect("unload must succeed");
        assert_eq!(
            loader.live_vm_count(bundle_id),
            0,
            "quiescent unload must drop the bundle's VM state (true reclaim)"
        );
        assert_eq!(
            loader.retired_vm_count(),
            0,
            "quiescent unload must NOT retire any state"
        );
    }

    /// A load→unload→load loop on the same bundle must not grow the loader's live
    /// map unboundedly: reclaim keeps memory bounded at one entry.
    #[test]
    fn unload_load_loop_is_bounded() {
        let loader: JsLoader = JsLoader::new(JsConfig {});
        let runtime: Arc<PolyplugRuntime> = polyplug::runtime::RuntimeBuilder::new()
            .loader(JsLoader::new(JsConfig {}))
            .build()
            .expect("runtime build must succeed");
        let (_dir, manifest): (tempfile::TempDir, ManifestData) =
            write_unload_bundle("js_unload_loop");
        let bundle_id: BundleId = BundleId::from_u64(manifest.id);

        for _ in 0..5 {
            loader
                .load(
                    &manifest,
                    &BundleSource::Path(manifest.path.clone()),
                    &runtime,
                )
                .expect("load must succeed");
            assert_eq!(
                loader.live_vm_count(bundle_id),
                1,
                "live map must hold exactly one entry per load"
            );
            loader
                .unload(bundle_id, &runtime)
                .expect("unload must succeed");
            assert_eq!(
                loader.live_vm_count(bundle_id),
                0,
                "unload must reclaim the entry each iteration"
            );
        }
        assert_eq!(
            loader.retired_vm_count(),
            0,
            "no iteration should have deferred reclaim"
        );
    }

    /// When a dispatch is in flight (the bundle's `in_dispatch_threads` is marked
    /// non-empty), unload must DEFER reclaim: the state is moved to the retired list
    /// (kept alive, no UAF) rather than dropped.
    #[test]
    fn unload_non_quiescent_defers_reclaim() {
        let loader: JsLoader = JsLoader::new(JsConfig {});
        let runtime: Arc<PolyplugRuntime> = polyplug::runtime::RuntimeBuilder::new()
            .loader(JsLoader::new(JsConfig {}))
            .build()
            .expect("runtime build must succeed");
        let (_dir, manifest): (tempfile::TempDir, ManifestData) =
            write_unload_bundle("js_unload_deferred");
        let bundle_id: BundleId = BundleId::from_u64(manifest.id);

        loader
            .load(
                &manifest,
                &BundleSource::Path(manifest.path.clone()),
                &runtime,
            )
            .expect("load must succeed");

        // Simulate an in-flight dispatch by registering a fake thread id in the
        // bundle's tracking vec — exactly the state the dispatch guard leaves while
        // a call is mid-flight on another thread.
        {
            let live: std::sync::MutexGuard<'_, HashMap<BundleId, Vec<SendVm>>> =
                loader.live.lock().unwrap_or_else(PoisonError::into_inner);
            let state: &Vec<SendVm> = live.get(&bundle_id).expect("bundle must be live");
            let mut threads: std::sync::MutexGuard<'_, Vec<ThreadId>> = state[0]
                .data()
                .in_dispatch_threads
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            threads.push(std::thread::current().id());
        }

        loader
            .unload(bundle_id, &runtime)
            .expect("unload must succeed even when non-quiescent");
        assert_eq!(
            loader.live_vm_count(bundle_id),
            0,
            "unload must remove the bundle from the live map"
        );
        assert_eq!(
            loader.retired_vm_count(),
            1,
            "non-quiescent unload must RETIRE the state (deferred reclaim), not drop it"
        );
    }

    /// Build a leaked JsLoaderData holding the given runtime/context and the
    /// persisted functions, returning a VmLoaderData pointing at it plus a
    /// borrow for direct flag inspection.
    ///
    /// The data is intentionally leaked so the raw pointer inside VmLoaderData
    /// stays valid for the whole test, mirroring the loader's `Box::into_raw`.
    fn make_loader_data(
        runtime: Runtime,
        ctx: Context,
        functions: Vec<Persistent<Function<'static>>>,
    ) -> (VmLoaderData, &'static JsLoaderData) {
        let boxed: Box<JsLoaderData> = Box::new(JsLoaderData {
            _runtime: runtime,
            ctx,
            functions,
            in_dispatch_threads: Mutex::new(Vec::new()),
        });
        let ptr: *mut JsLoaderData = Box::into_raw(boxed);
        // SAFETY: ptr was just produced by Box::into_raw and is never freed in the
        // test, so the &'static borrow is valid for the test's lifetime.
        let data_ref: &'static JsLoaderData = unsafe { &*ptr };
        let vm_loader_data: VmLoaderData = VmLoaderData {
            data: ptr as *mut core::ffi::c_void,
        };
        (vm_loader_data, data_ref)
    }

    /// A normal (non-reentrant) JS dispatch succeeds and clears the flag.
    #[test]
    fn js_dispatch_normal_call_succeeds() {
        let runtime: Runtime = Runtime::new().expect("runtime creation should succeed");
        let ctx: Context = Context::full(&runtime).expect("context creation should succeed");

        // A trivial guest function: ignores its (args, out) arguments, returns 0 (Ok).
        let func: Persistent<Function<'static>> = ctx.with(|ctx_ref: Ctx<'_>| {
            let f: Function<'_> = ctx_ref
                .eval::<Function<'_>, _>("(function(a, o) { return 0; })")
                .expect("eval should produce a function");
            Persistent::save(&ctx_ref, f)
        });

        let (vm_loader_data, data_ref): (VmLoaderData, &'static JsLoaderData) =
            make_loader_data(runtime, ctx, vec![func]);

        let mut out_buf: i32 = 0;
        // SAFETY: vm_loader_data wraps a live JsLoaderData; the guest function
        // ignores the forwarded args/out pointers.
        let err: AbiError = unsafe {
            js_dispatch(
                vm_loader_data,
                GuestContractInstance::null(),
                0,
                core::ptr::null(),
                &mut out_buf as *mut i32 as *mut (),
                core::ptr::null_mut(),
            )
        };
        assert!(err.is_ok(), "normal dispatch should return Ok");
        assert!(
            data_ref
                .in_dispatch_threads
                .lock()
                .expect("tracking mutex must not be poisoned")
                .is_empty(),
            "thread tracking must be empty after a normal dispatch"
        );
    }

    /// A genuine same-VM reentrant dispatch — triggered from inside a guest call
    /// via a native `reenter` bridge — returns ReentrantCall, and the VM stays
    /// usable for a later normal dispatch.
    #[test]
    fn js_dispatch_reentrant_call_is_rejected_and_vm_recovers() {
        let runtime: Runtime = Runtime::new().expect("runtime creation should succeed");
        let ctx: Context = Context::full(&runtime).expect("context creation should succeed");

        // The loader_data pointer is shared into the native bridge via an
        // Arc<AtomicUsize>; it is set after construction (below) and read inside.
        let loader_data_cell: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
        let cell_for_fn: Arc<AtomicUsize> = Arc::clone(&loader_data_cell);

        // Register a native `reenter` function that, while the outer dispatch is in
        // flight (flag set, Context::with held), re-invokes js_dispatch on the SAME
        // loader_data — simulating a plugin→plugin cross-call back into this VM.
        // It returns the nested call's AbiError code as f64 so JS can observe it.
        let func: Persistent<Function<'static>> = ctx.with(|ctx_ref: Ctx<'_>| {
            let reenter_fn: Function<'_> = Function::new(ctx_ref.clone(), move || -> f64 {
                let ptr_usize: usize = cell_for_fn.load(Ordering::Acquire);
                let vm_loader_data: VmLoaderData = VmLoaderData {
                    data: ptr_usize as *mut core::ffi::c_void,
                };
                // SAFETY: the cell holds the live leaked JsLoaderData pointer set up
                // by the test before dispatch; the guest ignores the forwarded
                // args/out pointers.
                let nested: AbiError = unsafe {
                    js_dispatch(
                        vm_loader_data,
                        GuestContractInstance::null(),
                        0,
                        core::ptr::null(),
                        core::ptr::null_mut(),
                        core::ptr::null_mut(),
                    )
                };
                nested.code as f64
            })
            .expect("reenter function creation should succeed");
            ctx_ref
                .globals()
                .set("reenter", reenter_fn)
                .expect("reenter global set should succeed");

            // The guest function calls reenter(), stashes the nested code on a global
            // so the test can read it, and returns 0 (Ok) for the outer call.
            let f: Function<'_> = ctx_ref
                .eval::<Function<'_>, _>(
                    "(function(a, o) { globalThis._nestedCode = reenter(); return 0; })",
                )
                .expect("eval should produce a function");
            Persistent::save(&ctx_ref, f)
        });

        let (vm_loader_data, data_ref): (VmLoaderData, &'static JsLoaderData) =
            make_loader_data(runtime, ctx, vec![func]);
        loader_data_cell.store(vm_loader_data.data as usize, Ordering::Release);

        // Outer dispatch: sets the flag, enters Context::with, runs the guest fn,
        // which calls reenter() → js_dispatch on the same VM.
        // SAFETY: vm_loader_data wraps the live leaked JsLoaderData.
        let outer: AbiError = unsafe {
            js_dispatch(
                vm_loader_data,
                GuestContractInstance::null(),
                0,
                core::ptr::null(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
            )
        };
        assert!(outer.is_ok(), "outer dispatch should complete Ok");

        // The nested dispatch must have been rejected with ReentrantCall.
        let nested_code: f64 = data_ref.ctx.with(|ctx_ref: Ctx<'_>| {
            ctx_ref
                .globals()
                .get::<&str, f64>("_nestedCode")
                .expect("nested code global must be set by the guest fn")
        });
        assert_eq!(
            nested_code as u32,
            AbiErrorCode::ReentrantCall as u32,
            "nested same-VM dispatch must return ReentrantCall"
        );

        // The tracking is cleared and the VM is still usable for a fresh dispatch.
        assert!(
            data_ref
                .in_dispatch_threads
                .lock()
                .expect("tracking mutex must not be poisoned")
                .is_empty(),
            "thread tracking must be empty after the outer dispatch returns"
        );
        // SAFETY: vm_loader_data still wraps the live leaked JsLoaderData.
        let recovered: AbiError = unsafe {
            js_dispatch(
                vm_loader_data,
                GuestContractInstance::null(),
                0,
                core::ptr::null(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
            )
        };
        assert!(
            recovered.is_ok(),
            "VM must remain usable after a rejected reentrant call"
        );
    }

    /// A concurrent dispatch from ANOTHER thread into the same Context must
    /// SUCCEED, not be rejected as reentrancy. The thread-aware guard only refuses
    /// a same-thread nested call; a cross-thread caller proceeds and rquickjs's
    /// internal `parallel` lock serializes the two calls.
    ///
    /// Choreography proves a true in-flight overlap: thread A's guest fn (running
    /// inside `Context::with`, holding the rquickjs lock) registers thread A in the
    /// tracking vec, then a native `block` bridge parks on a barrier. While A is
    /// parked mid-dispatch, the main thread (a different thread) dispatches into the
    /// SAME Context. That call passes the reentrancy check (different thread id) and
    /// blocks on rquickjs's internal lock held by A. Releasing the barrier lets A
    /// finish, freeing the lock so the concurrent call completes with Ok.
    #[test]
    fn js_dispatch_cross_thread_concurrent_call_succeeds() {
        let runtime: Runtime = Runtime::new().expect("runtime creation should succeed");
        let ctx: Context = Context::full(&runtime).expect("context creation should succeed");

        // `entered` lets the main thread know A is mid-dispatch (lock held);
        // `release` lets A finish only after the main thread launched its call.
        let entered: Arc<Barrier> = Arc::new(Barrier::new(2));
        let release: Arc<Barrier> = Arc::new(Barrier::new(2));
        let entered_for_fn: Arc<Barrier> = Arc::clone(&entered);
        let release_for_fn: Arc<Barrier> = Arc::clone(&release);

        // Native `block` bridge: signals A is inside the dispatch, then parks until
        // released. It runs inside Context::with, so the rquickjs lock is held.
        let func: Persistent<Function<'static>> = ctx.with(|ctx_ref: Ctx<'_>| {
            let block_fn: Function<'_> = Function::new(ctx_ref.clone(), move || {
                entered_for_fn.wait();
                release_for_fn.wait();
            })
            .expect("block function creation should succeed");
            ctx_ref
                .globals()
                .set("block", block_fn)
                .expect("block global set should succeed");

            let f: Function<'_> = ctx_ref
                .eval::<Function<'_>, _>("(function(a, o) { block(); return 0; })")
                .expect("eval should produce a function");
            Persistent::save(&ctx_ref, f)
        });
        // The concurrent caller dispatches THIS function (fn_id 1) — it must not
        // touch the barriers, or it would park with no partner and deadlock.
        let noop_func: Persistent<Function<'static>> = ctx.with(|ctx_ref: Ctx<'_>| {
            let f: Function<'_> = ctx_ref
                .eval::<Function<'_>, _>("(function(a, o) { return 0; })")
                .expect("eval should produce a function");
            Persistent::save(&ctx_ref, f)
        });

        let (vm_loader_data, data_ref): (VmLoaderData, &'static JsLoaderData) =
            make_loader_data(runtime, ctx, vec![func, noop_func]);

        // VmLoaderData is a thin pointer wrapper; move its address across the
        // thread boundary as a usize to satisfy Send, then rebuild it inside.
        let data_addr: usize = vm_loader_data.data as usize;

        let handle: std::thread::JoinHandle<AbiError> = std::thread::spawn(move || {
            let vm_loader_data_a: VmLoaderData = VmLoaderData {
                data: data_addr as *mut core::ffi::c_void,
            };
            // SAFETY: data_addr is the live leaked JsLoaderData pointer; it
            // outlives all threads in this test. The guest fn ignores its args.
            unsafe {
                js_dispatch(
                    vm_loader_data_a,
                    GuestContractInstance::null(),
                    0,
                    core::ptr::null(),
                    core::ptr::null_mut(),
                    core::ptr::null_mut(),
                )
            }
        });

        // Wait until thread A is confirmed inside its dispatch.
        entered.wait();

        // From THIS (different) thread, dispatch into the SAME Context. This must
        // not be rejected; it blocks on rquickjs's lock until A releases it.
        let main_handle: std::thread::JoinHandle<AbiError> = std::thread::spawn(move || {
            let vm_loader_data_b: VmLoaderData = VmLoaderData {
                data: data_addr as *mut core::ffi::c_void,
            };
            // SAFETY: same live leaked pointer as above. fn_id 1 is the no-op
            // function — dispatching fn_id 0 here would re-enter the barrier
            // choreography with no partner and deadlock.
            unsafe {
                js_dispatch(
                    vm_loader_data_b,
                    GuestContractInstance::null(),
                    1,
                    core::ptr::null(),
                    core::ptr::null_mut(),
                    core::ptr::null_mut(),
                )
            }
        });

        // Unblock thread A so it finishes and frees the lock, allowing the
        // concurrent dispatch to complete.
        release.wait();

        let a_result: AbiError = handle.join().expect("thread A must not panic");
        let b_result: AbiError = main_handle
            .join()
            .expect("concurrent thread must not panic");

        assert!(a_result.is_ok(), "the initial dispatch must succeed");
        assert!(
            b_result.is_ok(),
            "a concurrent cross-thread dispatch must succeed, not return ReentrantCall (got code {})",
            b_result.code
        );
        assert!(
            data_ref
                .in_dispatch_threads
                .lock()
                .expect("tracking mutex must not be poisoned")
                .is_empty(),
            "thread tracking must be empty after both dispatches return"
        );
    }
}

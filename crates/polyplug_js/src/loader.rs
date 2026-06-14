//! QuickJS in-process plugin loader implementation.
//!
//! Loads JS plugin bundles via the embedded QuickJS VM (rquickjs).
//! Each bundle gets its own QuickJS Runtime and Context for complete isolation
//! between bundles and between polyplug Runtime instances.
//! Uses VM dispatch to call JS functions through the QuickJS API.

use core::sync::atomic::AtomicU64;
use core::sync::atomic::Ordering;
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
use polyplug::loader::BundleLoader;
use polyplug::loader::BundleSource;
use polyplug::loader::ManifestData;
use polyplug::logger::LoggerHandle;
use polyplug_abi::AbiError;
use polyplug_abi::AbiErrorCode;
use polyplug_abi::BundleInitContext;
use polyplug_abi::CallArena;
use polyplug_abi::DispatchType;
use polyplug_abi::GuestContractHandle;
use polyplug_abi::GuestContractInstance;
use polyplug_abi::GuestContractInterface;
use polyplug_abi::HostApi;
use polyplug_abi::HostContractInstance;
use polyplug_abi::HostContractInterface;
use polyplug_abi::PluginDescriptor;
use polyplug_abi::StringView;
use polyplug_abi::SupportedLanguage;
use polyplug_abi::VmLoaderData;
use polyplug_abi::dispatch::dispatch_mechanisms::DispatchMechanisms;
use polyplug_abi::dispatch::vm_dispatch::VmDispatch;
use polyplug_abi::types::LogLevel;
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
    /// Instance-owned copy of the runtime's logger, taken at load time.
    ///
    /// Dispatch-time diagnostics have no `&Runtime` back-reference, so the
    /// per-VM data carries its own `Copy` of the handle. Same callback
    /// contract as `RuntimeConfig::log` — never invoked under a lock guard.
    pub logger: LoggerHandle,
}

/// Owning, thread-shareable handle to a bundle's [`JsLoaderData`].
///
/// `JsLoaderData` is `!Send`/`!Sync` because `Persistent<Function>` carries a raw
/// `*mut JSRuntime`. The previous design laundered this by leaking the box behind a
/// raw `*mut c_void` inside `VmLoaderData`; this newtype instead owns the box so it
/// can be dropped on unload, while restoring `Send + Sync` so the loader's `live`
/// collection (and therefore `JsLoader`) stays `Send + Sync` and a box can be moved
/// into the `Send + 'static` epoch-deferred reclaim closure on unload.
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
    ///
    /// Test-only since unload became uniform epoch-deferred reclaim: production code no
    /// longer inspects per-VM state at unload time (the epoch governs liveness), so the
    /// only remaining caller is the in-flight-marking unit test.
    #[cfg(test)]
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
    out_instance: *mut GuestContractInstance,
) {
    if !out_instance.is_null() {
        // SAFETY: out_instance is non-null (just checked) and writable per the ABI contract.
        unsafe { out_instance.write(GuestContractInstance::null()) };
    }
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
    out_err: *mut AbiError,
) {
    // SAFETY: loader_data wraps a valid pointer to JsLoaderData created by the
    // loader; args/out/arena satisfy the ABI dispatch contract for this call.
    let result: AbiError = unsafe { js_dispatch_impl(loader_data, fn_id, args, out, arena) };
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(result) };
    }
}

unsafe fn js_dispatch_impl(
    loader_data: VmLoaderData,
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
        Err(e) => {
            // Context::with has returned, so the QuickJS VM lock is released and
            // no loader lock guard is held — safe to invoke the host logger.
            data.logger.log(LogLevel::Error, "loader.js", || {
                format!("JS function call failed: {e}")
            });
            AbiError {
                code: AbiErrorCode::Generic as u32,
                message: StringView::null(),
            }
        }
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

/// Destroy a host-contract instance obtained from `get_host_contract` when the
/// contract is NOT a singleton.
///
/// `get_host_contract` mints a fresh instance per call for multi-instance
/// contracts; the caller owns it and must destroy it or it leaks. Singleton
/// contracts return a runtime-cached instance that must never be destroyed here.
///
/// # Safety
/// `iface` must be the valid, non-null `HostContractInterface` pointer that produced
/// `instance`; `instance` must be the value just returned by `get_host_contract` for
/// this `iface`.
unsafe fn destroy_host_instance_if_needed(
    iface: *const HostContractInterface,
    singleton: bool,
    instance: HostContractInstance,
) {
    if singleton {
        return;
    }
    // SAFETY: iface is a valid non-null HostContractInterface pointer (checked by the
    // caller); destroy_instance follows the self-passing ABI (its first argument is the
    // interface pointer), and `instance` is the value get_host_contract returned for it.
    unsafe { ((*iface).destroy_instance)(iface, instance) };
}

fn register_host_functions<'js>(
    ctx: &Ctx<'js>,
    polyplug_obj: &Object<'js>,
    host_interface: *const HostApi,
    bundle_name: &str,
    logger: LoggerHandle,
) -> Result<(), LoaderError> {
    // Store host interface pointer as JS globals on the polyplug object
    let host_interface_usize: usize = host_interface as usize;

    // Store as f64: u32 > INT32_MAX would be sign-extended by rquickjs to a negative
    // tagged int, causing f64::from_js or u32::from_js to fail on read-back.
    polyplug_obj
        .set("_hostVtableLo", (host_interface_usize as u32) as f64)
        .map_err(|e: rquickjs::Error| LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("JS runtime js-quickjs error: _hostVtableLo set failed: {e}"),
        })?;
    polyplug_obj
        .set(
            "_hostVtableHi",
            ((host_interface_usize >> 32) as u32) as f64,
        )
        .map_err(|e: rquickjs::Error| LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("JS runtime js-quickjs error: _hostVtableHi set failed: {e}"),
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
    .map_err(|e: rquickjs::Error| LoaderError::InitFailed {
        bundle: bundle_name.to_owned(),
        error: format!("JS runtime js-quickjs error: findByContract function creation failed: {e}"),
    })?;

    polyplug_obj
        .set("findByContract", find_by_contract_fn)
        .map_err(|e: rquickjs::Error| LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("JS runtime js-quickjs error: findByContract set failed: {e}"),
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
    .map_err(|e: rquickjs::Error| LoaderError::InitFailed {
        bundle: bundle_name.to_owned(),
        error: format!("JS runtime js-quickjs error: findByBundle function creation failed: {e}"),
    })?;

    polyplug_obj
        .set("findByBundle", find_by_bundle_fn)
        .map_err(|e: rquickjs::Error| LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("JS runtime js-quickjs error: findByBundle set failed: {e}"),
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
    .map_err(|e: rquickjs::Error| LoaderError::InitFailed {
        bundle: bundle_name.to_owned(),
        error: format!(
            "JS runtime js-quickjs error: findAllByContract function creation failed: {e}"
        ),
    })?;

    polyplug_obj
        .set("findAllByContract", find_all_by_contract_fn)
        .map_err(|e: rquickjs::Error| LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("JS runtime js-quickjs error: findAllByContract set failed: {e}"),
        })?;

    let resolve_guest_contract_fn: Function<'js> =
        Function::new(ctx.clone(), |ctx: Ctx<'js>, packed: u64| -> Option<u64> {
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
        })
        .map_err(|e: rquickjs::Error| LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!(
                "JS runtime js-quickjs error: resolveGuestContract function creation failed: {e}"
            ),
        })?;

    polyplug_obj
        .set("resolveGuestContract", resolve_guest_contract_fn)
        .map_err(|e: rquickjs::Error| LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("JS runtime js-quickjs error: resolveGuestContract set failed: {e}"),
        })?;

    // ── callGuestMethod ────────────────────────────────────────────────────────
    // Guarded peer-call path: find → resolve → create_instance → call_guest_method.
    // The contract_id and min_version come from the generated peer_callers.ts
    // constants so the caller never hard-codes raw numbers.
    //
    // Per-call create+destroy is intentional: the stateless contract model used
    // by all examples today has null instance.data and treats destroy_instance as
    // a no-op.  Stateful peers would need a retained instance-handle API (out of
    // scope for now; the bridge primitive is the right place to add it later).
    let call_guest_method_fn: Function<'js> = Function::new(
        ctx.clone(),
        |ctx: Ctx<'js>,
         contract_id_lo: u32,
         contract_id_hi: u32,
         min_version: u32,
         fn_id: u32,
         args_ptr: u64,
         out_ptr: u64|
         -> u32 {
            let contract_id: u64 = (contract_id_hi as u64) << 32 | contract_id_lo as u64;
            let hvt: *const HostApi = match get_host_interface_from_globals(&ctx) {
                Some(p) => p,
                None => return AbiErrorCode::Generic as u32,
            };
            // SAFETY: hvt is a valid 'static HostApi pointer stored by register_host_functions;
            // the self-passing ABI pattern is satisfied by passing hvt as the first argument.
            let handle: GuestContractHandle =
                unsafe { ((*hvt).find_guest_contract)(hvt, contract_id, min_version) };
            // SAFETY: hvt is valid (same guarantee as above); handle was just returned by
            // find_guest_contract so it is a well-formed GuestContractHandle.
            let iface: *const GuestContractInterface =
                unsafe { ((*hvt).resolve_guest_contract)(hvt, handle) };
            if iface.is_null() {
                return AbiErrorCode::NotFound as u32;
            }
            let mut instance: GuestContractInstance = GuestContractInstance::null();
            // SAFETY: iface is non-null and points to a valid GuestContractInterface
            // returned by resolve_guest_contract; null ctx is accepted for stateless contracts;
            // `instance` is a valid, writable out-param for the duration of the call.
            unsafe { ((*iface).create_instance)(hvt, core::ptr::null(), &mut instance) };
            // create_instance returns a null (null-id) handle for stateless/VM peers, but
            // host call_guest_method routes by instance.contract_id — stamp the id we resolved.
            instance.contract_id = GuestContractId::from_u64(contract_id);
            let mut err: AbiError = AbiError::ok();
            // SAFETY: hvt, instance, and iface are all valid; args_ptr/out_ptr are caller-supplied
            // addresses that the generated peer_callers.ts aligns via polyplug.alloc; a null arena
            // is the documented fallback for callers that carry no per-call arena; `err` is a
            // valid, writable out-param for the duration of the call.
            unsafe {
                ((*hvt).call_guest_method)(
                    hvt,
                    instance,
                    fn_id,
                    args_ptr as usize as *const core::ffi::c_void,
                    out_ptr as usize as *mut core::ffi::c_void,
                    core::ptr::null_mut(),
                    &mut err,
                )
            };
            // SAFETY: iface is non-null (checked above); instance was produced by create_instance
            // on this same interface.  Stateless contracts treat destroy_instance as a no-op.
            // Best-effort: stateful peers are out of scope for this bridge primitive.
            unsafe { ((*iface).destroy_instance)(hvt, instance) };
            err.code
        },
    )
    .map_err(|e: rquickjs::Error| LoaderError::InitFailed {
        bundle: bundle_name.to_owned(),
        error: format!(
            "JS runtime js-quickjs error: callGuestMethod function creation failed: {e}"
        ),
    })?;

    polyplug_obj
        .set("callGuestMethod", call_guest_method_fn)
        .map_err(|e: rquickjs::Error| LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("JS runtime js-quickjs error: callGuestMethod set failed: {e}"),
        })?;

    let register_vtable_fn: Function<'js> = Function::new(
        ctx.clone(),
        |ctx: Ctx<'js>,
         contract_lo: u32,
         contract_hi: u32,
         vtable_obj: Object<'js>,
         fn_count: u32,
         contract_name: String,
         contract_version: u32|
         -> Result<(), rquickjs::Error> {
            let contract_id: u64 = (contract_hi as u64) << 32 | contract_lo as u64;
            let fn_count_usize: usize = fn_count as usize;

            let mut functions: Vec<Persistent<Function<'static>>> =
                Vec::with_capacity(fn_count_usize);
            let functions_array: Object<'js> =
                match vtable_obj.get::<&str, Object<'js>>("functions") {
                    Ok(arr) => arr,
                    Err(_) => {
                        return Err(rquickjs::Exception::throw_message(
                            &ctx,
                            &format!(
                                "registerVtable: vtable for contract '{contract_name}' has no 'functions' array"
                            ),
                        ));
                    }
                };

            for i in 0..fn_count_usize {
                let func: Function<'js> = match functions_array.get::<u32, Function<'js>>(i as u32)
                {
                    Ok(f) => f,
                    Err(_) => {
                        return Err(rquickjs::Exception::throw_message(
                            &ctx,
                            &format!(
                                "registerVtable: vtable for contract '{contract_name}' declares fnCount={fn_count} but functions[{i}] is missing or not a function"
                            ),
                        ));
                    }
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
                    None => {
                        return Err(rquickjs::Exception::throw_message(
                            &ctx,
                            "registerVtable: registration slot missing from VM userdata (loader bug)",
                        ));
                    }
                };
            let mut cell: core::cell::RefMut<Option<JsRegistrationData>> = slot_guard.borrow_mut();
            *cell = Some(data);
            Ok(())
        },
    )
    .map_err(|e: rquickjs::Error| {
        LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!(
                "JS runtime js-quickjs error: registerVtable function creation failed: {e}"
            ),
        }
    })?;

    polyplug_obj
        .set("registerVtable", register_vtable_fn)
        .map_err(|e: rquickjs::Error| LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("JS runtime js-quickjs error: registerVtable set failed: {e}"),
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
    .map_err(|e: rquickjs::Error| LoaderError::InitFailed {
        bundle: bundle_name.to_owned(),
        error: format!("JS runtime js-quickjs error: alloc function creation failed: {e}"),
    })?;

    polyplug_obj
        .set("alloc", alloc_fn)
        .map_err(|e: rquickjs::Error| LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("JS runtime js-quickjs error: alloc set failed: {e}"),
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
    .map_err(|e: rquickjs::Error| LoaderError::InitFailed {
        bundle: bundle_name.to_owned(),
        error: format!("JS runtime js-quickjs error: arenaAlloc function creation failed: {e}"),
    })?;

    polyplug_obj
        .set("arenaAlloc", arena_alloc_fn)
        .map_err(|e: rquickjs::Error| LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("JS runtime js-quickjs error: arenaAlloc set failed: {e}"),
        })?;

    // log(level, scope, message) delivers guest log records to the host's
    // logging funnel (`RuntimeConfig::log` callback or the stderr default)
    // through the instance-owned LoggerHandle copy captured at load time —
    // per-VM captured state, no statics (Rule 12). `level` is validated via
    // LogLevel::from_u32; non-integral, out-of-range, or unknown values clamp
    // to LogLevel::Error, matching HostApi.log semantics. `scope` and `message`
    // are delivered verbatim; the suggested scope convention is
    // "guest.<plugin-name>".
    //
    // Lock analysis (why logging mid-dispatch cannot deadlock): this bridge
    // runs inside guest code, i.e. while js_dispatch's Context::with holds
    // QuickJS's internal `parallel` VM lock on the calling thread
    // (in_dispatch_threads is NOT held there — it is released before
    // Context::with). That is sound: the only code that enters this VM is
    // js_dispatch / the loader's own load path, and the host logging callback
    // is contractually forbidden from re-entering the runtime
    // (`RuntimeBuilder::logger` / `RuntimeConfig::log` callback contract), so
    // no path from inside the callback can reach js_dispatch — or any other
    // loader entry point — and therefore none can attempt the VM lock or any
    // loader Mutex. No runtime lock is held across a guest dispatch either.
    let log_fn: Function<'js> = Function::new(
        ctx.clone(),
        move |level: f64, scope: String, message: String| {
            let log_level: LogLevel = if level.fract() == 0.0 {
                LogLevel::from_u32(level as u32).unwrap_or(LogLevel::Error)
            } else {
                LogLevel::Error
            };
            logger.log(log_level, &scope, || message);
        },
    )
    .map_err(|e: rquickjs::Error| LoaderError::InitFailed {
        bundle: bundle_name.to_owned(),
        error: format!("JS runtime js-quickjs error: log function creation failed: {e}"),
    })?;

    polyplug_obj
        .set("log", log_fn)
        .map_err(|e: rquickjs::Error| LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("JS runtime js-quickjs error: log set failed: {e}"),
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
    .map_err(|e: rquickjs::Error| LoaderError::InitFailed {
        bundle: bundle_name.to_owned(),
        error: format!("JS runtime js-quickjs error: free function creation failed: {e}"),
    })?;

    polyplug_obj
        .set("free", free_fn)
        .map_err(|e: rquickjs::Error| LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("JS runtime js-quickjs error: free set failed: {e}"),
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
    .map_err(|e: rquickjs::Error| LoaderError::InitFailed {
        bundle: bundle_name.to_owned(),
        error: format!("JS runtime js-quickjs error: readI32 function creation failed: {e}"),
    })?;

    polyplug_obj
        .set("readI32", read_i32_fn)
        .map_err(|e: rquickjs::Error| LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("JS runtime js-quickjs error: readI32 set failed: {e}"),
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
    .map_err(|e: rquickjs::Error| LoaderError::InitFailed {
        bundle: bundle_name.to_owned(),
        error: format!("JS runtime js-quickjs error: writeI32 function creation failed: {e}"),
    })?;

    polyplug_obj
        .set("writeI32", write_i32_fn)
        .map_err(|e: rquickjs::Error| LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("JS runtime js-quickjs error: writeI32 set failed: {e}"),
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
    .map_err(|e: rquickjs::Error| LoaderError::InitFailed {
        bundle: bundle_name.to_owned(),
        error: format!("JS runtime js-quickjs error: readByte function creation failed: {e}"),
    })?;

    polyplug_obj
        .set("readByte", read_byte_fn)
        .map_err(|e: rquickjs::Error| LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("JS runtime js-quickjs error: readByte set failed: {e}"),
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
    .map_err(|e: rquickjs::Error| LoaderError::InitFailed {
        bundle: bundle_name.to_owned(),
        error: format!("JS runtime js-quickjs error: writeByte function creation failed: {e}"),
    })?;

    polyplug_obj
        .set("writeByte", write_byte_fn)
        .map_err(|e: rquickjs::Error| LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("JS runtime js-quickjs error: writeByte set failed: {e}"),
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
    .map_err(|e: rquickjs::Error| LoaderError::InitFailed {
        bundle: bundle_name.to_owned(),
        error: format!("JS runtime js-quickjs error: readMemory function creation failed: {e}"),
    })?;

    polyplug_obj
        .set("readMemory", read_memory_fn)
        .map_err(|e: rquickjs::Error| LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("JS runtime js-quickjs error: readMemory set failed: {e}"),
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
    .map_err(|e: rquickjs::Error| LoaderError::InitFailed {
        bundle: bundle_name.to_owned(),
        error: format!("JS runtime js-quickjs error: readU32 function creation failed: {e}"),
    })?;

    polyplug_obj
        .set("readU32", read_u32_fn)
        .map_err(|e: rquickjs::Error| LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("JS runtime js-quickjs error: readU32 set failed: {e}"),
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
    .map_err(|e: rquickjs::Error| LoaderError::InitFailed {
        bundle: bundle_name.to_owned(),
        error: format!("JS runtime js-quickjs error: writeU32 function creation failed: {e}"),
    })?;

    polyplug_obj
        .set("writeU32", write_u32_fn)
        .map_err(|e: rquickjs::Error| LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("JS runtime js-quickjs error: writeU32 set failed: {e}"),
        })?;

    // ── callHostContract ──────────────────────────────────────────────────────
    // Dispatches a call to a host-provided contract service.  Resolves the
    // HostContractInterface, reads the dispatch type, and invokes either the
    // native function pointer or the VM call hook using the canonical host-caller
    // pattern (null GuestContractInstance, null arena).
    let call_host_contract_fn: Function<'js> = Function::new(
        ctx.clone(),
        |ctx: Ctx<'js>,
         contract_id_lo: u32,
         contract_id_hi: u32,
         min_version: u32,
         fn_id: u32,
         args_ptr: u64,
         out_ptr: u64|
         -> u32 {
            let contract_id: u64 = (contract_id_hi as u64) << 32 | contract_id_lo as u64;
            let hvt: *const HostApi = match get_host_interface_from_globals(&ctx) {
                Some(p) => p,
                None => return AbiErrorCode::Generic as u32,
            };
            // SAFETY: hvt is a valid 'static HostApi pointer stored by register_host_functions;
            // the self-passing ABI pattern is satisfied by passing hvt as the first argument.
            let iface: *const HostContractInterface =
                unsafe { ((*hvt).resolve_host_contract_interface)(hvt, contract_id, min_version) };
            if iface.is_null() {
                return AbiErrorCode::NotFound as u32;
            }
            // SAFETY: hvt is valid (same guarantee as above); hvt and contract_id/min_version
            // match the resolve call that just succeeded so get_host_contract returns a valid
            // HostContractInstance.
            let instance: HostContractInstance =
                unsafe { ((*hvt).get_host_contract)(hvt, contract_id, min_version) };
            let args: *const core::ffi::c_void = args_ptr as usize as *const core::ffi::c_void;
            let out: *mut core::ffi::c_void = out_ptr as usize as *mut core::ffi::c_void;
            // SAFETY: iface is non-null (checked above); `singleton` is a plain bool
            // field that is safe to read through a valid non-null pointer.
            let singleton: bool = unsafe { (*iface).singleton };
            // SAFETY: iface is non-null (checked above); `dispatch_type` is a plain field
            // that is safe to read through a valid non-null pointer.
            let dt: DispatchType = unsafe { (*iface).dispatch_type };
            let code: u32 = match dt {
                DispatchType::Native => {
                    // SAFETY: iface is non-null (checked); dispatch_type == Native guarantees
                    // the `native` union variant is active, so reading it is sound.
                    let native: polyplug_abi::NativeDispatch = unsafe { (*iface).dispatch.native };
                    // Bounds- and null-check the host function table before indexing it,
                    // mirroring host_call_guest_method in crates/polyplug/src/runtime.rs.
                    if native.functions.is_null() || fn_id >= native.function_count {
                        // SAFETY: iface produced `instance`; the helper only destroys a
                        // non-singleton instance, releasing it before we bail out.
                        unsafe { destroy_host_instance_if_needed(iface, singleton, instance) };
                        return AbiErrorCode::FunctionNotAvailable as u32;
                    }
                    // SAFETY: fn_id < function_count and functions is non-null, so the slot
                    // at fn_id is within the host's static function-pointer array.
                    let fn_ptr: *const () = unsafe { *native.functions.add(fn_id as usize) };
                    if fn_ptr.is_null() {
                        // SAFETY: iface produced `instance`; the helper only destroys a
                        // non-singleton instance, releasing it before we bail out.
                        unsafe { destroy_host_instance_if_needed(iface, singleton, instance) };
                        return AbiErrorCode::FunctionNotAvailable as u32;
                    }
                    // SAFETY: fn_ptr came from the host's native dispatch table and has the documented
                    // (state, args, out) -> AbiError C signature; instance.data is the contract state.
                    let dispatch_fn: unsafe extern "C" fn(
                        *const core::ffi::c_void,
                        *const core::ffi::c_void,
                        *mut core::ffi::c_void,
                        *mut AbiError,
                    ) = unsafe { core::mem::transmute(fn_ptr) };
                    let mut err: AbiError = AbiError::ok();
                    // SAFETY: dispatch_fn is transmuted from a valid host native function pointer;
                    // instance.data is the contract-state pointer owned by the host; args and out
                    // are caller-supplied buffers aligned by the generated caller via polyplug.alloc;
                    // `err` is a valid, writable out-param for the duration of the call.
                    unsafe {
                        dispatch_fn(
                            instance.data as *const core::ffi::c_void,
                            args,
                            out,
                            &mut err,
                        )
                    };
                    err.code
                }
                DispatchType::VirtualMachine => {
                    let mut err: AbiError = AbiError::ok();
                    // SAFETY: iface is non-null; vm.call + loader_data are the host-provided VM
                    // dispatcher; a null GuestContractInstance + null arena match the canonical
                    // rust host-contract caller (host contracts carry no guest instance / per-call arena);
                    // `err` is a valid, writable out-param for the duration of the call.
                    unsafe {
                        ((*iface).dispatch.vm.call)(
                            (*iface).dispatch.vm.loader_data,
                            GuestContractInstance::null(),
                            fn_id,
                            args as *const (),
                            out as *mut (),
                            core::ptr::null_mut(),
                            &mut err,
                        )
                    };
                    err.code
                }
            };
            // A non-singleton host contract mints a fresh instance per get_host_contract;
            // the caller owns it and must destroy it after the dispatch, or it leaks.
            // Singleton contracts are cached by the runtime — leave them untouched.
            // SAFETY: iface produced `instance` via get_host_contract above; the helper
            // destroys it only for non-singleton contracts.
            unsafe { destroy_host_instance_if_needed(iface, singleton, instance) };
            code
        },
    )
    .map_err(|e: rquickjs::Error| LoaderError::InitFailed {
        bundle: bundle_name.to_owned(),
        error: format!(
            "JS runtime js-quickjs error: callHostContract function creation failed: {e}"
        ),
    })?;

    polyplug_obj
        .set("callHostContract", call_host_contract_fn)
        .map_err(|e: rquickjs::Error| LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("JS runtime js-quickjs error: callHostContract set failed: {e}"),
        })?;

    let read_f64_fn: Function<'js> = Function::new(ctx.clone(), |ptr_num: f64| -> f64 {
        let ptr: *const f64 = (ptr_num as u64) as usize as *const f64;
        if ptr.is_null() {
            return 0.0;
        }
        // SAFETY: ptr is a valid host-provided pointer to an 8-byte f64 return slot.
        unsafe { *ptr }
    })
    .map_err(|e: rquickjs::Error| LoaderError::InitFailed {
        bundle: bundle_name.to_owned(),
        error: format!("JS runtime js-quickjs error: readF64 function creation failed: {e}"),
    })?;

    polyplug_obj
        .set("readF64", read_f64_fn)
        .map_err(|e: rquickjs::Error| LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("JS runtime js-quickjs error: readF64 set failed: {e}"),
        })?;

    let read_f32_fn: Function<'js> = Function::new(ctx.clone(), |ptr_num: f64| -> f64 {
        let ptr: *const f32 = (ptr_num as u64) as usize as *const f32;
        if ptr.is_null() {
            return 0.0;
        }
        // SAFETY: ptr is a valid host-provided pointer to a 4-byte f32 return slot.
        unsafe { *ptr as f64 }
    })
    .map_err(|e: rquickjs::Error| LoaderError::InitFailed {
        bundle: bundle_name.to_owned(),
        error: format!("JS runtime js-quickjs error: readF32 function creation failed: {e}"),
    })?;

    polyplug_obj
        .set("readF32", read_f32_fn)
        .map_err(|e: rquickjs::Error| LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("JS runtime js-quickjs error: readF32 set failed: {e}"),
        })?;

    // Write counterparts of readF64/readF32: preserve the full float bit pattern
    // instead of routing through writeU32 (which integer-truncates and loses the
    // f32 encoding). Pointer arrives as f64 for the same sign-extension reason
    // as writeU32.
    let write_f64_fn: Function<'js> = Function::new(ctx.clone(), |ptr_num: f64, value: f64| {
        let ptr: *mut f64 = (ptr_num as u64) as usize as *mut f64;
        if ptr.is_null() {
            return;
        }
        // SAFETY: ptr is a valid host-provided pointer to an 8-byte f64 slot.
        unsafe {
            *ptr = value;
        }
    })
    .map_err(|e: rquickjs::Error| LoaderError::InitFailed {
        bundle: bundle_name.to_owned(),
        error: format!("JS runtime js-quickjs error: writeF64 function creation failed: {e}"),
    })?;

    polyplug_obj
        .set("writeF64", write_f64_fn)
        .map_err(|e: rquickjs::Error| LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("JS runtime js-quickjs error: writeF64 set failed: {e}"),
        })?;

    let write_f32_fn: Function<'js> = Function::new(ctx.clone(), |ptr_num: f64, value: f64| {
        let ptr: *mut f32 = (ptr_num as u64) as usize as *mut f32;
        if ptr.is_null() {
            return;
        }
        // SAFETY: ptr is a valid host-provided pointer to a 4-byte f32 slot.
        unsafe {
            *ptr = value as f32;
        }
    })
    .map_err(|e: rquickjs::Error| LoaderError::InitFailed {
        bundle: bundle_name.to_owned(),
        error: format!("JS runtime js-quickjs error: writeF32 function creation failed: {e}"),
    })?;

    polyplug_obj
        .set("writeF32", write_f32_fn)
        .map_err(|e: rquickjs::Error| LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("JS runtime js-quickjs error: writeF32 set failed: {e}"),
        })?;

    Ok(())
}

// ─── Init-bundle window guard ────────────────────────────────────────────────

/// RAII guard that keeps the runtime's per-thread init-bundle window open for the
/// duration of `load_inner`'s init **and** registration phases.
///
/// `host_register_guest_contract` attributes each registration to the bundle id at
/// the top of the runtime's init-bundle stack (`current_init_bundle_id`). The JS
/// `polyplug_init` only stashes a `JsRegistrationData` in userdata; the actual
/// `register_guest_contract` call happens later in `load_inner`. The window must
/// therefore stay open across BOTH phases, and `pop` must run on EVERY exit path —
/// including the `?` early-returns between init and the registration call. Dropping
/// this guard pops exactly once, whether the function returns Ok, returns Err via
/// `?`, or unwinds, so the stack never leaks an entry.
struct InitBundleGuard<'r> {
    runtime: &'r PolyplugRuntime,
}

impl<'r> InitBundleGuard<'r> {
    /// Push `bundle_id` onto the runtime's init-bundle stack and return a guard that
    /// pops it on drop.
    fn enter(runtime: &'r PolyplugRuntime, bundle_id: u64) -> Self {
        runtime.push_init_bundle_id(bundle_id);
        Self { runtime }
    }
}

impl Drop for InitBundleGuard<'_> {
    fn drop(&mut self) {
        self.runtime.pop_init_bundle_id();
    }
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
    /// Count of VM-state boxes scheduled for epoch-deferred reclamation.
    ///
    /// Test/diagnostic only: epoch collection timing is non-deterministic, but this
    /// counter is incremented the instant a box is handed to
    /// `crossbeam_epoch::pin().defer(...)`, so it deterministically proves the VM was
    /// scheduled for reclaim — NOT parked alive forever. Instance state (Rule 12).
    scheduled_reclaims: AtomicU64,
}

impl JsLoader {
    pub fn new(config: JsConfig) -> JsLoader {
        JsLoader {
            _config: config,
            live: Mutex::new(HashMap::new()),
            scheduled_reclaims: AtomicU64::new(0),
        }
    }

    /// Schedule one bundle's VM-state boxes for epoch-deferred drop and record the
    /// scheduling.
    ///
    /// SAFETY/why: each `SendVm` box is already unreachable by any *new* dispatch (the
    /// bundle has been removed from `live` / the registry before this is called). Any
    /// in-flight runtime-mediated call holds a crossbeam-epoch pin, so `defer` runs the
    /// drop — freeing the QuickJS `Context` and `Runtime` — only once no such reader
    /// remains; the global epoch coordinates that with the runtime's reader pins. Direct
    /// FFI host→VM callers must quiesce before unload per the documented host contract
    /// (docs/TRUST_MODEL.md). `SendVm` is `Send + 'static` (see its `unsafe impl Send`),
    /// so moving it into the deferred closure is sound — the box is reachable only from
    /// the deferred closure, and rquickjs's `parallel` lock still serializes the drop's
    /// VM teardown.
    fn schedule_reclaim(&self, state: Vec<SendVm>) {
        for vm in state {
            self.scheduled_reclaims.fetch_add(1, Ordering::Relaxed);
            crossbeam_epoch::pin().defer(move || drop(vm));
        }
    }

    /// Read the plugin's JS source from the on-disk bundle directory.
    ///
    /// Used by the [`BundleSource::Path`] flow. The file is resolved from the
    /// manifest's `file` field, defaulting to `bundle.js`.
    fn read_path_source(manifest: &ManifestData) -> Result<String, LoaderError> {
        let bundle_path: PathBuf = if !manifest.file.is_empty() {
            manifest.path.join(&manifest.file)
        } else {
            manifest.path.join("bundle.js")
        };
        std::fs::read_to_string(&bundle_path).map_err(|e: std::io::Error| {
            LoaderError::ManifestParse {
                path: bundle_path.display().to_string(),
                reason: e.to_string(),
            }
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
    ) -> Result<(), LoaderError> {
        let bundle_id: u64 = manifest.id;

        let qjs_runtime: Runtime =
            Runtime::new().map_err(|e: rquickjs::Error| LoaderError::InitFailed {
                bundle: manifest.name.clone(),
                error: format!("JS runtime init failed: QuickJS runtime init failed: {e}"),
            })?;

        let ctx: Context =
            Context::full(&qjs_runtime).map_err(|e: rquickjs::Error| LoaderError::InitFailed {
                bundle: manifest.name.clone(),
                error: format!("JS runtime js-quickjs error: context creation failed: {e}"),
            })?;

        // Get the HostApi pointer from the runtime.
        // This interface already has the runtime pointer set internally.
        let host_interface: *const HostApi = runtime.as_context_ptr();

        // Open the init-bundle window for BOTH the init call and the registration
        // call below: `host_register_guest_contract` attributes the registration to
        // the bundle id on top of this stack, and `register_guest_contract` runs later
        // in this function (init only stashes JsRegistrationData in userdata). The
        // guard's Drop pops once on every exit path — including the `?` early-returns
        // between here and the registration call — so the stack never leaks an entry
        // and the registration carries the real bundle id.
        let _init_window: InitBundleGuard<'_> = InitBundleGuard::enter(runtime, bundle_id);

        // In-memory sources (Code/Bytes) carry no bundle directory, so bundlePath
        // and BundleInitContext.bundle_path are empty for them.
        let bundle_dir_str: String = match bundle_dir {
            Some(dir) => dir.to_string_lossy().into_owned(),
            None => String::new(),
        };

        let registration_slot: Rc<RefCell<Option<JsRegistrationData>>> =
            Rc::new(RefCell::new(None));

        let init_outcome: Result<(), LoaderError> = ctx.with(|ctx_ref: Ctx<'_>| {
            ctx_ref
                .store_userdata(Rc::clone(&registration_slot))
                .map_err(
                    |_: UserDataError<Rc<RefCell<Option<JsRegistrationData>>>>| {
                        LoaderError::InitFailed {
                            bundle: manifest.name.clone(),
                            error: "JS runtime js-quickjs error: failed to store registration slot in userdata".to_owned(),
                        }
                    },
                )?;

            let globals: Object<'_> = ctx_ref.globals();
            let polyplug_obj: Object<'_> =
                Object::new(ctx_ref.clone()).map_err(|e: rquickjs::Error| {
                    LoaderError::InitFailed {
                        bundle: manifest.name.clone(),
                        error: format!("JS runtime js-quickjs error: object creation failed: {e}"),
                    }
                })?;
            register_host_functions(
                &ctx_ref,
                &polyplug_obj,
                host_interface,
                &manifest.name,
                runtime.logger(),
            )?;
            globals
                .set("polyplug", polyplug_obj)
                .map_err(|e: rquickjs::Error| {
                    LoaderError::InitFailed {
                        bundle: manifest.name.clone(),
                        error: format!("JS runtime js-quickjs error: global set failed: {e}"),
                    }
                })?;

            let set_bundle: String = format!("globalThis.bundlePath = {:?};", bundle_dir_str);
            ctx_ref
                .eval::<Value<'_>, _>(set_bundle.as_str())
                .map_err(|e: rquickjs::Error| {
                    LoaderError::InitFailed {
                        bundle: manifest.name.clone(),
                        error: format!("JS runtime js-quickjs error: bundlePath injection failed: {e}"),
                    }
                })?;

            ctx_ref
                .eval::<Value<'_>, _>(bundle_js)
                .map_err(|e: rquickjs::Error| {
                    LoaderError::InitFailed {
                        bundle: manifest.name.clone(),
                        error: format!("JS runtime js-quickjs error: bundle eval failed: {e}"),
                    }
                })?;

            let init_fn: Function<'_> = ctx_ref
                .globals()
                .get::<&str, Function<'_>>("polyplug_init")
                .map_err(|_| {
                    LoaderError::InitSymbolMissing {
                        bundle: bundle_dir_str.clone(),
                    }
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

            let init_value: Value<'_> = init_fn
                .call::<(f64, f64, f64, f64), Value<'_>>((host_lo, host_hi, ctx_lo, ctx_hi))
                .map_err(|e: rquickjs::Error| {
                    let thrown: Value<'_> = ctx_ref.catch();
                    let detail: String = match thrown.as_exception() {
                        Some(exc) => exc.message().unwrap_or_else(|| e.to_string()),
                        None => e.to_string(),
                    };
                    LoaderError::InitFailed {
                        bundle: manifest.name.clone(),
                        error: format!(
                            "JS runtime js-quickjs error: polyplug_init call failed: {detail}"
                        ),
                    }
                })?;

            // Honor the AbiError returned by polyplug_init. Generated guests
            // return `{ code, message }`; a bare number is also accepted.
            // `undefined` is treated as success. A non-zero code means the
            // guest refused to initialize — fail the load with that code and
            // message instead of silently treating the bundle as loaded.
            let (init_code, init_message): (u32, Option<String>) =
                if let Some(obj) = init_value.as_object() {
                    let code: u32 = obj.get::<&str, f64>("code").unwrap_or(0.0_f64) as u32;
                    let message: Option<String> =
                        obj.get::<&str, Option<String>>("message").unwrap_or(None);
                    (code, message)
                } else if let Some(num) = init_value.as_number() {
                    (num as u32, None)
                } else {
                    (0_u32, None)
                };
            if init_code != AbiErrorCode::Ok as u32 {
                let detail: String = match init_message {
                    Some(msg) if !msg.is_empty() => format!(" ({msg})"),
                    _ => String::new(),
                };
                return Err(LoaderError::InitFailed {
                    bundle: manifest.name.clone(),
                    error: format!(
                        "JS runtime js-quickjs error: polyplug_init returned error code {init_code}{detail}"
                    ),
                });
            }

            Ok::<(), LoaderError>(())
        });

        init_outcome?;

        let registration_data: JsRegistrationData = registration_slot
            .borrow_mut()
            .take()
            .ok_or_else(|| LoaderError::InitFailed {
                bundle: manifest.name.clone(),
                error: "JS runtime js-quickjs error: polyplug_init did not call registerVtable"
                    .to_owned(),
            })?;

        let loader_data: SendVm = SendVm(Box::new(JsLoaderData {
            _runtime: qjs_runtime,
            ctx,
            functions: registration_data.functions,
            in_dispatch_threads: Mutex::new(Vec::new()),
            logger: runtime.logger(),
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

        let mut abi_result: AbiError = AbiError::ok();
        // SAFETY: host_interface, descriptor, and static_interface are valid for this call.
        // The register_guest_contract function uses self-passing pattern; `abi_result`
        // is a valid, writable out-param for the duration of the call.
        unsafe {
            ((*host_interface).register_guest_contract)(
                host_interface,
                &descriptor,
                static_interface,
                &mut abi_result,
            )
        };

        if !abi_result.is_ok() {
            // The registry copy made during register_guest_contract may already point
            // at this box's heap address; schedule it for epoch-deferred drop rather
            // than dropping it inline here, which would dangle the registry's
            // bridge_data while a reader is pinned.
            self.schedule_reclaim(vec![loader_data]);
            return Err(LoaderError::InitFailed {
                bundle: manifest.name.clone(),
                error: format!(
                    "JS runtime js-quickjs error: register_guest_contract returned error code {:?}",
                    abi_result.code
                ),
            });
        }

        // Take ownership of this bundle's VM state. A reload of the same bundle id
        // REPLACES the prior VM entry and schedules the superseded VM for epoch-deferred
        // reclaim (mirroring the native loader): the global epoch keeps the old VM alive
        // for any in-flight dispatch and frees it once no reader is pinned, rather than
        // parking it until unload.
        let superseded: Option<Vec<SendVm>> = {
            let mut live: std::sync::MutexGuard<'_, HashMap<BundleId, Vec<SendVm>>> =
                self.live.lock().unwrap_or_else(PoisonError::into_inner);
            live.insert(BundleId::from_u64(bundle_id), vec![loader_data])
        };
        if let Some(old_state) = superseded {
            self.schedule_reclaim(old_state);
        }

        Ok(())
    }

    /// Number of live VM-state entries currently owned for `bundle_id`.
    #[cfg(test)]
    fn live_vm_count(&self, bundle_id: BundleId) -> usize {
        let live: std::sync::MutexGuard<'_, HashMap<BundleId, Vec<SendVm>>> =
            self.live.lock().unwrap_or_else(PoisonError::into_inner);
        live.get(&bundle_id).map(Vec::len).unwrap_or(0)
    }

    /// Number of VM-state boxes scheduled for epoch-deferred reclaim. Deterministic
    /// (incremented at scheduling time), so tests assert the resource was handed to
    /// the epoch collector without depending on its non-deterministic timing.
    #[cfg(test)]
    fn scheduled_reclaim_count(&self) -> u64 {
        self.scheduled_reclaims.load(Ordering::Relaxed)
    }
}

impl BundleLoader for JsLoader {
    fn loader_name(&self) -> &'static str {
        "js-quickjs"
    }

    fn loader_language(&self) -> SupportedLanguage {
        SupportedLanguage::JavaScript
    }

    fn supports_hot_reload(&self) -> bool {
        true
    }

    fn load(
        &self,
        manifest: &ManifestData,
        source: &BundleSource,
        runtime: &PolyplugRuntime,
    ) -> Result<(), LoaderError> {
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
                    LoaderError::InvalidSourceEncoding {
                        loader: "js-quickjs",
                        source_kind: source.kind(),
                        bundle: manifest.name.clone(),
                    }
                })?;
                self.load_inner(manifest, code, None, runtime)
            }
        }
    }

    fn reload(
        &self,
        manifest: &ManifestData,
        runtime: &PolyplugRuntime,
    ) -> Result<(), LoaderError> {
        // reload is path-based (the watcher only tracks on-disk bundles); the runtime
        // gates hot-reload before calling this.
        let bundle_js: String = JsLoader::read_path_source(manifest)?;
        self.load_inner(manifest, &bundle_js, Some(&manifest.path), runtime)
    }

    /// Reclaim the bundle's QuickJS VM via epoch-deferred drop.
    ///
    /// Called by the runtime AFTER `invalidate_bundle` has removed the bundle from
    /// the registry, so no dispatch can *resolve* this contract anew.
    ///
    /// # Host-coordination contract
    /// The bundle's VM-state boxes are removed from `live` and scheduled for
    /// epoch-deferred drop (see [`JsLoader::schedule_reclaim`]): each box's QuickJS
    /// `Context` and `Runtime` are freed only once no crossbeam-epoch reader is pinned,
    /// so any in-flight *runtime-mediated* call (which holds an epoch pin across
    /// `call_guest_method` dispatch) keeps the VM alive until it completes. Direct FFI
    /// host→VM callers the runtime does not mediate are covered by the documented
    /// trusted-same-process contract: exactly like hot-reload, the host MUST NOT call a
    /// bundle's contracts concurrently with unloading it (see `Runtime::unload_bundle`
    /// and docs/TRUST_MODEL.md).
    ///
    /// The VM is always epoch-reclaimed (never parked alive forever).
    fn unload(&self, bundle_id: BundleId, _runtime: &PolyplugRuntime) -> Result<(), LoaderError> {
        let state: Vec<SendVm> = {
            let mut live: std::sync::MutexGuard<'_, HashMap<BundleId, Vec<SendVm>>> =
                self.live.lock().unwrap_or_else(PoisonError::into_inner);
            match live.remove(&bundle_id) {
                Some(v) => v,
                None => return Ok(()),
            }
        };

        self.schedule_reclaim(state);

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
    fn js_quickjs_loader_name() {
        let loader: JsLoader = JsLoader::new(JsConfig {});
        assert_eq!(loader.loader_name(), "js-quickjs");
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
            loader: "js-quickjs".to_owned(),
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

    /// Unload removes the bundle from the loader's live map and SCHEDULES its VM state
    /// for epoch-deferred reclaim. Unload is uniform regardless of `in_dispatch_threads`:
    /// every unload epoch-reclaims, so even an in-flight call schedules reclaim (the
    /// epoch keeps the VM alive until that reader's pin clears).
    #[test]
    fn unload_removes_live_and_schedules_reclaim() {
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
            "unload must remove the bundle's VM state from the live map"
        );
        assert_eq!(
            loader.scheduled_reclaim_count(),
            1,
            "unload must schedule the VM state for epoch-deferred reclaim"
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
            // Driving the loader directly bypasses `Runtime::unload_bundle`'s registry
            // invalidation, so the test mirrors it explicitly.
            runtime
                .registry()
                .invalidate_bundle(bundle_id)
                .expect("invalidate must succeed");
            assert_eq!(
                loader.live_vm_count(bundle_id),
                0,
                "unload must reclaim the entry each iteration"
            );
        }
        assert_eq!(
            loader.scheduled_reclaim_count(),
            5,
            "each of the 5 unloads must schedule its VM state for epoch-deferred reclaim"
        );
    }

    /// A reload of the same bundle id must REPLACE the live VM entry and schedule the
    /// superseded VM for epoch-deferred reclaim — not park it in the live map until
    /// unload. This mirrors the native loader's reload-reclaim and keeps a reload loop
    /// from leaking one VM per reload.
    ///
    /// Driving the loader directly bypasses `Runtime`'s reload orchestration, so the
    /// test invalidates the prior registry registration between loads (so the second
    /// `register_guest_contract` is not rejected as a duplicate provider) — exactly the
    /// supersede path a real reload takes.
    #[test]
    fn reload_replaces_live_and_reclaims_superseded_vm() {
        let loader: JsLoader = JsLoader::new(JsConfig {});
        let runtime: Arc<PolyplugRuntime> = polyplug::runtime::RuntimeBuilder::new()
            .loader(JsLoader::new(JsConfig {}))
            .build()
            .expect("runtime build must succeed");
        let (_dir, manifest): (tempfile::TempDir, ManifestData) =
            write_unload_bundle("js_reload_reclaim");
        let bundle_id: BundleId = BundleId::from_u64(manifest.id);

        loader
            .load(
                &manifest,
                &BundleSource::Path(manifest.path.clone()),
                &runtime,
            )
            .expect("first load must succeed");
        assert_eq!(
            loader.live_vm_count(bundle_id),
            1,
            "first load installs exactly one live VM"
        );
        assert_eq!(
            loader.scheduled_reclaim_count(),
            0,
            "nothing is superseded by the first load"
        );

        // Drop the registry-side registration so the second load's
        // register_guest_contract is not a duplicate, exercising the supersede path.
        runtime
            .registry()
            .invalidate_bundle(bundle_id)
            .expect("invalidate must succeed");

        loader
            .load(
                &manifest,
                &BundleSource::Path(manifest.path.clone()),
                &runtime,
            )
            .expect("second load (reload) must succeed");

        assert_eq!(
            loader.live_vm_count(bundle_id),
            1,
            "reload replaces the live VM — the live map must not grow"
        );
        assert_eq!(
            loader.scheduled_reclaim_count(),
            1,
            "reload must schedule the superseded VM for epoch-deferred reclaim"
        );
    }

    /// Even when a dispatch is marked in flight (the bundle's `in_dispatch_threads`
    /// is non-empty), unload behaves uniformly: it removes the bundle from `live` and
    /// SCHEDULES its VM state for epoch-deferred reclaim. The in-flight reader's epoch
    /// pin — not an `in_dispatch_threads`-gated retire branch — is what keeps the VM
    /// alive until the call completes, so unload never parks the state forever.
    #[test]
    fn unload_schedules_reclaim_even_when_in_flight() {
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

        // Mark a fake in-flight dispatch in the bundle's tracking vec — exactly the
        // state the dispatch guard leaves while a call is mid-flight on another
        // thread. An in-flight mark must not change the uniform epoch-reclaim outcome.
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
            .expect("unload must succeed even when marked in-flight");
        assert_eq!(
            loader.live_vm_count(bundle_id),
            0,
            "unload must remove the bundle from the live map"
        );
        assert_eq!(
            loader.scheduled_reclaim_count(),
            1,
            "unload must schedule epoch-deferred reclaim even when marked in-flight"
        );
    }

    /// f64 params and returns must round-trip through a REAL loaded VM with
    /// full bit-pattern fidelity: the guest reads its argument with
    /// `polyplug.readF64` and writes its result with `polyplug.writeF64`.
    /// Before the writeF64/writeF32 bridge functions existed, the guest had no
    /// way to emit a float result (writeU32 integer-truncates), so this test
    /// is RED on the pre-fix loader.
    #[test]
    fn js_guest_f64_param_and_return_round_trip() {
        let loader: JsLoader = JsLoader::new(JsConfig {});
        let runtime: Arc<PolyplugRuntime> = polyplug::runtime::RuntimeBuilder::new()
            .loader(JsLoader::new(JsConfig {}))
            .build()
            .expect("runtime build must succeed");

        let contract_id: u64 = polyplug_utils::guest_contract_id("test.float", 1);
        let contract_lo: u32 = contract_id as u32;
        let contract_hi: u32 = (contract_id >> 32) as u32;
        let bundle_js: String = format!(
            r#"
function polyplug_init(host_lo, host_hi, ctx_lo, ctx_hi) {{
    var vtable = {{ functions: [ function(args_ptr, out_ptr) {{
        var v = polyplug.readF64(args_ptr);
        polyplug.writeF64(out_ptr, v * 2.0 + 0.25);
        return 0;
    }} ] }};
    polyplug.registerVtable({contract_lo}, {contract_hi}, vtable, 1, "test.float@1", 0x00010000);
}}
"#
        );
        let dir: tempfile::TempDir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("bundle.js"), bundle_js).expect("write bundle.js");
        let manifest: ManifestData = ManifestData {
            id: polyplug_utils::bundle_id("js_f64_round_trip"),
            name: "js_f64_round_trip".to_owned(),
            loader: "js-quickjs".to_owned(),
            file: "bundle.js".to_owned(),
            path: dir.path().to_path_buf(),
            version: String::new(),
            provides: Vec::new(),
            function_count: HashMap::new(),
            dependencies: Vec::new(),
            needs_reinit_on_dep_reload: false,
            bundle_dependencies: Vec::new(),
        };

        loader
            .load(
                &manifest,
                &BundleSource::Path(manifest.path.clone()),
                &runtime,
            )
            .expect("load must succeed");

        let handle: GuestContractHandle = runtime
            .find_guest_contract(contract_id, 0)
            .expect("contract must be registered");
        let iface: *const GuestContractInterface = runtime
            .resolve_guest_contract(handle)
            .expect("interface must resolve");
        assert!(!iface.is_null(), "resolved interface must be non-null");

        let arg: f64 = 1234.5625;
        let mut out_val: f64 = 0.0;
        let mut err: AbiError = AbiError::ok();
        // SAFETY: iface is non-null and was just resolved; the JS loader always
        // registers VirtualMachine dispatch, so the vm union variant is active.
        // arg/out_val are valid for the duration of the call; a null arena is
        // the documented host->alloc fallback.
        unsafe {
            assert_eq!((*iface).dispatch_type, DispatchType::VirtualMachine);
            ((*iface).dispatch.vm.call)(
                (*iface).dispatch.vm.loader_data,
                GuestContractInstance::null(),
                0,
                &arg as *const f64 as *const (),
                &mut out_val as *mut f64 as *mut (),
                core::ptr::null_mut(),
                &mut err,
            )
        };
        assert_eq!(
            err.code,
            AbiErrorCode::Ok as u32,
            "f64 dispatch must succeed"
        );
        // 1234.5625 * 2.0 + 0.25 = 2469.375 — exact in binary floating point,
        // so equality (not epsilon) proves the bit pattern survived both
        // directions. The pre-fix writeU32 path would have produced 2469.0
        // (integer truncation) or thrown (missing bridge fn).
        assert_eq!(out_val, 2469.375, "f64 must round-trip exactly");
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
            logger: LoggerHandle::default_stderr(),
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
            js_dispatch_impl(
                vm_loader_data,
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
                    js_dispatch_impl(
                        vm_loader_data,
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
            js_dispatch_impl(
                vm_loader_data,
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
            js_dispatch_impl(
                vm_loader_data,
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
                js_dispatch_impl(
                    vm_loader_data_a,
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
                js_dispatch_impl(
                    vm_loader_data_b,
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

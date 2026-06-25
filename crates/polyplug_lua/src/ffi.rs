//! FFI exports for polyplug_lua — `polyplug_lua_loader_create` / `polyplug_lua_loader_free`
//! plus the LuaJIT host-contract bridge (`polyplug_lua_host_vm_dispatch`,
//! `polyplug_lua_host_destroy_instance`).
//!
//! # Why the host-contract bridge exists
//! LuaJIT FFI callbacks cannot return structs by value (documented NYI), so a
//! LuaJIT host can never produce the native-dispatch thunk signature
//! `AbiError (*)(const void*, const void*, void*)` nor the VM-dispatch `call`
//! signature (struct parameters AND struct return). The generated Lua host
//! interface factories therefore register host contracts with VM dispatch whose
//! `call` points at `polyplug_lua_host_vm_dispatch` below; the trampoline
//! forwards to a scalar-only LuaJIT callback
//! (`u32 (*)(void* instance_data, u32 fn_id, const void*, void*)`) stored in a
//! `PolyplugLuaHostDispatchBridge` carried via `VmDispatch.loader_data`.
//!
//! # create_instance is a Lua callback, not a Rust trampoline
//! `create_instance` only takes pointer args and writes through an out-pointer
//! (no struct by value), so LuaJIT CAN create that callback directly — the
//! generated factory installs a Lua `create_instance` that builds a fresh impl
//! per instance and stamps the new instance id into the out `HostContractInstance`.
//! The old `polyplug_lua_host_create_instance` Rust trampoline (which returned
//! the registrant-owned `user_data` as a single shared instance) is therefore
//! SUPERSEDED and removed — per-instance state requires a fresh impl per call,
//! which only the Lua factory can build. `polyplug_lua_host_destroy_instance`
//! stays a Rust trampoline because it takes `HostContractInstance` BY VALUE
//! (LuaJIT cannot create that callback); it forwards the instance data to the
//! bridge's `destroy_callback`.

use core::ffi::c_void;

use polyplug::loader::BundleLoader;
use polyplug_abi::AbiError;
use polyplug_abi::AbiErrorCode;
use polyplug_abi::CallArena;
use polyplug_abi::GuestContractInstance;
use polyplug_abi::HostContractInstance;
use polyplug_abi::HostContractInterface;
use polyplug_abi::StringView;
use polyplug_abi::VmLoaderData;

use crate::{LuaConfig, LuaLoader};

#[repr(C)]
pub struct PolyplugLuaConfig {
    pub _reserved: u8,
}

/// Bridge between the ABI VM-dispatch convention and a LuaJIT-creatable callback.
///
/// The generated Lua host interface factory allocates one of these per
/// registered host contract (anchored for the program lifetime), stores a
/// scalar-only LuaJIT callback in `callback`, and points
/// `VmDispatch.loader_data.data` at it.
#[repr(C)]
pub struct PolyplugLuaHostDispatchBridge {
    /// Scalar-only dispatch callback:
    /// `(instance_data, fn_id, args, out) -> AbiErrorCode as u32`.
    /// `instance_data` is the `HostContractInstance::data` produced by the Lua
    /// `create_instance` callback (an instance id cast to a pointer); it routes
    /// the dispatch to the per-instance impl. LuaJIT can create this callback
    /// (no struct-by-value args or return).
    pub callback: Option<unsafe extern "C" fn(*mut c_void, u32, *const c_void, *mut c_void) -> u32>,
    /// Per-instance teardown callback: `(instance_data)`. Invoked by
    /// `polyplug_lua_host_destroy_instance` so the Lua factory can drop the
    /// impl keyed by the instance id. LuaJIT can create this callback (pointer
    /// arg only, no struct by value).
    pub destroy_callback: Option<unsafe extern "C" fn(*mut c_void)>,
}

/// VM-dispatch trampoline for host contracts implemented in a LuaJIT host.
///
/// Matches `VmDispatch.call`. Routes the call to the scalar LuaJIT callback in
/// the `PolyplugLuaHostDispatchBridge` carried by `loader_data` and widens the
/// returned `u32` code into an `AbiError` (empty message).
///
/// # Safety
/// `loader_data.data` must be null or point to a live `PolyplugLuaHostDispatchBridge`
/// that outlives every dispatch through the registered interface (the generated
/// factory anchors it for the program lifetime). `args`/`out` follow the
/// per-function ABI marshalling contract and are passed through untouched.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_lua_host_vm_dispatch(
    loader_data: VmLoaderData,
    instance: GuestContractInstance,
    fn_id: u32,
    args: *const (),
    out: *mut (),
    _arena: *mut CallArena,
    out_err: *mut AbiError,
) {
    // SAFETY: loader_data carries the bridge pointer per this function's safety
    // contract; the impl forwards instance.data/args/out untouched to the LuaJIT
    // callback.
    let result: AbiError =
        unsafe { polyplug_lua_host_vm_dispatch_impl(loader_data, instance.data, fn_id, args, out) };
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(result) };
    }
}

unsafe fn polyplug_lua_host_vm_dispatch_impl(
    loader_data: VmLoaderData,
    instance_data: *mut c_void,
    fn_id: u32,
    args: *const (),
    out: *mut (),
) -> AbiError {
    let bridge_ptr: *const PolyplugLuaHostDispatchBridge =
        loader_data.data as *const PolyplugLuaHostDispatchBridge;
    if bridge_ptr.is_null() {
        return AbiError {
            code: AbiErrorCode::InvalidPointer as u32,
            message: StringView::from_static(b"lua host dispatch bridge is null"),
        };
    }
    // SAFETY: bridge_ptr is non-null (checked above) and points to a live
    // PolyplugLuaHostDispatchBridge per this function's safety contract.
    let callback: Option<
        unsafe extern "C" fn(*mut c_void, u32, *const c_void, *mut c_void) -> u32,
    > = unsafe { (*bridge_ptr).callback };
    match callback {
        Some(cb) => {
            // SAFETY: cb is the LuaJIT callback installed by the generated
            // factory; instance_data routes to the per-instance impl and
            // args/out are forwarded untouched per the dispatch contract.
            let code: u32 = unsafe {
                cb(
                    instance_data,
                    fn_id,
                    args as *const c_void,
                    out as *mut c_void,
                )
            };
            if code == AbiErrorCode::Ok as u32 {
                AbiError::ok()
            } else {
                AbiError {
                    code,
                    message: StringView::from_static(b"lua host contract returned error"),
                }
            }
        }
        None => AbiError {
            code: AbiErrorCode::InvalidPointer as u32,
            message: StringView::from_static(b"lua host dispatch bridge has no callback"),
        },
    }
}

/// Bridge between the `RuntimeConfig::log` ABI signature and a LuaJIT-creatable
/// scalar-only log callback.
///
/// `RuntimeConfig::log` receives the `scope` and `message` `StringView`s BY
/// VALUE (deliberate — hot path), but LuaJIT FFI callbacks cannot receive
/// structs by value, so a Lua host can never install that callback directly.
/// The Lua host SDK instead allocates one of these (anchored on the Runtime
/// instance for its lifetime), points `RuntimeConfig::log_user_data` at it,
/// and sets `RuntimeConfig::log` to [`polyplug_lua_log_trampoline`], which
/// decomposes the views into ptr+len scalars and forwards.
#[repr(C)]
pub struct PolyplugLuaLogBridge {
    /// Scalar-only log callback:
    /// `(user_data, level, scope_ptr, scope_len, msg_ptr, msg_len)`.
    /// LuaJIT can create this callback (no struct-by-value parameters).
    pub callback:
        Option<unsafe extern "C" fn(*mut c_void, u32, *const u8, usize, *const u8, usize)>,
    /// Opaque pointer forwarded unchanged as the callback's first argument.
    pub user_data: *mut c_void,
}

/// Logger trampoline for LuaJIT hosts — exact `RuntimeConfig::log` signature.
///
/// Reads the [`PolyplugLuaLogBridge`] carried by `user_data`, decomposes the
/// by-value `StringView`s into ptr+len scalars, and forwards to the scalar
/// LuaJIT callback. A logger must never crash the runtime: a null `user_data`
/// or a null inner callback is a silent no-op.
///
/// # Safety
/// `user_data` must be null or point to a live `PolyplugLuaLogBridge` that
/// outlives the runtime (the Lua host SDK anchors it on the Runtime instance).
/// `scope` and `message` follow the `RuntimeConfig::log` contract: valid UTF-8
/// views for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_lua_log_trampoline(
    user_data: *mut c_void,
    level: u32,
    scope: StringView,
    message: StringView,
) {
    let bridge_ptr: *const PolyplugLuaLogBridge = user_data as *const PolyplugLuaLogBridge;
    if bridge_ptr.is_null() {
        return;
    }
    // SAFETY: bridge_ptr is non-null (checked above) and points to a live
    // PolyplugLuaLogBridge per this function's safety contract.
    let callback: Option<
        unsafe extern "C" fn(*mut c_void, u32, *const u8, usize, *const u8, usize),
    > = unsafe { (*bridge_ptr).callback };
    if let Some(cb) = callback {
        // SAFETY: bridge_ptr is non-null and live (see above); user_data is an
        // opaque pointer owned by the registrant and only forwarded.
        let inner_user_data: *mut c_void = unsafe { (*bridge_ptr).user_data };
        // SAFETY: cb is the LuaJIT callback installed by the Lua host SDK; the
        // scope/message ptr+len pairs come straight from the by-value
        // StringViews, which the RuntimeConfig::log contract guarantees are
        // valid for the duration of this call.
        unsafe {
            cb(
                inner_user_data,
                level,
                scope.ptr,
                scope.len,
                message.ptr,
                message.len,
            )
        };
    }
}

// NOTE: `polyplug_lua_host_create_instance` (a Rust trampoline returning the
// registrant-owned `user_data` as one shared instance) was SUPERSEDED by a Lua
// `create_instance` callback in the generated factory. Per-instance state needs
// a fresh impl built per call, which only the Lua factory can produce;
// create_instance takes pointer args + an out-pointer (no struct by value), so
// LuaJIT can create it directly. It is removed here, not blind-deleted.

/// `destroy_instance` trampoline for host contracts registered by a LuaJIT host.
///
/// Takes `HostContractInstance` BY VALUE, which LuaJIT FFI callbacks cannot
/// accept, so this must stay a native Rust trampoline. It reads the
/// `PolyplugLuaHostDispatchBridge` from `(*this).user_data` and forwards the
/// instance data to the bridge's `destroy_callback`, letting the Lua factory
/// drop the per-instance impl keyed by the instance id. Null `this`,
/// `user_data`, or absent `destroy_callback` is a silent no-op (teardown must
/// never crash the runtime).
///
/// # Safety
/// `this` must be null or a valid `HostContractInterface` pointer (self-passing
/// pattern; the runtime always passes the registered interface). When non-null,
/// `(*this).user_data` must be null or point to a live
/// `PolyplugLuaHostDispatchBridge` that outlives this call (the generated
/// factory anchors it for the program lifetime).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_lua_host_destroy_instance(
    this: *const HostContractInterface,
    instance: HostContractInstance,
) {
    if this.is_null() {
        return;
    }
    // SAFETY: this is non-null (checked above) and points at the registered
    // interface per the self-passing ABI contract.
    let bridge_ptr: *const PolyplugLuaHostDispatchBridge =
        unsafe { (*this).user_data } as *const PolyplugLuaHostDispatchBridge;
    if bridge_ptr.is_null() {
        return;
    }
    // SAFETY: bridge_ptr is non-null (checked above) and points to a live
    // PolyplugLuaHostDispatchBridge per this function's safety contract.
    let destroy_callback: Option<unsafe extern "C" fn(*mut c_void)> =
        unsafe { (*bridge_ptr).destroy_callback };
    if let Some(cb) = destroy_callback {
        // SAFETY: cb is the LuaJIT destroy callback installed by the generated
        // factory; instance.data is the instance id it stamped at create time.
        unsafe { cb(instance.data) };
    }
}

/// # Safety
/// `config` may be null. The returned pointer must be freed with `polyplug_lua_loader_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_lua_loader_create(
    config: *const PolyplugLuaConfig,
) -> *mut c_void {
    let _ = config;
    let loader: LuaLoader = LuaLoader::new(LuaConfig::default());
    // Double-box: inner Box<dyn BundleLoader> preserves the fat pointer (data + vtable),
    // outer Box stores it on the heap so we can pass a thin *mut c_void across FFI.
    let trait_obj: Box<dyn BundleLoader> = Box::new(loader);
    Box::into_raw(Box::new(trait_obj)) as *mut c_void
}

/// # Safety
/// `ptr` must be a non-freed pointer returned by `polyplug_lua_loader_create`, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_lua_loader_free(ptr: *mut c_void) {
    if ptr.is_null() {
        return;
    }
    // SAFETY: ptr was produced by polyplug_lua_loader_create via
    // Box::into_raw(Box::new(trait_obj)) where trait_obj: Box<dyn BundleLoader>.
    // The caller guarantees ptr is not used after this call.
    drop(unsafe { Box::<Box<dyn BundleLoader>>::from_raw(ptr as *mut Box<dyn BundleLoader>) });
}

#[cfg(test)]
mod tests {
    use core::ffi::c_void;
    use core::ptr;
    use core::slice;

    use polyplug_abi::StringView;

    use super::{PolyplugLuaLogBridge, polyplug_lua_log_trampoline};

    /// Record of one forwarded log call, written by the scalar test callback.
    struct Captured {
        calls: u32,
        level: u32,
        scope: String,
        message: String,
    }

    /// Scalar-only callback standing in for the LuaJIT-created one: same
    /// signature `(user_data, level, scope_ptr, scope_len, msg_ptr, msg_len)`.
    unsafe extern "C" fn capture_callback(
        user_data: *mut c_void,
        level: u32,
        scope_ptr: *const u8,
        scope_len: usize,
        msg_ptr: *const u8,
        msg_len: usize,
    ) {
        // SAFETY: the test passes a pointer to a live, exclusively-owned
        // Captured as the bridge user_data; no other reference exists during
        // the call.
        let captured: &mut Captured = unsafe { &mut *(user_data as *mut Captured) };
        captured.calls += 1;
        captured.level = level;
        // SAFETY: scope_ptr/scope_len and msg_ptr/msg_len come from the
        // trampoline's StringView decomposition; the test keeps the backing
        // byte slices alive for the duration of the call.
        let scope_bytes: &[u8] = unsafe { slice::from_raw_parts(scope_ptr, scope_len) };
        // SAFETY: same as scope_bytes above.
        let msg_bytes: &[u8] = unsafe { slice::from_raw_parts(msg_ptr, msg_len) };
        captured.scope = String::from_utf8_lossy(scope_bytes).into_owned();
        captured.message = String::from_utf8_lossy(msg_bytes).into_owned();
    }

    #[test]
    fn trampoline_forwards_level_scope_message_intact() {
        let mut captured = Captured {
            calls: 0,
            level: 0,
            scope: String::new(),
            message: String::new(),
        };
        let mut bridge = PolyplugLuaLogBridge {
            callback: Some(capture_callback),
            user_data: &mut captured as *mut Captured as *mut c_void,
        };
        let scope: StringView = StringView::from_static(b"manifest");
        let message: StringView = StringView::from_static(b"ByBundle dep 'x' has no bundle_id");
        // SAFETY: bridge is a live PolyplugLuaLogBridge on this test's stack;
        // the StringViews point at 'static byte literals.
        unsafe {
            polyplug_lua_log_trampoline(
                &mut bridge as *mut PolyplugLuaLogBridge as *mut c_void,
                2,
                scope,
                message,
            );
        }
        assert_eq!(captured.calls, 1);
        assert_eq!(captured.level, 2);
        assert_eq!(captured.scope, "manifest");
        assert_eq!(captured.message, "ByBundle dep 'x' has no bundle_id");
    }

    #[test]
    fn trampoline_forwards_empty_views() {
        let mut captured = Captured {
            calls: 0,
            level: 0,
            scope: String::from("stale"),
            message: String::from("stale"),
        };
        let mut bridge = PolyplugLuaLogBridge {
            callback: Some(capture_callback),
            user_data: &mut captured as *mut Captured as *mut c_void,
        };
        // SAFETY: bridge is a live PolyplugLuaLogBridge on this test's stack;
        // empty StringViews are valid (ptr may be dangling-but-aligned for
        // len 0 — from_static of an empty literal handles this).
        unsafe {
            polyplug_lua_log_trampoline(
                &mut bridge as *mut PolyplugLuaLogBridge as *mut c_void,
                1,
                StringView::from_static(b""),
                StringView::from_static(b""),
            );
        }
        assert_eq!(captured.calls, 1);
        assert_eq!(captured.level, 1);
        assert_eq!(captured.scope, "");
        assert_eq!(captured.message, "");
    }

    #[test]
    fn trampoline_is_noop_on_null_user_data() {
        // SAFETY: null user_data is explicitly allowed by the trampoline's
        // contract (no-op); the StringViews point at 'static byte literals.
        unsafe {
            polyplug_lua_log_trampoline(
                ptr::null_mut(),
                3,
                StringView::from_static(b"scope"),
                StringView::from_static(b"message"),
            );
        }
        // Reaching here without a crash IS the assertion.
    }

    #[test]
    fn trampoline_is_noop_on_null_inner_callback() {
        let mut bridge = PolyplugLuaLogBridge {
            callback: None,
            user_data: ptr::null_mut(),
        };
        // SAFETY: bridge is a live PolyplugLuaLogBridge on this test's stack
        // with a null inner callback, which the trampoline must tolerate.
        unsafe {
            polyplug_lua_log_trampoline(
                &mut bridge as *mut PolyplugLuaLogBridge as *mut c_void,
                3,
                StringView::from_static(b"scope"),
                StringView::from_static(b"message"),
            );
        }
        // Reaching here without a crash IS the assertion.
    }
}

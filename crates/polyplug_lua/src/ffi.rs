//! FFI exports for polyplug_lua — `polyplug_lua_loader_create` / `polyplug_lua_loader_free`
//! plus the LuaJIT host-contract bridge (`polyplug_lua_host_vm_dispatch`,
//! `polyplug_lua_host_create_instance`, `polyplug_lua_host_destroy_instance`).
//!
//! # Why the host-contract bridge exists
//! LuaJIT FFI callbacks cannot return structs by value (documented NYI), so a
//! LuaJIT host can never produce the native-dispatch thunk signature
//! `AbiError (*)(const void*, const void*, void*)` nor the VM-dispatch `call`
//! signature (struct parameters AND struct return), nor a `create_instance`
//! returning `HostContractInstance` by value. The generated Lua host interface
//! factories therefore register host contracts with VM dispatch whose `call`
//! points at `polyplug_lua_host_vm_dispatch` below; the trampoline forwards to
//! a scalar-only LuaJIT callback (`u32 (*)(u32, const void*, void*)`) stored in
//! a `PolyplugLuaHostDispatchBridge` carried via `VmDispatch.loader_data`.

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
    /// Scalar-only dispatch callback: `(fn_id, args, out) -> AbiErrorCode as u32`.
    /// LuaJIT can create this callback (no struct-by-value args or return).
    pub callback: Option<unsafe extern "C" fn(u32, *const c_void, *mut c_void) -> u32>,
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
    _instance: GuestContractInstance,
    fn_id: u32,
    args: *const (),
    out: *mut (),
    _arena: *mut CallArena,
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
    let callback: Option<unsafe extern "C" fn(u32, *const c_void, *mut c_void) -> u32> =
        unsafe { (*bridge_ptr).callback };
    match callback {
        Some(cb) => {
            // SAFETY: cb is the LuaJIT callback installed by the generated
            // factory; args/out are forwarded untouched per the dispatch contract.
            let code: u32 = unsafe { cb(fn_id, args as *const c_void, out as *mut c_void) };
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

/// `create_instance` stub for host contracts registered by a LuaJIT host.
///
/// LuaJIT callbacks cannot return `HostContractInstance` by value, so the
/// generated factory installs this native stub. Mirrors the generated Rust VM
/// factory stub: the instance is the registrant-owned `user_data` pointer.
///
/// # Safety
/// `this` must be null or a valid `HostContractInterface` pointer (self-passing
/// pattern; the runtime always passes the registered interface).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_lua_host_create_instance(
    this: *const HostContractInterface,
    _args: *const c_void,
) -> HostContractInstance {
    if this.is_null() {
        return HostContractInstance::null();
    }
    HostContractInstance {
        // SAFETY: this is non-null (checked above) and points at the registered
        // interface per the self-passing ABI contract; user_data is registrant-owned.
        data: unsafe { (*this).user_data },
    }
}

/// `destroy_instance` stub for host contracts registered by a LuaJIT host.
///
/// The instance is the registrant-owned `user_data` (see
/// `polyplug_lua_host_create_instance`) — nothing to free.
///
/// # Safety
/// Always safe; both parameters are ignored.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_lua_host_destroy_instance(
    _this: *const HostContractInterface,
    _instance: HostContractInstance,
) {
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

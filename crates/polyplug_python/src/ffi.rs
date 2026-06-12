// SAFETY: std::ptr/slice/str functions are functionally identical to core versions;
// we use std for consistency with FFI code patterns that may need std features.
#![allow(clippy::std_instead_of_core)]

use core::ffi::c_void;

use polyplug::loader::BundleLoader;
use polyplug::logger::LoggerHandle;
use polyplug_abi::AbiError;
use polyplug_abi::AbiErrorCode;
use polyplug_abi::CallArena;
use polyplug_abi::GuestContractInstance;
use polyplug_abi::HostContractInstance;
use polyplug_abi::HostContractInterface;
use polyplug_abi::StringView;
use polyplug_abi::VmLoaderData;
use polyplug_abi::types::LogLevel;

use crate::{PythonLoader, config::PythonConfig};

/// C-visible configuration passed to `polyplug_python_loader_create`.
///
/// `min_version_ptr` must point to a valid UTF-8 string of `min_version_len`
/// bytes in the form `"<major>.<minor>"` (e.g. `"3.11"`).
#[repr(C)]
pub struct PolyplugPythonConfig {
    pub min_version_ptr: *const u8,
    pub min_version_len: usize,
}

// ─── Host-contract bridge for ctypes hosts ───────────────────────────────────
//
// ctypes callbacks cannot return structs by value ("invalid result type for
// callback function"), so a Python host can never produce the native-dispatch
// thunk signature (`AbiError` return), the VM-dispatch `call` signature
// (struct parameters AND struct return), or a `create_instance` returning
// `HostContractInstance` by value. The generated Python host interface
// factories therefore register host contracts with VM dispatch whose `call`
// points at `polyplug_python_host_vm_dispatch` below; the trampoline forwards
// to a scalar-only ctypes callback (`u32 (*)(u32, const void*, void*)`)
// stored in a `PolyplugPythonHostDispatchBridge` carried via
// `VmDispatch.loader_data`. This mirrors the LuaJIT bridge in
// `crates/polyplug_lua/src/ffi.rs` (same struct-return NYI class).

/// Bridge between the ABI VM-dispatch convention and a ctypes-creatable callback.
#[repr(C)]
pub struct PolyplugPythonHostDispatchBridge {
    /// Scalar-only dispatch callback: `(fn_id, args, out) -> AbiErrorCode as u32`.
    pub callback: Option<unsafe extern "C" fn(u32, *const c_void, *mut c_void) -> u32>,
}

/// VM-dispatch trampoline for host contracts implemented in a ctypes host.
///
/// Matches `VmDispatch.call`. Routes the call to the scalar ctypes callback in
/// the `PolyplugPythonHostDispatchBridge` carried by `loader_data` and widens
/// the returned `u32` code into an `AbiError` (static message).
///
/// # Safety
/// `loader_data.data` must be null or point to a live
/// `PolyplugPythonHostDispatchBridge` that outlives every dispatch through the
/// registered interface (the generated factory anchors it on the interface's
/// keepalive tuple). `args`/`out` follow the per-function ABI marshalling
/// contract and are passed through untouched.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_python_host_vm_dispatch(
    loader_data: VmLoaderData,
    _instance: GuestContractInstance,
    fn_id: u32,
    args: *const (),
    out: *mut (),
    _arena: *mut CallArena,
    out_err: *mut AbiError,
) {
    // SAFETY: loader_data carries the bridge pointer per this function's safety
    // contract; the impl forwards args/out untouched to the ctypes callback.
    let result: AbiError =
        unsafe { polyplug_python_host_vm_dispatch_impl(loader_data, fn_id, args, out) };
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(result) };
    }
}

unsafe fn polyplug_python_host_vm_dispatch_impl(
    loader_data: VmLoaderData,
    fn_id: u32,
    args: *const (),
    out: *mut (),
) -> AbiError {
    let bridge_ptr: *const PolyplugPythonHostDispatchBridge =
        loader_data.data as *const PolyplugPythonHostDispatchBridge;
    if bridge_ptr.is_null() {
        return AbiError {
            code: AbiErrorCode::InvalidPointer as u32,
            message: StringView::from_static(b"python host dispatch bridge is null"),
        };
    }
    // SAFETY: bridge_ptr is non-null (checked above) and points to a live
    // PolyplugPythonHostDispatchBridge per this function's safety contract.
    let callback: Option<unsafe extern "C" fn(u32, *const c_void, *mut c_void) -> u32> =
        unsafe { (*bridge_ptr).callback };
    match callback {
        Some(cb) => {
            // SAFETY: cb is the ctypes callback installed by the generated
            // factory; args/out are forwarded untouched per the dispatch contract.
            let code: u32 = unsafe { cb(fn_id, args as *const c_void, out as *mut c_void) };
            if code == AbiErrorCode::Ok as u32 {
                AbiError::ok()
            } else {
                AbiError {
                    code,
                    message: StringView::from_static(b"python host contract returned error"),
                }
            }
        }
        None => AbiError {
            code: AbiErrorCode::InvalidPointer as u32,
            message: StringView::from_static(b"python host dispatch bridge has no callback"),
        },
    }
}

/// `create_instance` stub for host contracts registered by a ctypes host.
///
/// ctypes callbacks cannot return `HostContractInstance` by value, so the
/// generated factory installs this native stub. The instance is the
/// registrant-owned `user_data` pointer (self-passing pattern).
///
/// # Safety
/// `this` must be null or a valid `HostContractInterface` pointer (the runtime
/// always passes the registered interface).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_python_host_create_instance(
    this: *const HostContractInterface,
    _args: *const c_void,
    out_instance: *mut HostContractInstance,
) {
    let instance: HostContractInstance = if this.is_null() {
        HostContractInstance::null()
    } else {
        HostContractInstance {
            // SAFETY: this is non-null (checked above) and points at the registered
            // interface per the self-passing ABI contract; user_data is registrant-owned.
            data: unsafe { (*this).user_data },
        }
    };
    if !out_instance.is_null() {
        // SAFETY: out_instance is non-null (just checked) and writable per the ABI contract.
        unsafe { out_instance.write(instance) };
    }
}

/// `destroy_instance` stub for host contracts registered by a ctypes host.
///
/// The instance is the registrant-owned `user_data` (see
/// `polyplug_python_host_create_instance`) — nothing to free.
///
/// # Safety
/// Always safe; both parameters are ignored.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_python_host_destroy_instance(
    _this: *const HostContractInterface,
    _instance: HostContractInstance,
) {
}

/// Create a heap-allocated `PythonLoader` and return it as an opaque pointer.
///
/// Returns `null` on any error (null config pointer, null version pointer,
/// non-UTF-8 string, unparseable version string).
///
/// # Safety
/// - `config` must be a valid, non-null pointer to a `PolyplugPythonConfig`.
/// - `config.min_version_ptr` must point to at least `config.min_version_len`
///   readable bytes of valid UTF-8 for the duration of this call.
/// - The returned pointer must be freed by calling `polyplug_python_loader_free`
///   exactly once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_python_loader_create(
    config: *const PolyplugPythonConfig,
) -> *mut c_void {
    if config.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: caller guarantees config is a valid, non-null pointer.
    let cfg: &PolyplugPythonConfig = unsafe { &*config };
    if cfg.min_version_ptr.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: caller guarantees min_version_ptr points to min_version_len
    // valid bytes for the duration of this call.
    let bytes: &[u8] =
        unsafe { std::slice::from_raw_parts(cfg.min_version_ptr, cfg.min_version_len) };
    let version_str: &str = match std::str::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let min_version: (u32, u32) = match parse_version(version_str) {
        Some(v) => v,
        None => return std::ptr::null_mut(),
    };
    let loader: PythonLoader = PythonLoader::new(PythonConfig { min_version });
    // Double-box: inner Box<dyn BundleLoader> preserves the fat pointer (data + vtable),
    // outer Box stores it on the heap so we can pass a thin *mut c_void across FFI.
    let trait_obj: Box<dyn BundleLoader> = Box::new(loader);
    Box::into_raw(Box::new(trait_obj)) as *mut c_void
}

/// Free a `PythonLoader` previously returned by `polyplug_python_loader_create`.
///
/// Passing `null` is a no-op (safe). Passing any other pointer not returned by
/// `polyplug_python_loader_create` is undefined behaviour.
///
/// # Safety
/// - `ptr` must be either null or a pointer previously returned by
///   `polyplug_python_loader_create` and not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_python_loader_free(ptr: *mut c_void) {
    if ptr.is_null() {
        return;
    }
    // SAFETY: ptr was produced by Box::into_raw(Box::new(trait_obj)) inside
    // polyplug_python_loader_create where trait_obj: Box<dyn BundleLoader>.
    // Caller guarantees it has not been freed.
    unsafe {
        drop(Box::<Box<dyn BundleLoader>>::from_raw(
            ptr as *mut Box<dyn BundleLoader>,
        ))
    };
}

/// Parse `"major.minor"`. Runs inside the C FFI loader-create entry point,
/// BEFORE any `Runtime` (and therefore any host logger) exists — diagnostics
/// can only go to the pre-runtime default sink (stderr, Error/Warn).
fn parse_version(s: &str) -> Option<(u32, u32)> {
    let logger: LoggerHandle = LoggerHandle::default_stderr();
    let mut parts = s.splitn(2, '.');
    let major_str: &str = parts.next()?;
    let major: u32 = major_str
        .parse()
        .map_err(|e: std::num::ParseIntError| {
            logger.log(LogLevel::Error, "loader.python", || {
                format!("parse_version: failed to parse major '{major_str}' as u32: {e}")
            });
        })
        .ok()?;
    let minor_str: &str = parts.next()?;
    let minor: u32 = minor_str
        .parse()
        .map_err(|e: std::num::ParseIntError| {
            logger.log(LogLevel::Error, "loader.python", || {
                format!("parse_version: failed to parse minor '{minor_str}' as u32: {e}")
            });
        })
        .ok()?;
    Some((major, minor))
}

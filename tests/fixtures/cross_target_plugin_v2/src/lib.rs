//! Cross-dispatch target plugin (V2).
//!
//! Paired reload target for `cross_target_plugin` (V1). It registers the same
//! bundle identity (`cross_target_plugin`) and the same `cross.target@1`
//! contract, but its `add` returns `a.wrapping_add(b).wrapping_add(1000)`.
//!
//! Hot-reloading V1 → V2 and observing the +1000 delta proves cross-dispatch
//! re-resolves the live interface on every call (per-call routing), mirroring
//! the `reload_plugin_v1` / `reload_plugin_v2` pattern.

use polyplug_abi::AbiErrorCode;
use polyplug_abi::*;
use polyplug_utils::GuestContractId;

/// Delta applied by V2 so its behaviour is distinguishable from V1.
const V2_DELTA: u32 = 1000;

#[repr(C)]
pub struct AddArgs {
    pub a: u32,
    pub b: u32,
}

#[repr(C)]
struct TargetInstance {
    marker: u64,
}

const TARGET_INSTANCE_MARKER: u64 = 0xC0FF_EE00_7A86_E702;

/// Function 0: `add(a, b) -> a.wrapping_add(b).wrapping_add(V2_DELTA)` (V2).
///
/// # Safety
/// `args` must point to a valid `AddArgs`; `out` must point to a valid `u32`.
unsafe extern "C" fn target_add(
    _instance: GuestContractInstance,
    args: *const (),
    out: *mut (),
    out_err: *mut AbiError,
) {
    let __result_err: AbiError = (|| {
        if args.is_null() || out.is_null() {
            return AbiError {
                code: AbiErrorCode::InvalidPointer as u32,
                message: string_view_null(),
            };
        }
        // SAFETY: caller guarantees `args` points to a valid `AddArgs`; non-null
        // checked above.
        let add_args: &AddArgs = unsafe { &*(args as *const AddArgs) };
        let result: u32 = add_args.a.wrapping_add(add_args.b).wrapping_add(V2_DELTA);
        // SAFETY: caller guarantees `out` points to a valid `u32`; non-null checked
        // above.
        unsafe {
            core::ptr::write(out as *mut u32, result);
        }
        abi_error_ok()
    })();
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(__result_err) };
    }
}

/// # Safety
/// `_host`/`_args` follow the ABI calling convention but are unused here.
unsafe extern "C" fn create_instance(
    _host: *const HostApi,
    _args: *const (),
    out_instance: *mut GuestContractInstance,
) {
    let boxed: Box<TargetInstance> = Box::new(TargetInstance {
        marker: TARGET_INSTANCE_MARKER,
    });
    if !out_instance.is_null() {
        // SAFETY: out_instance is non-null (just checked) and writable per the ABI contract.
        unsafe {
            out_instance.write(GuestContractInstance {
                data: Box::into_raw(boxed) as *mut core::ffi::c_void,
                contract_id: GuestContractId::new("cross.target", 1),
            })
        };
    }
}

/// # Safety
/// `instance.data` must be a pointer returned by `create_instance` of this
/// contract, not yet destroyed.
unsafe extern "C" fn destroy_instance(_host: *const HostApi, instance: GuestContractInstance) {
    if instance.data.is_null() {
        return;
    }
    // SAFETY: `instance.data` was produced by this contract's `create_instance`
    // via `Box::into_raw`; reclaiming it once is sound.
    unsafe {
        drop(Box::from_raw(instance.data as *mut TargetInstance));
    }
}

#[repr(transparent)]
pub struct FnPtr(pub *const ());

// SAFETY: `FnPtr` wraps a 'static read-only function pointer; safe to share.
unsafe impl Send for FnPtr {}
// SAFETY: function pointers are inherently Sync (read-only 'static memory).
unsafe impl Sync for FnPtr {}

static TARGET_FNS: [FnPtr; 1] = [FnPtr(target_add as *const ())];

fn target_interface() -> GuestContractInterface {
    GuestContractInterface {
        contract_id: GuestContractId::new("cross.target", 1),
        contract_version: Version {
            major: 2,
            minor: 0,
            patch: 0,
        },
        dispatch_type: DispatchType::Native,
        create_instance,
        destroy_instance,
        dispatch: DispatchMechanisms {
            native: NativeDispatch {
                function_count: 1,
                functions: TARGET_FNS.as_ptr() as *const *const (),
            },
        },
    }
}

static DESCRIPTOR: PluginDescriptor = PluginDescriptor {
    name: StringView {
        ptr: b"cross_target_plugin".as_ptr(),
        len: 19,
    },
    contract_name: StringView {
        ptr: b"cross.target".as_ptr(),
        len: 12,
    },
    version: Version {
        major: 2,
        minor: 0,
        patch: 0,
    },
};

#[unsafe(no_mangle)]
pub extern "C" fn polyplug_abi_version() -> u32 {
    POLYPLUG_ABI_VERSION
}

/// Plugin init — registers the `cross.target@1` contract (V2 behaviour).
///
/// # Safety
/// `host_abi` must be a valid non-null `HostApi` pointer; `ctx` must be a valid
/// non-null `BundleInitContext` pointer (both supplied by the host).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_init(
    host_abi: *const HostApi,
    ctx: *const BundleInitContext,
) -> AbiError {
    if host_abi.is_null() || ctx.is_null() {
        return AbiError {
            code: AbiErrorCode::Generic as u32,
            message: string_view_null(),
        };
    }
    // SAFETY: `host_abi` is non-null and provided by the host runtime.
    let host: &HostApi = unsafe { &*host_abi };
    let interface: GuestContractInterface = target_interface();
    // Out-param ABI: register_guest_contract writes its AbiError through a
    // trailing pointer and returns void; init still surfaces it by value.
    let mut err: AbiError = AbiError {
        code: AbiErrorCode::Ok as u32,
        message: string_view_null(),
    };
    // SAFETY: `register_guest_contract` is a valid host function pointer.
    // `DESCRIPTOR` is 'static; `interface` outlives the synchronous call; &mut err is valid.
    unsafe {
        (host.register_guest_contract)(
            host_abi,
            &DESCRIPTOR as *const PluginDescriptor,
            &interface as *const GuestContractInterface,
            &mut err as *mut AbiError,
        );
    }
    err
}

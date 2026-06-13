//! Runtime test: polyplugc-GENERATED QuickJS guest wrappers marshal
//! NON-StringView signatures end to end.
//!
//! The generated guest ABI wrappers (`render_plugin_interface_quickjs`) were
//! historically hardcoded for StringView→StringView — every other shape was
//! broken by construction and `integration_js.rs` could not catch it because
//! its fixture (`test_plugin_js/bundle.js`) hand-rolls the ABI. This test
//! loads `tests/fixtures/test_plugin_js_generated/` — a bundle whose glue is
//! emitted by polyplugc and bundled by rolldown (see
//! `tests/fixtures/build_all.sh`) — and dispatches every signature shape of
//! `test.add@1` through a real `Runtime`:
//!
//!   fn0 `add(AddArgs { a: u32, b: u32 }) -> u32`   struct-by-value param
//!   fn1 `add_primitive(a: u32, b: u32) -> u32`     multi-scalar C-layout pack
//!   fn2 `version() -> StringView`                  no args, string return
//!   fn3 `reset()`                                  void/void
//!
//! QuickJS is vendored (rquickjs-sys), so this always runs (no skip path).

#![allow(clippy::expect_used)]
#![allow(clippy::undocumented_unsafe_blocks)]

use polyplug::error::RuntimeError;
use polyplug::runtime::Runtime;
use polyplug_abi::AbiError;
use polyplug_abi::AbiErrorCode;
use polyplug_abi::DispatchType;
use polyplug_abi::GuestContractHandle;
use polyplug_abi::GuestContractInstance;
use polyplug_abi::GuestContractInterface;
use polyplug_abi::StringView;
use polyplug_js::JsConfig;
use polyplug_js::JsLoader;
use polyplug_utils::guest_contract_id;
use std::sync::Arc;

const JS_GENERATED_PLUGIN: &str = env!("TEST_JS_GENERATED_PLUGIN");

#[repr(C)]
struct AddArgs {
    a: u32,
    b: u32,
}

/// Dispatch one function on the resolved vtable, mirroring the host-caller
/// convention (`integration_js.rs`): args points at the bare value / C-layout
/// pack, out at a slot sized for the return type, arena absent.
unsafe fn dispatch(
    vtable: &GuestContractInterface,
    fn_id: u32,
    args: *const (),
    out: *mut (),
) -> AbiError {
    let mut err: AbiError = AbiError::ok();
    unsafe {
        (vtable.dispatch.vm.call)(
            vtable.dispatch.vm.loader_data,
            GuestContractInstance::null(),
            fn_id,
            args,
            out,
            core::ptr::null_mut(),
            &mut err as *mut AbiError,
        )
    };
    err
}

#[test]
fn js_generated_guest_marshals_non_stringview_shapes() {
    let rt: Arc<Runtime> = Runtime::builder()
        .loader(JsLoader::new(JsConfig {}))
        .build()
        .expect("failed to build runtime");
    let loaded: Result<(), RuntimeError> =
        rt.load_bundle(std::path::Path::new(JS_GENERATED_PLUGIN));
    assert!(
        loaded.is_ok(),
        "generated-glue bundle must load (rebuild with: bash tests/fixtures/build_all.sh): {:?}",
        loaded.err()
    );

    let contract_id: u64 = guest_contract_id("test.add", 1);
    let handle: GuestContractHandle = rt
        .find_guest_contract(contract_id, 0)
        .expect("test.add must be registered by the generated init");
    let vtable_ptr: *const GuestContractInterface = rt
        .resolve_guest_contract(handle)
        .expect("handle must resolve");
    let vtable: &GuestContractInterface = unsafe { &*vtable_ptr };
    assert_eq!(vtable.dispatch_type, DispatchType::VirtualMachine);

    // fn1: add_primitive(u32, u32) -> u32 — multi-scalar C-layout pack.
    let args: AddArgs = AddArgs { a: 7, b: 35 };
    let mut out: u32 = 0_u32;
    let result: AbiError = unsafe {
        dispatch(
            vtable,
            1,
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };
    assert_eq!(
        result.code,
        AbiErrorCode::Ok as u32,
        "add_primitive must dispatch Ok"
    );
    assert_eq!(out, 42_u32, "add_primitive(7, 35) must equal 42");

    // fn0: add(AddArgs) -> u32 — struct-by-value parameter.
    let args: AddArgs = AddArgs { a: 12, b: 30 };
    let mut out: u32 = 0_u32;
    let result: AbiError = unsafe {
        dispatch(
            vtable,
            0,
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };
    assert_eq!(result.code, AbiErrorCode::Ok as u32, "add must dispatch Ok");
    assert_eq!(out, 42_u32, "add({{12, 30}}) must equal 42");

    // fn2: version() -> StringView — no args, string return.
    let mut out_sv: StringView = StringView {
        ptr: core::ptr::null(),
        len: 0,
    };
    let result: AbiError = unsafe {
        dispatch(
            vtable,
            2,
            core::ptr::null(),
            &mut out_sv as *mut StringView as *mut (),
        )
    };
    assert_eq!(
        result.code,
        AbiErrorCode::Ok as u32,
        "version must dispatch Ok"
    );
    assert!(!out_sv.ptr.is_null(), "version must return a non-null view");
    let version: &str = unsafe {
        core::str::from_utf8(core::slice::from_raw_parts(out_sv.ptr, out_sv.len))
            .expect("version must be UTF-8")
    };
    assert_eq!(version, "test_adder 1.0.0");

    // fn3: reset() — void/void.
    let result: AbiError = unsafe { dispatch(vtable, 3, core::ptr::null(), core::ptr::null_mut()) };
    assert_eq!(
        result.code,
        AbiErrorCode::Ok as u32,
        "reset must dispatch Ok"
    );
}

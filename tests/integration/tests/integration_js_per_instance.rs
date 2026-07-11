//! Integration test: JavaScript (QuickJS) VM guests have **real per-instance state**.
//!
//! # Why this exists
//!
//! JS contracts dispatch through the `polyplug_js` QuickJS loader. Before the
//! per-instance work, the loader's `create_instance` was a null stub and
//! `destroy_instance` a no-op, and dispatch ignored the instance handle — so every
//! "instance" of a JS contract shared one implementation object. That is a latent
//! correctness bug for any stateful JS plugin.
//!
//! The loader now owns per-instance state: `create_instance` calls the contract's
//! factory (carried on the registered vtable's `factory` field and reached via the
//! interface's `loader_data`) to build a fresh impl object, mints a non-zero
//! instance id, and persists the impl keyed by that id in a per-contract registry;
//! dispatch resolves the impl from the instance handle and passes it as the JS
//! handler's first argument; `destroy_instance` drops it. A null instance handle
//! resolves to a per-contract default impl built once at load (stateless / low-level
//! paths).
//!
//! This test loads ONE stateful `iso.Counter@1` bundle into ONE runtime, creates
//! TWO instances through the runtime's host-mediated `HostApi.create_guest_instance`
//! (the exact path the generated host/peer callers use), advances each a different
//! number of times, and asserts their counts are independent. If instances shared
//! state, both would observe the combined total.
//!
//! The bundle is a hand-written `bundle.js` returning `[registrations, abiError]`
//! directly — exactly the registration shape `polyplugc` generates and the other JS
//! integration tests use — so the test needs no rolldown/deno bundling step.

#![allow(clippy::expect_used)]
#![allow(clippy::undocumented_unsafe_blocks)]

use polyplug::runtime::Runtime;
use polyplug_abi::AbiError;
use polyplug_abi::AbiErrorCode;
use polyplug_abi::DispatchType;
use polyplug_abi::GuestContractHandle;
use polyplug_abi::GuestContractInstance;
use polyplug_abi::GuestContractInterface;
use polyplug_abi::HostApi;

use polyplug_js::JsLoader;
use polyplug_utils::bundle_id;
use polyplug_utils::guest_contract_id;
use std::path::PathBuf;
use std::sync::Arc;

const BUNDLE_NAME: &str = "iso_counter_js";

/// `iso.Counter@1`: `inc() -> i32` (advance and return the new count) and
/// `get() -> i32` (read the current count). Both are no-arg, so the only state is
/// the per-instance counter — the per-instance discriminator. The factory builds a
/// fresh `{ count: 0 }` per instance, so two instances are independent.
fn counter_bundle_js() -> String {
    let contract_id: u64 = guest_contract_id("iso.Counter", 1);
    let contract_lo: u32 = contract_id as u32;
    let contract_hi: u32 = (contract_id >> 32) as u32;
    format!(
        r#"
function polyplug_init(host_lo, host_hi, ctx_lo, ctx_hi, bridge) {{
    var iface = {{
        contractLo: {contract_lo},
        contractHi: {contract_hi},
        fnCount: 2,
        contractName: "iso.Counter@1",
        version: 0x00010000,
        // A fresh impl object per instance: the counter lives here, so two
        // instances never share state. The factory receives the bridge + host
        // vtable explicitly (no global — Rule 12).
        factory: function(bridge, hostLo, hostHi) {{ return {{ count: 0 }}; }},
        functions: [
            // fn0 = inc(): advance this instance's own counter, return the new value.
            function(impl, args_ptr, out_ptr, arena, bridge) {{
                impl.count = impl.count + 1;
                bridge.writeI32(out_ptr, impl.count);
                return 0;
            }},
            // fn1 = get(): read this instance's own counter.
            function(impl, args_ptr, out_ptr, arena, bridge) {{
                bridge.writeI32(out_ptr, impl.count);
                return 0;
            }}
        ]
    }};
    var registrations = [{{
        contractLo: iface.contractLo,
        contractHi: iface.contractHi,
        interface: iface,
        fnCount: iface.fnCount,
        contractName: iface.contractName,
        version: iface.version
    }}];
    return [registrations, {{ code: 0, message: "" }}];
}}
"#
    )
}

/// Write the `iso.Counter@1` JS bundle (flat `bundle.js` + manifest) into a temp
/// dir and return the bundle directory.
fn write_counter_bundle(tmp: &std::path::Path) -> PathBuf {
    let dir: PathBuf = tmp.join("counter");
    std::fs::create_dir_all(&dir).expect("create counter bundle dir");
    std::fs::write(dir.join("bundle.js"), counter_bundle_js()).expect("write bundle.js");

    let id_val: u64 = bundle_id(BUNDLE_NAME);
    let manifest: String = format!(
        "id = {id_val}\n\
         name = \"{BUNDLE_NAME}\"\n\
         loader = \"js-quickjs\"\n\
         file = \"bundle.js\"\n\
         version = \"1.0.0\"\n\
         provides = [\"iso.Counter@1\"]\n\
         needs_reinit_on_dep_reload = false\n\n\
         [function_count]\n\
         \"iso.Counter@1\" = 2\n",
    );
    std::fs::write(dir.join("manifest.toml"), manifest).expect("write manifest.toml");
    dir
}

/// Resolve the live `iso.Counter@1` interface in `rt`.
fn resolve_counter(rt: &Runtime) -> *const GuestContractInterface {
    let contract_id: u64 = guest_contract_id("iso.Counter", 1);
    let handle: GuestContractHandle = rt
        .find_guest_contract(contract_id, 0)
        .expect("iso.Counter must be registered after load");
    let vtable_ptr: *const GuestContractInterface = rt
        .resolve_guest_contract(handle)
        .expect("handle must resolve to a live interface");
    // SAFETY: the interface is live for the loaded bundle; the QuickJS VM stays
    // loaded for the runtime lifetime.
    let vtable: &GuestContractInterface = unsafe { &*vtable_ptr };
    assert_eq!(
        vtable.dispatch_type,
        DispatchType::VirtualMachine,
        "js loader must use VM dispatch"
    );
    vtable_ptr
}

/// Dispatch a no-arg `i32`-returning function (`fn_id`) on a specific `instance`
/// and return the value the guest wrote, asserting the call succeeded.
fn dispatch_no_arg_i32(
    vtable_ptr: *const GuestContractInterface,
    instance: GuestContractInstance,
    fn_id: u32,
) -> i32 {
    // SAFETY: vtable_ptr is a live interface from `resolve_counter`.
    let vtable: &GuestContractInterface = unsafe { &*vtable_ptr };
    let mut out: i32 = 0;
    let mut err: AbiError = AbiError::ok();
    // SAFETY: VM dispatch is active (asserted in `resolve_counter`); the function
    // takes no args (null `args`), `out` points to a live i32 matching the declared
    // return, a null arena selects the host-alloc fallback, and `instance` was
    // produced by this contract's create_instance. All outlive the call.
    unsafe {
        (vtable.dispatch.vm.call)(
            vtable.adapter_context,
            vtable.dispatch.vm.loader_data,
            instance,
            fn_id,
            core::ptr::null(),
            &mut out as *mut i32 as *mut (),
            core::ptr::null_mut(),
            &mut err as *mut AbiError,
        )
    };
    assert_eq!(
        err.code,
        AbiErrorCode::Ok as u32,
        "iso.Counter dispatch (fn {fn_id}) must succeed"
    );
    out
}

/// One runtime, one stateful contract, two live instances advanced a different
/// number of times. Independent counts prove the loader keys state per instance.
#[test]
fn two_instances_of_one_js_contract_have_independent_state() {
    const INC_FN: u32 = 0;
    const GET_FN: u32 = 1;

    let tmp: tempfile::TempDir = tempfile::TempDir::new().expect("tempdir");
    let bundle: PathBuf = write_counter_bundle(tmp.path());

    let loader: JsLoader = JsLoader::new();
    let rt: Arc<Runtime> = Runtime::builder()
        .loader(loader)
        .build()
        .expect("build runtime");
    rt.load_bundle(&bundle).expect("load must succeed");

    let vtable_ptr: *const GuestContractInterface = resolve_counter(&rt);

    // Create two instances through the runtime's host-mediated path (the exact
    // mechanism the generated host/peer callers use — it fills in loader_data).
    let host: *const HostApi = rt.host_abi();
    // SAFETY: `host` is the runtime's own non-null 'static HostApi pointer.
    let host_api: &HostApi = unsafe { &*host };

    let mut instance_a: GuestContractInstance = GuestContractInstance::null();
    let mut instance_b: GuestContractInstance = GuestContractInstance::null();
    // SAFETY: `host`/`vtable_ptr` are valid; null `args` is honoured by the factory;
    // each instance is written through the trailing out-param.
    unsafe {
        (host_api.create_guest_instance)(
            host,
            vtable_ptr,
            core::ptr::null(),
            &mut instance_a as *mut GuestContractInstance,
        );
        (host_api.create_guest_instance)(
            host,
            vtable_ptr,
            core::ptr::null(),
            &mut instance_b as *mut GuestContractInstance,
        );
    }
    assert!(
        !instance_a.data.is_null() && !instance_b.data.is_null(),
        "iso.Counter is stateful: create_guest_instance must return non-null data"
    );
    assert_ne!(
        instance_a.data, instance_b.data,
        "two instances must have distinct handles"
    );

    // Advance A three times, B once.
    for expected in 1..=3 {
        assert_eq!(
            dispatch_no_arg_i32(vtable_ptr, instance_a, INC_FN),
            expected,
            "instance A inc must return its own running count"
        );
    }
    assert_eq!(
        dispatch_no_arg_i32(vtable_ptr, instance_b, INC_FN),
        1,
        "instance B's first inc must be 1, unaffected by A's increments"
    );

    // Reads must reflect each instance's OWN count, not a shared total.
    assert_eq!(
        dispatch_no_arg_i32(vtable_ptr, instance_a, GET_FN),
        3,
        "instance A must keep its own count of 3"
    );
    assert_eq!(
        dispatch_no_arg_i32(vtable_ptr, instance_b, GET_FN),
        1,
        "instance B must keep its own count of 1 (state is NOT shared)"
    );

    // The null/default instance is a separate impl, untouched by A and B.
    assert_eq!(
        dispatch_no_arg_i32(vtable_ptr, GuestContractInstance::null(), GET_FN),
        0,
        "the stateless default instance must be independent of A and B"
    );

    // SAFETY: both instances were produced by create_guest_instance above and not
    // yet destroyed; the interface stays valid for the runtime's lifetime.
    unsafe {
        (host_api.destroy_guest_instance)(host, vtable_ptr, instance_a);
        (host_api.destroy_guest_instance)(host, vtable_ptr, instance_b);
    }
}

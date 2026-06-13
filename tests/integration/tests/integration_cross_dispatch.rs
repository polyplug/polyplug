//! Cross-dispatch integration tests — plugin→plugin calls through real loaded
//! bundles, routed by `HostApi::call_guest_method` / `Runtime::call_guest_method`.
//!
//! These exercise the host-mediated cross-dispatch path against genuinely loaded
//! cdylib (and VM) bundles, not mocked interfaces:
//!
//! * native↔native — `cross.caller` (loaded bundle) calls `cross.target`
//!   (loaded bundle) through `host->call_guest_method` inside its own dispatch
//!   function, proving the full guest→host→guest chain.
//! * NotFound — cross-calling an unloaded contract id yields
//!   `AbiErrorCode::NotFound`.
//! * VM routing — `Runtime::call_guest_method` into a loaded Lua bundle takes the
//!   6-arg `vm.call` path and returns the right value.
//! * reload routing — per-call re-resolution lands the second call on the
//!   hot-reloaded V2 behaviour.

#![allow(clippy::expect_used)]
#![allow(clippy::undocumented_unsafe_blocks)]

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use polyplug::runtime::Runtime;
use polyplug_abi::AbiErrorCode;
use polyplug_abi::GuestContractHandle;
use polyplug_abi::GuestContractInstance;
use polyplug_abi::GuestContractInterface;
use polyplug_abi::HostApi;
use polyplug_abi::runtime::RuntimeConfig;
use polyplug_lua::LuaConfig;
use polyplug_lua::LuaLoader;
use polyplug_native::NativeConfig;
use polyplug_native::NativeLoader;
use polyplug_utils::GuestContractId;
use polyplug_utils::guest_contract_id;

const CROSS_CALLER_PLUGIN_DIR: &str = env!("CROSS_CALLER_PLUGIN_DIR");
const CROSS_TARGET_PLUGIN_DIR: &str = env!("CROSS_TARGET_PLUGIN_DIR");
const CROSS_TARGET_PLUGIN_V2_DIR: &str = env!("CROSS_TARGET_PLUGIN_V2_DIR");
const CROSS_TARGET_PLUGIN_V2_SO: &str = env!("CROSS_TARGET_PLUGIN_V2_SO");
const LUA_PLUGIN: &str = env!("TEST_LUA_PLUGIN");
const TEST_PLUGIN_DIR: &str = env!("TEST_PLUGIN_DIR");

/// Wire layout shared with both cross-dispatch fixtures' `add(a, b)`.
#[repr(C)]
struct AddArgs {
    a: u32,
    b: u32,
}

fn native_runtime() -> Arc<Runtime> {
    Runtime::builder()
        .config(RuntimeConfig {
            hot_reload_enabled: true,
            ..RuntimeConfig::default()
        })
        .loader(NativeLoader::new(NativeConfig::default()))
        .build()
        .expect("build runtime with native loader")
}

fn lua_runtime() -> Arc<Runtime> {
    Runtime::builder()
        .loader(LuaLoader::new(LuaConfig::default()))
        .build()
        .expect("build runtime with lua loader")
}

fn resolve_interface(rt: &Runtime, contract_id: u64) -> *const GuestContractInterface {
    let handle: GuestContractHandle = rt
        .find_guest_contract(contract_id, 0)
        .expect("contract must be registered after load");
    rt.resolve_guest_contract(handle)
        .expect("handle must resolve to a live interface")
}

// ─── (a) native ↔ native through real loaded bundles ──────────────────────────

#[test]
fn native_to_native_cross_call() {
    let rt: Arc<Runtime> = native_runtime();
    rt.load_bundle(Path::new(CROSS_TARGET_PLUGIN_DIR))
        .expect("load cross_target_plugin");
    rt.load_bundle(Path::new(CROSS_CALLER_PLUGIN_DIR))
        .expect("load cross_caller_plugin");

    let caller_id: u64 = guest_contract_id("cross.caller", 1);
    let caller_iface_ptr: *const GuestContractInterface = resolve_interface(&rt, caller_id);
    // SAFETY: `caller_iface_ptr` came from `resolve_guest_contract` for a live
    // handle; the runtime is kept alive for the whole test, so the registry
    // storage behind it outlives this borrow.
    let caller_iface: &GuestContractInterface = unsafe { &*caller_iface_ptr };

    // Create a real caller instance so it captures the runtime's HostApi pointer.
    let host: *const HostApi = rt.host_abi();
    // SAFETY: `create_instance` is a valid factory pointer; `host` is the
    // runtime's own 'static HostApi pointer (non-null) per the ABI contract;
    // the instance is written through the trailing out-param.
    let mut caller_instance: GuestContractInstance = GuestContractInstance::null();
    unsafe {
        (caller_iface.create_instance)(
            host,
            core::ptr::null(),
            &mut caller_instance as *mut GuestContractInstance,
        )
    };
    assert!(
        !caller_instance.is_null(),
        "cross.caller create_instance must produce a non-null instance"
    );

    // Dispatch the caller's fn 0; internally it cross-calls cross.target via
    // host->call_guest_method and returns the target's add result.
    let args: AddArgs = AddArgs { a: 7, b: 35 };
    let mut out: u32 = 0;
    // SAFETY: cross.caller dispatch uses the native 3-arg ABI; `caller_instance`
    // is a live instance for that contract; `args`/`out` match its wire layout.
    let err: polyplug_abi::AbiError = unsafe {
        rt.call_guest_method(
            caller_instance,
            0,
            &args as *const AddArgs as *const core::ffi::c_void,
            &mut out as *mut u32 as *mut core::ffi::c_void,
            core::ptr::null_mut(),
        )
    };
    assert_eq!(
        err.code,
        AbiErrorCode::Ok as u32,
        "cross-call must succeed (V1 target)"
    );
    assert_eq!(
        out, 42,
        "result must prove the cross-call reached cross.target V1 (7 + 35)"
    );

    // SAFETY: `caller_instance` was produced by this contract's create_instance
    // and has not been destroyed; destroy it once.
    unsafe { (caller_iface.destroy_instance)(host, caller_instance) };
}

// ─── (a2) native test.add through the runtime's real dispatch path ─────────────

/// Regression: dispatch `test_plugin`'s `test.add` function 0 through the
/// runtime's genuine native-dispatch path (`Runtime::call_guest_method`), which
/// transmutes every native slot to the frozen 3-arg signature
/// `extern "C" fn(GuestContractInstance, *const (), *mut ()) -> AbiError`.
///
/// `test_plugin`'s `plugin_add` previously hand-wrote the stale 2-arg form
/// `fn(*const (), *mut ())`. Dispatching it here would then call a 2-arg fn with
/// 3 args: on the SysV ABI the instance handle lands in the first integer
/// register and `args`/`out` shift, so `out` is written through the wrong
/// pointer — silent memory corruption / SIGSEGV. No prior test dispatched
/// `test.add` function 0 through the runtime, so the bug stayed latent. With the
/// fixture corrected to the canonical 3-arg signature, this call returns Ok and
/// writes `a + b` to the real `out`.
///
/// `test.add`'s `create_instance` is a stateless stub returning a fully-null
/// handle (`contract_id == 0`), which the runtime would route to `NotFound`.
/// Routing keys solely on `instance.contract_id`, so the instance here carries
/// `test.add`'s id with a null `data` (an explicitly valid stateless handle) to
/// reach the contract's interface and exercise the transmute-and-call path.
#[test]
fn native_test_add_through_runtime_dispatch() {
    let rt: Arc<Runtime> = native_runtime();
    rt.load_bundle(Path::new(TEST_PLUGIN_DIR))
        .expect("load test_plugin");

    let instance: GuestContractInstance = GuestContractInstance {
        data: core::ptr::null_mut(),
        contract_id: GuestContractId::new("test.add", 1),
    };

    let args: AddArgs = AddArgs { a: 3, b: 5 };
    let mut out: u32 = 0;
    // SAFETY: routing keys on `instance.contract_id` (test.add@1, a single loaded
    // provider); a null `data` is a valid stateless handle. `args`/`out` match
    // test.add's `add(a, b) -> u32` wire layout. The runtime transmutes slot 0 to
    // the frozen native 3-arg signature and calls it — the production path.
    let err: polyplug_abi::AbiError = unsafe {
        rt.call_guest_method(
            instance,
            0,
            &args as *const AddArgs as *const core::ffi::c_void,
            &mut out as *mut u32 as *mut core::ffi::c_void,
            core::ptr::null_mut(),
        )
    };
    assert_eq!(
        err.code,
        AbiErrorCode::Ok as u32,
        "test.add dispatch through the runtime must return Ok"
    );
    assert_eq!(
        out, 8,
        "add(3, 5) must write 8 to the real out pointer (proves 3-arg ABI parity)"
    );
}

// ─── (b) NotFound path ────────────────────────────────────────────────────────

#[test]
fn cross_call_unloaded_contract_returns_not_found() {
    let rt: Arc<Runtime> = native_runtime();
    // Load only the caller; the target contract is intentionally absent.
    rt.load_bundle(Path::new(CROSS_CALLER_PLUGIN_DIR))
        .expect("load cross_caller_plugin");

    // Fabricate an instance stamped with an unloaded contract id. Routing keys on
    // contract_id, so re-resolution is what must fail (NotFound); instance.data is
    // irrelevant here (null data is valid for stateless contracts).
    let unknown: GuestContractInstance = GuestContractInstance {
        data: core::ptr::null_mut(),
        contract_id: GuestContractId::new("cross.target", 1),
    };
    let args: AddArgs = AddArgs { a: 1, b: 2 };
    let mut out: u32 = 0;
    // SAFETY: the call must fail to resolve before any dispatch occurs, so the
    // (unused) args/out pointers are never read by a target.
    let err: polyplug_abi::AbiError = unsafe {
        rt.call_guest_method(
            unknown,
            0,
            &args as *const AddArgs as *const core::ffi::c_void,
            &mut out as *mut u32 as *mut core::ffi::c_void,
            core::ptr::null_mut(),
        )
    };
    assert_eq!(
        err.code,
        AbiErrorCode::NotFound as u32,
        "cross-calling an unloaded contract must return NotFound"
    );
}

// ─── (c) VM routing via Runtime::call_guest_method ────────────────────────────

#[test]
fn cross_call_routes_into_loaded_lua_vm() {
    let rt: Arc<Runtime> = lua_runtime();
    rt.load_bundle(Path::new(LUA_PLUGIN))
        .expect("load lua test bundle");

    let add_id: u64 = guest_contract_id("test.add", 1);
    // Fabricate a VM instance handle: the Lua loader's dispatch is stateless and
    // ignores instance data, so a non-null marker plus the correct contract_id
    // is the host-side handle that selects the loaded Lua contract.
    let mut marker: u8 = 0;
    let lua_instance: GuestContractInstance = GuestContractInstance {
        data: &mut marker as *mut u8 as *mut core::ffi::c_void,
        contract_id: GuestContractId::from_u64(add_id),
    };

    let args: AddArgs = AddArgs { a: 3, b: 5 };
    let mut out: u32 = 0;
    // SAFETY: fn 0 of the Lua test.add contract is `add(AddArgs) -> u32`; args/out
    // match. A null arena selects the VM per-value fallback path.
    let err: polyplug_abi::AbiError = unsafe {
        rt.call_guest_method(
            lua_instance,
            0,
            &args as *const AddArgs as *const core::ffi::c_void,
            &mut out as *mut u32 as *mut core::ffi::c_void,
            core::ptr::null_mut(),
        )
    };
    assert_eq!(
        err.code,
        AbiErrorCode::Ok as u32,
        "VM-routed cross-call must succeed"
    );
    assert_eq!(out, 8, "VM routing must reach the real Lua add(3, 5)");
}

// ─── (d) reload-during-cross-call routing ─────────────────────────────────────

#[test]
fn cross_call_reresolves_after_reload() {
    let rt: Arc<Runtime> = native_runtime();
    rt.load_bundle(Path::new(CROSS_TARGET_PLUGIN_DIR))
        .expect("load cross_target_plugin V1");

    let target_id: u64 = guest_contract_id("cross.target", 1);
    let target_iface_ptr: *const GuestContractInterface = resolve_interface(&rt, target_id);
    // SAFETY: live interface pointer; runtime outlives the borrow.
    let target_iface: &GuestContractInterface = unsafe { &*target_iface_ptr };
    let host: *const HostApi = rt.host_abi();
    // SAFETY: valid factory; `host` is the runtime's non-null 'static HostApi;
    // the instance is written through the trailing out-param.
    let mut target_instance: GuestContractInstance = GuestContractInstance::null();
    unsafe {
        (target_iface.create_instance)(
            host,
            core::ptr::null(),
            &mut target_instance as *mut GuestContractInstance,
        )
    };
    assert!(
        !target_instance.is_null(),
        "target instance must be non-null"
    );

    let call = |inst: GuestContractInstance| -> (u32, u32) {
        let args: AddArgs = AddArgs { a: 10, b: 20 };
        let mut out: u32 = 0;
        // SAFETY: cross.target fn 0 is `add(AddArgs) -> u32`; args/out match and
        // `inst` belongs to that contract.
        let err: polyplug_abi::AbiError = unsafe {
            rt.call_guest_method(
                inst,
                0,
                &args as *const AddArgs as *const core::ffi::c_void,
                &mut out as *mut u32 as *mut core::ffi::c_void,
                core::ptr::null_mut(),
            )
        };
        (err.code, out)
    };

    let (code_v1, out_v1): (u32, u32) = call(target_instance);
    assert_eq!(code_v1, AbiErrorCode::Ok as u32, "V1 call must succeed");
    assert_eq!(out_v1, 30, "V1 add(10, 20) must equal 30");

    // Destroy the V1 instance before reloading; create a fresh handle for V2.
    // SAFETY: `target_instance` was produced by V1's create_instance, not freed.
    unsafe { (target_iface.destroy_instance)(host, target_instance) };

    rt.reload_bundle(&PathBuf::from(CROSS_TARGET_PLUGIN_V2_SO))
        .expect("hot-reload cross_target_plugin V1 -> V2");
    // Touch the V2 bundle dir so its provisioning path is referenced.
    assert!(
        Path::new(CROSS_TARGET_PLUGIN_V2_DIR).exists(),
        "V2 bundle dir must exist"
    );

    // Re-resolve after reload and build a fresh instance from the live (V2)
    // interface, then cross-call again. The +1000 delta proves per-call routing
    // landed on V2.
    let target_iface_ptr_v2: *const GuestContractInterface = resolve_interface(&rt, target_id);
    // SAFETY: live interface pointer post-reload; runtime outlives the borrow.
    let target_iface_v2: &GuestContractInterface = unsafe { &*target_iface_ptr_v2 };
    // SAFETY: valid factory; `host` is the runtime's non-null 'static HostApi;
    // the instance is written through the trailing out-param.
    let mut target_instance_v2: GuestContractInstance = GuestContractInstance::null();
    unsafe {
        (target_iface_v2.create_instance)(
            host,
            core::ptr::null(),
            &mut target_instance_v2 as *mut GuestContractInstance,
        )
    };
    assert!(
        !target_instance_v2.is_null(),
        "V2 target instance must be non-null"
    );

    let (code_v2, out_v2): (u32, u32) = call(target_instance_v2);
    assert_eq!(code_v2, AbiErrorCode::Ok as u32, "V2 call must succeed");
    assert_eq!(
        out_v2, 1030,
        "second call must land on V2 behaviour (10 + 20 + 1000)"
    );

    // SAFETY: produced by V2's create_instance, not freed.
    unsafe { (target_iface_v2.destroy_instance)(host, target_instance_v2) };
}

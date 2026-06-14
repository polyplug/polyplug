//! Integration test: Lua VM guests have **real per-instance state**.
//!
//! # Why this exists
//!
//! Lua contracts dispatch through the `polyplug_lua` VM loader. Before the
//! per-instance work, the loader's `create_instance` was a null stub and
//! `destroy_instance` a no-op, and dispatch ignored the instance handle — so every
//! "instance" of a Lua contract shared one implementation table. That is a latent
//! correctness bug for any stateful Lua plugin.
//!
//! The loader now owns per-instance state: `create_instance` calls the contract's
//! author factory (carried through the handler entry's `factory` field and reached
//! via the interface's `loader_data`) to build a fresh impl, mints a non-zero
//! instance id, and keys the impl (as an mlua `RegistryKey`) in a per-contract
//! registry; dispatch resolves the impl from the instance handle and passes it as
//! the Lua handler's first argument; `destroy_instance` drops it. A null instance
//! handle resolves to a per-contract default impl built once at load (stateless /
//! low-level paths).
//!
//! This test loads ONE stateful `iso.Counter@1` bundle into ONE runtime, creates
//! TWO instances through the runtime's host-mediated `HostApi.create_guest_instance`
//! (the exact path the generated host/peer callers use), advances each a different
//! number of times, and asserts their counts are independent. If instances shared
//! state, both would observe the combined total.

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
use polyplug_codegen::GenerateConfig;
use polyplug_codegen::GenerateOutput;
use polyplug_codegen::Lang;
use polyplug_codegen::Side;
use polyplug_lua::LuaConfig;
use polyplug_lua::LuaLoader;
use polyplug_utils::guest_contract_id;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

const BUNDLE_NAME: &str = "iso_counter_lua";

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("parent of tests/integration")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

/// `iso.Counter@1`: `inc() -> i32` (advance and return the new count) and
/// `get() -> i32` (read the current count). Both are no-arg, so the only state is
/// the per-instance counter — the per-instance discriminator.
const COUNTER_API_TOML: &str = "[[plugin_contract]]\n\
     name = \"iso.Counter\"\n\
     version = \"1.0.0\"\n\n\
     [[plugin_contract.functions]]\n\
     name = \"inc\"\n\
     return = \"i32\"\n\n\
     [[plugin_contract.functions]]\n\
     name = \"get\"\n\
     return = \"i32\"\n";

/// Write the stateful `iso.Counter@1` Lua bundle into `tmp/<dir_name>`: generate
/// the guest glue, write the hand-written impl whose counter lives on the
/// instance, vendor the current guest SDK, and return the bundle directory.
fn write_counter_bundle(tmp: &Path, dir_name: &str) -> PathBuf {
    let bundle_dir: PathBuf = tmp.join(dir_name);
    std::fs::create_dir_all(&bundle_dir).expect("create counter bundle dir");

    std::fs::write(bundle_dir.join("api.toml"), COUNTER_API_TOML).expect("write api.toml");

    let bundle_toml: String = format!(
        "[bundle]\n\
         name = \"{name}\"\n\
         version = \"1.0.0\"\n\
         api = \"api.toml\"\n\
         loader = \"lua\"\n\
         file = \"entry.lua\"\n\n\
         [[plugin]]\n\
         name = \"counter\"\n\
         version = \"1.0.0\"\n\
         implements = [\"iso.Counter@1.0\"]\n",
        name = BUNDLE_NAME,
    );
    let bundle_toml_path: PathBuf = bundle_dir.join("bundle.toml");
    std::fs::write(&bundle_toml_path, bundle_toml).expect("write bundle.toml");

    let gen_dir: PathBuf = bundle_dir.join("generated");
    let config: GenerateConfig = GenerateConfig {
        api_toml: bundle_toml_path,
        lang: Lang::Lua,
        side: Side::Guest,
        out_dir: gen_dir.clone(),
    };
    let output: GenerateOutput = polyplugc::generate(config).expect("polyplugc generate (lua)");
    for file in &output.files {
        let file_path: PathBuf = gen_dir.join(&file.path);
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).expect("create generated parent dir");
        }
        std::fs::write(&file_path, &file.content).expect("write generated file");
    }

    std::fs::rename(
        gen_dir.join("manifest.toml"),
        bundle_dir.join("manifest.toml"),
    )
    .expect("move manifest.toml to bundle root");

    // The only hand-written source: a stateful impl whose counter lives on the
    // instance. The factory builds a fresh impl per create_instance, so two
    // instances are independent.
    let entry_lua: &str = "local contracts = require('generated.guest.contracts')\n\
         \n\
         local function new_counter(host)\n\
         \x20   local self = { count = 0 }\n\
         \x20   function self:inc()\n\
         \x20       self.count = self.count + 1\n\
         \x20       return self.count\n\
         \x20   end\n\
         \x20   function self:get()\n\
         \x20       return self.count\n\
         \x20   end\n\
         \x20   return self\n\
         end\n\
         \n\
         contracts.set_counter_factory(new_counter)\n\
         \n\
         return contracts\n";
    std::fs::write(bundle_dir.join("entry.lua"), entry_lua).expect("write entry.lua");

    vendor_lua_sdk(&bundle_dir);

    bundle_dir
}

/// Vendor the CURRENT lua guest SDK into the bundle dir. The loader already puts
/// the SDK source dirs on `package.path`, but vendoring mirrors the on-disk
/// example layout and the existing `integration_peer_caller_lua` harness, keeping
/// `require("polyplug_guest")` / `require("polyplug_abi")` resolvable from the
/// bundle directory.
fn vendor_lua_sdk(bundle_dir: &Path) {
    let sdk_root: PathBuf = workspace_root().join("sdks").join("lua");

    // polyplug_guest.lua + polyplug_abi.lua at bundle root.
    std::fs::copy(
        sdk_root.join("guest").join("polyplug_guest.lua"),
        bundle_dir.join("polyplug_guest.lua"),
    )
    .expect("vendor polyplug_guest.lua");
    std::fs::copy(
        sdk_root.join("abi").join("polyplug_abi.lua"),
        bundle_dir.join("polyplug_abi.lua"),
    )
    .expect("vendor polyplug_abi.lua");

    // abi.lua — required as `require("abi")` by polyplug_abi.lua — at bundle root.
    std::fs::copy(
        sdk_root.join("abi").join("abi.lua"),
        bundle_dir.join("abi.lua"),
    )
    .expect("vendor abi.lua");
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
    // SAFETY: the interface is live for the loaded bundle; the lua VM stays loaded
    // for the runtime lifetime.
    let vtable: &GuestContractInterface = unsafe { &*vtable_ptr };
    assert_eq!(
        vtable.dispatch_type,
        DispatchType::VirtualMachine,
        "lua loader must use VM dispatch"
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
fn two_instances_of_one_lua_contract_have_independent_state() {
    const INC_FN: u32 = 0;
    const GET_FN: u32 = 1;

    let tmp: tempfile::TempDir = tempfile::TempDir::new().expect("tempdir");
    let bundle: PathBuf = write_counter_bundle(tmp.path(), "counter");

    let loader: LuaLoader = LuaLoader::new(LuaConfig::default());
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

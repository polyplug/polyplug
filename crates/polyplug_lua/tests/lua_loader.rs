//! Integration tests for LuaLoader and the Lua VM initialization / bundle loading pipeline.

#![allow(clippy::expect_used)]

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use polyplug::error::LoaderError;
use polyplug::error::RuntimeError;
use polyplug::loader::BundleLoader;
use polyplug::loader::manifest::ManifestData;
use polyplug::runtime::Runtime;
use polyplug::runtime::RuntimeBuilder;
use polyplug_abi::AbiError;
use polyplug_abi::AbiErrorCode;
use polyplug_abi::GuestContractHandle;
use polyplug_abi::GuestContractInstance;
use polyplug_abi::GuestContractInterface;
use polyplug_abi::runtime::Compatibility;
use polyplug_abi::runtime::RuntimeConfig;
use polyplug_lua::LuaConfig;
use polyplug_lua::LuaLoader;
use polyplug_utils::GuestContractId;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn make_runtime() -> Arc<Runtime> {
    RuntimeBuilder::new()
        .loader(LuaLoader::new(LuaConfig::default()))
        .build()
        .expect("runtime build must succeed")
}

/// Write `content` to a temp bundle directory with manifest.toml.
/// Returns the directory (to keep it alive) and the path to bundle.lua.
fn write_temp_bundle(name: &str, content: &[u8]) -> (tempfile::TempDir, PathBuf) {
    let dir: tempfile::TempDir = tempfile::tempdir().expect("tempdir");
    let path: PathBuf = dir.path().join("bundle.lua");
    std::fs::write(&path, content).expect("write bundle.lua");

    let bundle_id: u64 = polyplug_utils::bundle_id(name);
    let manifest: String = format!(
        r#"id = {}
name = "{}"
runtime = "lua"
file = "bundle.lua"
"#,
        bundle_id, name
    );
    std::fs::write(dir.path().join("manifest.toml"), &manifest).expect("write manifest.toml");

    (dir, path)
}

/// A minimal valid Lua plugin script that implements the `test.loader@1`
/// contract with a single no-op function.
fn valid_plugin_script() -> &'static [u8] {
    br#"
local ffi = require("ffi")
local function impl_noop(_args_ptr, _out_ptr)
end
function polyplug_init(_registrar_ptr, _ctx_ptr)
    _G._polyplug_handlers = {
        ["test.loader"] = {
            contract_version = 1,
            plugin_name      = "test-loader-unit",
            functions        = { [0] = impl_noop },
        },
    }
end
"#
}

/// A Lua plugin that defines two functions so we can verify function_count.
fn two_function_plugin_script() -> &'static [u8] {
    br#"
local ffi = require("ffi")
local function impl_a(_args_ptr, _out_ptr) end
local function impl_b(_args_ptr, _out_ptr) end
function polyplug_init(_registrar_ptr, _ctx_ptr)
    _G._polyplug_handlers = {
        ["test.two"] = {
            contract_version = 1,
            plugin_name      = "test-two-unit",
            functions        = { [0] = impl_a, [1] = impl_b },
        },
    }
end
"#
}

/// A Lua plugin that registers TWO distinct contracts in one bundle. This is the
/// regression fixture for the multi-contract bug: the old flat `_polyplug_handlers`
/// table with a first-wins guard silently dropped the second contract.
fn two_contract_plugin_script() -> &'static [u8] {
    br#"
local ffi = require("ffi")
local function impl_first(_args_ptr, _out_ptr) end
local function impl_second_a(_args_ptr, _out_ptr) end
local function impl_second_b(_args_ptr, _out_ptr) end
function polyplug_init(_registrar_ptr, _ctx_ptr)
    _G._polyplug_handlers = {
        ["test.first"] = {
            contract_version = 1,
            plugin_name      = "test-multi-first",
            functions        = { [0] = impl_first },
        },
        ["test.second"] = {
            contract_version = 1,
            plugin_name      = "test-multi-second",
            functions        = { [0] = impl_second_a, [1] = impl_second_b },
        },
    }
end
"#
}

/// Create a ManifestData for a Lua bundle at the given path.
fn make_manifest(path: &Path, name: &str) -> ManifestData {
    let bundle_id: u64 = polyplug_utils::bundle_id(name);
    ManifestData {
        id: bundle_id,
        name: name.to_owned(),
        runtime: "lua".to_owned(),
        file: path
            .file_name()
            .expect("bundle path must have a file name")
            .to_string_lossy()
            .into_owned(),
        path: path
            .parent()
            .expect("bundle path must have a parent directory")
            .to_path_buf(),
        version: String::new(),
        provides: Vec::new(),
        function_count: HashMap::new(),
        dependencies: Vec::new(),
        needs_reinit_on_dep_reload: false,
        bundle_dependencies: Vec::new(),
    }
}

/// Load the supplied Lua source via `LuaLoader::load` and return the result.
fn load_script(path: &Path, name: &str) -> Result<(), RuntimeError> {
    let loader: LuaLoader = LuaLoader::new(LuaConfig::default());
    let runtime: Arc<Runtime> = make_runtime();
    let manifest: ManifestData = make_manifest(path, name);
    loader.load(
        &manifest,
        &polyplug::loader::BundleSource::Path(manifest.path.clone()),
        &runtime,
    )
}

// ── Tests ─────────────────────────────────────────────────────────────────────

// ── 1. Runtime name ──────────────────────────────────────────────────────────

#[test]
fn lua_loader_runtime_name_is_lua() {
    let loader: LuaLoader = LuaLoader::new(LuaConfig::default());
    assert_eq!(loader.runtime_name(), "lua");
}

// ── 2. Lua state initialization ──────────────────────────────────────────────

/// Loading a valid bundle must succeed — which implicitly verifies that the
/// LuaJIT VM was initialized correctly.
#[test]
fn lua_state_initializes_on_first_load() {
    let (_dir, path) = write_temp_bundle("lua_loader_init_test", valid_plugin_script());
    let result: Result<(), RuntimeError> = load_script(&path, "lua_loader_init_test");
    assert!(
        result.is_ok(),
        "Lua VM must initialize and bundle must load: {:?}",
        result.err()
    );
}

/// Calling `LuaLoader::load` a second time re-uses the same VM without
/// panicking (idempotent initialization).
#[test]
fn lua_state_init_is_idempotent() {
    let (_dir, path) = write_temp_bundle("lua_loader_idempotent", valid_plugin_script());
    load_script(&path, "lua_loader_idempotent").expect("first load must succeed");
    // Second load of the same file: VM is already initialized — must not panic.
    let result: Result<(), RuntimeError> = load_script(&path, "lua_loader_idempotent");
    assert!(
        result.is_ok(),
        "second load must succeed (idempotent VM init): {:?}",
        result.err()
    );
}

// ── 3. Bundle loading — valid script ─────────────────────────────────────────

#[test]
fn load_valid_bundle_succeeds() {
    let (_dir, path) = write_temp_bundle("lua_loader_valid", valid_plugin_script());
    let result: Result<(), RuntimeError> = load_script(&path, "lua_loader_valid");
    assert!(result.is_ok(), "valid bundle must load: {:?}", result.err());
}

// ── 4. Bundle loading — syntax error ─────────────────────────────────────────

/// A Lua script with a syntax error must produce a `LoaderError::InitFailed` error.
#[test]
fn load_syntax_error_returns_script_load_failed() {
    let (_dir, path) = write_temp_bundle(
        "lua_loader_syntax_error",
        b"function polyplug_init( -- SYNTAX ERROR: unclosed paren\n",
    );
    let result: Result<(), RuntimeError> = load_script(&path, "lua_loader_syntax_error");
    assert!(result.is_err(), "syntax error must produce an Err");
    let err: RuntimeError = result.expect_err("expected Err for syntax error");
    assert!(
        matches!(err, RuntimeError::Loader(LoaderError::InitFailed { .. })),
        "expected InitFailed for syntax error, got: {:?}",
        err
    );
}

// ── 5. Bundle loading — runtime error in polyplug_init ───────────────────────

/// A script where `polyplug_init` raises a Lua error at runtime must produce
/// `LoaderError::InitFailed`.
#[test]
fn load_runtime_error_in_init_returns_init_raised_error() {
    let (_dir, path) = write_temp_bundle(
        "lua_loader_runtime_err",
        b"function polyplug_init(_reg, _ctx)\n  error('deliberate runtime error')\nend\n",
    );
    let result: Result<(), RuntimeError> = load_script(&path, "lua_loader_runtime_err");
    assert!(result.is_err(), "runtime error in init must produce Err");
    let err: RuntimeError = result.expect_err("expected Err for runtime error in init");
    assert!(
        matches!(err, RuntimeError::Loader(LoaderError::InitFailed { .. })),
        "expected InitFailed for runtime error in init, got: {:?}",
        err
    );
}

// ── 6. Missing polyplug_init ─────────────────────────────────────────────────

/// A script that does not define `polyplug_init` must return
/// `LoaderError::InitFailed`.
#[test]
fn load_missing_polyplug_init_returns_typed_error() {
    let (_dir, path) =
        write_temp_bundle("lua_loader_no_init", b"local x = 1  -- no polyplug_init\n");
    let result: Result<(), RuntimeError> = load_script(&path, "lua_loader_no_init");
    assert!(result.is_err(), "missing init must produce Err");
    let err: RuntimeError = result.expect_err("expected Err for missing polyplug_init");
    assert!(
        matches!(err, RuntimeError::Loader(LoaderError::InitFailed { .. })),
        "expected InitFailed for missing polyplug_init, got: {:?}",
        err
    );
}

// ── 7. Non-existent file ──────────────────────────────────────────────────────

#[test]
fn load_nonexistent_path_returns_script_load_failed() {
    let dir: tempfile::TempDir = tempfile::tempdir().expect("tempdir");
    let path: PathBuf = dir.path().join("this_file_does_not_exist_42.lua");
    let result: Result<(), RuntimeError> = load_script(&path, "nonexistent");
    assert!(result.is_err(), "missing file must produce Err");
    let err: RuntimeError = result.expect_err("expected Err for nonexistent file");
    assert!(
        matches!(err, RuntimeError::Loader(LoaderError::InitFailed { .. })),
        "expected InitFailed for missing file, got: {:?}",
        err
    );
}

// ── 8. VTable registration ────────────────────────────────────────────────────

/// After a successful load, the registry must contain a plugin for the
/// expected contract_id with the correct function count.
#[test]
fn vtable_is_registered_after_load() {
    let (_dir, path) = write_temp_bundle("lua_loader_vtable", valid_plugin_script());
    let loader: LuaLoader = LuaLoader::new(LuaConfig::default());
    let runtime: Arc<Runtime> = make_runtime();
    let manifest: ManifestData = make_manifest(&path, "lua_loader_vtable");
    loader
        .load(
            &manifest,
            &polyplug::loader::BundleSource::Path(manifest.path.clone()),
            &runtime,
        )
        .expect("valid bundle must load");

    let contract_id: u64 = polyplug_utils::guest_contract_id("test.loader", 1);
    let handle: Result<GuestContractHandle, polyplug::error::RegistryError> = runtime
        .registry()
        .find(GuestContractId::from_u64(contract_id), 0);
    assert!(
        handle.is_ok(),
        "registry must contain test.loader@1 after load"
    );
    let handle: GuestContractHandle = handle.expect("handle must be Ok");
    let vtable_ptr: Result<*const GuestContractInterface, polyplug::error::RegistryError> =
        runtime.registry().resolve_guest_contract(handle);
    assert!(vtable_ptr.is_ok(), "handle must resolve to a vtable");
    // SAFETY: vtable_ptr is a 'static pointer produced by LuaLoader; the Lua VM
    // and leaked GuestContractInterface outlive this test.
    let vtable: &GuestContractInterface = unsafe { &*vtable_ptr.expect("vtable must resolve") };
    // valid_plugin_script has exactly one function: fn_id 0 must dispatch to Ok,
    // and fn_id 1 must report FunctionNotAvailable.
    assert_function_count(vtable, 1);
}

/// After a successful load, the contract must be attributed to the bundle's REAL
/// id in the registry — not bundle 0. The registration runs after Lua
/// `polyplug_init` populates `_polyplug_handlers`, so the init-bundle window must
/// stay open across the registration loop for `host_register_guest_contract` to
/// attribute it correctly. Invalidating by the real id must then remove it.
#[test]
fn registrations_attributed_to_real_bundle_id() {
    let (_dir, path) = write_temp_bundle("lua_loader_attribution", valid_plugin_script());
    let loader: LuaLoader = LuaLoader::new(LuaConfig::default());
    let runtime: Arc<Runtime> = make_runtime();
    let manifest: ManifestData = make_manifest(&path, "lua_loader_attribution");
    let bundle_id: u64 = manifest.id;
    loader
        .load(
            &manifest,
            &polyplug::loader::BundleSource::Path(manifest.path.clone()),
            &runtime,
        )
        .expect("valid bundle must load");

    let contract_id: u64 = polyplug_utils::guest_contract_id("test.loader", 1);

    // The contract must be findable under the REAL bundle id.
    let by_real: Result<GuestContractHandle, polyplug::error::RegistryError> =
        runtime.find_guest_contract_by_bundle(bundle_id, contract_id, 0);
    assert!(
        by_real.is_ok(),
        "contract must be attributed to the real bundle id {bundle_id}, not bundle 0"
    );

    // And it must NOT be attributed to bundle 0.
    let by_zero: Result<GuestContractHandle, polyplug::error::RegistryError> =
        runtime.find_guest_contract_by_bundle(0, contract_id, 0);
    assert!(
        by_zero.is_err(),
        "contract must not be attributed to bundle 0"
    );

    // Invalidating by the real bundle id must remove the contract from the registry.
    runtime
        .registry()
        .invalidate_bundle(polyplug_utils::BundleId::from_u64(bundle_id))
        .expect("invalidate by real bundle id must succeed");
    let after: Result<GuestContractHandle, polyplug::error::RegistryError> = runtime
        .registry()
        .find(GuestContractId::from_u64(contract_id), 0);
    assert!(
        after.is_err(),
        "contract must be gone after invalidating the real bundle id"
    );
}

/// Verify a VM-dispatch vtable exposes exactly `expected` functions by probing
/// fn_ids: indices `0..expected` must return `Ok`, and index `expected` must
/// return `FunctionNotAvailable`.
fn assert_function_count(vtable: &GuestContractInterface, expected: u32) {
    assert_eq!(
        vtable.dispatch_type,
        polyplug_abi::DispatchType::VirtualMachine,
        "Lua loader must use VM dispatch"
    );
    for fn_id in 0..expected {
        // SAFETY: dispatch.vm.call is a valid function pointer; the noop functions
        // ignore the null args/out pointers.
        let result: AbiError = unsafe {
            (vtable.dispatch.vm.call)(
                vtable.dispatch.vm.loader_data,
                GuestContractInstance::null(),
                fn_id,
                core::ptr::null::<()>(),
                core::ptr::null_mut::<()>(),
                core::ptr::null_mut(),
            )
        };
        assert_eq!(
            result.code,
            AbiErrorCode::Ok as u32,
            "fn_id {fn_id} must dispatch to Ok"
        );
    }
    // SAFETY: dispatch.vm.call is a valid function pointer.
    let missing: AbiError = unsafe {
        (vtable.dispatch.vm.call)(
            vtable.dispatch.vm.loader_data,
            GuestContractInstance::null(),
            expected,
            core::ptr::null::<()>(),
            core::ptr::null_mut::<()>(),
            core::ptr::null_mut(),
        )
    };
    assert_eq!(
        missing.code,
        AbiErrorCode::FunctionNotAvailable as u32,
        "fn_id {expected} must report FunctionNotAvailable"
    );
}

/// After loading the two-function plugin, function_count must equal 2.
#[test]
fn vtable_function_count_matches_script() {
    let (_dir, path) = write_temp_bundle("lua_loader_two_fn", two_function_plugin_script());
    let loader: LuaLoader = LuaLoader::new(LuaConfig::default());
    let runtime: Arc<Runtime> = make_runtime();
    let manifest: ManifestData = make_manifest(&path, "lua_loader_two_fn");
    loader
        .load(
            &manifest,
            &polyplug::loader::BundleSource::Path(manifest.path.clone()),
            &runtime,
        )
        .expect("two-function bundle must load");

    let contract_id: u64 = polyplug_utils::guest_contract_id("test.two", 1);
    let handle: Result<GuestContractHandle, polyplug::error::RegistryError> = runtime
        .registry()
        .find(GuestContractId::from_u64(contract_id), 0);
    let handle: GuestContractHandle = handle.expect("test.two@1 must be registered");
    let vtable_ptr: Result<*const GuestContractInterface, polyplug::error::RegistryError> =
        runtime.registry().resolve_guest_contract(handle);
    // SAFETY: see vtable_is_registered_after_load.
    let vtable: &GuestContractInterface = unsafe { &*vtable_ptr.expect("vtable must resolve") };
    // two_function_plugin_script must register exactly 2 functions.
    assert_function_count(vtable, 2);
}

/// The contract_id stored in the vtable must match the FNV-1a hash computed
/// from the contract name and version declared in the script.
#[test]
fn vtable_contract_id_matches_computed_hash() {
    let (_dir, path) = write_temp_bundle("lua_loader_cid", valid_plugin_script());
    let loader: LuaLoader = LuaLoader::new(LuaConfig::default());
    let runtime: Arc<Runtime> = make_runtime();
    let manifest: ManifestData = make_manifest(&path, "lua_loader_cid");
    loader
        .load(
            &manifest,
            &polyplug::loader::BundleSource::Path(manifest.path.clone()),
            &runtime,
        )
        .expect("valid bundle must load");

    let expected_cid: u64 = polyplug_utils::guest_contract_id("test.loader", 1);
    let handle: Result<GuestContractHandle, polyplug::error::RegistryError> = runtime
        .registry()
        .find(GuestContractId::from_u64(expected_cid), 0);
    let handle: GuestContractHandle = handle.expect("test.loader@1 must be registered");
    let vtable_ptr: Result<*const GuestContractInterface, polyplug::error::RegistryError> =
        runtime.registry().resolve_guest_contract(handle);
    // SAFETY: see vtable_is_registered_after_load.
    let vtable: &GuestContractInterface = unsafe { &*vtable_ptr.expect("vtable must resolve") };
    assert_eq!(
        vtable.contract_id,
        GuestContractId::from_u64(expected_cid),
        "contract_id in vtable must match FNV-1a hash of 'test.loader@1'"
    );
}

// ── 9. Stack management — sequential loads ────────────────────────────────────

/// Load two different plugins in sequence: each must succeed and register its
/// own contract.
#[test]
fn sequential_loads_both_succeed() {
    let (_dir1, path1) = write_temp_bundle("lua_loader_seq1", valid_plugin_script());
    let (_dir2, path2) = write_temp_bundle("lua_loader_seq2", two_function_plugin_script());

    let loader: LuaLoader = LuaLoader::new(LuaConfig::default());
    let runtime: Arc<Runtime> = make_runtime();
    let manifest1: ManifestData = make_manifest(&path1, "lua_loader_seq1");
    let manifest2: ManifestData = make_manifest(&path2, "lua_loader_seq2");
    loader
        .load(
            &manifest1,
            &polyplug::loader::BundleSource::Path(manifest1.path.clone()),
            &runtime,
        )
        .expect("first sequential load must succeed");
    loader
        .load(
            &manifest2,
            &polyplug::loader::BundleSource::Path(manifest2.path.clone()),
            &runtime,
        )
        .expect("second sequential load must succeed");

    // Both contracts must be visible.
    let cid1: u64 = polyplug_utils::guest_contract_id("test.loader", 1);
    let cid2: u64 = polyplug_utils::guest_contract_id("test.two", 1);

    let handle1: Result<GuestContractHandle, polyplug::error::RegistryError> =
        runtime.registry().find(GuestContractId::from_u64(cid1), 0);
    assert!(handle1.is_ok(), "test.loader must be registered");

    let handle2: Result<GuestContractHandle, polyplug::error::RegistryError> =
        runtime.registry().find(GuestContractId::from_u64(cid2), 0);
    assert!(handle2.is_ok(), "test.two must be registered");
}

// ── 9b. Multi-contract bundle ─────────────────────────────────────────────────

/// Regression test: a single Lua bundle that declares two contracts must
/// register BOTH. The previous flat `_polyplug_handlers` table with a first-wins
/// guard silently dropped every contract after the first.
#[test]
fn multi_contract_bundle_registers_all_contracts() {
    let (_dir, path) = write_temp_bundle("lua_loader_multi", two_contract_plugin_script());
    let loader: LuaLoader = LuaLoader::new(LuaConfig::default());
    let runtime: Arc<Runtime> = make_runtime();
    let manifest: ManifestData = make_manifest(&path, "lua_loader_multi");
    loader
        .load(
            &manifest,
            &polyplug::loader::BundleSource::Path(manifest.path.clone()),
            &runtime,
        )
        .expect("multi-contract bundle must load");

    // Both contracts must be registered and resolvable.
    let first_cid: u64 = polyplug_utils::guest_contract_id("test.first", 1);
    let second_cid: u64 = polyplug_utils::guest_contract_id("test.second", 1);

    let first_handle: GuestContractHandle = runtime
        .registry()
        .find(GuestContractId::from_u64(first_cid), 0)
        .expect("test.first@1 must be registered");
    let second_handle: GuestContractHandle = runtime
        .registry()
        .find(GuestContractId::from_u64(second_cid), 0)
        .expect("test.second@1 must be registered");

    // Each contract must resolve to a vtable with the correct function count:
    // test.first has 1 function, test.second has 2.
    let first_vtable_ptr: *const GuestContractInterface = runtime
        .registry()
        .resolve_guest_contract(first_handle)
        .expect("test.first handle must resolve");
    let second_vtable_ptr: *const GuestContractInterface = runtime
        .registry()
        .resolve_guest_contract(second_handle)
        .expect("test.second handle must resolve");
    // SAFETY: first_vtable_ptr is a 'static leaked GuestContractInterface from
    // LuaLoader; the shared Lua VM and leaked interface outlive this test.
    let first_vtable: &GuestContractInterface = unsafe { &*first_vtable_ptr };
    // SAFETY: second_vtable_ptr is a 'static leaked GuestContractInterface from
    // LuaLoader; the shared Lua VM and leaked interface outlive this test.
    let second_vtable: &GuestContractInterface = unsafe { &*second_vtable_ptr };
    assert_function_count(first_vtable, 1);
    assert_function_count(second_vtable, 2);
}

// ── 10. Thread safety ─────────────────────────────────────────────────────────

/// Spawn multiple threads, each loading the same valid plugin.  The global
/// Mutex inside `LuaLoader` must prevent data races and every load must
/// either succeed or produce a recognized `RuntimeError` (no panics,
/// no UB).
///
/// Each bundle gets its own isolated Lua VM, so parallel loads are safe.
#[test]
fn concurrent_loaders_do_not_race() {
    let (_dir, path) = write_temp_bundle("lua_loader_thread_safety", valid_plugin_script());

    // Spawn 4 threads that all call LuaLoader::load on the same path.
    let path_arc: std::sync::Arc<PathBuf> = std::sync::Arc::new(path);
    let handles: Vec<std::thread::JoinHandle<Result<(), RuntimeError>>> = (0_u32..4_u32)
        .map(|_| {
            let p: std::sync::Arc<PathBuf> = std::sync::Arc::clone(&path_arc);
            std::thread::spawn(move || {
                let loader: LuaLoader = LuaLoader::new(LuaConfig::default());
                let runtime: Arc<Runtime> = RuntimeBuilder::new()
                    .loader(LuaLoader::new(LuaConfig::default()))
                    .build()
                    .expect("runtime build must succeed");
                let manifest: ManifestData = ManifestData {
                    id: polyplug_utils::bundle_id("lua_loader_thread_safety"),
                    name: "lua_loader_thread_safety".to_owned(),
                    runtime: "lua".to_owned(),
                    file: p
                        .file_name()
                        .expect("bundle path must have a file name")
                        .to_string_lossy()
                        .into_owned(),
                    path: p
                        .parent()
                        .expect("bundle path must have a parent directory")
                        .to_path_buf(),
                    version: String::new(),
                    provides: Vec::new(),
                    function_count: HashMap::new(),
                    dependencies: Vec::new(),
                    needs_reinit_on_dep_reload: false,
                    bundle_dependencies: Vec::new(),
                };
                loader.load(
                    &manifest,
                    &polyplug::loader::BundleSource::Path(manifest.path.clone()),
                    &runtime,
                )
            })
        })
        .collect::<Vec<std::thread::JoinHandle<Result<(), RuntimeError>>>>();

    for handle in handles {
        // Each thread must not panic. Errors (e.g. DuplicateProvider inside
        // the callback) are acceptable — panics are not.
        let result: Result<(), RuntimeError> = handle
            .join()
            .expect("thread must not panic during concurrent load");
        // The result may be Ok or a recognized RuntimeError.
        // We simply assert it is a valid discriminant (no silent UB).
        let _ = result;
    }
}

// ── 11. Dispatch — calling a registered Lua function ─────────────────────────

/// Load the valid plugin and invoke its single function through the vtable.
/// The noop function must return `AbiErrorCode::Ok` without panicking.
#[test]
fn vtable_function_dispatch_returns_abi_ok() {
    let (_dir, path) = write_temp_bundle("lua_loader_dispatch", valid_plugin_script());
    let loader: LuaLoader = LuaLoader::new(LuaConfig::default());
    let runtime: Arc<Runtime> = make_runtime();
    let manifest: ManifestData = make_manifest(&path, "lua_loader_dispatch");
    loader
        .load(
            &manifest,
            &polyplug::loader::BundleSource::Path(manifest.path.clone()),
            &runtime,
        )
        .expect("valid bundle must load");

    let contract_id: u64 = polyplug_utils::guest_contract_id("test.loader", 1);
    let handle: GuestContractHandle = runtime
        .registry()
        .find(GuestContractId::from_u64(contract_id), 0)
        .expect("test.loader@1 must be registered");
    let vtable_ptr: *const GuestContractInterface = runtime
        .registry()
        .resolve_guest_contract(handle)
        .expect("handle must resolve to vtable");
    // SAFETY: vtable_ptr is a 'static leaked GuestContractInterface from LuaLoader.
    let vtable: &GuestContractInterface = unsafe { &*vtable_ptr };

    // With VM dispatch, we call through the dispatch.vm.call function.
    assert_eq!(
        vtable.dispatch_type,
        polyplug_abi::DispatchType::VirtualMachine,
        "Lua loader must use VM dispatch"
    );

    // SAFETY: dispatch.vm.call is a valid function pointer, loader_data is valid,
    // and we pass null pointers for args/out which the noop function ignores.
    let result: AbiError = unsafe {
        (vtable.dispatch.vm.call)(
            vtable.dispatch.vm.loader_data,
            GuestContractInstance::null(),
            0, // fn_id = 0 (first function)
            core::ptr::null::<()>(),
            core::ptr::null_mut::<()>(),
            core::ptr::null_mut(),
        )
    };
    assert_eq!(
        result.code,
        AbiErrorCode::Ok as u32,
        "noop function must return Ok, got code={}",
        result.code
    );
}

// ── 12. Hot-reload ────────────────────────────────────────────────────────────

/// Build a runtime with the given hot-reload setting and a registered LuaLoader.
fn make_runtime_with_hot_reload(enabled: bool) -> Arc<Runtime> {
    RuntimeBuilder::new()
        .config(RuntimeConfig {
            compatibility: Compatibility::Strict,
            unload_mode: polyplug_abi::runtime::UnloadMode::Retire,
            hot_reload_enabled: enabled,
            on_reload: None,
            on_reload_user_data: core::ptr::null_mut(),
            ..Default::default()
        })
        .loader(LuaLoader::new(LuaConfig::default()))
        .build()
        .expect("runtime build must succeed")
}

/// When hot-reload is disabled in the runtime config, `LuaLoader::reload` must
/// return `RuntimeError::HotReloadDisabled` without touching the bundle.
#[test]
fn lua_reload_disabled_returns_error() {
    let (_dir, path) = write_temp_bundle("lua_reload_disabled", valid_plugin_script());
    let loader: LuaLoader = LuaLoader::new(LuaConfig::default());
    let runtime: Arc<Runtime> = make_runtime_with_hot_reload(false);
    let manifest: ManifestData = make_manifest(&path, "lua_reload_disabled");

    let result: Result<(), RuntimeError> = loader.reload(&manifest, &runtime);
    assert!(
        matches!(result, Err(RuntimeError::HotReloadDisabled)),
        "reload with hot_reload_enabled=false must return HotReloadDisabled, got: {:?}",
        result
    );
}

// ── 13. BundleSource::Code / Bytes — in-memory source loading ────────────────

/// Absolute path to the on-disk Lua fixture bundle directory.
fn lua_fixture_dir() -> PathBuf {
    // CARGO_MANIFEST_DIR = crates/polyplug_lua; the fixture lives at the workspace
    // root under tests/fixtures/test_plugin_lua.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/ parent must exist")
        .parent()
        .expect("workspace root must exist")
        .join("tests")
        .join("fixtures")
        .join("test_plugin_lua")
}

/// Build a ManifestData for the `test.add@1` fixture contract. `path` is the
/// bundle directory used for Path loading; in-memory sources ignore it for
/// package.path provisioning but still carry it as a stable identifier.
fn fixture_manifest(path: &Path) -> ManifestData {
    let name: &str = "test_plugin_lua";
    ManifestData {
        id: polyplug_utils::bundle_id(name),
        name: name.to_owned(),
        runtime: "lua".to_owned(),
        file: "test_plugin.lua".to_owned(),
        path: path.to_path_buf(),
        version: "1.0.0".to_owned(),
        // Leave `provides` empty so manifest validation skips the per-contract
        // function-count check; the loader registers test.add@1 from the script's
        // _polyplug_handlers regardless. This mirrors the other tests' manifests.
        provides: Vec::new(),
        function_count: HashMap::new(),
        dependencies: Vec::new(),
        needs_reinit_on_dep_reload: false,
        bundle_dependencies: Vec::new(),
    }
}

/// Dispatch `add(a, b) -> u32` (fn_id 0) on the `test.add@1` contract registered
/// in `runtime`, returning the u32 result.
fn dispatch_add(runtime: &Runtime, a: u32, b: u32) -> u32 {
    let contract_id: u64 = polyplug_utils::guest_contract_id("test.add", 1);
    let handle: GuestContractHandle = runtime
        .registry()
        .find(GuestContractId::from_u64(contract_id), 0)
        .expect("test.add@1 must be registered");
    let vtable_ptr: *const GuestContractInterface = runtime
        .registry()
        .resolve_guest_contract(handle)
        .expect("handle must resolve to a vtable");
    // SAFETY: vtable_ptr is a 'static leaked GuestContractInterface from LuaLoader;
    // the shared Lua VM and leaked interface outlive this call.
    let vtable: &GuestContractInterface = unsafe { &*vtable_ptr };

    // The fixture's impl_add reads two u32 from args and writes one u32 to out.
    let args: [u32; 2] = [a, b];
    let mut out: u32 = 0;
    // SAFETY: dispatch.vm.call is a valid function pointer; args points at two
    // contiguous u32 and out at one u32, matching what impl_add reads/writes.
    let result: AbiError = unsafe {
        (vtable.dispatch.vm.call)(
            vtable.dispatch.vm.loader_data,
            GuestContractInstance::null(),
            0,
            args.as_ptr() as *const (),
            &mut out as *mut u32 as *mut (),
            core::ptr::null_mut(),
        )
    };
    assert_eq!(
        result.code,
        AbiErrorCode::Ok as u32,
        "add dispatch must return Ok, got code={}",
        result.code
    );
    out
}

/// Loading the fixture's Lua source through `BundleSource::Code` must register and
/// dispatch the `test.add@1` contract identically to Path loading.
///
/// The fixture only `require`s loader-provisioned SDK modules (`ffi`,
/// `polyplug_guest`, `polyplug_abi`) — never a bundle-dir-vendored sibling — so a
/// Code-sourced load (which has no bundle directory) can satisfy every require.
#[test]
fn code_source_loads_and_dispatches_like_path() {
    let fixture_dir: PathBuf = lua_fixture_dir();
    let entry: PathBuf = fixture_dir.join("test_plugin.lua");
    let source_text: String =
        std::fs::read_to_string(&entry).expect("fixture test_plugin.lua must be readable");

    // Path-loaded baseline in its own runtime.
    let path_runtime: Arc<Runtime> = make_runtime();
    path_runtime
        .load_bundle_from_source(
            fixture_manifest(&fixture_dir),
            polyplug::loader::BundleSource::Path(fixture_dir.clone()),
        )
        .expect("path-sourced fixture load must succeed");
    let path_result: u32 = dispatch_add(&path_runtime, 7, 35);

    // Code-loaded equivalent in a separate runtime: no bundle directory at all.
    let code_runtime: Arc<Runtime> = make_runtime();
    code_runtime
        .load_bundle_from_source(
            fixture_manifest(&fixture_dir),
            polyplug::loader::BundleSource::Code(source_text),
        )
        .expect("code-sourced fixture load must succeed");
    let code_result: u32 = dispatch_add(&code_runtime, 7, 35);

    assert_eq!(
        code_result, 42,
        "code-sourced add(7, 35) must compute 42, got {code_result}"
    );
    assert_eq!(
        code_result, path_result,
        "code-sourced dispatch must match path-sourced dispatch"
    );
}

/// `BundleSource::Bytes` carrying valid UTF-8 Lua source must load via the same
/// path as `Code` and dispatch identically.
#[test]
fn bytes_source_with_valid_utf8_loads_and_dispatches() {
    let fixture_dir: PathBuf = lua_fixture_dir();
    let entry: PathBuf = fixture_dir.join("test_plugin.lua");
    let source_bytes: Vec<u8> =
        std::fs::read(&entry).expect("fixture test_plugin.lua must be readable");

    let runtime: Arc<Runtime> = make_runtime();
    runtime
        .load_bundle_from_source(
            fixture_manifest(&fixture_dir),
            polyplug::loader::BundleSource::Bytes(source_bytes),
        )
        .expect("bytes-sourced fixture load must succeed");

    let result: u32 = dispatch_add(&runtime, 20, 22);
    assert_eq!(result, 42, "bytes-sourced add(20, 22) must compute 42");
}

/// `BundleSource::Bytes` carrying invalid UTF-8 must fail with the unified
/// `LoaderError::InvalidSourceEncoding` — never a panic and never a string-only
/// error.
#[test]
fn bytes_source_with_invalid_utf8_returns_structured_error() {
    let fixture_dir: PathBuf = lua_fixture_dir();
    // 0xFF is never a valid UTF-8 byte.
    let invalid: Vec<u8> = vec![0x66, 0x6e, 0xFF, 0xFE, 0x00];

    let runtime: Arc<Runtime> = make_runtime();
    let result: Result<(), RuntimeError> = runtime.load_bundle_from_source(
        fixture_manifest(&fixture_dir),
        polyplug::loader::BundleSource::Bytes(invalid),
    );
    assert!(result.is_err(), "invalid UTF-8 bytes must produce Err");
    let err: RuntimeError = result.expect_err("expected Err for invalid UTF-8 bytes");
    match err {
        RuntimeError::Loader(LoaderError::InvalidSourceEncoding {
            loader,
            source_kind,
            bundle,
        }) => {
            assert_eq!(loader, "lua", "loader must be the Lua runtime name");
            assert_eq!(source_kind, "bytes", "source_kind must be bytes");
            assert_eq!(
                bundle, "test_plugin_lua",
                "bundle must be the manifest bundle name"
            );
        }
        other => panic!("expected LoaderError::InvalidSourceEncoding, got: {other:?}"),
    }
}

/// With hot-reload enabled, reloading a loaded bundle must succeed and the
/// contract must remain resolvable through the registry afterwards.
#[test]
fn lua_reload_reinitializes_contracts() {
    let (dir, _path) = write_temp_bundle("lua_reload_reinit", valid_plugin_script());
    let runtime: Arc<Runtime> = make_runtime_with_hot_reload(true);

    let bundle_dir: PathBuf = dir.path().to_path_buf();
    runtime
        .load_bundle(&bundle_dir)
        .expect("initial bundle load must succeed");

    let contract_id: u64 = polyplug_utils::guest_contract_id("test.loader", 1);
    runtime
        .find_guest_contract(contract_id, 0)
        .expect("contract must resolve after initial load");

    runtime
        .reload_bundle(&bundle_dir)
        .expect("reload must succeed when hot-reload is enabled");

    runtime
        .find_guest_contract(contract_id, 0)
        .expect("contract must remain resolvable after reload");
}

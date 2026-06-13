//! Integration test: Python guest→guest peer caller at runtime.
//!
//! This proves the **generated** Python peer-caller code (`peer_callers.py`)
//! executes end-to-end inside a real `Runtime`. It is the test that closes the
//! gap #75 fixed: before the fix, `peer_callers.py` was generated but unreachable
//! — `resolve()` needed a host pointer no idiomatic python guest could obtain.
//! Now the author factory (`polyplug_create_<plugin>` via `set_<plugin>_factory`)
//! receives the host pointer at `polyplug_init` time and the impl passes it to
//! `resolve(host_ptr)` explicitly — no host pointer is stored in the guest SDK.
//!
//! The flow:
//!   1. `polyplugc::generate` emits the Python guest glue for a `data.Transformer@1`
//!      bundle that declares a `[[dependency]]` on `pipeline.Validator` — so the
//!      generator emits `PipelineValidatorPeer` into `peer_callers.py`.
//!   2. A hand-written `consumer.py` implements `transform(input: str)` by
//!      resolving that generated peer caller with the host pointer its factory
//!      received and calling `validate(...)`, marshalling `str` <-> `StringView`
//!      at the boundary.
//!   3. The current python guest SDK (`polyplug_guest` + `polyplug_abi`) is
//!      vendored into the bundle's `site-packages/` (NOT a stale fixture copy).
//!   4. A Lua **provider** bundle registers `pipeline.Validator@1` whose
//!      `validate(StringView) -> StringView` returns `"PEER:"` + input.
//!   5. Both bundles load into one `Runtime` (PythonLoader + LuaLoader). The
//!      provider loads first so `pipeline.Validator@1` is registered when the
//!      Python consumer resolves it.
//!   6. The test dispatches the consumer's `transform("hello")` through VM
//!      dispatch and asserts the result equals `"PEER:hello"` — proving the
//!      generated Python peer caller routed through `call_guest_method`.
//!
//! Uses the embedded CPython interpreter (pyo3), so it runs wherever the other
//! `integration_python` loader tests run.

#![allow(clippy::expect_used)]
#![allow(clippy::undocumented_unsafe_blocks)]

use polyplug::runtime::Runtime;
use polyplug_abi::AbiError;
use polyplug_abi::AbiErrorCode;
use polyplug_abi::DispatchType;
use polyplug_abi::GuestContractHandle;
use polyplug_abi::GuestContractInstance;
use polyplug_abi::GuestContractInterface;
use polyplug_abi::StringView;
use polyplug_codegen::GenerateConfig;
use polyplug_codegen::GenerateOutput;
use polyplug_codegen::Lang;
use polyplug_codegen::Side;
use polyplug_lua::LuaConfig;
use polyplug_lua::LuaLoader;
use polyplug_python::PythonConfig;
use polyplug_python::PythonLoader;
use polyplug_utils::bundle_id;
use polyplug_utils::guest_contract_id;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

// ─── Paths ──────────────────────────────────────────────────────────────────────

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("parent of tests/integration")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

// ─── Python consumer bundle (generated glue + hand-written impl + vendored SDK) ──

/// Generate the Python guest glue for a `data.Transformer@1` bundle that depends
/// on `pipeline.Validator`, write the hand-written `consumer.py`, vendor the
/// current guest SDK into `site-packages/`, and return the bundle directory.
fn build_python_consumer(tmp: &Path) -> PathBuf {
    let bundle_dir: PathBuf = tmp.join("peer_consumer_python");
    std::fs::create_dir_all(&bundle_dir).expect("create python consumer dir");

    let api_path: PathBuf = workspace_root().join("examples").join("api.toml");
    let bundle_toml: String = format!(
        "[bundle]\n\
         name = \"peer_consumer_python\"\n\
         version = \"1.0.0\"\n\
         api = \"{api}\"\n\
         loader = \"python\"\n\
         file = \"consumer.py\"\n\n\
         [[plugin]]\n\
         name = \"transformer\"\n\
         version = \"1.0.0\"\n\
         implements = [\"data.Transformer@1.0\"]\n\n\
         [[dependency]]\n\
         kind = \"contract\"\n\
         contract = \"pipeline.Validator\"\n\
         min_version = \"1.0\"\n",
        api = api_path.to_string_lossy().replace('\\', "/"),
    );
    let bundle_toml_path: PathBuf = bundle_dir.join("bundle.toml");
    std::fs::write(&bundle_toml_path, bundle_toml).expect("write bundle.toml");

    // Generate the python guest glue (contracts.py, peer_callers.py, …) + manifest.
    let gen_dir: PathBuf = bundle_dir.join("generated");
    let config: GenerateConfig = GenerateConfig {
        api_toml: bundle_toml_path,
        lang: Lang::Python,
        side: Side::Guest,
        out_dir: gen_dir.clone(),
    };
    let output: GenerateOutput = polyplugc::generate(config).expect("polyplugc generate (python)");
    for file in &output.files {
        let file_path: PathBuf = gen_dir.join(&file.path);
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).expect("create generated parent dir");
        }
        std::fs::write(&file_path, &file.content).expect("write generated file");
    }

    // The loader discovers manifest.toml at the bundle root.
    std::fs::rename(
        gen_dir.join("manifest.toml"),
        bundle_dir.join("manifest.toml"),
    )
    .expect("move manifest.toml to bundle root");

    // The only hand-written source: transform() resolves the generated peer caller
    // with the host pointer the author factory received (no SDK-level host storage)
    // and calls validate(). We marshal str <-> StringView at the boundary; to_str
    // copies the borrowed result before `peer` is garbage-collected.
    let consumer_py: &str = "from generated.guest.contracts import (\n\
         \x20   TRANSFORMERDataTransformerPlugin,\n\
         \x20   set_transformer_factory,\n\
         \x20   polyplug_init,\n\
         )\n\
         from generated.guest.peer_callers import PipelineValidatorPeer\n\
         from polyplug_guest import alloc_string, to_str\n\
         \n\
         \n\
         class TransformerImpl(TRANSFORMERDataTransformerPlugin):\n\
         \x20   def __init__(self, host_ptr: int) -> None:\n\
         \x20       self._host_ptr = host_ptr\n\
         \n\
         \x20   def transform(self, input: str) -> str:\n\
         \x20       peer = PipelineValidatorPeer.resolve(self._host_ptr)\n\
         \x20       if peer is None:\n\
         \x20           return \"ERROR:peer-unavailable\"\n\
         \x20       sv_in = alloc_string(self._host_ptr, input)\n\
         \x20       sv_out = peer.validate(sv_in)\n\
         \x20       return to_str(sv_out)\n\
         \n\
         \n\
         set_transformer_factory(TransformerImpl)\n";
    std::fs::write(bundle_dir.join("consumer.py"), consumer_py).expect("write consumer.py");

    // Vendor the CURRENT guest SDK into site-packages/ (the loader prepends
    // <bundle>/site-packages to sys.path). Copy the real SDK — NOT a stale
    // fixture copy — so the helpers match the generated glue exactly.
    let site: PathBuf = bundle_dir.join("site-packages");
    let sdk_root: PathBuf = workspace_root().join("sdks").join("python");

    let guest_dst: PathBuf = site.join("polyplug_guest");
    std::fs::create_dir_all(&guest_dst).expect("create polyplug_guest dir");
    std::fs::copy(
        sdk_root
            .join("guest")
            .join("polyplug_guest")
            .join("__init__.py"),
        guest_dst.join("__init__.py"),
    )
    .expect("vendor polyplug_guest");

    let abi_src: PathBuf = sdk_root.join("polyplug_abi").join("polyplug_abi");
    let abi_dst: PathBuf = site.join("polyplug_abi");
    std::fs::create_dir_all(&abi_dst).expect("create polyplug_abi dir");
    for name in ["__init__.py", "abi.py", "string_view_helper.py"] {
        std::fs::copy(abi_src.join(name), abi_dst.join(name))
            .unwrap_or_else(|e| panic!("vendor polyplug_abi/{name}: {e}"));
    }

    // polyplug_abi.abi falls back to `from polyplug.abi.abi import *`, so the
    // canonical generated ABI module must be reachable as the `polyplug` package.
    let polyplug_abi_pkg: PathBuf = site.join("polyplug").join("abi");
    std::fs::create_dir_all(&polyplug_abi_pkg).expect("create polyplug/abi dir");
    std::fs::write(site.join("polyplug").join("__init__.py"), b"").expect("polyplug __init__");
    std::fs::write(polyplug_abi_pkg.join("__init__.py"), b"").expect("polyplug/abi __init__");
    std::fs::copy(
        sdk_root.join("abi").join("abi.py"),
        polyplug_abi_pkg.join("abi.py"),
    )
    .expect("vendor polyplug/abi/abi.py");

    bundle_dir
}

// ─── Lua provider bundle (pipeline.Validator@1) ─────────────────────────────────

fn provider_lua_src() -> &'static str {
    r#"
local ffi = require("ffi")
local polyplug_guest = require("polyplug_guest")
local polyplug_abi = require("polyplug_abi")

local function impl_validate(args_ptr, out_ptr)
    local in_sv = ffi.cast("const StringView*", ffi.cast("uintptr_t", args_ptr))
    local s = polyplug_abi.to_str(in_sv[0])
    local result = "PEER:" .. s
    local out_view = polyplug_guest.alloc_string_arena(result)
    local out_sv = ffi.cast("StringView*", ffi.cast("uintptr_t", out_ptr))
    out_sv[0] = out_view
end

function polyplug_init(registrar_ptr, ctx_ptr)
    _G._polyplug_handlers = {
        ["pipeline.Validator"] = {
            contract_version = 1,
            plugin_name = "test-validator-provider-python",
            functions = { [0] = impl_validate },
        },
    }
end
"#
}

/// Write a Lua provider bundle for `pipeline.Validator@1` and return its dir.
fn build_lua_provider(tmp: &Path) -> PathBuf {
    let dir: PathBuf = tmp.join("peer_provider_lua_python");
    std::fs::create_dir_all(&dir).expect("create provider dir");

    let id_val: u64 = bundle_id("peer_provider_lua_python");
    let manifest: String = format!(
        "name = \"peer_provider_lua_python\"\n\
         id = {id_val}\n\
         bundle_name = \"peer_provider_lua_python\"\n\
         version = \"1.0.0\"\n\
         loader = \"lua\"\n\
         file = \"provider.lua\"\n\
         provides = [\"pipeline.Validator@1\"]\n\
         needs_reinit_on_dep_reload = false\n\n\
         [function_count]\n\
         \"pipeline.Validator@1\" = 1\n",
    );
    std::fs::write(dir.join("manifest.toml"), manifest).expect("write manifest.toml");
    std::fs::write(dir.join("provider.lua"), provider_lua_src()).expect("write provider.lua");

    let fixtures_lua: PathBuf = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("parent of tests/integration")
        .join("fixtures")
        .join("test_plugin_lua");
    let polyplug_dir: PathBuf = dir.join("polyplug");
    std::fs::create_dir_all(&polyplug_dir).expect("create polyplug dir");
    let fixture_polyplug: PathBuf = fixtures_lua.join("polyplug");
    for entry in std::fs::read_dir(&fixture_polyplug).expect("read fixture polyplug dir") {
        let entry: std::fs::DirEntry = entry.expect("dir entry");
        std::fs::copy(entry.path(), polyplug_dir.join(entry.file_name()))
            .expect("copy polyplug sdk file");
    }
    std::fs::copy(
        fixtures_lua.join("polyplug_abi.lua"),
        dir.join("polyplug_abi.lua"),
    )
    .expect("copy polyplug_abi.lua");
    std::fs::copy(
        fixtures_lua.join("polyplug_guest.lua"),
        dir.join("polyplug_guest.lua"),
    )
    .expect("copy polyplug_guest.lua");

    dir
}

// ─── Test ──────────────────────────────────────────────────────────────────────

#[test]
fn python_peer_caller_validate_roundtrip() {
    let tmp: tempfile::TempDir = tempfile::tempdir().expect("tempdir");

    let provider_dir: PathBuf = build_lua_provider(tmp.path());
    let consumer_dir: PathBuf = build_python_consumer(tmp.path());

    let rt: Arc<Runtime> = Runtime::builder()
        .loader(PythonLoader::new(PythonConfig::default()))
        .loader(LuaLoader::new(LuaConfig::default()))
        .build()
        .expect("build runtime");

    // Load the provider FIRST so pipeline.Validator@1 is registered when the
    // Python consumer's peer caller resolves it.
    rt.load_bundle(&provider_dir)
        .expect("lua provider bundle must load");
    rt.load_bundle(&consumer_dir)
        .expect("python consumer bundle must load");

    // Resolve the consumer contract (data.Transformer@1).
    let transformer_id: u64 = guest_contract_id("data.Transformer", 1);
    let handle: GuestContractHandle = rt
        .find_guest_contract(transformer_id, 0)
        .expect("data.Transformer must be registered after load");
    let interface_ptr: *const GuestContractInterface = rt
        .resolve_guest_contract(handle)
        .expect("handle must resolve");
    // SAFETY: interface_ptr is live for the runtime lifetime.
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };
    assert_eq!(
        interface.dispatch_type,
        DispatchType::VirtualMachine,
        "python guests use VM dispatch"
    );

    let input: &[u8] = b"hello";
    let input_view: StringView = StringView {
        ptr: input.as_ptr(),
        len: input.len(),
    };
    let mut out_view: StringView = StringView::null();
    // SAFETY: VM dispatch — the vm union is active; args is a *const StringView,
    // out a *mut StringView per transform's ABI; null arena selects host->alloc.
    let mut err: AbiError = AbiError::ok();
    unsafe {
        (interface.dispatch.vm.call)(
            interface.dispatch.vm.loader_data,
            GuestContractInstance::null(),
            0_u32,
            &input_view as *const StringView as *const (),
            &mut out_view as *mut StringView as *mut (),
            core::ptr::null_mut(),
            &mut err as *mut AbiError,
        )
    };
    assert_eq!(
        err.code,
        AbiErrorCode::Ok as u32,
        "transform must return Ok; got code={}",
        err.code
    );
    assert!(
        !out_view.ptr.is_null(),
        "returned StringView must not be null"
    );

    // SAFETY: out_view points to host-allocated bytes valid until bundle unload.
    let result_bytes: &[u8] = unsafe { core::slice::from_raw_parts(out_view.ptr, out_view.len) };
    let result: &str = core::str::from_utf8(result_bytes).expect("result is UTF-8");

    assert_eq!(
        result, "PEER:hello",
        "transform must return 'PEER:hello' proving the generated Python peer caller round-trip"
    );
}

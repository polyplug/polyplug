//! Integration test: C++ guest→guest peer caller at runtime.
//!
//! This proves the **generated** C++ peer-caller code (`peer_callers.hpp`)
//! executes end-to-end inside a real `Runtime` — not just that it compiles.
//! Unlike the Rust peer test (which replicates the generated pattern inline),
//! this test compiles the actual generated `PipelineValidatorContractPeer`
//! class into a native cdylib and dispatches through it.
//!
//! The flow:
//!   1. `polyplugc::generate` emits the C++ guest glue for a `data.Transformer@1`
//!      bundle that declares a `[[dependency]]` on `pipeline.Validator` — so the
//!      generator emits `PipelineValidatorContractPeer` into `peer_callers.hpp`.
//!   2. A hand-written `consumer.cpp` implements `transform(StringView)` by
//!      resolving that generated peer caller and calling its `validate(input)`,
//!      then copying the borrowed result into a host-allocated string.
//!   3. `c++` compiles the bundle into a native cdylib (the only hand-written
//!      file is `consumer.cpp`; every generated header is used verbatim).
//!   4. A Lua **provider** bundle registers `pipeline.Validator@1` whose
//!      `validate(StringView) -> StringView` returns `"PEER:"` + input.
//!   5. Both bundles load into one `Runtime` (NativeLoader + LuaLoader). The
//!      provider loads first so `pipeline.Validator@1` is registered when the
//!      C++ consumer resolves it through `host->find_guest_contract`.
//!   6. The test dispatches the consumer's `transform("hello")` via the native
//!      vtable and asserts the result equals `"PEER:hello"` — proving the
//!      generated C++ peer caller routed through `call_guest_method` (the #72
//!      contract_id-stamping fix) and marshalled the StringView round-trip.
//!
//! Skips cleanly when `c++` is unavailable (mirrors the other cpp codegen tests).

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
use polyplug_native::NativeConfig;
use polyplug_native::NativeLoader;
use polyplug_utils::bundle_id;
use polyplug_utils::guest_contract_id;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
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

/// Platform-specific cdylib filename for the compiled C++ consumer bundle.
fn consumer_lib_filename() -> &'static str {
    if cfg!(target_os = "macos") {
        "libpeer_consumer_cpp.dylib"
    } else if cfg!(target_os = "windows") {
        "peer_consumer_cpp.dll"
    } else {
        "libpeer_consumer_cpp.so"
    }
}

/// True when a C++ driver (`c++`) is available on PATH.
fn cpp_available() -> bool {
    Command::new("c++")
        .arg("--version")
        .output()
        .map(|o: std::process::Output| o.status.success())
        .unwrap_or(false)
}

// ─── C++ consumer bundle (generated glue + hand-written impl, compiled) ─────────

/// Generate the C++ guest glue for a `data.Transformer@1` bundle that depends on
/// `pipeline.Validator`, write the hand-written `consumer.cpp`, compile it into a
/// native cdylib, and return the bundle directory ready for the NativeLoader.
fn build_cpp_consumer(tmp: &Path) -> PathBuf {
    let bundle_dir: PathBuf = tmp.join("peer_consumer_cpp");
    std::fs::create_dir_all(&bundle_dir).expect("create cpp consumer dir");

    // bundle.toml: implements data.Transformer@1.0, declares a contract dependency
    // on pipeline.Validator — this is what makes the generator emit the peer caller.
    let api_path: PathBuf = workspace_root().join("examples").join("api.toml");
    let bundle_toml: String = format!(
        "[bundle]\n\
         name = \"peer_consumer_cpp\"\n\
         version = \"1.0.0\"\n\
         api = \"{api}\"\n\
         loader = \"native\"\n\n\
         [bundle.file]\n\
         linux.x86_64 = \"libpeer_consumer_cpp.so\"\n\
         linux.aarch64 = \"libpeer_consumer_cpp.so\"\n\
         macos.x86_64 = \"libpeer_consumer_cpp.dylib\"\n\
         macos.aarch64 = \"libpeer_consumer_cpp.dylib\"\n\
         windows.x86_64 = \"peer_consumer_cpp.dll\"\n\n\
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

    // Generate the cpp guest glue (types/contracts/interfaces/init/peer_callers)
    // plus the discovery manifest.toml.
    let gen_dir: PathBuf = bundle_dir.join("generated");
    let config: GenerateConfig = GenerateConfig {
        api_toml: bundle_toml_path,
        lang: Lang::Cpp,
        side: Side::Guest,
        out_dir: gen_dir.clone(),
    };
    let output: GenerateOutput = polyplugc::generate(config).expect("polyplugc generate (cpp)");
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
    // and calls validate(input). The peer's returned view borrows its caller arena,
    // so we copy it into a host-allocated string before the caller is destroyed.
    let consumer_cpp: &str = "#include \"guest/init.hpp\"\n\
         #include \"guest/peer_callers.hpp\"\n\
         #include <string>\n\
         \n\
         namespace polyplug_plugin {\n\
         class TransformerImpl : public DataTransformerGuestContract {\n\
         public:\n\
         explicit TransformerImpl(const HostApi* host) : host_(host) {}\n\
         StringView transform(StringView input) override {\n\
         auto peer = PipelineValidatorContractPeer::resolve(host_);\n\
         if (!peer) {\n\
         return polyplug::alloc_string(host_, \"ERROR:peer-unavailable\");\n\
         }\n\
         StringView borrowed = peer->validate(input);\n\
         // Copy out of the peer's arena before `peer` is destroyed at scope exit.\n\
         return polyplug::alloc_string(host_, polyplug::abi::to_string(borrowed));\n\
         }\n\
         private:\n\
         const HostApi* host_;\n\
         };\n\
         DataTransformerGuestContract* polyplug_create_transformer(const HostApi* host) { return new TransformerImpl(host); }\n\
         }  // namespace polyplug_plugin\n";
    let consumer_src: PathBuf = bundle_dir.join("consumer.cpp");
    std::fs::write(&consumer_src, consumer_cpp).expect("write consumer.cpp");

    // Compile the bundle into a native cdylib next to its manifest.
    let cpp_abi_include: PathBuf = workspace_root().join("sdks").join("cpp").join("abi");
    let cpp_guest_include: PathBuf = workspace_root().join("sdks").join("cpp").join("guest");
    let out_lib: PathBuf = bundle_dir.join(consumer_lib_filename());

    let build: std::process::Output = Command::new("c++")
        .arg("-std=c++20")
        .arg("-fPIC")
        .arg("-shared")
        .arg("-O0")
        .arg("-I")
        .arg(&gen_dir)
        .arg("-I")
        .arg(&cpp_abi_include)
        .arg("-I")
        .arg(&cpp_guest_include)
        .arg(&consumer_src)
        .arg("-o")
        .arg(&out_lib)
        .output()
        .expect("failed to spawn c++ compiler");
    assert!(
        build.status.success(),
        "c++ build of cpp peer consumer failed (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        build.status.code(),
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr),
    );
    assert!(
        out_lib.exists(),
        "c++ build produced no cdylib at {}",
        out_lib.display()
    );

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
            plugin_name = "test-validator-provider-cpp",
            functions = { [0] = impl_validate },
        },
    }
end
"#
}

/// Write a Lua provider bundle for `pipeline.Validator@1` and return its dir.
fn build_lua_provider(tmp: &Path) -> PathBuf {
    let dir: PathBuf = tmp.join("peer_provider_lua");
    std::fs::create_dir_all(&dir).expect("create provider dir");

    let id_val: u64 = bundle_id("peer_provider_lua");
    let manifest: String = format!(
        "name = \"peer_provider_lua\"\n\
         id = {id_val}\n\
         bundle_name = \"peer_provider_lua\"\n\
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

    // Copy the Lua SDK files so `require("polyplug_guest")` resolves at load time.
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
fn cpp_peer_caller_validate_roundtrip() {
    if !cpp_available() {
        eprintln!("skipping cpp peer caller test: c++ compiler not available");
        return;
    }

    let tmp: tempfile::TempDir = tempfile::tempdir().expect("tempdir");

    let provider_dir: PathBuf = build_lua_provider(tmp.path());
    let consumer_dir: PathBuf = build_cpp_consumer(tmp.path());

    let rt: Arc<Runtime> = Runtime::builder()
        .loader(NativeLoader::new(NativeConfig::default()))
        .loader(LuaLoader::new(LuaConfig::default()))
        .build()
        .expect("build runtime");

    // Load the provider FIRST so pipeline.Validator@1 is registered when the
    // C++ consumer's peer caller resolves it.
    rt.load_bundle(&provider_dir)
        .expect("lua provider bundle must load");
    rt.load_bundle(&consumer_dir)
        .expect("cpp consumer bundle must load");

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
        DispatchType::Native,
        "cpp guests use native dispatch"
    );

    // Native dispatch fn 0 (transform): the generated wrapper signature is
    //   extern "C" AbiError(GuestContractInstance instance, const void* args, void* out).
    // SAFETY: functions[0] is the transform ABI wrapper.
    let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(0) };
    // SAFETY: transmute to the generated 3-arg native dispatch signature.
    let dispatch_fn: unsafe extern "C" fn(GuestContractInstance, *const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };

    let input: &[u8] = b"hello";
    let input_view: StringView = StringView {
        ptr: input.as_ptr(),
        len: input.len(),
    };
    let mut out_view: StringView = StringView::null();
    // The generated create_instance constructs the C++ implementation via the
    // author factory (polyplug_create_transformer) and carries it — plus the
    // host pointer — in instance.data; dispatch requires a real instance.
    // SAFETY: host_abi is the runtime's live HostApi pointer; create_instance
    // is the generated factory thunk on the resolved interface.
    let host_abi: *const polyplug_abi::HostApi = rt.host_abi();
    let instance: GuestContractInstance =
        unsafe { (interface.create_instance)(host_abi, core::ptr::null()) };
    assert!(
        !instance.data.is_null(),
        "create_instance must produce a non-null instance payload"
    );
    // SAFETY: instance was created above; args is a *const StringView, out is a
    // *mut StringView per transform's ABI.
    let err: AbiError = unsafe {
        dispatch_fn(
            instance,
            &input_view as *const StringView as *const (),
            &mut out_view as *mut StringView as *mut (),
        )
    };
    assert_eq!(
        err.code,
        AbiErrorCode::Ok as u32,
        "transform must return Ok; got code={}",
        err.code
    );
    // SAFETY: instance was created by create_instance; destroy exactly once.
    // out_view points to host-allocated bytes (alloc_string), which outlive the
    // instance — destroying before reading the view below is safe.
    unsafe { (interface.destroy_instance)(host_abi, instance) };
    assert!(
        !out_view.ptr.is_null(),
        "returned StringView must not be null"
    );

    // SAFETY: out_view points to host-allocated bytes (alloc_string) that stay
    // valid until the runtime frees them on bundle unload (not yet).
    let result_bytes: &[u8] = unsafe { core::slice::from_raw_parts(out_view.ptr, out_view.len) };
    let result: &str = core::str::from_utf8(result_bytes).expect("result is UTF-8");

    assert_eq!(
        result, "PEER:hello",
        "transform must return 'PEER:hello' proving the generated C++ peer caller round-trip"
    );
}

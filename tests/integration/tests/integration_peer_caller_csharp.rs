//! Integration test: C# guest→guest peer caller at runtime.
//!
//! This proves the **generated** C# peer-caller code (`PeerCallers.cs`) executes
//! end-to-end inside a real `Runtime` — not just that it compiles. The generated
//! `PipelineValidatorContractPeer` is compiled into a `dotnet` bundle and
//! dispatched through.
//!
//! The flow:
//!   1. `polyplugc::generate` emits the C# guest glue for a `data.Transformer@1`
//!      bundle that declares a `[[dependency]]` on `pipeline.Validator` — so the
//!      generator emits `PipelineValidatorContractPeer` into `PeerCallers.cs`.
//!   2. A hand-written `Plugin.cs` implements `Transform(StringView)` by resolving
//!      that generated peer caller and calling `Validate(input)`, then copying the
//!      borrowed result into a host-allocated string.
//!   3. `dotnet publish` builds the bundle (the only hand-written file is
//!      `Plugin.cs`; every generated file is used verbatim), emitting the plugin
//!      assembly plus its `Polyplug.Abi`/`Polyplug.Guest` dependencies and
//!      runtimeconfig — the exact layout `examples/build_all.sh` produces.
//!   4. A Lua **provider** bundle registers `pipeline.Validator@1` whose
//!      `validate(StringView) -> StringView` returns `"PEER:"` + input.
//!   5. Both bundles load into one `Runtime` (DotnetLoader + LuaLoader). The
//!      provider loads first so `pipeline.Validator@1` is registered when the
//!      C# consumer resolves it through `host->find_guest_contract`.
//!   6. The test dispatches the consumer's `transform("hello")` via the native
//!      vtable and asserts the result equals `"PEER:hello"` — proving the
//!      generated C# peer caller routed through `CallGuestMethod` (the #72
//!      contract_id-stamping fix) and marshalled the StringView round-trip.
//!
//! Skips cleanly when `dotnet` is unavailable (mirrors the other dotnet tests).

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
use polyplug_dotnet::DotnetConfig;
use polyplug_dotnet::DotnetLoader;
use polyplug_dotnet::HostfxrLocation;
use polyplug_lua::LuaConfig;
use polyplug_lua::LuaLoader;
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

/// True when `dotnet` is on PATH.
fn dotnet_available() -> bool {
    Command::new("dotnet")
        .arg("--version")
        .output()
        .map(|o: std::process::Output| o.status.success())
        .unwrap_or(false)
}

/// Canonicalize for the dotnet toolchain: resolves the macOS /var ->
/// /private/var symlink, then strips Windows' verbatim prefix MSBuild cannot
/// import. Mirrors `crates/polyplugc/tests/generate_e2e_native.rs`.
fn canonicalize_for_toolchain(path: &Path) -> PathBuf {
    let canonical: PathBuf = path.canonicalize().expect("canonicalize tempdir");
    if cfg!(windows) {
        let s: String = canonical.to_string_lossy().into_owned();
        if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
            PathBuf::from(format!(r"\\{rest}"))
        } else if let Some(rest) = s.strip_prefix(r"\\?\") {
            PathBuf::from(rest)
        } else {
            canonical
        }
    } else {
        canonical
    }
}

// ─── C# consumer bundle (generated glue + hand-written impl, published) ─────────

/// Generate the C# guest glue for a `data.Transformer@1` bundle that depends on
/// `pipeline.Validator`, write `Plugin.cs` + a `.csproj`, `dotnet publish` into a
/// bundle directory, and return that directory ready for the DotnetLoader.
fn build_csharp_consumer(tmp_root: &Path) -> PathBuf {
    let project_dir: PathBuf = tmp_root.join("peer_consumer_csharp_proj");
    let gen_dir: PathBuf = project_dir.join("generated");
    std::fs::create_dir_all(&project_dir).expect("create project dir");

    // bundle.toml: implements data.Transformer@1.0, declares a contract dependency
    // on pipeline.Validator — what makes the generator emit the peer caller. The
    // assembly name is unique so the per-bundle collectible ALC (#68) stays isolated.
    let api_path: PathBuf = workspace_root().join("examples").join("api.toml");
    let bundle_toml: String = format!(
        "[bundle]\n\
         name = \"peer_consumer_csharp\"\n\
         version = \"1.0.0\"\n\
         api = \"{api}\"\n\
         loader = \"dotnet\"\n\
         file = \"peer_consumer_csharp.dll\"\n\n\
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
    let bundle_toml_path: PathBuf = project_dir.join("bundle.toml");
    std::fs::write(&bundle_toml_path, bundle_toml).expect("write bundle.toml");

    // Generate the csharp guest glue + manifest.toml into project_dir/generated;
    // the SDK's default globbing compiles every *.cs under the project dir.
    let config: GenerateConfig = GenerateConfig {
        api_toml: bundle_toml_path,
        lang: Lang::CSharp,
        side: Side::Guest,
        out_dir: gen_dir.clone(),
    };
    let output: GenerateOutput = polyplugc::generate(config).expect("polyplugc generate (csharp)");
    for file in &output.files {
        let file_path: PathBuf = gen_dir.join(&file.path);
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).expect("create generated parent dir");
        }
        std::fs::write(&file_path, &file.content).expect("write generated file");
    }

    // The only hand-written source: Transform() resolves the generated peer caller
    // and calls Validate(input). The peer's returned view borrows its arena, so we
    // copy it into a host-allocated string before the peer is disposed at scope exit.
    let plugin_cs: &str = "using System.Runtime.CompilerServices;\n\
         using Polyplug.Guest;\n\
         using Polyplug.Abi;\n\
         using Polyplug.Generated;\n\
         \n\
         public sealed class TransformerPlugin : IDataTransformerGuestContract\n\
         {\n\
         // Host handle for this runtime, captured at instance creation.\n\
         private readonly IntPtr _host;\n\
         \n\
         public TransformerPlugin(IntPtr host)\n\
         {\n\
         _host = host;\n\
         }\n\
         \n\
         public StringView Transform(StringView input)\n\
         {\n\
         using var peer = PipelineValidatorContractPeer.Resolve(_host);\n\
         if (peer is null)\n\
         {\n\
         return PolyplugHost.AllocString(_host, \"ERROR:peer-unavailable\");\n\
         }\n\
         StringView borrowed = peer.Validate(input);\n\
         return PolyplugHost.AllocString(_host, StringViewHelper.ToString(borrowed));\n\
         }\n\
         }\n\
         \n\
         public static class Registration\n\
         {\n\
         [ModuleInitializer]\n\
         public static void Register()\n\
         {\n\
         TransformerInterfaces.SetTransformerFactory(host => new TransformerPlugin(host));\n\
         }\n\
         }\n";
    std::fs::write(project_dir.join("Plugin.cs"), plugin_cs).expect("write Plugin.cs");

    let guest_csproj: PathBuf = workspace_root()
        .join("sdks")
        .join("csharp")
        .join("guest")
        .join("Polyplug.Guest.csproj");
    assert!(
        guest_csproj.exists(),
        "in-tree guest SDK csproj must exist at {}",
        guest_csproj.display()
    );
    let csproj: String = format!(
        "<Project Sdk=\"Microsoft.NET.Sdk\">\n\
         <PropertyGroup>\n\
         <TargetFramework>net10.0</TargetFramework>\n\
         <Nullable>enable</Nullable>\n\
         <ImplicitUsings>enable</ImplicitUsings>\n\
         <AllowUnsafeBlocks>true</AllowUnsafeBlocks>\n\
         <AssemblyName>peer_consumer_csharp</AssemblyName>\n\
         </PropertyGroup>\n\
         <ItemGroup>\n\
         <ProjectReference Include=\"{}\" />\n\
         </ItemGroup>\n\
         </Project>\n",
        guest_csproj.display(),
    );
    std::fs::write(project_dir.join("Plugin.csproj"), csproj).expect("write Plugin.csproj");

    // Publish into the bundle dir — emits the plugin DLL + Polyplug.Abi/Guest deps
    // + runtimeconfig, exactly as examples/build_all.sh does for C# guests.
    let bundle_dir: PathBuf = tmp_root.join("peer_consumer_csharp_bundle");
    let build: std::process::Output = Command::new("dotnet")
        .arg("publish")
        .arg(project_dir.join("Plugin.csproj"))
        .arg("-c")
        .arg("Release")
        .arg("-o")
        .arg(&bundle_dir)
        .output()
        .expect("failed to spawn dotnet publish");
    assert!(
        build.status.success(),
        "dotnet publish of csharp peer consumer failed (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        build.status.code(),
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr),
    );

    // The loader discovers manifest.toml at the bundle root.
    std::fs::copy(
        gen_dir.join("manifest.toml"),
        bundle_dir.join("manifest.toml"),
    )
    .expect("copy manifest.toml to bundle root");
    assert!(
        bundle_dir.join("peer_consumer_csharp.dll").exists(),
        "publish produced no plugin assembly at {}",
        bundle_dir.display()
    );

    bundle_dir
}

// ─── Lua provider bundle (pipeline.Validator@1) ─────────────────────────────────

fn provider_lua_src() -> &'static str {
    r#"
local ffi = require("ffi")
local polyplug_guest = require("polyplug_guest")
local polyplug_abi = require("polyplug_abi")

-- Stateless provider: the factory returns a fresh (empty) instance per
-- create_instance; the loader passes it back as each handler's first argument.
local function make_validator(host)
    return {}
end

local function impl_validate(instance, args_ptr, out_ptr)
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
            plugin_name = "test-validator-provider-csharp",
            factory = make_validator,
            functions = { [0] = impl_validate },
        },
    }
end
"#
}

/// Write a Lua provider bundle for `pipeline.Validator@1` and return its dir.
fn build_lua_provider(tmp: &Path) -> PathBuf {
    let dir: PathBuf = tmp.join("peer_provider_lua_csharp");
    std::fs::create_dir_all(&dir).expect("create provider dir");

    let id_val: u64 = bundle_id("peer_provider_lua_csharp");
    let manifest: String = format!(
        "name = \"peer_provider_lua_csharp\"\n\
         id = {id_val}\n\
         bundle_name = \"peer_provider_lua_csharp\"\n\
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
fn csharp_peer_caller_validate_roundtrip() {
    if !dotnet_available() {
        eprintln!("skipping csharp peer caller test: dotnet not available");
        return;
    }

    let tmp: tempfile::TempDir = tempfile::tempdir().expect("tempdir");
    let tmp_root: PathBuf = canonicalize_for_toolchain(tmp.path());

    let provider_dir: PathBuf = build_lua_provider(&tmp_root);
    let consumer_dir: PathBuf = build_csharp_consumer(&tmp_root);

    let rt: Arc<Runtime> = Runtime::builder()
        .loader(DotnetLoader::new(DotnetConfig {
            min_framework: String::from("net10.0"),
            hostfxr: HostfxrLocation::Auto,
        }))
        .loader(LuaLoader::new(LuaConfig::default()))
        .build()
        .expect("build runtime");

    // Load the provider FIRST so pipeline.Validator@1 is registered when the
    // C# consumer's peer caller resolves it.
    rt.load_bundle(&provider_dir)
        .expect("lua provider bundle must load");
    rt.load_bundle(&consumer_dir)
        .expect("csharp consumer bundle must load");

    // Resolve the consumer contract (data.Transformer@1).
    let transformer_id: u64 = guest_contract_id("data.Transformer", 1);
    let handle: GuestContractHandle = rt
        .find_guest_contract(transformer_id, 0)
        .expect("data.Transformer must be registered after load");
    let interface_ptr: *const GuestContractInterface = rt
        .resolve_guest_contract(handle)
        .expect("handle must resolve");
    // SAFETY: CLR keeps the assembly loaded for the process lifetime.
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };
    assert_eq!(
        interface.dispatch_type,
        DispatchType::Native,
        "C# generated guest must register NATIVE dispatch"
    );

    // Native dispatch fn 0 (transform): fn(GuestContractInstance, *const (), *mut ()) -> AbiError.
    // SAFETY: functions[0] is the transform ABI thunk.
    let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(0) };
    // SAFETY: transmute to the canonical native dispatch out-param fn pointer.
    let dispatch_fn: unsafe extern "C" fn(
        GuestContractInstance,
        *const (),
        *mut (),
        *mut AbiError,
    ) = unsafe { core::mem::transmute(fn_ptr) };

    let input: &[u8] = b"hello";
    let input_view: StringView = StringView {
        ptr: input.as_ptr(),
        len: input.len(),
    };
    let mut out_view: StringView = StringView::null();
    // The generated CreateInstance constructs the C# implementation via the
    // author factory and carries it in instance.Data (GCHandle); dispatch
    // requires a real instance.
    // SAFETY: host_abi is the runtime's live HostApi pointer; create_instance
    // is the generated factory thunk on the resolved interface.
    let host_abi: *const polyplug_abi::HostApi = rt.host_abi();
    let mut instance: GuestContractInstance = GuestContractInstance::null();
    unsafe {
        (interface.create_instance)(
            polyplug_abi::VmLoaderData::null(),
            host_abi,
            core::ptr::null(),
            &mut instance as *mut GuestContractInstance,
        )
    };
    assert!(
        !instance.data.is_null(),
        "create_instance must produce a non-null instance payload"
    );
    // SAFETY: instance was created above; args is a *const StringView, out a
    // *mut StringView per transform's ABI.
    let mut err: AbiError = AbiError::ok();
    unsafe {
        dispatch_fn(
            instance,
            &input_view as *const StringView as *const (),
            &mut out_view as *mut StringView as *mut (),
            &mut err as *mut AbiError,
        )
    };
    assert_eq!(
        err.code,
        AbiErrorCode::Ok as u32,
        "transform must return Ok; got code={}",
        err.code
    );
    // SAFETY: instance was created by create_instance; destroy exactly once.
    // out_view points to host-allocated bytes (AllocString), which outlive the
    // instance.
    unsafe { (interface.destroy_instance)(polyplug_abi::VmLoaderData::null(), host_abi, instance) };
    assert!(
        !out_view.ptr.is_null(),
        "returned StringView must not be null"
    );

    // SAFETY: out_view points to host-allocated bytes (AllocString) valid until
    // the runtime frees them on bundle unload (not yet).
    let result_bytes: &[u8] = unsafe { core::slice::from_raw_parts(out_view.ptr, out_view.len) };
    let result: &str = core::str::from_utf8(result_bytes).expect("result is UTF-8");

    assert_eq!(
        result, "PEER:hello",
        "transform must return 'PEER:hello' proving the generated C# peer caller round-trip"
    );
}

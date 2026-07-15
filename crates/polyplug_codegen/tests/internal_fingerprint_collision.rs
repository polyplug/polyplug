#![allow(clippy::expect_used)]

use std::env::join_paths;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::{Command, Output};

use polyplug_codegen::{
    GenerateOutput, InternalCSharpGenerateConfig, InternalCppGenerateConfig,
    InternalJavaScriptGenerateConfig, InternalLuaGenerateConfig, InternalPythonGenerateConfig,
    InternalRustGenerateConfig, OutputLayout, generate_internal_cpp, generate_internal_csharp,
    generate_internal_javascript, generate_internal_lua, generate_internal_python,
    generate_internal_rust, write_output,
};
use polyplug_utils::bundle_id;
use tempfile::TempDir;

const BUNDLE_NAME: &str = "fingerprint_collision";
const AUTHORED_TYPES: [&str; 4] = [
    "INTERNAL_GENERATION_FINGERPRINT",
    "InternalGenerationFingerprint",
    "InternalGuestFingerprint",
    "InternalHostFingerprint",
];

struct CollisionFixture {
    _temp: TempDir,
    bundle: PathBuf,
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace crates directory")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn internal_namespace() -> PathBuf {
    Path::new("internal").join(format!("{BUNDLE_NAME}-{:016x}", bundle_id(BUNDLE_NAME)))
}

fn write_fixture() -> CollisionFixture {
    let temp = tempfile::tempdir().expect("create collision fixture");
    let api = temp.path().join("api.toml");
    let bundle = temp.path().join("bundle.toml");
    fs::write(
        &api,
        "[[types]]\nname = \"INTERNAL_GENERATION_FINGERPRINT\"\nfields = [{ name = \"value\", type = \"u32\" }]\n\n[[types]]\nname = \"InternalGenerationFingerprint\"\nfields = [{ name = \"value\", type = \"u32\" }]\n\n[[types]]\nname = \"InternalGuestFingerprint\"\nfields = [{ name = \"value\", type = \"u32\" }]\n\n[[types]]\nname = \"InternalHostFingerprint\"\nfields = [{ name = \"value\", type = \"u32\" }]\n\n[[guest_contract]]\nname = \"collision.Plugin\"\nversion = \"1.0\"\n\n[[guest_contract.functions]]\nname = \"read\"\nreturn = \"INTERNAL_GENERATION_FINGERPRINT\"\n",
    )
    .expect("write collision API");
    fs::write(
        &bundle,
        "[bundle]\nname = \"fingerprint_collision\"\nversion = \"1.0\"\napi = \"api.toml\"\n\n[[plugin]]\nname = \"provider\"\nimplements = [\"collision.Plugin@1.0\"]\n",
    )
    .expect("write collision bundle");
    CollisionFixture {
        _temp: temp,
        bundle,
    }
}

fn assert_source_diagnostics(output: &GenerateOutput, language: &str) {
    let content = output
        .files
        .iter()
        .map(|file| file.content.as_str())
        .collect::<String>();
    for name in AUTHORED_TYPES {
        assert!(
            content.contains(name),
            "{language} diagnostic output omitted authored collision-shaped declaration {name}"
        );
    }
    assert!(
        content.contains("_polyplug")
            || content.contains("POLYPLUG_INTERNAL_GENERATION_FINGERPRINT"),
        "{language} diagnostic output omitted reserved fingerprint sentinel"
    );
}

fn assert_command_succeeds(output: Output, label: &str) {
    assert!(
        output.status.success(),
        "{label} failed with {:?}:\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn csharp_guest_namespace() -> String {
    format!(
        "Polyplug.Generated.Internal.Bundle{BUNDLE_NAME}{:016X}.Guest",
        bundle_id(BUNDLE_NAME)
    )
}

fn to_file_url(path: &Path) -> String {
    let normalized = path.to_string_lossy().replace('\\', "/");
    if normalized.starts_with('/') {
        format!("file://{normalized}")
    } else {
        format!("file:///{normalized}")
    }
}

fn lua_path_literal(path: &Path) -> String {
    let path = path.display().to_string();
    for delimiter_len in 0.. {
        let delimiter = "=".repeat(delimiter_len);
        let close = format!("]{delimiter}]");
        if !path.contains(&close) {
            return format!("[{delimiter}[{path}]{delimiter}]");
        }
    }
    unreachable!("a finite path has an available Lua long-string delimiter")
}

fn write_csharp_project(project: &Path) {
    let root = workspace_root();
    fs::write(
        project.join("Collision.csproj"),
        format!(
            "<Project Sdk=\"Microsoft.NET.Sdk\">\n    <PropertyGroup>\n        <TargetFramework>net10.0</TargetFramework>\n        <OutputType>Exe</OutputType>\n        <Nullable>enable</Nullable>\n        <ImplicitUsings>enable</ImplicitUsings>\n        <AllowUnsafeBlocks>true</AllowUnsafeBlocks>\n    </PropertyGroup>\n    <ItemGroup>\n        <ProjectReference Include=\"{}\" />\n        <ProjectReference Include=\"{}\" />\n    </ItemGroup>\n</Project>\n",
            root.join("sdks/csharp/guest/Polyplug.Guest.csproj").display(),
            root.join("sdks/csharp/host/Polyplug.Host.csproj").display(),
        ),
    )
    .expect("write C# collision project");
}

fn run_rust_consumer(output: &GenerateOutput) {
    let project = tempfile::tempdir().expect("create isolated Rust collision project");
    let source = project.path().join("src");
    fs::create_dir_all(&source).expect("create Rust collision source directory");
    write_output(output, &source).expect("write Rust collision output");
    fs::rename(source.join(internal_namespace()), source.join("generated"))
        .expect("place Rust collision module");
    let root = workspace_root();
    fs::write(
        project.path().join("Cargo.toml"),
        format!(
            "[package]\nname = \"internal_fingerprint_collision_consumer\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\npolyplug = {{ path = \"{}\" }}\npolyplug_abi = {{ path = \"{}\" }}\npolyplug_common = {{ path = \"{}\" }}\npolyplug_guest = {{ path = \"{}\" }}\npolyplug_utils = {{ path = \"{}\" }}\n\n[workspace]\n",
            root.join("crates/polyplug").display(),
            root.join("crates/polyplug_abi").display(),
            root.join("crates/polyplug_common").display(),
            root.join("sdks/rust/guest").display(),
            root.join("crates/polyplug_utils").display(),
        ),
    )
    .expect("write Rust collision Cargo manifest");
    fs::write(
        source.join("main.rs"),
        "mod generated;\n\nuse core::mem::size_of;\n\nfn main() {\n    let authored_sizes = [\n        size_of::<generated::guest::domain::INTERNAL_GENERATION_FINGERPRINT>(),\n        size_of::<generated::guest::domain::InternalGenerationFingerprint>(),\n        size_of::<generated::guest::domain::InternalGuestFingerprint>(),\n        size_of::<generated::guest::domain::InternalHostFingerprint>(),\n    ];\n    assert!(authored_sizes.into_iter().all(|size| size > 0));\n}\n",
    )
    .expect("write Rust collision consumer");
    assert_command_succeeds(
        Command::new("cargo")
            .arg("run")
            .current_dir(project.path())
            .output()
            .expect("run Rust collision consumer"),
        "Rust collision consumer",
    );
}

fn run_cpp_consumer(output: &GenerateOutput) {
    let project = tempfile::tempdir().expect("create isolated C++ collision project");
    let generated = project.path().join("generated");
    write_output(output, &generated).expect("write C++ collision output");
    let profile = generated.join(internal_namespace());
    let namespace = format!("polyplug_generated::bundle_{:016x}", bundle_id(BUNDLE_NAME));
    let driver = project.path().join("collision.cpp");
    fs::write(
        &driver,
        format!(
            "#include <cstddef>\n#include \"guest/internal_plugin.hpp\"\n#include \"host/types.hpp\"\n\nint main() {{\n    using namespace {namespace};\n    const auto authored_size = sizeof(INTERNAL_GENERATION_FINGERPRINT)\n        + sizeof(InternalGenerationFingerprint)\n        + sizeof(InternalGuestFingerprint)\n        + sizeof(InternalHostFingerprint);\n    return authored_size > 0 ? 0 : 1;\n}}\n"
        ),
    )
    .expect("write C++ collision consumer");
    let root = workspace_root();
    let executable = project.path().join("collision");
    assert_command_succeeds(
        Command::new("g++")
            .args(["-std=c++20"])
            .arg(&driver)
            .arg("-I")
            .arg(&profile)
            .arg("-I")
            .arg(root.join("sdks/cpp/host"))
            .arg("-I")
            .arg(root.join("sdks/cpp/guest"))
            .arg("-I")
            .arg(root.join("sdks/cpp/abi"))
            .arg("-o")
            .arg(&executable)
            .output()
            .expect("compile C++ collision consumer"),
        "C++ collision consumer compile",
    );
    assert_command_succeeds(
        Command::new(&executable)
            .output()
            .expect("run C++ collision consumer"),
        "C++ collision consumer execution",
    );
}

fn run_csharp_consumer(output: &GenerateOutput) {
    let project = tempfile::tempdir().expect("create isolated C# collision project");
    write_output(output, project.path()).expect("write C# collision output");
    write_csharp_project(project.path());
    let namespace = csharp_guest_namespace();
    fs::write(
        project.path().join("Program.cs"),
        format!(
            "using Guest = global::{namespace};\n\nvar authoredDeclarations = new[] {{\n    typeof(Guest.INTERNAL_GENERATION_FINGERPRINT),\n    typeof(Guest.InternalGenerationFingerprint),\n    typeof(Guest.InternalGuestFingerprint),\n    typeof(Guest.InternalHostFingerprint),\n}};\nif (authoredDeclarations.Any(type => type.Namespace != \"{namespace}\")) throw new InvalidOperationException(\"authored declaration was replaced\");\nConsole.WriteLine(\"csharp-collision-ok\");\n"
        ),
    )
    .expect("write C# collision consumer");
    let output = Command::new("dotnet")
        .args(["run", "--project"])
        .arg(project.path().join("Collision.csproj"))
        .args(["-c", "Release", "--nologo"])
        .output()
        .expect("run C# collision consumer");
    assert_command_succeeds(output, "C# collision consumer");
}

fn run_python_consumer(output: &GenerateOutput) {
    let project = tempfile::tempdir().expect("create isolated Python collision project");
    let generated = project.path().join("generated");
    write_output(output, &generated).expect("write Python collision output");
    fs::rename(
        generated.join(internal_namespace()),
        project.path().join("collision_profile"),
    )
    .expect("make Python collision profile importable");
    fs::write(
        project.path().join("driver.py"),
        "from collision_profile import internal\nfrom collision_profile.guest import types\n\nnames = (\n    \"INTERNAL_GENERATION_FINGERPRINT\",\n    \"InternalGenerationFingerprint\",\n    \"InternalGuestFingerprint\",\n    \"InternalHostFingerprint\",\n)\nfor name in names:\n    authored = getattr(types, name)()\n    authored.value = 7\n    assert authored.value == 7\nassert internal._polyplug_internal_generation_fingerprint > 0\nprint(\"python-collision-ok\")\n",
    )
    .expect("write Python collision consumer");
    let root = workspace_root();
    let python_paths = join_paths([
        project.path().to_path_buf(),
        root.join("tests/fixtures/test_plugin_python/site-packages"),
    ])
    .expect("Python search paths are valid");
    assert_command_succeeds(
        Command::new("python3")
            .arg(project.path().join("driver.py"))
            .env("PYTHONPATH", python_paths)
            .output()
            .expect("run Python collision consumer"),
        "Python collision consumer",
    );
}

fn run_lua_consumer(output: &GenerateOutput) {
    let project = tempfile::tempdir().expect("create isolated Lua collision project");
    let generated = project.path().join("generated");
    write_output(output, &generated).expect("write Lua collision output");
    let profile = generated.join(internal_namespace()).join("init.lua");
    let script = project.path().join("driver.lua");
    fs::write(
        &script,
        format!(
            "local ffi = require(\"ffi\")\nffi.cdef[[\ntypedef struct {{ const char *ptr; size_t len; }} StringView;\ntypedef struct {{ void *ptr; size_t len; size_t capacity; }} Buffer;\ntypedef struct {{ void *data; uint64_t contract_id; }} GuestContractInstance;\ntypedef struct {{ uint32_t index; uint32_t generation; }} GuestContractHandle;\ntypedef struct {{ uint64_t contract_id; }} GuestContractInterface;\ntypedef struct {{ uint32_t code; void *message; }} AbiError;\n]]\npackage.preload[\"polyplug.loaders.lua\"] = function()\n    return {{ internal_plugin_bridge = function() return {{}} end }}\nend\nlocal profile = dofile({})\nfor _, name in ipairs({{\n    \"INTERNAL_GENERATION_FINGERPRINT\",\n    \"InternalGenerationFingerprint\",\n    \"InternalGuestFingerprint\",\n    \"InternalHostFingerprint\",\n}}) do\n    local authored = ffi.new(name)\n    authored.value = 7\n    assert(authored.value == 7)\nend\nassert(profile.guest ~= nil and profile.host ~= nil)\nprint(\"lua-collision-ok\")\n",
            lua_path_literal(&profile)
        ),
    )
    .expect("write Lua collision consumer");
    assert_command_succeeds(
        Command::new("luajit")
            .arg(&script)
            .output()
            .expect("run Lua collision consumer"),
        "Lua collision consumer",
    );
}

fn run_javascript_consumer(output: &GenerateOutput) {
    let project = tempfile::tempdir().expect("create isolated JavaScript collision project");
    let generated = project.path().join("generated");
    write_output(output, &generated).expect("write JavaScript collision output");
    let profile = generated.join(internal_namespace());
    let root = workspace_root();
    let import_map = project.path().join("deno-imports.json");
    fs::write(
        &import_map,
        format!(
            "{{\"imports\":{{\"@polyplug/abi\":\"{}\",\"@polyplug/host\":\"{}\",\"@polyplug/loaders/js\":\"{}\"}}}}",
            to_file_url(&root.join("sdks/js/abi/polyplug_abi.ts")),
            to_file_url(&root.join("sdks/js/host/mod.js")),
            to_file_url(&root.join("sdks/js/loaders/js/mod.ts")),
        ),
    )
    .expect("write JavaScript collision import map");
    let types = to_file_url(&profile.join("guest/types.ts"));
    let internal = to_file_url(&profile.join("internal.ts"));
    fs::write(
        project.path().join("driver.ts"),
        format!(
            "import type {{ INTERNAL_GENERATION_FINGERPRINT, InternalGenerationFingerprint, InternalGuestFingerprint, InternalHostFingerprint }} from \"{types}\";\n\nconst authored: [INTERNAL_GENERATION_FINGERPRINT, InternalGenerationFingerprint, InternalGuestFingerprint, InternalHostFingerprint] = [\n    {{ value: 7 }},\n    {{ value: 7 }},\n    {{ value: 7 }},\n    {{ value: 7 }},\n];\nif (authored.some(value => value.value !== 7)) throw new Error(\"authored declaration was overwritten\");\nconst profile = await import(\"{internal}\");\nif (typeof profile._polyplugInternalGenerationFingerprint !== \"bigint\") throw new Error(\"generated root did not initialize\");\nconsole.log(\"javascript-collision-ok\");\n"
        ),
    )
    .expect("write JavaScript collision consumer");
    assert_command_succeeds(
        Command::new("deno")
            .arg("run")
            .arg("--import-map")
            .arg(&import_map)
            .args(["--allow-env", "--allow-read", "--allow-ffi"])
            .arg(project.path().join("driver.ts"))
            .output()
            .expect("run JavaScript collision consumer"),
        "JavaScript collision consumer",
    );
}

#[test]
fn internal_fingerprint_sentinels_do_not_collide_with_authored_names() {
    let fixture = write_fixture();
    let layout = OutputLayout::unified();
    let rust = generate_internal_rust(InternalRustGenerateConfig {
        bundle_toml: fixture.bundle.clone(),
        layout: layout.clone(),
    })
    .expect("generate Rust collision profile");
    let cpp_project = tempfile::tempdir().expect("create isolated C++ generator project");
    let cpp = generate_internal_cpp(InternalCppGenerateConfig {
        bundle_toml: fixture.bundle.clone(),
        out_dir: cpp_project.path().join("out"),
        layout: layout.clone(),
    })
    .expect("generate C++ collision profile");
    let csharp_project = tempfile::tempdir().expect("create isolated C# generator project");
    let csharp = generate_internal_csharp(InternalCSharpGenerateConfig {
        bundle_toml: fixture.bundle.clone(),
        out_dir: csharp_project.path().join("out"),
        layout: layout.clone(),
    })
    .expect("generate C# collision profile");
    let python_project = tempfile::tempdir().expect("create isolated Python generator project");
    let python = generate_internal_python(InternalPythonGenerateConfig {
        bundle_toml: fixture.bundle.clone(),
        out_dir: python_project.path().join("out"),
        layout: layout.clone(),
    })
    .expect("generate Python collision profile");
    let lua_project = tempfile::tempdir().expect("create isolated Lua generator project");
    let lua = generate_internal_lua(InternalLuaGenerateConfig {
        bundle_toml: fixture.bundle.clone(),
        out_dir: lua_project.path().join("out"),
        layout: layout.clone(),
    })
    .expect("generate Lua collision profile");
    let javascript_project =
        tempfile::tempdir().expect("create isolated JavaScript generator project");
    let javascript = generate_internal_javascript(InternalJavaScriptGenerateConfig {
        bundle_toml: fixture.bundle,
        out_dir: javascript_project.path().join("out"),
        layout,
    })
    .expect("generate JavaScript collision profile");

    for (language, output) in [
        ("Rust", &rust),
        ("C++", &cpp),
        ("C#", &csharp),
        ("Python", &python),
        ("Lua", &lua),
        ("JavaScript", &javascript),
    ] {
        assert_source_diagnostics(output, language);
    }

    run_rust_consumer(&rust);
    run_cpp_consumer(&cpp);
    run_csharp_consumer(&csharp);
    run_python_consumer(&python);
    run_lua_consumer(&lua);
    run_javascript_consumer(&javascript);
}

#![allow(clippy::expect_used)]

#[cfg(windows)]
use core::iter::once;
use std::collections::HashSet;
#[cfg(windows)]
use std::env::{join_paths, split_paths, var_os};
use std::ffi::OsString;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Output;

use polyplug_codegen::{
    GenerateConfig, GenerateOutput, InternalCppGenerateConfig, Lang, OutputDestination,
    OutputLayout, OutputPartition, PolyplugcError, Side, ValidatedImport, generate,
    generate_internal_cpp, write_output,
};
use tempfile::TempDir;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace crates directory")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn build_polyplug_runtime(root: &Path) -> PathBuf {
    let build = Command::new(env!("CARGO"))
        .args(["build", "-p", "polyplug"])
        .current_dir(root)
        .status()
        .expect("build real polyplug runtime");
    assert!(build.success(), "build real polyplug runtime");
    root.join("target").join("debug")
}

fn msvc_import_library_candidates(runtime_dir: &Path) -> [PathBuf; 2] {
    [
        runtime_dir.join("polyplug.dll.lib"),
        runtime_dir.join("polyplug.lib"),
    ]
}

#[cfg(windows)]
fn discover_msvc_import_library(runtime_dir: &Path) -> Result<PathBuf, String> {
    let candidates = msvc_import_library_candidates(runtime_dir);
    candidates
        .iter()
        .find(|candidate| candidate.is_file())
        .cloned()
        .ok_or_else(|| {
            let expected = candidates
                .iter()
                .map(|candidate| candidate.display().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "Cargo did not produce the MSVC polyplug import library in {}. Expected one of: {expected}. Run `cargo build -p polyplug` with the x86_64-pc-windows-msvc toolchain.",
                runtime_dir.display()
            )
        })
}

fn msvc_cpp_runtime_args(
    driver: &Path,
    generated: &Path,
    root: &Path,
    executable: &Path,
    import_library: &Path,
) -> Vec<OsString> {
    let mut executable_flag = OsString::from("/Fe");
    executable_flag.push(executable);
    let mut generated_include = OsString::from("/I");
    generated_include.push(generated);
    let mut host_include = OsString::from("/I");
    host_include.push(root.join("sdks").join("cpp").join("host"));
    let mut abi_include = OsString::from("/I");
    abi_include.push(root.join("sdks").join("cpp").join("abi"));
    vec![
        OsString::from("/nologo"),
        OsString::from("/std:c++20"),
        OsString::from("/EHsc"),
        generated_include,
        host_include,
        abi_include,
        driver.as_os_str().to_os_string(),
        executable_flag,
        OsString::from("/link"),
        import_library.as_os_str().to_os_string(),
    ]
}

fn compile_cpp_runtime_driver(
    driver: &Path,
    generated: &Path,
    root: &Path,
    runtime_dir: &Path,
    executable_name: &str,
) -> Output {
    #[cfg(windows)]
    {
        let executable = generated.join(executable_name).with_extension("exe");
        let import_library = discover_msvc_import_library(runtime_dir)
            .expect("discover Cargo-generated MSVC polyplug import library");
        return Command::new("cl.exe")
            .args(msvc_cpp_runtime_args(
                driver,
                generated,
                root,
                &executable,
                &import_library,
            ))
            .current_dir(generated)
            .output()
            .expect("compile generated C++ runtime driver with MSVC");
    }
    #[cfg(not(windows))]
    {
        let executable = generated.join(executable_name);
        Command::new("g++")
            .arg("-std=c++20")
            .arg(driver)
            .arg("-I")
            .arg(generated)
            .arg("-I")
            .arg(root.join("sdks").join("cpp").join("host"))
            .arg("-I")
            .arg(root.join("sdks").join("cpp").join("abi"))
            .arg("-L")
            .arg(runtime_dir)
            .arg("-lpolyplug")
            .arg(format!("-Wl,-rpath,{}", runtime_dir.display()))
            .arg("-o")
            .arg(executable)
            .output()
            .expect("compile generated C++ runtime driver")
    }
}

fn runtime_driver_path(generated: &Path, executable_name: &str) -> PathBuf {
    let executable = generated.join(executable_name);
    if cfg!(windows) {
        executable.with_extension("exe")
    } else {
        executable
    }
}

fn run_cpp_runtime_driver(executable: &Path, runtime_dir: &Path) -> Output {
    let mut command = Command::new(executable);
    #[cfg(windows)]
    {
        let existing = var_os("PATH");
        let paths = existing.as_deref().map(split_paths).into_iter().flatten();
        let path = join_paths(once(runtime_dir.to_path_buf()).chain(paths))
            .expect("runtime PATH entries must be valid");
        command.env("PATH", path);
    }
    #[cfg(not(windows))]
    {
        let _ = runtime_dir;
    }
    command.output().expect("run generated C++ runtime driver")
}

#[test]
fn msvc_runtime_driver_command_uses_cargo_import_library() {
    let root = Path::new("workspace");
    let generated = Path::new("generated");
    let runtime_dir = Path::new("target").join("debug");
    let import_library = msvc_import_library_candidates(&runtime_dir)
        .into_iter()
        .next()
        .expect("MSVC import library candidate");
    let driver = generated.join("driver.cpp");
    let executable = generated.join("driver.exe");
    let args = msvc_cpp_runtime_args(&driver, generated, root, &executable, &import_library);

    let mut executable_flag = OsString::from("/Fe");
    executable_flag.push(&executable);
    assert!(args.iter().any(|arg| arg == driver.as_os_str()));
    assert!(args.iter().any(|arg| arg == import_library.as_os_str()));
    assert!(args.iter().any(|arg| arg == &OsString::from("/link")));
    assert!(
        args.iter().any(|arg| arg == &executable_flag),
        "MSVC command must produce the requested executable"
    );
}

fn write_api(path: &Path, contract: &str) {
    fs::write(
        path,
        format!(
            r#"
[[enum]]
name = "Mode"
repr = "u32"
[[enum.variants]]
name = "Ready"
value = "0"

[[types]]
name = "Inner"
fields = [
  {{ name = "label", type = "StringView" }},
  {{ name = "bytes", type = "Buffer" }},
]

[[types]]
name = "Envelope"
fields = [
  {{ name = "inner", type = "Inner" }},
  {{ name = "entries", type = "Array<Inner>" }},
  {{ name = "mode", type = "Mode" }},
]

[[guest_contract]]
name = "{contract}"
version = "1.0"

[[guest_contract.functions]]
name = "metadata"
return = "Envelope"

[[guest_contract.functions]]
name = "read"
params = [{{ name = "address", type = "u64" }}, {{ name = "size", type = "u32" }}]
return = "Buffer"

[[guest_contract.functions]]
name = "write"
params = [{{ name = "address", type = "u64" }}, {{ name = "bytes", type = "Buffer" }}]
return = "u32"

[[guest_contract.functions]]
name = "inspect"
params = [
  {{ name = "label", type = "StringView" }},
  {{ name = "mode", type = "Mode" }},
  {{ name = "entries", type = "Array<Inner>" }},
]
return = "StringView"

[[guest_contract.functions]]
name = "take_inner"
params = [{{ name = "inner", type = "Inner" }}]
return = "u32"
"#
        ),
    )
    .expect("write API fixture");
}

fn write_primitive_api(path: &Path, contract: &str) {
    fs::write(
        path,
        format!(
            r#"
[[guest_contract]]
name = "{contract}"
version = "1.0"

[[guest_contract.functions]]
name = "increment"
params = [{{ name = "value", type = "u32" }}]
return = "u32"
"#
        ),
    )
    .expect("write primitive API fixture");
}

fn write_abi_only_api(path: &Path, contract: &str) {
    fs::write(
        path,
        format!(
            r#"
[[guest_contract]]
name = "{contract}"
version = "1.0"

[[guest_contract.functions]]
name = "transform"
params = [
  {{ name = "label", type = "StringView" }},
  {{ name = "bytes", type = "Buffer" }},
]
return = "Buffer"
"#
        ),
    )
    .expect("write ABI-only API fixture");
}

fn primitive_internal_output(
    temp: &TempDir,
    name: &str,
    contract: &str,
    layout: OutputLayout,
) -> GenerateOutput {
    let api = temp.path().join(format!("{name}.toml"));
    let bundle = temp.path().join(format!("{name}.bundle.toml"));
    write_primitive_api(&api, contract);
    write_bundle(
        &bundle,
        &format!("{name}.toml"),
        name,
        &format!("{name}.provider"),
        contract,
    );
    generate_internal_cpp(InternalCppGenerateConfig {
        bundle_toml: bundle,
        out_dir: temp.path().join("out"),
        layout,
    })
    .expect("generate primitive internal C++ profile")
}

fn write_bundle(path: &Path, api: &str, bundle_name: &str, plugin_name: &str, contract: &str) {
    fs::write(
        path,
        format!(
            "[bundle]\nname = \"{bundle_name}\"\nversion = \"1.0\"\napi = \"{api}\"\n\n[[plugin]]\nname = \"{plugin_name}\"\nimplements = [\"{contract}@1.0\"]\n"
        ),
    )
    .expect("write internal bundle fixture");
}

fn internal_output(temp: &TempDir, name: &str, contract: &str) -> GenerateOutput {
    internal_output_with_layout(temp, name, contract, OutputLayout::unified())
}

fn internal_output_with_layout(
    temp: &TempDir,
    name: &str,
    contract: &str,
    layout: OutputLayout,
) -> GenerateOutput {
    let api = temp.path().join(format!("{name}.toml"));
    let bundle = temp.path().join(format!("{name}.bundle.toml"));
    write_api(&api, contract);
    write_bundle(
        &bundle,
        &format!("{name}.toml"),
        name,
        &format!("{name}.provider"),
        contract,
    );
    generate_internal_cpp(InternalCppGenerateConfig {
        bundle_toml: bundle,
        out_dir: temp.path().join("out"),
        layout,
    })
    .expect("generate internal C++ profile")
}

#[cfg(not(windows))]
#[test]
fn partitioned_cpp_outputs_use_external_domain_and_contract_headers() {
    let temp = TempDir::new().expect("create partitioned C++ fixture");
    let api = temp.path().join("api.toml");
    let bundle = temp.path().join("bundle.toml");
    write_api(&api, "platform.Plugin");
    write_bundle(
        &bundle,
        "api.toml",
        "external_cpp",
        "external.provider",
        "platform.Plugin",
    );
    let bundle_source = fs::read_to_string(&bundle).expect("read external C++ bundle");
    fs::write(
        &bundle,
        bundle_source.replacen(
            "[bundle]\n",
            "[bundle]\nloader = \"python\"\nfile = \"external_cpp.py\"\n",
            1,
        ),
    )
    .expect("add external C++ loader metadata");

    let domain_root = temp.path().join("domain");
    let contracts_root = temp.path().join("contracts");
    let domain_import =
        ValidatedImport::parse(Lang::Cpp, "guest/domain.hpp").expect("valid domain include");
    let contracts_import = ValidatedImport::parse(Lang::Cpp, "guest/guest_contracts.hpp")
        .expect("valid guest contracts include");
    let guest_layout = OutputLayout {
        bindings: OutputDestination::Inline,
        domain_types: OutputDestination::Emit {
            root: domain_root.clone(),
            import: domain_import.clone(),
        },
        guest_contracts: OutputDestination::Emit {
            root: contracts_root.clone(),
            import: contracts_import.clone(),
        },
    };
    let host_layout = OutputLayout {
        bindings: OutputDestination::Inline,
        domain_types: OutputDestination::ImportOnly {
            import: domain_import,
        },
        guest_contracts: OutputDestination::Omit,
    };

    let guest = generate(GenerateConfig {
        api_toml: bundle,
        lang: Lang::Cpp,
        side: Side::Guest,
        layout: guest_layout,
    })
    .expect("generate partitioned C++ guest bindings");
    let host = generate(GenerateConfig {
        api_toml: api,
        lang: Lang::Cpp,
        side: Side::Host,
        layout: host_layout,
    })
    .expect("generate host bindings against external domain headers");
    assert!(guest.files.iter().any(|file| {
        file.partition == OutputPartition::DomainTypes && file.path == Path::new("guest/domain.hpp")
    }));
    assert!(guest.files.iter().any(|file| {
        file.partition == OutputPartition::GuestContracts
            && file.path == Path::new("guest/guest_contracts.hpp")
    }));
    let domain = guest
        .files
        .iter()
        .find(|file| file.partition == OutputPartition::DomainTypes)
        .expect("guest domain header");
    assert!(!domain.content.contains("CONTRACT_ID"));
    let guest_metadata = guest
        .files
        .iter()
        .find(|file| file.path == Path::new("guest/types.hpp"))
        .expect("guest binding metadata");
    assert!(
        guest_metadata
            .content
            .contains("PLATFORM_PLUGIN_CONTRACT_ID")
    );
    assert_eq!(
        guest_metadata.references,
        vec![OutputPartition::DomainTypes]
    );
    let host_metadata = host
        .files
        .iter()
        .find(|file| file.path == Path::new("host/types.hpp"))
        .expect("host binding metadata");
    assert!(
        host_metadata
            .content
            .contains("#include \"guest/domain.hpp\"")
    );
    assert_eq!(host_metadata.references, vec![OutputPartition::DomainTypes]);
    let contracts = guest
        .files
        .iter()
        .find(|file| file.partition == OutputPartition::GuestContracts)
        .expect("guest provider contracts");
    assert!(contracts.content.contains("#include \"guest/domain.hpp\""));
    let interfaces = guest
        .files
        .iter()
        .find(|file| file.path == Path::new("guest/interfaces.hpp"))
        .expect("guest ABI bindings");

    assert!(
        interfaces
            .content
            .contains("#include \"guest/guest_contracts.hpp\"")
    );
    let bindings_root = temp.path().join("bindings");
    write_output(&guest, &bindings_root).expect("write partitioned guest bindings");
    write_output(&host, &bindings_root).expect("write partitioned host bindings");
    let guest_syntax = bindings_root.join("guest_bindings_syntax.cpp");
    fs::write(
        &guest_syntax,
        "#include \"guest/init.hpp\"\nint main() { return 0; }\n",
    )
    .expect("write external guest bindings syntax check");
    let root = workspace_root();
    let guest_compile = Command::new("g++")
        .args(["-std=c++20", "-fsyntax-only"])
        .arg(&guest_syntax)
        .arg("-I")
        .arg(&bindings_root)
        .arg("-I")
        .arg(&domain_root)
        .arg("-I")
        .arg(&contracts_root)
        .arg("-I")
        .arg(root.join("sdks/cpp/host"))
        .arg("-I")
        .arg(root.join("sdks/cpp/guest"))
        .arg("-I")
        .arg(root.join("sdks/cpp/abi"))
        .output()
        .expect("syntax-check partitioned external C++ guest bindings");
    assert!(
        guest_compile.status.success(),
        "partitioned external C++ guest bindings did not compile:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&guest_compile.stdout),
        String::from_utf8_lossy(&guest_compile.stderr),
    );
    let driver = bindings_root.join("partitioned.cpp");
    fs::write(
        &driver,
        r#"
#include <cstdint>

#include "host/host_callers.hpp"
#include "guest/guest_contracts.hpp"

namespace domain = polyplug_generated;

class Provider final : public polyplug_plugin::PlatformPluginGuestContract {
public:
    domain::Envelope metadata() override {
        return domain::Envelope{
            domain::Inner{},
            domain::ArrayOf_Inner{},
            domain::Mode::Ready,
        };
    }

    Buffer read(uint64_t, uint32_t) override {
        return Buffer{};
    }

    uint32_t write(uint64_t, Buffer) override {
        return ++writes_;
    }

    StringView inspect(StringView, const domain::Mode& mode,
                       const domain::ArrayOf_Inner& entries) override {
        if (mode != domain::Mode::Ready || entries.len != 0U) {
            return StringView{};
        }
        static constexpr uint8_t ok[] = {'o', 'k'};
        return StringView{ok, 2U};
    }

    uint32_t take_inner(const domain::Inner&) override {
        return writes_;
    }

private:
    uint32_t writes_ = 0U;
};

int main() {
    Provider provider;
    auto envelope = provider.metadata();
    if (envelope.mode != domain::Mode::Ready) return 1;
    if (provider.write(0U, Buffer{}) != 1U) return 2;
    domain::ArrayOf_Inner entries{};
    if (provider.inspect(StringView{}, domain::Mode::Ready, entries).len != 2U) return 3;
    return provider.take_inner(envelope.inner) == 1U ? 0 : 4;
}
"#,
    )
    .expect("write partitioned C++ driver");

    let root = workspace_root();
    let runtime_dir = build_polyplug_runtime(&root);
    let executable = bindings_root.join("partitioned");
    let compile = Command::new("g++")
        .args(["-std=c++20"])
        .arg(&driver)
        .arg("-I")
        .arg(&bindings_root)
        .arg("-I")
        .arg(&domain_root)
        .arg("-I")
        .arg(&contracts_root)
        .arg("-I")
        .arg(root.join("sdks/cpp/host"))
        .arg("-I")
        .arg(root.join("sdks/cpp/abi"))
        .arg("-L")
        .arg(&runtime_dir)
        .arg("-lpolyplug")
        .arg(format!("-Wl,-rpath,{}", runtime_dir.display()))
        .arg("-o")
        .arg(&executable)
        .output()
        .expect("compile partitioned C++ driver");
    assert!(
        compile.status.success(),
        "partitioned C++ driver did not compile:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr),
    );
    let execution = run_cpp_runtime_driver(&executable, &runtime_dir);
    assert!(
        execution.status.success(),
        "partitioned C++ driver failed with {:?}:\nstdout: {}\nstderr: {}",
        execution.status.code(),
        String::from_utf8_lossy(&execution.stdout),
        String::from_utf8_lossy(&execution.stderr),
    );
}

#[test]
fn cpp_layout_rejects_omitted_required_domain_types() {
    let temp = TempDir::new().expect("create omitted-domain C++ fixture");
    let api = temp.path().join("api.toml");
    write_api(&api, "platform.Plugin");
    let error = match generate(GenerateConfig {
        api_toml: api,
        lang: Lang::Cpp,
        side: Side::Host,
        layout: OutputLayout {
            bindings: OutputDestination::Inline,
            domain_types: OutputDestination::Omit,
            guest_contracts: OutputDestination::Omit,
        },
    }) {
        Err(error) => error,
        Ok(_) => panic!("host binding metadata requires the domain partition"),
    };
    assert!(
        matches!(error, PolyplugcError::ValidationFailed { ref message }
            if message.contains("references omitted domain types partition")),
        "unexpected omission error: {error}"
    );
}

#[cfg(not(windows))]
#[test]
fn primitive_cpp_host_omits_domain_types_and_runs() {
    let temp = TempDir::new().expect("create primitive C++ host fixture");
    let api = temp.path().join("primitive.toml");
    write_primitive_api(&api, "math.Counter");
    let layout = OutputLayout {
        bindings: OutputDestination::Inline,
        domain_types: OutputDestination::Omit,
        guest_contracts: OutputDestination::Omit,
    };
    let output = generate(GenerateConfig {
        api_toml: api,
        lang: Lang::Cpp,
        side: Side::Host,
        layout,
    })
    .expect("primitive C++ host may omit domain declarations");
    let metadata = output
        .files
        .iter()
        .find(|file| file.path == Path::new("host/types.hpp"))
        .expect("host binding metadata");
    assert!(
        metadata.references.is_empty() && !metadata.content.contains("domain.hpp"),
        "primitive metadata must not depend on domain declarations: {metadata:?}"
    );
    let generated = temp.path().join("generated");
    write_output(&output, &generated).expect("write primitive C++ host bindings");
    assert!(
        !generated.join("host/domain.hpp").exists(),
        "omitted domain partition must not be written"
    );
    let driver = generated.join("primitive_host.cpp");
    fs::write(
        &driver,
        r#"
#include "host/types.hpp"
#include "host/host_callers.hpp"

int main() {
    return polyplug_generated::MATH_COUNTER_CONTRACT_ID == 0U ? 1 : 0;
}
"#,
    )
    .expect("write primitive C++ host driver");
    let root = workspace_root();
    let executable = generated.join("primitive_host");
    let compile = Command::new("g++")
        .args(["-std=c++20", "-Werror"])
        .arg(&driver)
        .arg("-I")
        .arg(&generated)
        .arg("-I")
        .arg(root.join("sdks/cpp/host"))
        .arg("-I")
        .arg(root.join("sdks/cpp/abi"))
        .arg("-o")
        .arg(&executable)
        .output()
        .expect("compile primitive C++ host driver");
    assert!(
        compile.status.success(),
        "primitive C++ host driver did not compile:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr),
    );
    let execution = Command::new(&executable)
        .output()
        .expect("run primitive C++ host driver");
    assert!(
        execution.status.success(),
        "primitive C++ host driver failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&execution.stdout),
        String::from_utf8_lossy(&execution.stderr),
    );
}

#[cfg(not(windows))]
#[test]
fn primitive_internal_cpp_profile_omits_domain_types_and_runs() {
    let temp = TempDir::new().expect("create primitive internal C++ fixture");
    let name = "primitive_cpp_omit_internal";
    let output = primitive_internal_output(
        &temp,
        name,
        "math.Counter",
        OutputLayout {
            bindings: OutputDestination::Inline,
            domain_types: OutputDestination::Omit,
            guest_contracts: OutputDestination::Inline,
        },
    );
    let metadata = output
        .files
        .iter()
        .find(|file| file.path.ends_with(Path::new("host/types.hpp")))
        .expect("internal host metadata");
    assert!(
        metadata.references.is_empty() && !metadata.content.contains("domain.hpp"),
        "primitive internal metadata must not depend on domain declarations: {metadata:?}"
    );
    let internal_header = output
        .files
        .iter()
        .find(|file| file.path.ends_with(Path::new("guest/internal_plugin.hpp")))
        .expect("internal plugin header");
    let bindings = internal_header
        .content
        .lines()
        .find_map(|line| {
            line.strip_prefix("namespace ")
                .and_then(|value| value.strip_suffix("::internal_plugin {"))
        })
        .expect("bundle-specific bindings namespace");
    let generated = temp.path().join("generated");
    write_output(&output, &generated).expect("write primitive internal C++ bindings");
    assert!(
        !generated.join("domain").exists(),
        "omitted domain partition must not be written"
    );
    let driver = generated.join("primitive_internal.cpp");
    fs::write(
        &driver,
        r#"
#include "INTERNAL_HEADER"

namespace bindings = BINDINGS;

class Provider final : public bindings::plugin::MathCounterGuestContract {
public:
    uint32_t increment(uint32_t value) override {
        return value + 1U;
    }
};

int main() {
    return Provider{}.increment(41U) == 42U ? 0 : 1;
}
"#
        .replace("INTERNAL_HEADER", &internal_header.path.to_string_lossy())
        .replace("BINDINGS", bindings),
    )
    .expect("write primitive internal C++ driver");
    let root = workspace_root();
    let executable = generated.join("primitive_internal");
    let compile = Command::new("g++")
        .args(["-std=c++20", "-Werror"])
        .arg(&driver)
        .arg("-I")
        .arg(&generated)
        .arg("-I")
        .arg(root.join("sdks/cpp/host"))
        .arg("-I")
        .arg(root.join("sdks/cpp/guest"))
        .arg("-I")
        .arg(root.join("sdks/cpp/abi"))
        .arg("-o")
        .arg(&executable)
        .output()
        .expect("compile primitive internal C++ driver");
    assert!(
        compile.status.success(),
        "primitive internal C++ driver did not compile:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr),
    );
    let execution = Command::new(&executable)
        .output()
        .expect("run primitive internal C++ driver");
    assert!(
        execution.status.success(),
        "primitive internal C++ driver failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&execution.stdout),
        String::from_utf8_lossy(&execution.stderr),
    );
}

#[cfg(not(windows))]
#[test]
fn abi_only_cpp_guest_contracts_omit_domain_types_compile() {
    let temp = TempDir::new().expect("create ABI-only C++ guest fixture");
    let api = temp.path().join("abi_only.toml");
    write_abi_only_api(&api, "storage.Blob");
    let output = generate(GenerateConfig {
        api_toml: api,
        lang: Lang::Cpp,
        side: Side::Guest,
        layout: OutputLayout {
            bindings: OutputDestination::Inline,
            domain_types: OutputDestination::Omit,
            guest_contracts: OutputDestination::Inline,
        },
    })
    .expect("generate ABI-only C++ guest bindings");
    let contracts = output
        .files
        .iter()
        .find(|file| file.partition == OutputPartition::GuestContracts)
        .expect("ABI-only guest contracts");
    assert!(
        contracts.references.is_empty(),
        "ABI-only guest contracts must not reference domain types: {contracts:?}"
    );
    assert!(
        contracts.content.contains("#include \"polyplug/abi.hpp\"")
            && !contracts.content.contains("types.hpp")
            && !contracts.content.contains("domain.hpp"),
        "ABI-only guest contracts must include only ABI declarations: {}",
        contracts.content
    );
    let generated = temp.path().join("generated");
    write_output(&output, &generated).expect("write ABI-only C++ guest bindings");
    let driver = generated.join("abi_only_guest.cpp");
    fs::write(
        &driver,
        r#"
#include "guest/guest_contracts.hpp"

class Provider final : public polyplug_plugin::StorageBlobGuestContract {
public:
    Buffer transform(StringView, Buffer bytes) override {
        return bytes;
    }
};

int main() {
    return Provider{}.transform(StringView{}, Buffer{}).len == 0U ? 0 : 1;
}
"#,
    )
    .expect("write ABI-only C++ guest driver");
    let root = workspace_root();
    let compile = Command::new("g++")
        .args(["-std=c++20", "-Werror", "-fsyntax-only"])
        .arg(&driver)
        .arg("-I")
        .arg(&generated)
        .arg("-I")
        .arg(root.join("sdks/cpp/guest"))
        .arg("-I")
        .arg(root.join("sdks/cpp/abi"))
        .output()
        .expect("syntax-check ABI-only C++ guest contracts");
    assert!(
        compile.status.success(),
        "ABI-only C++ guest contracts did not compile:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr),
    );
}

#[cfg(not(windows))]
#[test]
fn declaration_free_internal_cpp_inline_domain_emit_contracts_prefixed_compile() {
    let temp = TempDir::new().expect("create declaration-free internal C++ fixture");
    let name = "abi_only_cpp_internal";
    let namespace = format!("internal/{name}-{:016x}", polyplug_utils::bundle_id(name));
    let api = temp.path().join("abi_only_internal.toml");
    let bundle = temp.path().join("abi_only_internal.bundle.toml");
    write_abi_only_api(&api, "storage.Blob");
    write_bundle(
        &bundle,
        "abi_only_internal.toml",
        name,
        "abi_only_internal.provider",
        "storage.Blob",
    );
    let generated = temp.path().join("generated");
    let contracts_root = generated.join("contracts");
    let output = generate_internal_cpp(InternalCppGenerateConfig {
        bundle_toml: bundle,
        out_dir: temp.path().join("out"),
        layout: OutputLayout {
            bindings: OutputDestination::Inline,
            domain_types: OutputDestination::Inline,
            guest_contracts: OutputDestination::Emit {
                root: contracts_root.clone(),
                import: ValidatedImport::parse(
                    Lang::Cpp,
                    format!("contracts/{namespace}/guest/guest_contracts.hpp"),
                )
                .expect("valid prefixed guest contracts include"),
            },
        },
    })
    .expect("generate declaration-free internal C++ profile");
    let contracts = output
        .files
        .iter()
        .find(|file| file.partition == OutputPartition::GuestContracts)
        .expect("declaration-free internal guest contracts");
    assert!(
        contracts.references.is_empty(),
        "declaration-free contracts must not reference domain types: {contracts:?}"
    );
    assert!(
        contracts.content.contains("#include \"polyplug/abi.hpp\"")
            && !contracts.content.contains("types.hpp")
            && !contracts.content.contains("domain.hpp"),
        "declaration-free contracts must include only ABI declarations: {}",
        contracts.content
    );
    let interfaces = output
        .files
        .iter()
        .find(|file| file.path.ends_with(Path::new("guest/interfaces.hpp")))
        .expect("internal ABI bindings");
    assert!(
        interfaces
            .content
            .contains(&format!("contracts/{namespace}/guest/guest_contracts.hpp")),
        "internal interfaces must retain the prefixed guest-contract import: {}",
        interfaces.content
    );
    let internal_header = output
        .files
        .iter()
        .find(|file| file.path.ends_with(Path::new("guest/internal_plugin.hpp")))
        .expect("internal plugin header");
    let bindings = internal_header
        .content
        .lines()
        .find_map(|line| {
            line.strip_prefix("namespace ")
                .and_then(|value| value.strip_suffix("::internal_plugin {"))
        })
        .expect("bundle-specific bindings namespace");
    write_output(&output, &generated).expect("write declaration-free internal C++ bindings");
    let driver = generated.join("declaration_free_internal.cpp");
    fs::write(
        &driver,
        r#"
#include "INTERNAL_HEADER"

namespace bindings = BINDINGS;

class Provider final : public bindings::plugin::StorageBlobGuestContract {
public:
    Buffer transform(StringView, Buffer bytes) override {
        return bytes;
    }
};

int main() {
    return Provider{}.transform(StringView{}, Buffer{}).len == 0U ? 0 : 1;
}
"#
        .replace("INTERNAL_HEADER", &internal_header.path.to_string_lossy())
        .replace("BINDINGS", bindings),
    )
    .expect("write declaration-free internal C++ driver");
    let root = workspace_root();
    let compile = Command::new("g++")
        .args(["-std=c++20", "-Werror", "-fsyntax-only"])
        .arg(&driver)
        .arg("-I")
        .arg(&generated)
        .arg("-I")
        .arg(&contracts_root)
        .arg("-I")
        .arg(root.join("sdks/cpp/host"))
        .arg("-I")
        .arg(root.join("sdks/cpp/guest"))
        .arg("-I")
        .arg(root.join("sdks/cpp/abi"))
        .output()
        .expect("syntax-check declaration-free internal C++ bindings");
    assert!(
        compile.status.success(),
        "declaration-free internal C++ bindings did not compile:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr),
    );
}

#[cfg(not(windows))]
#[test]
fn partitioned_internal_cpp_profile_registers_dispatches_and_unloads() {
    let temp = TempDir::new().expect("create partitioned internal C++ fixture");
    let name = "cpp_partitioned_internal";
    let namespace = format!("internal/{name}-{:016x}", polyplug_utils::bundle_id(name));
    let generated = temp.path().join("generated");
    let layout = OutputLayout {
        bindings: OutputDestination::Inline,
        domain_types: OutputDestination::Emit {
            root: generated.join("domain"),
            import: ValidatedImport::parse(
                Lang::Cpp,
                format!("domain/{namespace}/host/domain.hpp"),
            )
            .expect("valid internal domain include"),
        },
        guest_contracts: OutputDestination::Emit {
            root: generated.join("contracts"),
            import: ValidatedImport::parse(
                Lang::Cpp,
                format!("contracts/{namespace}/guest/guest_contracts.hpp"),
            )
            .expect("valid internal guest contracts include"),
        },
    };
    let output = internal_output_with_layout(&temp, name, "platform.Plugin", layout);
    let internal_header_path = Path::new("guest/internal_plugin.hpp");
    let internal_header = output
        .files
        .iter()
        .find(|file| file.path.ends_with(internal_header_path))
        .expect("internal plugin header");
    let domain = output
        .files
        .iter()
        .find(|file| file.partition == OutputPartition::DomainTypes)
        .expect("internal domain header");
    assert!(!domain.content.contains("CONTRACT_ID"));
    let metadata = output
        .files
        .iter()
        .find(|file| file.path.ends_with(Path::new("host/types.hpp")))
        .expect("internal binding metadata");
    assert!(metadata.content.contains("PLATFORM_PLUGIN_CONTRACT_ID"));
    assert_eq!(metadata.references, vec![OutputPartition::DomainTypes]);
    let contracts = output
        .files
        .iter()
        .find(|file| file.partition == OutputPartition::GuestContracts)
        .expect("internal guest contracts");
    assert_eq!(contracts.references, vec![OutputPartition::DomainTypes]);
    let interfaces = output
        .files
        .iter()
        .find(|file| file.path.ends_with(Path::new("guest/interfaces.hpp")))
        .expect("internal ABI bindings");
    assert!(
        interfaces
            .content
            .contains("contracts/internal/cpp_partitioned_internal-")
    );
    write_output(&output, &generated).expect("write partitioned internal C++ profile");

    let bindings = internal_header
        .content
        .lines()
        .find_map(|line| {
            line.strip_prefix("namespace ")
                .and_then(|value| value.strip_suffix("::internal_plugin {"))
        })
        .expect("bundle-specific bindings namespace");
    let driver = generated.join("partitioned_internal.cpp");
    fs::write(
        &driver,
        r#"
#include <cstdint>
#include <memory>

#include "INTERNAL_HEADER"

namespace bindings = BINDINGS;

class Provider final : public bindings::plugin::PlatformPluginGuestContract {
public:
    bindings::Envelope metadata() override {
        return bindings::Envelope{
            bindings::Inner{},
            bindings::ArrayOf_Inner{},
            bindings::Mode::Ready,
        };
    }

    Buffer read(uint64_t, uint32_t) override {
        return Buffer{};
    }

    uint32_t write(uint64_t, Buffer) override {
        return ++writes_;
    }

    StringView inspect(StringView, const bindings::Mode& mode,
                       const bindings::ArrayOf_Inner& entries) override {
        if (mode != bindings::Mode::Ready || entries.len != 0U) {
            return StringView{};
        }
        static constexpr uint8_t ok[] = {'o', 'k'};
        return StringView{ok, 2U};
    }

    uint32_t take_inner(const bindings::Inner&) override {
        return writes_;
    }

private:
    uint32_t writes_ = 0U;
};

int main() {
    auto runtime = polyplug::Runtime::builder().build();
    uint64_t bundle_id = 0U;
    {
        auto registration = bindings::internal_plugin::register_internal_plugin(
            runtime,
            [](const HostApi*) { return std::make_unique<Provider>(); });
        bundle_id = registration.internal_plugin_id;
        if (registration.cpp_partitioned_internal_provider_platform_plugin.write(0U, Buffer{}) != 1U) {
            return 1;
        }
        bindings::ArrayOf_Inner entries{};
        if (registration.cpp_partitioned_internal_provider_platform_plugin.inspect(
                StringView{}, bindings::Mode::Ready, entries).len != 2U) {
            return 2;
        }
        auto envelope = registration.cpp_partitioned_internal_provider_platform_plugin.metadata();
        if (registration.cpp_partitioned_internal_provider_platform_plugin.take_inner(envelope.inner) != 1U) {
            return 3;
        }
    }
    runtime.unload_bundle(bundle_id);
    return 0;
}
"#
        .replace("INTERNAL_HEADER", &internal_header.path.to_string_lossy())
        .replace("BINDINGS", bindings),
    )
    .expect("write partitioned internal C++ driver");
    let root = workspace_root();
    let runtime_dir = build_polyplug_runtime(&root);
    let executable = runtime_driver_path(&generated, "partitioned_internal");
    let compile = compile_cpp_runtime_driver(
        &driver,
        &generated,
        &root,
        &runtime_dir,
        "partitioned_internal",
    );
    assert!(
        compile.status.success(),
        "partitioned internal C++ driver did not compile:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr),
    );
    let execution = run_cpp_runtime_driver(&executable, &runtime_dir);
    assert!(
        execution.status.success(),
        "partitioned internal C++ driver failed with {:?}:\nstdout: {}\nstderr: {}",
        execution.status.code(),
        String::from_utf8_lossy(&execution.stdout),
        String::from_utf8_lossy(&execution.stderr),
    );
}

#[test]
fn internal_cpp_profile_is_opt_in_artifactless_and_typed() {
    let temp = TempDir::new().expect("create temporary profile fixture");
    let output = internal_output(&temp, "cpp_internal_profile", "platform.Plugin");
    let paths = output
        .files
        .iter()
        .map(|file| file.path.clone())
        .collect::<HashSet<PathBuf>>();
    let namespace = Path::new("internal").join(format!(
        "cpp_internal_profile-{:016x}",
        polyplug_utils::bundle_id("cpp_internal_profile")
    ));
    let internal_header_path = Path::new("guest").join("internal_plugin.hpp");
    let callers_path = Path::new("host").join("host_callers.hpp");
    let interfaces_path = Path::new("guest").join("interfaces.hpp");
    assert!(paths.iter().all(|path| path.starts_with(&namespace)));
    assert!(
        paths
            .iter()
            .any(|path| path.ends_with(&internal_header_path))
    );
    assert!(paths.iter().any(|path| path.ends_with(&callers_path)));
    assert!(
        !paths
            .iter()
            .any(|path| path.ends_with(Path::new("manifest.toml")))
    );

    let internal = output
        .files
        .iter()
        .find(|file| file.path.ends_with(&internal_header_path))
        .expect("internal registrar header")
        .content
        .as_str();
    for required in [
        "struct InternalPluginRegistration",
        "InternalPluginRegistration register_internal_plugin",
        "std::move",
        "create_instance",
        "destroy_instance",
        "thread_local std::exception_ptr construction_failure_",
        "internal_plugin.rethrow_factory_failure",
        "runtime.unload_bundle(committed.bundle_id)",
        "std::rethrow_exception(primary)",
    ] {
        assert!(
            internal.contains(required),
            "missing `{required}` in generated internal profile"
        );
    }
    let callers = output
        .files
        .iter()
        .find(|file| file.path.ends_with(&callers_path))
        .expect("internal host caller header")
        .content
        .as_str();
    assert!(callers.contains("if (iface == interface_) {"));
    assert!(callers.contains("cached_revision_ = polyplug_load_revision(revision_host_);"));
    let interfaces = output
        .files
        .iter()
        .find(|file| file.path.ends_with(&interfaces_path))
        .expect("internal ABI dispatch header")
        .content
        .as_str();
    for required in ["metadata", "read", "write", "inspect", "take_inner"] {
        assert!(
            interfaces.contains(required),
            "missing typed ABI dispatch for `{required}`"
        );
    }

    let external_bundle = temp.path().join("external.bundle.toml");
    fs::write(
        &external_bundle,
        "[bundle]\nname = \"cpp_external_profile\"\nversion = \"1.0\"\napi = \"cpp_internal_profile.toml\"\nloader = \"python\"\nfile = \"external.py\"\n\n[[plugin]]\nname = \"external.provider\"\nimplements = [\"platform.Plugin@1.0\"]\n",
    )
    .expect("write external bundle fixture");
    let external = generate(GenerateConfig {
        api_toml: external_bundle,
        lang: Lang::Cpp,
        side: Side::Guest,
        layout: OutputLayout::unified(),
    })
    .expect("generate external C++ bindings");
    assert!(
        external
            .files
            .iter()
            .all(|file| !file.path.starts_with(Path::new("guest").join("internal"))),
        "external generation must not emit internal-plugin artifacts"
    );
}

#[test]
fn two_internal_cpp_bundles_compile_without_symbol_collisions() {
    let temp = TempDir::new().expect("create temporary coexistence fixture");
    let first = internal_output(&temp, "cpp_first", "first.Plugin");
    let second = internal_output(&temp, "cpp_second", "second.Plugin");
    let paths = first
        .files
        .iter()
        .chain(&second.files)
        .map(|file| file.path.clone())
        .collect::<HashSet<_>>();
    assert_eq!(paths.len(), first.files.len() + second.files.len());

    let generated = temp.path().join("generated");
    write_output(&first, &generated).expect("write first internal C++ profile");
    write_output(&second, &generated).expect("write second internal C++ profile");
    let internal_header_path = Path::new("guest").join("internal_plugin.hpp");
    let first_header = first
        .files
        .iter()
        .find(|file| file.path.ends_with(&internal_header_path))
        .expect("first internal header")
        .path
        .clone();
    let second_header = second
        .files
        .iter()
        .find(|file| file.path.ends_with(&internal_header_path))
        .expect("second internal header")
        .path
        .clone();
    let consumer = generated.join("coexist.cpp");
    fs::write(
        &consumer,
        format!(
            "#include \"{}\"\n#include \"{}\"\nint main() {{ return 0; }}\n",
            first_header.display(),
            second_header.display()
        ),
    )
    .expect("write C++ coexistence consumer");
    let root = workspace_root();
    let status = Command::new("g++")
        .args(["-std=c++20", "-fsyntax-only"])
        .arg(&consumer)
        .arg("-I")
        .arg(&generated)
        .arg("-I")
        .arg(root.join("sdks/cpp/host"))
        .arg("-I")
        .arg(root.join("sdks/cpp/abi"))
        .status()
        .expect("invoke g++ for generated coexistence consumer");
    assert!(
        status.success(),
        "two generated internal C++ bundles must compile together"
    );
}

#[test]
fn internal_cpp_profile_registers_dispatches_and_retries_with_real_runtime() {
    let temp = TempDir::new().expect("create temporary C++ E2E fixture");
    let output = internal_output(&temp, "cpp_runtime_profile", "platform.Plugin");
    let generated = temp.path().join("generated");
    write_output(&output, &generated).expect("write internal C++ profile");
    let internal_header_path = Path::new("guest").join("internal_plugin.hpp");
    let internal_header = output
        .files
        .iter()
        .find(|file| file.path.ends_with(&internal_header_path))
        .expect("internal registration header");
    let header_path = internal_header.path.display().to_string();
    let namespace = internal_header
        .content
        .lines()
        .find_map(|line| {
            line.strip_prefix("namespace ")
                .and_then(|value| value.strip_suffix("::internal_plugin {"))
        })
        .expect("bundle-scoped C++ namespace");
    let driver = generated.join("runtime_profile.cpp");
    let source = r#"
#include <cstdint>
#include <memory>
#include <stdexcept>
#include <utility>

#include "INTERNAL_HEADER"

namespace bindings = BINDINGS;

namespace {
int factory_drops = 0;

struct DropProbe {
    ~DropProbe() { ++factory_drops; }
};

class Provider final : public bindings::plugin::PlatformPluginGuestContract {
public:
    bindings::Envelope metadata() override {
        return bindings::Envelope{};
    }

    Buffer read(uint64_t, uint32_t) override {
        static uint8_t bytes[] = {4U, 5U, 6U};
        return Buffer{bytes, 3U, 3U};
    }

    uint32_t write(uint64_t address, Buffer) override {
        if (address == 0U) {
            throw std::runtime_error("write rejected");
        }
        return ++writes_;
    }

    StringView inspect(
        StringView,
        const bindings::Mode& mode,
        const bindings::ArrayOf_Inner&) override {
        if (mode != bindings::Mode::Ready) {
            throw std::runtime_error("unexpected mode");
        }
        static constexpr uint8_t text[] = {'o', 'k'};
        return StringView{text, 2U};
    }

    uint32_t take_inner(const bindings::Inner&) override {
        return 41U;
    }

private:
    uint32_t writes_ = 0U;
};
}  // namespace

int main() {
    auto runtime = polyplug::Runtime::builder().build();
    uint8_t payload[] = {9U};
    Buffer buffer{payload, 1U, 1U};
    static constexpr uint8_t label[] = {'x'};
    bindings::Inner inner{StringView{label, 1U}, buffer};
    bindings::ArrayOf_Inner entries{};

    {
        auto registration = bindings::internal_plugin::register_internal_plugin(
            runtime,
            [](const HostApi*) { return std::make_unique<Provider>(); });
        if (registration.internal_plugin_id != bindings::internal_plugin::INTERNAL_PLUGIN_ID) return 1;
        if (registration.cpp_runtime_profile_provider_platform_plugin.write(9U, buffer) != 1U) return 2;
        if (registration.cpp_runtime_profile_provider_platform_plugin.read(0U, 3U).len != 3U) return 3;
        if (registration.cpp_runtime_profile_provider_platform_plugin.inspect(
                StringView{label, 1U}, bindings::Mode::Ready, entries).len != 2U) return 4;
        if (registration.cpp_runtime_profile_provider_platform_plugin.take_inner(inner) != 41U) return 5;
        auto second = bindings::PlatformPluginContract::create(
            runtime.find_guest_contract(
                bindings::PLATFORM_PLUGIN_CONTRACT_ID, 1U),
            runtime.host());
        if (!second || second->write(9U, buffer) != 1U) return 6;
        try {
            registration.cpp_runtime_profile_provider_platform_plugin.write(0U, buffer);
            return 7;
        } catch (const std::runtime_error&) {
        }
        try {
            bindings::internal_plugin::register_internal_plugin(
                runtime,
                [probe = std::make_unique<DropProbe>()](const HostApi*) {
                    return std::make_unique<Provider>();
                });
            return 8;
        } catch (const std::runtime_error&) {
        }
        if (factory_drops != 1) return 9;
    }

    runtime.unload_bundle(bindings::internal_plugin::INTERNAL_PLUGIN_ID);
    auto fresh = bindings::internal_plugin::register_internal_plugin(
        runtime,
        [](const HostApi*) { return std::make_unique<Provider>(); });
    if (fresh.cpp_runtime_profile_provider_platform_plugin.write(9U, buffer) != 1U) return 10;
    return 0;
}
"#;
    fs::write(
        &driver,
        source
            .replace("INTERNAL_HEADER", &header_path)
            .replace("BINDINGS", namespace),
    )
    .expect("write generated C++ runtime driver");

    let root = workspace_root();
    let runtime_dir = build_polyplug_runtime(&root);
    let executable = runtime_driver_path(&generated, "runtime_profile");
    let compile =
        compile_cpp_runtime_driver(&driver, &generated, &root, &runtime_dir, "runtime_profile");
    assert!(
        compile.status.success(),
        "generated C++ runtime driver did not compile:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr),
    );
    let execution = run_cpp_runtime_driver(&executable, &runtime_dir);
    assert!(
        execution.status.success(),
        "generated C++ runtime driver failed with {:?}:\nstdout: {}\nstderr: {}",
        execution.status.code(),
        String::from_utf8_lossy(&execution.stdout),
        String::from_utf8_lossy(&execution.stderr),
    );
}

#[test]
fn internal_cpp_profile_binds_exact_handles_and_rolls_back_failed_factories() {
    let temp = TempDir::new().expect("create exact-handle fixture");
    let api = temp.path().join("shared.toml");
    fs::write(
        &api,
        "[[guest_contract]]\nname = \"shared.Guest\"\nversion = \"1.0\"\n\n[[guest_contract.functions]]\nname = \"value\"\nreturn = \"u32\"\n",
    )
    .expect("write shared API");
    let older_bundle = temp.path().join("older.toml");
    fs::write(
        &older_bundle,
        "[bundle]\nname = \"older_shared\"\nversion = \"1.0\"\napi = \"shared.toml\"\n\n[[plugin]]\nname = \"older.provider\"\nimplements = [\"shared.Guest@1.0\"]\n",
    )
    .expect("write older bundle");
    let current_bundle = temp.path().join("current.toml");
    fs::write(
        &current_bundle,
        "[bundle]\nname = \"current_shared\"\nversion = \"1.0\"\napi = \"shared.toml\"\n\n[[plugin]]\nname = \"first.provider\"\nimplements = [\"shared.Guest@1.0\"]\n\n[[plugin]]\nname = \"second.provider\"\nimplements = [\"shared.Guest@1.0\"]\n",
    )
    .expect("write current bundle");
    let older = generate_internal_cpp(InternalCppGenerateConfig {
        bundle_toml: older_bundle,
        out_dir: temp.path().join("generated"),
        layout: Default::default(),
    })
    .expect("generate older profile");
    let current = generate_internal_cpp(InternalCppGenerateConfig {
        bundle_toml: current_bundle,
        out_dir: temp.path().join("generated"),
        layout: Default::default(),
    })
    .expect("generate current profile");
    let generated = temp.path().join("generated");
    write_output(&older, &generated).expect("write older profile");
    write_output(&current, &generated).expect("write current profile");
    let internal_header_path = Path::new("guest").join("internal_plugin.hpp");
    let older_header = older
        .files
        .iter()
        .find(|file| file.path.ends_with(&internal_header_path))
        .expect("older header");
    let current_header = current
        .files
        .iter()
        .find(|file| file.path.ends_with(&internal_header_path))
        .expect("current header");
    let older_namespace = older_header
        .content
        .lines()
        .find_map(|line| {
            line.strip_prefix("namespace ")
                .and_then(|value| value.strip_suffix("::internal_plugin {"))
        })
        .expect("older namespace");
    let current_namespace = current_header
        .content
        .lines()
        .find_map(|line| {
            line.strip_prefix("namespace ")
                .and_then(|value| value.strip_suffix("::internal_plugin {"))
        })
        .expect("current namespace");
    let driver = generated.join("exact_handles.cpp");
    let source = r#"
#include <cstdint>
#include <memory>
#include <stdexcept>
#include <string_view>

#include "OLDER_HEADER"
#include "CURRENT_HEADER"

namespace older = OLDER_BINDINGS;
namespace current = CURRENT_BINDINGS;

namespace {
int first_factory_calls = 0;
int throwing_factory_calls = 0;
int null_factory_calls = 0;
int retry_first_factory_calls = 0;
int retry_second_factory_calls = 0;
int first_destroyed = 0;
int older_factory_calls = 0;
}

class Older final : public older::plugin::SharedGuestGuestContract {
public:
    uint32_t calls_ = 0;
    uint32_t value() override { return ++calls_; }
};

class First final : public current::plugin::SharedGuestGuestContract {
public:
    ~First() override { ++first_destroyed; }
    uint32_t value() override { return 11U; }
};

class Second final : public current::plugin::SharedGuestGuestContract {
public:
    uint32_t value() override { return 21U; }
};


int main() {
    auto runtime = polyplug::Runtime::builder().build();
    uint64_t older_id = 0U;
    uint64_t current_id = 0U;
    {
        auto old_registration = older::internal_plugin::register_internal_plugin(
            runtime,
            [](const HostApi*) {
                ++older_factory_calls;
                return std::make_unique<Older>();
            });
        older_id = old_registration.internal_plugin_id;
        if (old_registration.older_provider_shared_guest.value() != 1U || older_factory_calls != 1) return 10;

        try {
            current::internal_plugin::register_internal_plugin(
                runtime,
                [](const HostApi*) {
                    ++first_factory_calls;
                    return std::make_unique<First>();
                },
                [](const HostApi*) -> std::unique_ptr<Second> {
                    ++throwing_factory_calls;
                    throw std::runtime_error("second factory exploded");
                });
            return 1;
        } catch (const std::runtime_error& error) {
            if (std::string_view(error.what()) != "second factory exploded") return 2;
        }
        if (first_factory_calls != 1 || throwing_factory_calls != 1 || first_destroyed != 1) return 3;

        try {
            current::internal_plugin::register_internal_plugin(
                runtime,
                [](const HostApi*) {
                    ++first_factory_calls;
                    return std::make_unique<First>();
                },
                [](const HostApi*) -> std::unique_ptr<Second> {
                    ++null_factory_calls;
                    return nullptr;
                });
            return 4;
        } catch (const std::runtime_error& error) {
            if (std::string_view(error.what()) != "generated internal provider factory returned null") return 5;
        }
        if (first_factory_calls != 2 || null_factory_calls != 1 || first_destroyed != 2) return 6;

        auto registration = current::internal_plugin::register_internal_plugin(
            runtime,
            [](const HostApi*) {
                ++retry_first_factory_calls;
                return std::make_unique<First>();
            },
            [](const HostApi*) {
                ++retry_second_factory_calls;
                return std::make_unique<Second>();
            });
        current_id = registration.internal_plugin_id;
        if (retry_first_factory_calls != 1 || retry_second_factory_calls != 1) return 7;
        if (registration.first_provider_shared_guest.value() != 11U) return 8;
        if (registration.second_provider_shared_guest.value() != 21U) return 9;
        if (old_registration.older_provider_shared_guest.value() != 2U || older_factory_calls != 1) return 11;
    }
    runtime.unload_bundle(current_id);
    runtime.unload_bundle(older_id);
    return 0;
}
"#;
    fs::write(
        &driver,
        source
            .replace("OLDER_HEADER", &older_header.path.to_string_lossy())
            .replace("CURRENT_HEADER", &current_header.path.to_string_lossy())
            .replace("OLDER_BINDINGS", older_namespace)
            .replace("CURRENT_BINDINGS", current_namespace),
    )
    .expect("write exact-handle driver");
    let root = workspace_root();
    let runtime_dir = build_polyplug_runtime(&root);
    let executable = runtime_driver_path(&generated, "exact_handles");
    let compile =
        compile_cpp_runtime_driver(&driver, &generated, &root, &runtime_dir, "exact_handles");
    assert!(
        compile.status.success(),
        "exact-handle driver did not compile:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr),
    );
    let execution = run_cpp_runtime_driver(&executable, &runtime_dir);
    assert!(
        execution.status.success(),
        "exact-handle driver failed with {:?}:\nstdout: {}\nstderr: {}",
        execution.status.code(),
        String::from_utf8_lossy(&execution.stdout),
        String::from_utf8_lossy(&execution.stderr),
    );
}

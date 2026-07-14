#[path = "support/failure.rs"]
mod failure;

use failure::PanicOnFailure;

use std::fs;
use std::path::Path;
use std::process::Command;

use polyplug_codegen::{
    GenerateConfig, GenerateOutput, Lang, OutputDestination, OutputLayout, Side, ValidatedImport,
    generate,
};
use tempfile::tempdir;

fn generated<'a>(output: &'a GenerateOutput, path: &str) -> &'a str {
    output
        .files
        .iter()
        .find(|file| file.path == Path::new(path))
        .unwrap_or_else(|| panic!("missing generated {path}"))
        .content
        .as_str()
}

#[test]
fn csharp_attributes_cover_public_semantic_surfaces_once() {
    let temp = tempdir().or_panic("temporary api directory");
    let api = temp.path().join("api.toml");
    fs::write(
        &api,
        r#"
[langs.csharp]
attributes = ["System.CLSCompliant(true)"]

[[types]]
name = "Packet"
langs = { csharp = { attributes = ["System.Serializable"] } }
[[types.fields]]
name = "code"
type = "u32"
langs = { csharp = { attributes = ["System.NonSerialized"] } }

[[enum]]
name = "Mode"
repr = "u32"
langs = { csharp = { attributes = ["System.Flags"] } }
[[enum.variants]]
name = "Fast"
value = "1"
langs = { csharp = { attributes = ["System.Obsolete(\"variant\")"] } }

[[guest_contract]]
name = "sample.Plugin"
version = "1.0.0"
langs = { csharp = { attributes = ["System.Obsolete(\"guest\")"] } }
[[guest_contract.functions]]
name = "invoke"
langs = { csharp = { attributes = ["System.Obsolete(\"guest function\")"] } }
[guest_contract.functions.return]
type = "u32"
langs = { csharp = { attributes = ["System.Runtime.InteropServices.MarshalAs(System.Runtime.InteropServices.UnmanagedType.U4)"] } }
[[guest_contract.functions.params]]
name = "value"
type = "u32"
langs = { csharp = { attributes = ["System.ComponentModel.DefaultValue(1)", "System.Runtime.InteropServices.In"] } }

[[host_contract]]
name = "host.Logger"
version = "1.0.0"
langs = { csharp = { attributes = ["System.Obsolete(\"host\")"] } }
[[host_contract.functions]]
name = "log"
langs = { csharp = { attributes = ["System.Obsolete(\"host function\")"] } }
[host_contract.functions.return]
type = "u32"
langs = { csharp = { attributes = ["System.Runtime.InteropServices.MarshalAs(System.Runtime.InteropServices.UnmanagedType.U4)"] } }
[[host_contract.functions.params]]
name = "level"
type = "u32"
langs = { csharp = { attributes = ["System.ComponentModel.DefaultValue(1)", "System.Runtime.InteropServices.In"] } }
"#,
    )
    .or_panic("write api");

    let host = generate(GenerateConfig {
        api_toml: api.clone(),
        lang: Lang::CSharp,
        side: Side::Host,
        layout: OutputLayout::unified(),
    })
    .or_panic("generate C# host");
    let guest = generate(GenerateConfig {
        api_toml: api,
        lang: Lang::CSharp,
        side: Side::Guest,
        layout: OutputLayout::unified(),
    })
    .or_panic("generate C# guest");

    let guest_types = generated(&guest, "guest/Types.cs");
    assert_eq!(
        guest_types
            .matches("[assembly: System.CLSCompliant(true)]")
            .count(),
        1
    );
    assert!(guest_types.contains("[System.Serializable]"));
    assert!(guest_types.contains("[System.NonSerialized]"));
    assert!(guest_types.contains("[System.Flags]"));
    assert!(guest_types.contains("[System.Obsolete(\"variant\")]\n    Fast"));
    assert!(!generated(&guest, "guest/Contracts.cs").contains("[assembly:"));
    assert!(!generated(&guest, "guest/Interfaces.cs").contains("[assembly:"));

    let guest_contracts = generated(&guest, "guest/Contracts.cs");
    assert!(
        guest_contracts.contains("[System.Obsolete(\"guest\")]"),
        "{guest_contracts}"
    );
    assert!(guest_contracts.contains("[System.Obsolete(\"guest function\")]"));
    assert!(guest_contracts.contains("[return: System.Runtime.InteropServices.MarshalAs(System.Runtime.InteropServices.UnmanagedType.U4)]"));
    assert!(
        guest_contracts.contains("[System.ComponentModel.DefaultValue(1)]\n        [System.Runtime.InteropServices.In]\n        uint value"),
        "{guest_contracts}"
    );

    let host_callers = generated(&host, "host/Callers.cs");
    assert!(host_callers.contains(
        "[System.Obsolete(\"guest\")]\npublic sealed unsafe class SamplePluginContractCaller"
    ));
    assert!(host_callers.contains("[System.Obsolete(\"guest function\")]"));
    assert!(host_callers.contains("[return: System.Runtime.InteropServices.MarshalAs(System.Runtime.InteropServices.UnmanagedType.U4)]"));
    assert!(host_callers.contains("[System.ComponentModel.DefaultValue(1)]\n        [System.Runtime.InteropServices.In]\n        uint value"));

    let host_contracts = generated(&host, "host/Contracts.cs");
    assert!(host_contracts.contains("[System.Obsolete(\"host\")]\npublic interface IHostLogger"));
    assert!(host_contracts.contains("[System.Obsolete(\"host function\")]"));
    assert!(host_contracts.contains("[return: System.Runtime.InteropServices.MarshalAs(System.Runtime.InteropServices.UnmanagedType.U4)]"));
    assert!(host_contracts.contains("[System.ComponentModel.DefaultValue(1)]\n        [System.Runtime.InteropServices.In]\n        uint level"));

    let guest_host_callers = generated(&guest, "guest/HostContracts.cs");
    assert!(guest_host_callers.contains("[System.ComponentModel.DefaultValue(1)]\n        [System.Runtime.InteropServices.In]\n        uint level"));

    let compile_root = temp.path().join("compile");
    fs::create_dir_all(&compile_root).or_panic("create generated C# project");
    fs::write(
        compile_root.join("Types.cs"),
        generated(&host, "host/Types.cs"),
    )
    .or_panic("write generated C# types");
    fs::write(
        compile_root.join("Contracts.cs"),
        generated(&host, "host/Contracts.cs"),
    )
    .or_panic("write generated C# contracts");
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .or_panic("workspace root");
    let host_sdk = workspace.join("sdks/csharp/host/Polyplug.Host.csproj");
    fs::write(
        compile_root.join("Generated.csproj"),
        format!(
            r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><TargetFramework>net10.0</TargetFramework><AllowUnsafeBlocks>true</AllowUnsafeBlocks></PropertyGroup><ItemGroup><ProjectReference Include="{}" /></ItemGroup></Project>"#,
            host_sdk.display()
        ),
    )
    .or_panic("write generated C# project");
    let build = Command::new("dotnet")
        .arg("build")
        .arg("--nologo")
        .arg("--verbosity")
        .arg("quiet")
        .current_dir(&compile_root)
        .output()
        .or_panic("run dotnet build");
    assert!(
        build.status.success(),
        "generated C# build failed:\n{}\n{}",
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr)
    );
    assert!(
        guest_host_callers
            .contains("[System.Obsolete(\"host\")]\npublic sealed class HostLoggerContract")
    );
    assert!(guest_host_callers.contains("[System.Obsolete(\"host function\")]"));

    let split_guest = generate(GenerateConfig {
        api_toml: temp.path().join("api.toml"),
        lang: Lang::CSharp,
        side: Side::Guest,
        layout: OutputLayout {
            bindings: OutputDestination::Inline,
            domain_types: OutputDestination::Emit {
                root: temp.path().join("domain"),
                import: ValidatedImport::parse(Lang::CSharp, "Sample.Domain")
                    .or_panic("valid C# domain import"),
            },
            guest_contracts: OutputDestination::Emit {
                root: temp.path().join("contracts"),
                import: ValidatedImport::parse(Lang::CSharp, "Sample.Contracts")
                    .or_panic("valid C# contract import"),
            },
        },
    })
    .or_panic("generate split C# guest");
    assert_eq!(
        generated(&split_guest, "guest/Types.cs")
            .matches("[assembly: System.CLSCompliant(true)]")
            .count(),
        1
    );
    assert!(!generated(&split_guest, "guest/DomainTypes.cs").contains("[assembly:"));
    assert!(!generated(&split_guest, "guest/GuestContracts.cs").contains("[assembly:"));
}

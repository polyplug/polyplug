#![allow(clippy::expect_used)]

use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Output;

use polyplug_codegen::{
    GenerateConfig, InternalCSharpGenerateConfig, Lang, OutputDestination, OutputLayout,
    OutputPartition, PolyplugcError, Side, generate, generate_internal_csharp, write_output,
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

fn canonicalize_for_toolchain(path: &Path) -> PathBuf {
    let canonical = path
        .canonicalize()
        .expect("canonicalize generated C# project");
    if cfg!(windows) {
        let path = canonical.to_string_lossy().into_owned();
        if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
            PathBuf::from(format!(r"\\{rest}"))
        } else if let Some(rest) = path.strip_prefix(r"\\?\") {
            PathBuf::from(rest)
        } else {
            canonical
        }
    } else {
        canonical
    }
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
    .expect("write primitive C# API fixture");
}

fn write_rich_api(path: &Path, contract: &str) {
    fs::write(
        path,
        format!(
            r#"
[[types]]
name = "Counter"
fields = [{{ name = "value", type = "u32" }}]

[[guest_contract]]
name = "{contract}"
version = "1.0"

[[guest_contract.functions]]
name = "increment"
params = [{{ name = "value", type = "Counter" }}]
return = "Counter"
"#
        ),
    )
    .expect("write rich C# API fixture");
}

fn write_bundle(path: &Path, api: &str, name: &str, contract: &str) {
    fs::write(
        path,
        format!(
            "[bundle]\nname = \"{name}\"\nversion = \"1.0\"\napi = \"{api}\"\n\n[[plugin]]\nname = \"provider\"\nimplements = [\"{contract}@1.0\"]\n"
        ),
    )
    .expect("write primitive C# bundle fixture");
}

fn write_project(project: &Path, references: &[PathBuf]) {
    let references = references
        .iter()
        .map(|reference| {
            format!(
                "        <ProjectReference Include=\"{}\" />",
                reference.display()
            )
        })
        .collect::<Vec<String>>()
        .join("\n");
    fs::write(
        project.join("Primitive.csproj"),
        format!(
            "<Project Sdk=\"Microsoft.NET.Sdk\">\n    <PropertyGroup>\n        <TargetFramework>net10.0</TargetFramework>\n        <OutputType>Exe</OutputType>\n        <Nullable>enable</Nullable>\n        <ImplicitUsings>enable</ImplicitUsings>\n        <AllowUnsafeBlocks>true</AllowUnsafeBlocks>\n    </PropertyGroup>\n    <ItemGroup>\n{references}\n    </ItemGroup>\n</Project>\n"
        ),
    )
    .expect("write C# test project");
    fs::write(
        project.join("Program.cs"),
        "Console.WriteLine(\"primitive-omit-ok\");\n",
    )
    .expect("write C# test driver");
}

fn run_project(project: &Path) -> Output {
    let project = canonicalize_for_toolchain(project);
    Command::new("dotnet")
        .args(["run", "--project"])
        .arg(project.join("Primitive.csproj"))
        .args(["-c", "Release", "--nologo"])
        .output()
        .expect("run generated C# project")
}

fn assert_project_runs(project: &Path, label: &str) {
    let output = run_project(project);
    assert!(
        output.status.success(),
        "{label} did not compile and run:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("primitive-omit-ok"),
        "{label} did not run its generated driver:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn assert_no_domain_types_using(root: &Path) {
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        for entry in fs::read_dir(&path).expect("read generated C# directory") {
            let entry = entry.expect("read generated C# entry");
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "cs") {
                let content = fs::read_to_string(&path).expect("read generated C# file");
                assert!(
                    !content.contains("using Polyplug.Generated.DomainTypes;"),
                    "primitive output must not import omitted domain declarations: {}\n{content}",
                    path.display(),
                );
            }
        }
    }
}

#[test]
fn primitive_csharp_host_omits_domain_types_and_runs() {
    let temp = TempDir::new().expect("create primitive C# host fixture");
    let api = temp.path().join("primitive.toml");
    write_primitive_api(&api, "math.Counter");
    let output = generate(GenerateConfig {
        api_toml: api,
        lang: Lang::CSharp,
        side: Side::Host,
        layout: OutputLayout {
            bindings: OutputDestination::Inline,
            domain_types: OutputDestination::Omit,
            guest_contracts: OutputDestination::Omit,
        },
    })
    .expect("primitive C# host may omit domain declarations");
    assert!(
        output
            .files
            .iter()
            .filter(|file| file.partition == OutputPartition::Bindings)
            .all(|file| !file.references.contains(&OutputPartition::DomainTypes)),
        "primitive C# host bindings must not reference domain declarations"
    );
    let project = temp.path().join("host");
    fs::create_dir_all(&project).expect("create primitive C# host project");
    write_output(&output, &project).expect("write primitive C# host bindings");
    assert_no_domain_types_using(&project);
    let root = workspace_root();
    write_project(
        &project,
        &[root.join("sdks/csharp/host/Polyplug.Host.csproj")],
    );
    assert_project_runs(&project, "primitive C# host");
}

#[test]
fn primitive_internal_csharp_profile_omits_domain_types_and_runs() {
    let temp = TempDir::new().expect("create primitive internal C# fixture");
    let name = "primitive_csharp_omit_internal";
    let api = temp.path().join("primitive.toml");
    let bundle = temp.path().join("primitive.bundle.toml");
    write_primitive_api(&api, "math.Counter");
    write_bundle(&bundle, "primitive.toml", name, "math.Counter");
    let output = generate_internal_csharp(InternalCSharpGenerateConfig {
        bundle_toml: bundle,
        out_dir: temp.path().join("out"),
        layout: OutputLayout {
            bindings: OutputDestination::Inline,
            domain_types: OutputDestination::Omit,
            guest_contracts: OutputDestination::Inline,
        },
    })
    .expect("primitive internal C# profile may omit domain declarations");
    assert!(
        output
            .files
            .iter()
            .filter(|file| file.partition != OutputPartition::DomainTypes)
            .all(|file| !file.references.contains(&OutputPartition::DomainTypes)),
        "primitive internal C# bindings must not reference domain declarations"
    );
    let project = temp.path().join("internal");
    fs::create_dir_all(&project).expect("create primitive internal C# project");
    write_output(&output, &project).expect("write primitive internal C# bindings");
    assert_no_domain_types_using(&project);
    let root = workspace_root();
    write_project(
        &project,
        &[
            root.join("sdks/csharp/guest/Polyplug.Guest.csproj"),
            root.join("sdks/csharp/host/Polyplug.Host.csproj"),
        ],
    );
    assert_project_runs(&project, "primitive internal C# profile");
}

#[test]
fn rich_csharp_profiles_reject_omitted_domain_types() {
    let temp = TempDir::new().expect("create rich C# omission fixture");
    let api = temp.path().join("rich.toml");
    write_rich_api(&api, "math.Counter");
    let layout = OutputLayout {
        bindings: OutputDestination::Inline,
        domain_types: OutputDestination::Omit,
        guest_contracts: OutputDestination::Inline,
    };
    let ordinary = match generate(GenerateConfig {
        api_toml: api.clone(),
        lang: Lang::CSharp,
        side: Side::Host,
        layout: layout.clone(),
    }) {
        Err(error) => error,
        Ok(_) => panic!("rich C# host bindings require canonical domain declarations"),
    };
    assert!(
        matches!(ordinary, PolyplugcError::ValidationFailed { ref message }
            if message.contains("references omitted domain types partition")),
        "unexpected ordinary C# omission error: {ordinary}"
    );
    let bundle = temp.path().join("rich.bundle.toml");
    write_bundle(&bundle, "rich.toml", "rich_csharp_omit", "math.Counter");
    let internal = match generate_internal_csharp(InternalCSharpGenerateConfig {
        bundle_toml: bundle,
        out_dir: temp.path().join("out"),
        layout,
    }) {
        Err(error) => error,
        Ok(_) => panic!("rich internal C# bindings require canonical domain declarations"),
    };
    assert!(
        matches!(internal, PolyplugcError::ValidationFailed { ref message }
            if message.contains("references omitted domain types partition")),
        "unexpected internal C# omission error: {internal}"
    );
}

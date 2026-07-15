#![allow(clippy::expect_used)]

use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Output;

use polyplug_codegen::{
    GenerateConfig, InternalCSharpGenerateConfig, Lang, OutputDestination, OutputLayout,
    OutputPartition, PolyplugcError, Side, ValidatedImport, generate, generate_internal_csharp,
    write_output,
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

fn replace_domain_marker(root: &Path) {
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        for entry in fs::read_dir(path).expect("read generated domain directory") {
            let path = entry.expect("read generated domain entry").path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "cs") {
                let content = fs::read_to_string(&path).expect("read generated domain source");
                if let Some(start) = content.find("Value = ") {
                    let end = content[start..]
                        .find("UL;")
                        .map(|offset| start + offset + 3)
                        .expect("domain fingerprint terminator");
                    let mut mismatched = content;
                    mismatched.replace_range(start..end, "Value = 0x0UL;");
                    fs::write(path, mismatched).expect("write mismatched domain source");
                    return;
                }
            }
        }
    }
    panic!("generated domain fingerprint marker");
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

#[test]
fn internal_csharp_profile_rejects_mismatched_fingerprint_at_module_initialization() {
    let temp = TempDir::new().expect("create C# fingerprint fixture");
    let api = temp.path().join("api.toml");
    let bundle = temp.path().join("bundle.toml");
    write_primitive_api(&api, "game_engine.Plugin");
    write_bundle(
        &bundle,
        "api.toml",
        "csharp_fingerprint",
        "game_engine.Plugin",
    );
    let mut output = generate_internal_csharp(InternalCSharpGenerateConfig {
        bundle_toml: bundle,
        out_dir: temp.path().join("out"),
        layout: OutputLayout::unified(),
    })
    .expect("generate C# profile");
    let host_fingerprint = output
        .files
        .iter_mut()
        .find(|file| file.path.ends_with("host/_polyplug_fingerprint.cs"))
        .expect("host fingerprint partition");
    let start = host_fingerprint
        .content
        .find("Value = ")
        .expect("fingerprint constant");
    let end = host_fingerprint.content[start..]
        .find("UL;")
        .map(|offset| start + offset + 3)
        .expect("fingerprint constant terminator");
    host_fingerprint
        .content
        .replace_range(start..end, "Value = 0x0UL;");
    let project = temp.path().join("project");
    fs::create_dir_all(&project).expect("create fingerprint project");
    write_output(&output, &project).expect("write mismatched C# profile");
    let root = workspace_root();
    write_project(
        &project,
        &[
            root.join("sdks/csharp/guest/Polyplug.Guest.csproj"),
            root.join("sdks/csharp/host/Polyplug.Host.csproj"),
        ],
    );
    let result = run_project(&project);
    assert!(
        !result.status.success()
            && String::from_utf8_lossy(&result.stderr)
                .contains("generated internal partitions are incompatible"),
        "mismatched C# partitions must fail during generated module initialization:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
}

#[test]
fn split_csharp_profile_rejects_mismatched_domain_assembly_at_module_initialization() {
    let temp = TempDir::new().expect("create split C# fingerprint fixture");
    let api = temp.path().join("api.toml");
    let bundle = temp.path().join("bundle.toml");
    let bindings = temp.path().join("bindings");
    let domain = temp.path().join("domain");
    let contracts = temp.path().join("contracts");
    write_rich_api(&api, "game_engine.Plugin");
    write_bundle(
        &bundle,
        "api.toml",
        "csharp_split_fingerprint",
        "game_engine.Plugin",
    );
    let layout = OutputLayout {
        bindings: OutputDestination::Inline,
        domain_types: OutputDestination::Emit {
            root: domain.clone(),
            import: ValidatedImport::parse(Lang::CSharp, "Fingerprint.Domain")
                .expect("domain namespace import"),
        },
        guest_contracts: OutputDestination::Emit {
            root: contracts.clone(),
            import: ValidatedImport::parse(Lang::CSharp, "Fingerprint.Contracts")
                .expect("contract namespace import"),
        },
    };
    let output = generate_internal_csharp(InternalCSharpGenerateConfig {
        bundle_toml: bundle,
        out_dir: bindings.clone(),
        layout,
    })
    .expect("generate split C# profile");
    write_output(&output, &bindings).expect("write split generated C# assemblies");
    let root = workspace_root();
    write_project(
        &domain,
        &[root.join("sdks/csharp/guest/Polyplug.Guest.csproj")],
    );
    write_project(
        &contracts,
        &[
            domain.join("Primitive.csproj"),
            root.join("sdks/csharp/guest/Polyplug.Guest.csproj"),
        ],
    );
    write_project(
        &bindings,
        &[
            domain.join("Primitive.csproj"),
            contracts.join("Primitive.csproj"),
            root.join("sdks/csharp/guest/Polyplug.Guest.csproj"),
            root.join("sdks/csharp/host/Polyplug.Host.csproj"),
        ],
    );
    for (project, name) in [
        (&domain, "SplitFingerprintDomain"),
        (&contracts, "SplitFingerprintContracts"),
        (&bindings, "SplitFingerprintBindings"),
    ] {
        let project_file = project.join("Primitive.csproj");
        let source = fs::read_to_string(&project_file).expect("read split project");
        fs::write(
            &project_file,
            source.replacen(
                "<PropertyGroup>",
                &format!("<PropertyGroup>\n        <AssemblyName>{name}</AssemblyName>"),
                1,
            ),
        )
        .expect("name split project assembly");
    }
    assert_project_runs(&bindings, "matching generated split C# assemblies");
    replace_domain_marker(&domain);
    let mismatch_build = Command::new("dotnet")
        .args(["build", "Primitive.csproj", "-c", "Release", "--nologo"])
        .current_dir(&domain)
        .output()
        .expect("rebuild mismatched domain assembly");
    assert!(
        mismatch_build.status.success(),
        "mismatched domain assembly did not build:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&mismatch_build.stdout),
        String::from_utf8_lossy(&mismatch_build.stderr),
    );
    let dll = "SplitFingerprintDomain.dll";
    fs::copy(
        domain.join("bin/Release/net10.0").join(dll),
        bindings.join("bin/Release/net10.0").join(dll),
    )
    .expect("replace deployed domain assembly");
    let result = Command::new("dotnet")
        .arg(bindings.join("bin/Release/net10.0/SplitFingerprintBindings.dll"))
        .output()
        .expect("execute binding assembly without rebuilding");
    assert!(
        !result.status.success()
            && String::from_utf8_lossy(&result.stderr)
                .contains("generated internal partitions are incompatible"),
        "replaced generated domain assembly must fail generated module initialization:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
}

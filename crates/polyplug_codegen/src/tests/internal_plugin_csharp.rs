//! Focused output and compile contracts for explicit C# internal generation.

#![allow(clippy::expect_used)]

use crate::{
    GenerateConfig, GenerateOutput, InternalCSharpGenerateConfig, Lang, Side, generate,
    generate_internal_csharp, write_output,
};
use core::iter::once;
use polyplug_utils::bundle_id;
use std::collections::{BTreeMap, HashSet};
use std::env::consts::OS;
use std::env::{join_paths, split_paths, var_os};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

fn output_map(output: GenerateOutput) -> BTreeMap<PathBuf, String> {
    output
        .files
        .into_iter()
        .map(|file| (file.path, file.content))
        .collect()
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("codegen crate parent")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn native_library_search_variable() -> &'static str {
    match OS {
        "windows" => "PATH",
        "macos" => "DYLD_LIBRARY_PATH",
        _ => "LD_LIBRARY_PATH",
    }
}

fn prepend_native_library_search_path(native_dir: &Path, existing: Option<&OsStr>) -> OsString {
    let existing_paths = existing.map(split_paths).into_iter().flatten();
    join_paths(once(native_dir.to_path_buf()).chain(existing_paths))
        .expect("native library search paths must be valid")
}

fn configure_native_library_search_path(command: &mut Command, native_dir: &Path) {
    let variable = native_library_search_variable();
    let value = prepend_native_library_search_path(native_dir, var_os(variable).as_deref());
    command.env(variable, value);
}

#[test]
fn native_library_search_path_prepends_and_preserves_entries() {
    let native_dir = Path::new("native");
    let existing = join_paths([Path::new("existing")]).expect("create existing native search path");
    let paths = split_paths(&prepend_native_library_search_path(
        native_dir,
        Some(existing.as_os_str()),
    ))
    .collect::<Vec<_>>();
    assert_eq!(
        paths,
        vec![native_dir.to_path_buf(), PathBuf::from("existing")]
    );
}

fn write_api(path: &Path, contract_name: &str) {
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
  {{ name = "name", type = "StringView" }},
  {{ name = "bytes", type = "Buffer" }},
]

[[types]]
name = "Envelope"
fields = [
  {{ name = "inner", type = "Inner" }},
  {{ name = "entries", type = "Array<Inner>" }},
]

[[plugin_contract]]
name = "{contract_name}"
version = "1.0"

[[plugin_contract.functions]]
name = "scalar"
params = [{{ name = "left", type = "u32" }}, {{ name = "right", type = "u32" }}]
return = "u32"

[[plugin_contract.functions]]
name = "text"
params = [{{ name = "label", type = "StringView" }}, {{ name = "mode", type = "Mode" }}]
return = "StringView"

[[plugin_contract.functions]]
name = "buffer"
params = [{{ name = "bytes", type = "Buffer" }}]
return = "Buffer"

[[plugin_contract.functions]]
name = "nested"
params = [{{ name = "value", type = "Inner" }}]
return = "Envelope"

[[plugin_contract.functions]]
name = "array"
params = [{{ name = "values", type = "Array<Inner>" }}]
return = "Array<Inner>"

[[plugin_contract.functions]]
name = "fail"
"#
        ),
    )
    .expect("write API TOML");
}

fn write_internal_bundle(
    path: &Path,
    api_name: &str,
    bundle_name: &str,
    plugin_name: &str,
    contract_name: &str,
) {
    fs::write(
        path,
        format!(
            "[bundle]\nname = \"{bundle_name}\"\nversion = \"1.0\"\napi = \"{api_name}\"\n\n[[plugin]]\nname = \"{plugin_name}\"\nimplements = [\"{contract_name}@1.0\"]\n"
        ),
    )
    .expect("write artifactless internal bundle TOML");
}

#[test]
fn internal_csharp_profile_uses_identity_namespaces_and_typed_registration() {
    let temp = tempfile::tempdir().expect("create temporary directory");
    let api = temp.path().join("api.toml");
    let bundle = temp.path().join("bundle.toml");
    write_api(&api, "shape.Contract");
    write_internal_bundle(
        &bundle,
        "api.toml",
        "csharp_profile",
        "shape_provider",
        "shape.Contract",
    );

    let output = output_map(
        generate_internal_csharp(InternalCSharpGenerateConfig {
            bundle_toml: bundle,
            out_dir: temp.path().join("out"),
        })
        .expect("generate C# internal profile"),
    );

    let namespace = Path::new("internal").join(format!(
        "csharp_profile-{:016x}",
        bundle_id("csharp_profile")
    ));
    assert!(
        output.keys().all(|path| path.starts_with(&namespace)),
        "every C# profile output must be bundle-identity namespaced: {output:#?}"
    );
    let registration = output
        .values()
        .find(|content| content.contains("public sealed class RegistrationInput"))
        .expect("generated C# registration facade");
    assert!(registration.contains("RegisterInternalPlugin"));
    assert!(registration.contains("Interlocked.Exchange(ref _consumed, 1)"));
    assert!(registration.contains("public ulong BundleId"));
    assert!(registration.contains("public GuestContractHandle[] Handles"));
    assert!(registration.contains("CreateFromCommittedHandle(runtime, published.Handles[0])"));
    assert!(registration.contains("ShapeProviderShapeContract"));
    let resident = output
        .values()
        .find(|content| content.contains("internal static class InternalPluginFactory"))
        .expect("generated private internal plugin adapter");
    assert!(!resident.contains("loader ="));
    assert!(!resident.contains("file ="));
    let callers = output
        .values()
        .find(|content| content.contains("public sealed unsafe class ShapeContractContractCaller"))
        .expect("generated typed host caller");
    assert!(callers.contains("GuestContractInstance inst = default"));
    assert!(callers.contains("plugin call failed: code={err.Code}"));
    assert!(
        callers.contains(
            "if (iface == _interface) { _cachedRevision = LiveRevision(); return true; }"
        )
    );
    let types = output
        .values()
        .find(|content| content.contains("public struct Envelope"))
        .expect("generated nested C# value types");
    assert!(types.contains("public struct Inner"));
    let interfaces = output
        .values()
        .find(|content| content.contains("catch (Polyplug.Guest.GuestException ex)"))
        .expect("generated C# error translation adapter");
    assert!(interfaces.contains("ShapeContractContractScalarArgs"));
}

#[test]
fn external_csharp_generation_emits_no_internal_profile_files() {
    let temp = tempfile::tempdir().expect("create temporary directory");
    let api = temp.path().join("api.toml");
    write_api(&api, "external.Contract");

    let output = generate(GenerateConfig {
        api_toml: api,
        lang: Lang::CSharp,
        side: Side::Guest,
        out_dir: temp.path().join("out"),
    })
    .expect("generate external C# guest bindings");

    let paths: Vec<PathBuf> = output.files.iter().map(|file| file.path.clone()).collect();
    assert_eq!(
        paths,
        vec![
            PathBuf::from("guest/Types.cs"),
            PathBuf::from("guest/Contracts.cs"),
            PathBuf::from("guest/Interfaces.cs"),
            PathBuf::from("guest/Init.cs"),
        ],
        "external C# generation must emit only canonical guest bindings"
    );
    assert!(output.files.iter().all(|file| {
        !file.content.contains("InternalPluginBundle")
            && !file.content.contains("InternalPlugin")
            && !file.content.contains("RegistrationInput")
    }));
}

#[test]
fn two_internal_csharp_profiles_with_different_apis_compile_together() {
    let temp = tempfile::tempdir().expect("create temporary directory");
    let first_api = temp.path().join("first-api.toml");
    let second_api = temp.path().join("second-api.toml");
    let first_bundle = temp.path().join("first-bundle.toml");
    let second_bundle = temp.path().join("second-bundle.toml");
    write_api(&first_api, "first.Contract");
    write_api(&second_api, "second.Contract");
    write_internal_bundle(
        &first_bundle,
        "first-api.toml",
        "first_csharp_profile",
        "first_provider",
        "first.Contract",
    );
    write_internal_bundle(
        &second_bundle,
        "second-api.toml",
        "second_csharp_profile",
        "second_provider",
        "second.Contract",
    );

    let first = generate_internal_csharp(InternalCSharpGenerateConfig {
        bundle_toml: first_bundle,
        out_dir: temp.path().join("out"),
    })
    .expect("generate first internal C# profile");
    let second = generate_internal_csharp(InternalCSharpGenerateConfig {
        bundle_toml: second_bundle,
        out_dir: temp.path().join("out"),
    })
    .expect("generate second internal C# profile");
    let mut paths = HashSet::new();
    for file in first.files.iter().chain(second.files.iter()) {
        assert!(
            paths.insert(file.path.clone()),
            "C# internal profiles emitted colliding path `{}`",
            file.path.display()
        );
    }
    let mut files = first.files;
    files.extend(second.files);
    let source = temp.path().join("source");
    write_output(&GenerateOutput { files }, &source).expect("write generated C# profiles");

    let root = workspace_root();
    let abi = root.join("sdks/csharp/abi/Polyplug.Abi.csproj");
    let guest = root.join("sdks/csharp/guest/Polyplug.Guest.csproj");
    let host = root.join("sdks/csharp/host/Polyplug.Host.csproj");
    let project = source.join("Profiles.csproj");
    fs::write(
        &project,
        format!(
            r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <TargetFramework>net10.0</TargetFramework>
    <AllowUnsafeBlocks>true</AllowUnsafeBlocks>
    <Nullable>enable</Nullable>
    <ImplicitUsings>enable</ImplicitUsings>
  </PropertyGroup>
  <ItemGroup>
    <ProjectReference Include="{}" />
    <ProjectReference Include="{}" />
    <ProjectReference Include="{}" />
  </ItemGroup>
</Project>
"#,
            abi.display(),
            guest.display(),
            host.display(),
        ),
    )
    .expect("write C# compile project");
    let output = Command::new("dotnet")
        .args(["build", "--nologo", "--verbosity", "quiet"])
        .arg(&project)
        .current_dir(&source)
        .output()
        .expect("run focused C# generated-profile compilation");
    assert!(
        output.status.success(),
        "two internal C# profiles with distinct APIs must compile together\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn generated_internal_csharp_profile_registers_and_dispatches_real_runtime_shapes() {
    let temp = tempfile::tempdir().expect("create temporary directory");
    let api = temp.path().join("api.toml");
    let bundle = temp.path().join("bundle.toml");
    write_api(&api, "profile.Contract");
    fs::write(
        &bundle,
        "[bundle]\nname = \"profilee2e\"\nversion = \"1.0\"\napi = \"api.toml\"\n\n[[plugin]]\nname = \"first\"\nimplements = [\"profile.Contract@1.0\"]\n\n[[plugin]]\nname = \"second\"\nimplements = [\"profile.Contract@1.0\"]\n",
    )
    .expect("write same-contract multi-provider internal bundle TOML");
    let output = generate_internal_csharp(InternalCSharpGenerateConfig {
        bundle_toml: bundle,
        out_dir: temp.path().join("out"),
    })
    .expect("generate executable C# internal profile");
    let source = temp.path().join("source");
    write_output(&output, &source).expect("write executable C# profile");

    let root = workspace_root();
    let abi = root.join("sdks/csharp/abi/Polyplug.Abi.csproj");
    let guest = root.join("sdks/csharp/guest/Polyplug.Guest.csproj");
    let host = root.join("sdks/csharp/host/Polyplug.Host.csproj");
    let namespace = format!(
        "Polyplug.Generated.Internal.Bundleprofilee2e{:016X}",
        bundle_id("profilee2e")
    );
    fs::write(
        source.join("Program.cs"),
        format!(
            r#"using System;

using Polyplug.Abi;
using Polyplug.Guest;
using Polyplug.Host;

using Generated = {namespace};
using Guest = {namespace}.Guest;
using Host = {namespace}.Host;

sealed class ProviderFirst(nint host) : Guest.IProfileContractGuestContract
{{
    private readonly nint _host = host;
    private uint _calls;

    public uint Scalar(uint left, uint right) => left + right + _calls++;
    public StringView Text(StringView label, Guest.Mode mode) => PolyplugHost.AllocString(_host, "ok");
    public Polyplug.Abi.Buffer Buffer(Polyplug.Abi.Buffer bytes) => bytes;
    public Guest.Envelope Nested(ref Guest.Inner value) => default;
    public Guest.ArrayOf_Inner Array(ref Guest.ArrayOf_Inner values) => values;
    public void Fail() => throw new GuestException(7, "expected");
}}

sealed class ProviderSecond : Guest.IProfileContractGuestContract
{{
    private uint _calls;

    public uint Scalar(uint left, uint right) => left + right + _calls++;
    public StringView Text(StringView label, Guest.Mode mode) => default;
    public Polyplug.Abi.Buffer Buffer(Polyplug.Abi.Buffer bytes) => bytes;
    public Guest.Envelope Nested(ref Guest.Inner value) => default;
    public Guest.ArrayOf_Inner Array(ref Guest.ArrayOf_Inner values) => values;
    public void Fail() => throw new GuestException(7, "expected");
}}

static class Program
{{
    static int Main()
    {{
        Runtime runtime = new();
        var registered = Generated.InternalPlugin.Register(
            runtime,
            new Generated.RegistrationInput(
                host => new ProviderFirst(host),
                _ => new ProviderSecond()));
        var caller = registered.FirstProfileContractContract;
        var other = registered.SecondProfileContractContract;
        try
        {{
            var scalar = new Host.ProfileContractContractScalarArgs {{ Left = 1, Right = 2 }};
            if (caller.Scalar(scalar) != 3 || caller.Scalar(scalar) != 4 || other.Scalar(scalar) != 3)
                throw new InvalidOperationException("committed handles did not preserve independent same-contract providers");
            var text = new Host.ProfileContractContractTextArgs {{
                Label = default,
                Mode = Host.Mode.Ready,
            }};
            if (caller.Text(text).Len != 2)
                throw new InvalidOperationException("string dispatch failed");
            if (caller.Buffer(default).Len != 0)
                throw new InvalidOperationException("buffer dispatch failed");
            _ = caller.Nested(default);
            _ = caller.Array(default);
            bool guestFailure = false;
            try
            {{
                caller.Fail();
            }}
            catch (InvalidOperationException)
            {{
                guestFailure = true;
            }}
            if (!guestFailure)
                throw new InvalidOperationException("generated guest error did not reach caller");

            var failed = new Generated.RegistrationInput(
                host => new ProviderFirst(host),
                _ => new ProviderSecond());
            bool duplicateFailure = false;
            try
            {{
                Generated.InternalPlugin.Register(runtime, failed);
            }}
            catch (InvalidOperationException)
            {{
                duplicateFailure = true;
            }}
            if (!duplicateFailure)
                throw new InvalidOperationException("duplicate registration unexpectedly succeeded");
            bool consumedFailure = false;
            try
            {{
                Generated.InternalPlugin.Register(runtime, failed);
            }}
            catch (InvalidOperationException)
            {{
                consumedFailure = true;
            }}
            if (!consumedFailure)
                throw new InvalidOperationException("consumed input unexpectedly retried");
        }}
        finally
        {{
            caller.Dispose();
            other.Dispose();
        }}

        runtime.UnloadBundle(registered.BundleId);
        var retry = Generated.InternalPlugin.Register(
            runtime,
            new Generated.RegistrationInput(
                host => new ProviderFirst(host),
                _ => new ProviderSecond()));
        retry.FirstProfileContractContract.Dispose();
        retry.SecondProfileContractContract.Dispose();
        runtime.UnloadBundle(retry.BundleId);
        GC.KeepAlive(runtime);
        return 0;
    }}
}}
"#
        ),
    )
    .expect("write C# internal profile executable");
    let project = source.join("Profiles.csproj");
    fs::write(
        &project,
        format!(
            r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <TargetFramework>net10.0</TargetFramework>
    <AllowUnsafeBlocks>true</AllowUnsafeBlocks>
    <Nullable>enable</Nullable>
    <ImplicitUsings>enable</ImplicitUsings>
  </PropertyGroup>
  <ItemGroup>
    <ProjectReference Include="{}" />
    <ProjectReference Include="{}" />
    <ProjectReference Include="{}" />
  </ItemGroup>
</Project>
"#,
            abi.display(),
            guest.display(),
            host.display(),
        ),
    )
    .expect("write C# internal profile executable project");
    let native_dir = root.join("target").join("debug");
    let mut command = Command::new("dotnet");
    command
        .args(["run", "--nologo", "--verbosity", "quiet"])
        .arg("--project")
        .arg(&project)
        .current_dir(&source);
    configure_native_library_search_path(&mut command, &native_dir);
    let output = command
        .output()
        .expect("run generated C# internal profile executable");
    assert!(
        output.status.success(),
        "generated C# profile must register and dispatch against the real runtime\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

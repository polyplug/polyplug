//! End-to-end compile proofs for `polyplugc generate --lang cpp` and
//! `--lang csharp`.
//!
//! These mirror the Rust reference in `generate_e2e.rs`: the test runs the
//! `polyplugc` binary against the representative decoder bundle fixture, the
//! TEST writes the minimal project shell + plugin impl (the only hand-written
//! files), and then the language toolchain builds it. A successful shared
//! library / assembly proves the generated guest glue compiles with zero hand
//! edits to any emitted file.
//!
//! Toolchains (`c++`, `dotnet`) are guaranteed by CI (see
//! `examples/build_all.sh`), so there is no skip-if-missing logic — a missing
//! toolchain is a hard failure, as CLAUDE.md requires.
//!
//! Run with:
//!   cargo test --test generate_e2e_native --package polyplugc

#![allow(clippy::expect_used)]

use std::ffi::OsStr;
use std::fs;
use std::fs::DirEntry;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Output;
use tempfile::TempDir;
use tempfile::tempdir;

/// Absolute path to the repository root, derived from this crate's manifest dir
/// (`<repo>/crates/polyplugc`).
fn repo_root() -> PathBuf {
    let manifest_dir: PathBuf = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(Path::parent)
        .expect("crate manifest dir must have a grandparent (the repo root)")
        .to_path_buf()
}

/// Absolute path to the representative Rust decoder bundle used by all e2e
/// tests. The `--lang` flag drives codegen independently of the bundle's own
/// `runtime`, so this single fixture exercises every generator's StringView
/// param/return surface.
fn example_bundle_toml() -> PathBuf {
    repo_root()
        .join("examples")
        .join("guests")
        .join("rust")
        .join("decoder")
        .join("bundle.toml")
}

/// Run the `polyplugc` binary with `args`, returning the captured output.
fn run_polyplugc(args: &[&OsStr]) -> Output {
    let bin: &str = env!("CARGO_BIN_EXE_polyplugc");
    Command::new(bin)
        .args(args)
        .output()
        .expect("failed to spawn polyplugc binary")
}

/// Canonicalize for toolchain consumption: resolves the macOS /var -> /private/var
/// symlink, then strips Windows' verbatim prefix (\\?\ / \\?\UNC\) which MSBuild
/// cannot import (MSB4019, `$(MSBuildProjectExtensionsPath)` evaluates to `\\%3f\…`).
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

/// Generate the guest glue for the decoder fixture into `out_dir` for `lang`.
fn generate_into(out_dir: &Path, lang: &str) {
    let output: Output = run_polyplugc(&[
        "generate".as_ref(),
        "--bundle".as_ref(),
        example_bundle_toml().as_os_str(),
        "--lang".as_ref(),
        lang.as_ref(),
        "--out".as_ref(),
        out_dir.as_os_str(),
    ]);
    assert!(
        output.status.success(),
        "polyplugc generate --lang {lang} failed (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// C++: generate → c++ -shared -fPIC must produce a shared library.
//
// The generated headers `#include "polyplug/abi.hpp"` and
// `"polyplug/guest.hpp"`; the test supplies the two in-tree SDK include roots
// (`sdks/cpp/abi`, `sdks/cpp/guest`) plus the generated dir on the include
// path. The cpp guest helpers (`alloc_string(host, s)`, …) are header-only
// `inline` definitions, so no `-lpolyplug` link is required — the host
// function pointers are resolved at call time via `host->`, not at link time.
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn cpp_generated_glue_compiles() {
    let tmp: TempDir = tempdir().expect("tempdir");
    let project_dir: PathBuf = tmp.path().join("plugin");
    let gen_dir: PathBuf = project_dir.join("gen");
    fs::create_dir_all(&project_dir).expect("create project dir");

    generate_into(&gen_dir, "cpp");
    assert!(
        gen_dir.join("guest/init.hpp").exists(),
        "generated guest/init.hpp must exist at {}",
        gen_dir.join("guest/init.hpp").display()
    );

    // The only hand-written source: a minimal plugin impl modeled on
    // examples/guests/cpp/decoder/decoder.cpp. It includes the generated entry
    // header (which pulls in interfaces/contracts) and provides the stub
    // implementation + factory the generator forward-declares.
    let plugin_cpp: &str = "#include \"guest/init.hpp\"\n\
         #include <string>\n\
         #include <algorithm>\n\
         \n\
         namespace polyplug_plugin {\n\
         class DecoderImpl : public PipelineDecoderGuestContract {\n\
         public:\n\
         explicit DecoderImpl(const HostApi* host) : host_(host) {}\n\
         StringView decode(StringView input) override {\n\
         std::string s = polyplug::abi::to_string(input);\n\
         std::replace(s.begin(), s.end(), ',', '|');\n\
         return polyplug::alloc_string(host_, \"DECODED:\" + s);\n\
         }\n\
         private:\n\
         const HostApi* host_;\n\
         };\n\
         PipelineDecoderGuestContract* polyplug_create_decoder(const HostApi* host) { return new DecoderImpl(host); }\n\
         }  // namespace polyplug_plugin\n";
    let plugin_src: PathBuf = project_dir.join("plugin.cpp");
    fs::write(&plugin_src, plugin_cpp).expect("write plugin.cpp");

    let cpp_abi_include: PathBuf = repo_root().join("sdks").join("cpp").join("abi");
    let cpp_guest_include: PathBuf = repo_root().join("sdks").join("cpp").join("guest");
    let out_lib: PathBuf = project_dir.join("libplugin.so");

    // Compiler conventions mirror examples/build_all.sh (g++ -std=c++20 -fPIC
    // -shared). `c++` is the portable driver name resolving to the platform
    // compiler (g++/clang++).
    let build: Output = Command::new("c++")
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
        .arg(&plugin_src)
        .arg("-o")
        .arg(&out_lib)
        .output()
        .expect("failed to spawn c++ compiler");
    assert!(
        build.status.success(),
        "c++ build of generated cpp glue failed (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        build.status.code(),
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr),
    );
    assert!(
        out_lib.exists(),
        "c++ build reported success but no shared library was produced at {}",
        out_lib.display(),
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// C#: generate → dotnet build must produce the plugin assembly.
//
// The test writes a minimal .csproj (net10.0, matching the SDK projects) with a
// ProjectReference to the in-tree guest SDK (which transitively references the
// ABI SDK), plus a Plugin.cs modeled on examples/guests/csharp/decoder/Decoder.cs.
// The generated `.cs` files live under the project dir and are picked up by the
// SDK's default compile globbing — no explicit <Compile Include> is needed.
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn csharp_generated_glue_compiles() {
    let tmp: TempDir = tempdir().expect("tempdir");
    // Canonicalize: on macOS the tempdir lives under the /var/folders →
    // /private/var/folders symlink, and MSBuild's ProjectReference path
    // relativization mixes the resolved and unresolved forms (MSB3202,
    // "../../../..//Users/..." misses by one level). Canonicalizing makes
    // every path the test writes agree with what dotnet resolves. On Windows
    // the helper also strips the verbatim prefix MSBuild cannot import.
    let tmp_root: PathBuf = canonicalize_for_toolchain(tmp.path());
    let project_dir: PathBuf = tmp_root.join("plugin");
    let gen_dir: PathBuf = project_dir.join("generated");
    fs::create_dir_all(&project_dir).expect("create project dir");

    generate_into(&gen_dir, "csharp");
    assert!(
        gen_dir.join("guest/Interfaces.cs").exists(),
        "generated guest/Interfaces.cs must exist at {}",
        gen_dir.join("guest/Interfaces.cs").display()
    );

    let guest_csproj: PathBuf = repo_root()
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
         <AssemblyName>plugin</AssemblyName>\n\
         </PropertyGroup>\n\
         <ItemGroup>\n\
         <ProjectReference Include=\"{}\" />\n\
         </ItemGroup>\n\
         </Project>\n",
        guest_csproj.display(),
    );
    fs::write(project_dir.join("Plugin.csproj"), csproj).expect("write Plugin.csproj");

    // The only hand-written source: a minimal plugin impl modeled on
    // examples/guests/csharp/decoder/Decoder.cs.
    let plugin_cs: &str = "using System.Runtime.CompilerServices;\n\
         using Polyplug.Guest;\n\
         using Polyplug.Abi;\n\
         \n\
         public sealed class DecoderPlugin : IPipelineDecoderGuestContract\n\
         {\n\
         private readonly IntPtr _host;\n\
         \n\
         public DecoderPlugin(IntPtr host)\n\
         {\n\
         _host = host;\n\
         }\n\
         \n\
         public StringView Decode(StringView input)\n\
         {\n\
         string s = StringViewHelper.ToString(input).Replace(',', '|');\n\
         return PolyplugHost.AllocString(_host, $\"DECODED:{s}\");\n\
         }\n\
         }\n\
         \n\
         public static class Registration\n\
         {\n\
         [ModuleInitializer]\n\
         public static void Register()\n\
         {\n\
         DecoderInterfaces.SetDecoderFactory(host => new DecoderPlugin(host));\n\
         }\n\
         }\n";
    fs::write(project_dir.join("Plugin.cs"), plugin_cs).expect("write Plugin.cs");

    // Keep the build hermetic: route the plugin assembly output into the
    // tempdir. The referenced SDK projects still build into their own
    // (gitignored) bin/obj under sdks/csharp, exactly as examples/build_all.sh
    // does.
    let out_dir: PathBuf = project_dir.join("out");
    let build: Output = Command::new("dotnet")
        .arg("build")
        .arg(project_dir.join("Plugin.csproj"))
        .arg("-c")
        .arg("Release")
        .arg(format!("-p:OutputPath={}", out_dir.display()))
        .output()
        .expect("failed to spawn dotnet build");
    assert!(
        build.status.success(),
        "dotnet build of generated csharp glue failed (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        build.status.code(),
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr),
    );

    let produced_assembly: bool = fs::read_dir(&out_dir)
        .expect("output dir must exist after a successful build")
        .filter_map(Result::ok)
        .any(|entry: DirEntry| entry.file_name().to_string_lossy() == "plugin.dll");
    assert!(
        produced_assembly,
        "dotnet build succeeded but no plugin.dll was produced in {}",
        out_dir.display(),
    );
}

//! Build script for polyplug_dotnet.
//!
//! Compiles the managed byte-array loading bridge
//! (`sdks/csharp/loaders/DotnetByteBridge`) with `dotnet build` and exposes the resulting
//! assembly path to the crate via the `POLYPLUG_DOTNET_BYTE_BRIDGE_DLL` env var, which the
//! loader embeds with `include_bytes!`.
//!
//! When the .NET SDK is unavailable the build still succeeds: the env var is set to the
//! sentinel `BYTE_BRIDGE_UNAVAILABLE`, and `BundleSource::Bytes` loads return a structured
//! error at runtime instead of failing the Rust build.

use std::env;
use std::fs;
use std::io::Error as IoError;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::ExitStatus;

fn main() {
    let manifest_dir: PathBuf = PathBuf::from(env_var("CARGO_MANIFEST_DIR"));
    let workspace_root: PathBuf = manifest_dir
        .parent()
        .and_then(|p: &Path| p.parent())
        .map(Path::to_path_buf)
        .unwrap_or(manifest_dir.clone());

    let bridge_dir: PathBuf = workspace_root
        .join("sdks")
        .join("csharp")
        .join("loaders")
        .join("DotnetByteBridge");
    let bridge_dll: PathBuf = bridge_dir
        .join("bin")
        .join("Release")
        .join("net10.0")
        .join("Polyplug.Loaders.DotnetByteBridge.dll");

    // Re-run when the bridge sources change.
    println!(
        "cargo:rerun-if-changed={}",
        bridge_dir.join("ByteBridge.cs").to_string_lossy()
    );
    println!(
        "cargo:rerun-if-changed={}",
        bridge_dir
            .join("Polyplug.Loaders.DotnetByteBridge.csproj")
            .to_string_lossy()
    );

    // Always emit a file at a stable OUT_DIR path so `include_bytes!` has something to
    // embed regardless of whether the bridge built. An empty embedded blob signals
    // "bridge unavailable" to the runtime, which then rejects `Bytes` loads cleanly.
    let out_dir: PathBuf = PathBuf::from(env_var("OUT_DIR"));
    let embed_path: PathBuf = out_dir.join("byte_bridge.dll");

    let built: bool = build_bridge(&bridge_dir);

    let bridge_available: bool = if built && bridge_dll.exists() {
        match fs::copy(&bridge_dll, &embed_path) {
            Ok(_) => true,
            Err(e) => {
                println!(
                    "cargo:warning=polyplug_dotnet: failed to stage byte-load bridge dll: {e}"
                );
                let _: Result<(), IoError> = fs::write(&embed_path, []);
                false
            }
        }
    } else {
        println!(
            "cargo:warning=polyplug_dotnet: byte-load bridge unavailable (dotnet SDK missing or build failed); BundleSource::Bytes will be unsupported at runtime"
        );
        let _: Result<(), IoError> = fs::write(&embed_path, []);
        false
    };

    // `include_bytes!` reads from this stable path; the bool tells the runtime whether the
    // embedded blob is a real assembly.
    println!(
        "cargo:rustc-env=POLYPLUG_DOTNET_BYTE_BRIDGE_DLL={}",
        embed_path.to_string_lossy()
    );
    println!(
        "cargo:rustc-env=POLYPLUG_DOTNET_BYTE_BRIDGE_AVAILABLE={}",
        if bridge_available { "1" } else { "0" }
    );
}

/// Build the managed bridge with `dotnet build -c Release`. Returns `true` on success.
fn build_bridge(bridge_dir: &Path) -> bool {
    let status: Result<ExitStatus, IoError> = Command::new("dotnet")
        .arg("build")
        .arg("-c")
        .arg("Release")
        .arg("--nologo")
        .arg(bridge_dir)
        .status();
    matches!(status, Ok(s) if s.success())
}

fn env_var(key: &str) -> String {
    env::var(key).unwrap_or_default()
}

//! Build script for the integration test package.
//!
//! Emits cargo:rustc-env variables for paths to pre-built test fixtures.
//! Artifacts are built by crates/polyplug/build.rs — this script only locates them.
//!
//! Build scripts are permitted to use `.expect()` and `panic!()` freely.

#![allow(clippy::expect_used)]

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let manifest_dir: PathBuf =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set"));

    // Workspace root is two levels up from tests/integration/
    let workspace_root: PathBuf = manifest_dir
        .parent()
        .expect("parent of tests/integration")
        .parent()
        .expect("workspace root")
        .to_path_buf();

    let fixtures_dir: PathBuf = workspace_root.join("tests").join("fixtures");

    // Platform-specific shared library filename for test_plugin
    let test_plugin_filename: &str = if cfg!(target_os = "macos") {
        "libtest_plugin.dylib"
    } else if cfg!(target_os = "windows") {
        "test_plugin.dll"
    } else {
        "libtest_plugin.so"
    };

    // TEST_PLUGIN_SO — native Rust test plugin (built by crates/polyplug/build.rs)
    let test_plugin_so: PathBuf = fixtures_dir.join(test_plugin_filename);
    if test_plugin_so.exists() {
        println!(
            "cargo:rustc-env=TEST_PLUGIN_SO={}",
            test_plugin_so.display()
        );
    } else {
        println!("cargo:rustc-env=TEST_PLUGIN_SO=");
    }

    // TEST_PLUGIN_CPP_SO — C++ test plugin (built by crates/polyplug/build.rs via g++)
    let cpp_so_filename: &str = if cfg!(target_os = "macos") {
        "libtest_plugin_cpp.dylib"
    } else {
        "libtest_plugin_cpp.so"
    };
    let cpp_so: PathBuf = fixtures_dir.join(cpp_so_filename);
    if cpp_so.exists() {
        println!("cargo:rustc-env=TEST_PLUGIN_CPP_SO={}", cpp_so.display());
    } else {
        println!("cargo:rustc-env=TEST_PLUGIN_CPP_SO=");
    }

    // TEST_CSHARP_PLUGIN_DLL — C# plugin (built by crates/polyplug/build.rs via dotnet)
    let csharp_dll: PathBuf =
        workspace_root.join("tests/fixtures/csharp_plugin/bin/Debug/net10.0/CsharpPlugin.dll");
    let dotnet_available: bool = csharp_dll.exists()
        && Command::new("dotnet")
            .arg("--version")
            .output()
            .map(|o: std::process::Output| o.status.success())
            .unwrap_or(false);
    if dotnet_available {
        println!(
            "cargo:rustc-env=TEST_CSHARP_PLUGIN_DLL={}",
            csharp_dll.display()
        );
    } else {
        println!("cargo:rustc-env=TEST_CSHARP_PLUGIN_DLL=DOTNET_NOT_AVAILABLE");
    }

    // TEST_PYTHON_PLUGIN — Python plugin script
    let python_available: bool = Command::new("python3")
        .arg("--version")
        .output()
        .map(|o: std::process::Output| o.status.success())
        .unwrap_or(false);
    if python_available {
        println!(
            "cargo:rustc-env=TEST_PYTHON_PLUGIN={}",
            fixtures_dir.join("test_plugin.py").display()
        );
    } else {
        println!("cargo:rustc-env=TEST_PYTHON_PLUGIN=PYTHON_NOT_AVAILABLE");
    }

    // TEST_LUA_PLUGIN — Lua plugin script (mlua vendored — always available)
    println!(
        "cargo:rustc-env=TEST_LUA_PLUGIN={}",
        fixtures_dir.join("test_plugin.lua").display()
    );

    // TEST_JS_PLUGIN — QuickJS plugin bundle directory
    println!(
        "cargo:rustc-env=TEST_JS_PLUGIN={}",
        fixtures_dir.join("test_plugin_js").display()
    );

    // TEST_JS_DENO_PLUGIN — Deno plugin directory
    println!(
        "cargo:rustc-env=TEST_JS_DENO_PLUGIN={}",
        fixtures_dir.join("test_plugin_js_deno").display()
    );
}

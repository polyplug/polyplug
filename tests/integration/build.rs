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

    let target_dir: PathBuf = env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| workspace_root.join("target"));

    // Determine profile from OUT_DIR components
    let out_dir: PathBuf = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR not set"));
    let profile: &str = if out_dir.components().any(|c| c.as_os_str() == "release") {
        "release"
    } else {
        "debug"
    };

    let fixtures_dir: PathBuf = workspace_root.join("tests").join("fixtures");

    // Platform-specific shared library filename for polyplug
    let polyplug_lib_filename: &str = if cfg!(target_os = "macos") {
        "libpolyplug.dylib"
    } else if cfg!(target_os = "windows") {
        "polyplug.dll"
    } else {
        "libpolyplug.so"
    };

    // POLYPLUG_SO — main polyplug library (required for deno test)
    let polyplug_so: PathBuf = target_dir.join(profile).join(polyplug_lib_filename);
    if polyplug_so.exists() {
        println!("cargo:rustc-env=POLYPLUG_SO={}", polyplug_so.display());
    } else {
        println!("cargo:rustc-env=POLYPLUG_SO=");
    }

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

    // TEST_PLUGIN_DIR — directory containing manifest.toml + .so for the native test plugin
    let test_plugin_dir: PathBuf = fixtures_dir.join("test_plugin_dir");
    println!(
        "cargo:rustc-env=TEST_PLUGIN_DIR={}",
        test_plugin_dir.display()
    );

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

    // TEST_PYTHON_PLUGIN — Python plugin bundle directory
    let python_available: bool = Command::new("python3")
        .arg("--version")
        .output()
        .map(|o: std::process::Output| o.status.success())
        .unwrap_or(false);
    if python_available {
        println!(
            "cargo:rustc-env=TEST_PYTHON_PLUGIN={}",
            fixtures_dir.join("test_plugin_python").display()
        );
    } else {
        println!("cargo:rustc-env=TEST_PYTHON_PLUGIN=PYTHON_NOT_AVAILABLE");
    }

    // TEST_LUA_PLUGIN — Lua plugin bundle directory (mlua vendored — always available)
    println!(
        "cargo:rustc-env=TEST_LUA_PLUGIN={}",
        fixtures_dir.join("test_plugin_lua").display()
    );

    // TEST_JS_PLUGIN — QuickJS plugin bundle directory
    println!(
        "cargo:rustc-env=TEST_JS_PLUGIN={}",
        fixtures_dir.join("test_plugin_js").display()
    );

    // RELOAD_PLUGIN_V1_DIR, RELOAD_PLUGIN_V2_DIR — reload test plugin directories
    println!(
        "cargo:rustc-env=RELOAD_PLUGIN_V1_DIR={}",
        fixtures_dir.join("reload_plugin_v1").display()
    );
    println!(
        "cargo:rustc-env=RELOAD_PLUGIN_V2_DIR={}",
        fixtures_dir.join("reload_plugin_v2").display()
    );

    // RELOAD_PLUGIN_V1_SO, RELOAD_PLUGIN_V2_SO — reload test plugin shared libraries
    let reload_so_filename: &str = if cfg!(target_os = "macos") {
        "libreload_plugin_v1.dylib"
    } else {
        "libreload_plugin_v1.so"
    };
    let reload_v1_so: PathBuf = fixtures_dir
        .join("reload_plugin_v1")
        .join(reload_so_filename);
    if reload_v1_so.exists() {
        println!(
            "cargo:rustc-env=RELOAD_PLUGIN_V1_SO={}",
            reload_v1_so.display()
        );
    } else {
        println!("cargo:rustc-env=RELOAD_PLUGIN_V1_SO=");
    }

    let reload_v2_so_filename: &str = if cfg!(target_os = "macos") {
        "libreload_plugin_v2.dylib"
    } else {
        "libreload_plugin_v2.so"
    };
    let reload_v2_so: PathBuf = fixtures_dir
        .join("reload_plugin_v2")
        .join(reload_v2_so_filename);
    if reload_v2_so.exists() {
        println!(
            "cargo:rustc-env=RELOAD_PLUGIN_V2_SO={}",
            reload_v2_so.display()
        );
    } else {
        println!("cargo:rustc-env=RELOAD_PLUGIN_V2_SO=");
    }

    // DEPENDER_PLUGIN_DIR — depender test plugin directory
    println!(
        "cargo:rustc-env=DEPENDER_PLUGIN_DIR={}",
        fixtures_dir.join("depender_plugin").display()
    );

    // ─── Host contract test fixtures (from examples/host_contracts/logger/) ─────
    // These are built by the examples build script, we just locate them here.

    let host_contracts_dir: PathBuf = workspace_root
        .join("examples")
        .join("host_contracts")
        .join("logger")
        .join("plugins");

    // HOST_CONTRACT_RUST_PLUGIN — Rust worker plugin with host contract
    let rust_worker_dir: PathBuf = host_contracts_dir.join("rust_worker");
    if rust_worker_dir.join("manifest.toml").exists() {
        println!(
            "cargo:rustc-env=HOST_CONTRACT_RUST_PLUGIN={}",
            rust_worker_dir.display()
        );
    } else {
        println!("cargo:rustc-env=HOST_CONTRACT_RUST_PLUGIN=");
    }

    // HOST_CONTRACT_CPP_PLUGIN — C++ worker plugin with host contract
    let cpp_worker_dir: PathBuf = host_contracts_dir.join("cpp_worker");
    if cpp_worker_dir.join("manifest.toml").exists() {
        println!(
            "cargo:rustc-env=HOST_CONTRACT_CPP_PLUGIN={}",
            cpp_worker_dir.display()
        );
    } else {
        println!("cargo:rustc-env=HOST_CONTRACT_CPP_PLUGIN=");
    }

    // HOST_CONTRACT_CSHARP_PLUGIN — C# worker plugin with host contract
    let csharp_worker_dir: PathBuf = host_contracts_dir.join("csharp_worker");
    let csharp_worker_available: bool = csharp_worker_dir.join("manifest.toml").exists()
        && csharp_worker_dir.join("CsharpWorker.dll").exists()
        && Command::new("dotnet")
            .arg("--version")
            .output()
            .map(|o: std::process::Output| o.status.success())
            .unwrap_or(false);
    if csharp_worker_available {
        println!(
            "cargo:rustc-env=HOST_CONTRACT_CSHARP_PLUGIN={}",
            csharp_worker_dir.display()
        );
    } else {
        println!("cargo:rustc-env=HOST_CONTRACT_CSHARP_PLUGIN=DOTNET_NOT_AVAILABLE");
    }

    // HOST_CONTRACT_PYTHON_PLUGIN — Python worker plugin with host contract
    let python_worker_dir: PathBuf = host_contracts_dir.join("python_worker");
    let python_worker_available: bool = python_worker_dir.join("manifest.toml").exists()
        && python_worker_dir.join("plugin.py").exists()
        && Command::new("python3")
            .arg("--version")
            .output()
            .map(|o: std::process::Output| o.status.success())
            .unwrap_or(false);
    if python_worker_available {
        println!(
            "cargo:rustc-env=HOST_CONTRACT_PYTHON_PLUGIN={}",
            python_worker_dir.display()
        );
    } else {
        println!("cargo:rustc-env=HOST_CONTRACT_PYTHON_PLUGIN=PYTHON_NOT_AVAILABLE");
    }

    // HOST_CONTRACT_LUA_PLUGIN — Lua worker plugin with host contract
    let lua_worker_dir: PathBuf = host_contracts_dir.join("lua_worker");
    if lua_worker_dir.join("manifest.toml").exists() {
        println!(
            "cargo:rustc-env=HOST_CONTRACT_LUA_PLUGIN={}",
            lua_worker_dir.display()
        );
    } else {
        println!("cargo:rustc-env=HOST_CONTRACT_LUA_PLUGIN=");
    }

    // HOST_CONTRACT_JS_PLUGIN — JavaScript worker plugin with host contract
    let js_worker_dir: PathBuf = host_contracts_dir.join("js_worker");
    if js_worker_dir.join("manifest.toml").exists() {
        println!(
            "cargo:rustc-env=HOST_CONTRACT_JS_PLUGIN={}",
            js_worker_dir.display()
        );
    } else {
        println!("cargo:rustc-env=HOST_CONTRACT_JS_PLUGIN=");
    }
}

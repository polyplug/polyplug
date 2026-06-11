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

    // POLYPLUG_SO — main polyplug library (required for deno test).
    // The path is deterministic from target dir + profile + platform filename.
    // Always emit the computed path; consumers check existence at runtime.
    // Gating on `.exists()` here is racy: this build script may run before
    // the `polyplug` cdylib is built, which would cache an empty value forever.
    let polyplug_so: PathBuf = target_dir.join(profile).join(polyplug_lib_filename);
    println!("cargo:rustc-env=POLYPLUG_SO={}", polyplug_so.display());

    // Platform-specific shared library filename for test_plugin
    let test_plugin_filename: &str = if cfg!(target_os = "macos") {
        "libtest_plugin.dylib"
    } else if cfg!(target_os = "windows") {
        "test_plugin.dll"
    } else {
        "libtest_plugin.so"
    };

    // TEST_PLUGIN_SO — native Rust test plugin (built by crates/polyplug/build.rs).
    // Path is deterministic; always emit it, consumers check existence at runtime.
    let test_plugin_so: PathBuf = fixtures_dir.join(test_plugin_filename);
    println!(
        "cargo:rustc-env=TEST_PLUGIN_SO={}",
        test_plugin_so.display()
    );

    // TEST_PLUGIN_DIR — directory containing manifest.toml + .so for the native test plugin
    let test_plugin_dir: PathBuf = fixtures_dir.join("test_plugin_dir");
    println!(
        "cargo:rustc-env=TEST_PLUGIN_DIR={}",
        test_plugin_dir.display()
    );

    // TEST_PLUGIN_CPP_SO — C++ test plugin (built by crates/polyplug/build.rs via g++)
    let cpp_so_filename: &str = if cfg!(target_os = "macos") {
        "libtest_plugin_cpp.dylib"
    } else if cfg!(target_os = "windows") {
        "test_plugin_cpp.dll"
    } else {
        "libtest_plugin_cpp.so"
    };
    // Path is deterministic; always emit it. The C++ plugin is only built when
    // g++ is available, so consumers check existence at runtime and skip cleanly
    // when the artifact is genuinely absent.
    let cpp_so: PathBuf = fixtures_dir.join(cpp_so_filename);
    println!("cargo:rustc-env=TEST_PLUGIN_CPP_SO={}", cpp_so.display());

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

    // TEST_JS_GENERATED_PLUGIN — QuickJS bundle built from polyplugc-GENERATED
    // guest glue (non-StringView signatures; see integration_js_generated_guest.rs)
    println!(
        "cargo:rustc-env=TEST_JS_GENERATED_PLUGIN={}",
        fixtures_dir.join("test_plugin_js_generated").display()
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
    } else if cfg!(target_os = "windows") {
        "reload_plugin_v1.dll"
    } else {
        "libreload_plugin_v1.so"
    };
    let reload_v1_so: PathBuf = fixtures_dir
        .join("reload_plugin_v1")
        .join(reload_so_filename);
    // Path is deterministic; always emit it, consumers check existence at runtime.
    println!(
        "cargo:rustc-env=RELOAD_PLUGIN_V1_SO={}",
        reload_v1_so.display()
    );

    let reload_v2_so_filename: &str = if cfg!(target_os = "macos") {
        "libreload_plugin_v2.dylib"
    } else if cfg!(target_os = "windows") {
        "reload_plugin_v2.dll"
    } else {
        "libreload_plugin_v2.so"
    };
    let reload_v2_so: PathBuf = fixtures_dir
        .join("reload_plugin_v2")
        .join(reload_v2_so_filename);
    // Path is deterministic; always emit it, consumers check existence at runtime.
    println!(
        "cargo:rustc-env=RELOAD_PLUGIN_V2_SO={}",
        reload_v2_so.display()
    );

    // DEPENDER_PLUGIN_DIR — depender test plugin directory
    println!(
        "cargo:rustc-env=DEPENDER_PLUGIN_DIR={}",
        fixtures_dir.join("depender_plugin").display()
    );

    // Cross-dispatch fixtures (plugin→plugin via HostApi::call_guest_method).
    // CROSS_CALLER_PLUGIN_DIR    — bundle providing cross.caller@1
    // CROSS_TARGET_PLUGIN_DIR    — bundle providing cross.target@1 (V1)
    // CROSS_TARGET_PLUGIN_V2_DIR — paired reload bundle for cross.target@1 (V2)
    // CROSS_TARGET_PLUGIN_V2_SO  — the V2 cdylib used by reload_bundle()
    println!(
        "cargo:rustc-env=CROSS_CALLER_PLUGIN_DIR={}",
        fixtures_dir.join("cross_caller_plugin").display()
    );
    println!(
        "cargo:rustc-env=CROSS_TARGET_PLUGIN_DIR={}",
        fixtures_dir.join("cross_target_plugin").display()
    );
    println!(
        "cargo:rustc-env=CROSS_TARGET_PLUGIN_V2_DIR={}",
        fixtures_dir.join("cross_target_plugin_v2").display()
    );

    let cross_target_v2_so_filename: &str = if cfg!(target_os = "macos") {
        "libcross_target_plugin_v2.dylib"
    } else if cfg!(target_os = "windows") {
        "cross_target_plugin_v2.dll"
    } else {
        "libcross_target_plugin_v2.so"
    };
    let cross_target_v2_so: PathBuf = fixtures_dir
        .join("cross_target_plugin_v2")
        .join(cross_target_v2_so_filename);
    // Path is deterministic; always emit it, consumers check existence at runtime.
    println!(
        "cargo:rustc-env=CROSS_TARGET_PLUGIN_V2_SO={}",
        cross_target_v2_so.display()
    );
}

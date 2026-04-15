//! Build script for polyplug crate.
//!
//! Sets environment variables for test fixtures located in tests/fixtures/.
//! These are used by integration tests that need compiled plugin binaries.

#![allow(clippy::expect_used)]

use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let manifest_dir: PathBuf =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set"));

    // Workspace root is two levels up from crates/polyplug
    // crates/polyplug -> crates -> polyplug (workspace root)
    let workspace_root: PathBuf = manifest_dir
        .parent()
        .expect("parent of crates/polyplug")
        .parent()
        .expect("parent of crates")
        .to_path_buf();

    let fixtures_dir: PathBuf = workspace_root.join("tests").join("fixtures");

    // Platform-specific shared library filename
    let plugin_ext: &str = if cfg!(target_os = "macos") {
        "dylib"
    } else if cfg!(target_os = "windows") {
        "dll"
    } else {
        "so"
    };

    // TEST_PLUGIN_SO — native Rust test plugin
    let test_plugin_so: PathBuf = fixtures_dir.join(format!("libtest_plugin.{}", plugin_ext));
    println!(
        "cargo:rustc-env=TEST_PLUGIN_SO={}",
        test_plugin_so.display()
    );

    // MEMORY_PLUGIN_SO — memory test plugin
    let memory_plugin_so: PathBuf = fixtures_dir.join(format!("libmemory_plugin.{}", plugin_ext));
    println!(
        "cargo:rustc-env=MEMORY_PLUGIN_SO={}",
        memory_plugin_so.display()
    );

    // ERROR_PLUGIN_SO — error test plugin
    let error_plugin_so: PathBuf = fixtures_dir.join(format!("liberror_plugin.{}", plugin_ext));
    println!(
        "cargo:rustc-env=ERROR_PLUGIN_SO={}",
        error_plugin_so.display()
    );

    // NO_INIT_PLUGIN_SO — plugin without polyplug_init
    let no_init_plugin_so: PathBuf = fixtures_dir.join(format!("libno_init_plugin.{}", plugin_ext));
    println!(
        "cargo:rustc-env=NO_INIT_PLUGIN_SO={}",
        no_init_plugin_so.display()
    );

    // DEPENDER_PLUGIN_SO — dependency test plugin
    let depender_plugin_so: PathBuf =
        fixtures_dir.join(format!("libdepender_plugin.{}", plugin_ext));
    println!(
        "cargo:rustc-env=DEPENDER_PLUGIN_SO={}",
        depender_plugin_so.display()
    );

    // RELOAD_PLUGIN_V1_SO, RELOAD_PLUGIN_V2_SO — reload test plugins
    let reload_v1_so: PathBuf = fixtures_dir.join(format!("libreload_plugin_v1.{}", plugin_ext));
    println!(
        "cargo:rustc-env=RELOAD_PLUGIN_V1_SO={}",
        reload_v1_so.display()
    );
    let reload_v2_so: PathBuf = fixtures_dir.join(format!("libreload_plugin_v2.{}", plugin_ext));
    println!(
        "cargo:rustc-env=RELOAD_PLUGIN_V2_SO={}",
        reload_v2_so.display()
    );

    // TEST_PLUGIN_CPP_SO — C++ test plugin
    let test_plugin_cpp_so: PathBuf =
        fixtures_dir.join(format!("libtest_plugin_cpp.{}", plugin_ext));
    println!(
        "cargo:rustc-env=TEST_PLUGIN_CPP_SO={}",
        test_plugin_cpp_so.display()
    );

    // TEST_PLUGIN_CPP_THROW_SO — C++ throw test plugin
    let test_plugin_cpp_throw_so: PathBuf =
        fixtures_dir.join(format!("libtest_plugin_cpp_throw.{}", plugin_ext));
    println!(
        "cargo:rustc-env=TEST_PLUGIN_CPP_THROW_SO={}",
        test_plugin_cpp_throw_so.display()
    );

    // TEST_PLUGIN_DIR — directory containing manifest.toml + .so for the native test plugin
    let test_plugin_dir: PathBuf = fixtures_dir.join("test_plugin_dir");
    println!(
        "cargo:rustc-env=TEST_PLUGIN_DIR={}",
        test_plugin_dir.display()
    );

    // NO_INIT_PLUGIN_DIR — plugin without polyplug_init directory
    let no_init_plugin_dir: PathBuf = fixtures_dir.join("no_init_plugin");
    println!(
        "cargo:rustc-env=NO_INIT_PLUGIN_DIR={}",
        no_init_plugin_dir.display()
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

    // DEPENDER_PLUGIN_DIR — depender test plugin directory
    println!(
        "cargo:rustc-env=DEPENDER_PLUGIN_DIR={}",
        fixtures_dir.join("depender_plugin").display()
    );

    // Test plugin directories for language runtimes
    println!(
        "cargo:rustc-env=TEST_PYTHON_PLUGIN={}",
        fixtures_dir.join("test_plugin_python").display()
    );
    println!(
        "cargo:rustc-env=TEST_LUA_PLUGIN={}",
        fixtures_dir.join("test_plugin_lua").display()
    );
    println!(
        "cargo:rustc-env=TEST_JS_PLUGIN={}",
        fixtures_dir.join("test_plugin_js").display()
    );

    // polyplugc binary path for codegen tests
    // CARGO_BIN_EXE_polyplugc is only set when running tests from the polyplugc crate
    // So we need to compute the path ourselves
    let target_dir = workspace_root.join("target").join("release");
    let polyplugc_path = target_dir.join("polyplugc");
    println!(
        "cargo:rustc-env=CARGO_BIN_EXE_polyplugc={}",
        polyplugc_path.display()
    );
}

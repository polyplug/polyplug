//! Build script for polyplugc.
//!
//! Emits cargo:rustc-env variables needed by integration tests:
//!   - POLYPLUG_SO       — path to the built libpolyplug shared library
//!   - TEST_PLUGIN_DIR   — path to tests/fixtures/test_plugin_dir/
//!
//! Build scripts are permitted to use `.expect()` and `panic!()` freely.

#![allow(clippy::expect_used)]

use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let manifest_dir: PathBuf =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set"));

    // Workspace root is two levels up from crates/polyplugc/
    let workspace_root: PathBuf = manifest_dir
        .parent()
        .expect("parent of crates/polyplugc")
        .parent()
        .expect("workspace root")
        .to_path_buf();

    let out_dir: PathBuf = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR not set"));

    // Determine profile from OUT_DIR components
    let profile: &str = if out_dir.components().any(|c| c.as_os_str() == "release") {
        "release"
    } else {
        "debug"
    };

    let target_dir: PathBuf = env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| workspace_root.join("target"));

    // Platform-specific shared library name for polyplug
    let polyplug_lib_filename: &str = if cfg!(target_os = "macos") {
        "libpolyplug.dylib"
    } else if cfg!(target_os = "windows") {
        "polyplug.dll"
    } else {
        "libpolyplug.so"
    };

    let polyplug_so: PathBuf = target_dir.join(profile).join(polyplug_lib_filename);

    // Emit path if already built; empty string otherwise (test will skip at runtime)
    if polyplug_so.exists() {
        println!("cargo:rustc-env=POLYPLUG_SO={}", polyplug_so.display());
    } else {
        println!("cargo:rustc-env=POLYPLUG_SO=");
    }

    // TEST_PLUGIN_DIR — created by crates/polyplug/build.rs
    let test_plugin_dir: PathBuf = workspace_root
        .join("tests")
        .join("fixtures")
        .join("test_plugin_dir");

    println!(
        "cargo:rustc-env=TEST_PLUGIN_DIR={}",
        test_plugin_dir.display()
    );
}

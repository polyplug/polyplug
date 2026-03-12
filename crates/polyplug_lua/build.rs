// build.rs for polyplug-lua
// allow expect in build scripts (no better error handling mechanism here)
#![allow(clippy::expect_used)]
fn main() {
    // Emit the guest Lua library directory so that LuaLoader's
    // env!("POLYPLUG_GUEST_LUA_DIR") resolves at compile time.
    // This MUST be in polyplug-lua's own build.rs — cargo:rustc-env only
    // affects the crate that emits it, not downstream crates.
    let manifest_dir: std::path::PathBuf = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // workspace root is two levels up from crates/polyplug-lua/
    let workspace_root: std::path::PathBuf = manifest_dir
        .parent()
        .expect("crates/ parent must exist")
        .parent()
        .expect("workspace root must exist")
        .to_path_buf();
    let guest_lua_dir: std::path::PathBuf = workspace_root.join("guest-libs").join("lua");
    println!(
        "cargo:rustc-env=POLYPLUG_GUEST_LUA_DIR={}",
        guest_lua_dir.display()
    );
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../guest-libs/lua/polyplug_guest.lua");
}

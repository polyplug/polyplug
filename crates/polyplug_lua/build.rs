// build.rs for polyplug_lua
// allow expect in build scripts (no better error handling mechanism here)
#![allow(clippy::expect_used)]

use std::path::PathBuf;
fn main() {
    // Emit the guest Lua library directory so that LuaLoader's
    // env!("POLYPLUG_GUEST_LUA_DIR") resolves at compile time.
    // This MUST be in polyplug_lua's own build.rs — cargo:rustc-env only
    // affects the crate that emits it, not downstream crates.
    let manifest_dir: PathBuf = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // workspace root is two levels up from crates/polyplug_lua/
    let workspace_root: PathBuf = manifest_dir
        .parent()
        .expect("crates/ parent must exist")
        .parent()
        .expect("workspace root must exist")
        .to_path_buf();
    let guest_lua_dir: PathBuf = workspace_root.join("sdks").join("lua").join("guest");
    let abi_lua_dir: PathBuf = workspace_root.join("sdks").join("lua").join("abi");
    println!(
        "cargo:rustc-env=POLYPLUG_GUEST_LUA_DIR={}",
        guest_lua_dir.display()
    );
    println!(
        "cargo:rustc-env=POLYPLUG_ABI_LUA_DIR={}",
        abi_lua_dir.display()
    );
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../sdks/lua/guest/polyplug_guest.lua");
    println!("cargo:rerun-if-changed=../../sdks/lua/abi/polyplug_abi.lua");
}

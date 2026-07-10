//! polyplug_lua: LuaJIT plugin loader for the polyplug runtime.

#[cfg(all(feature = "vendored-luajit", feature = "external-luajit"))]
compile_error!(
    "`vendored-luajit` and `external-luajit` are mutually exclusive; use \
     `--no-default-features --features external-luajit` when linking a target-provided LuaJIT"
);

#[cfg(not(any(feature = "vendored-luajit", feature = "external-luajit")))]
compile_error!("enable exactly one of `vendored-luajit` or `external-luajit`");

pub mod bridge;
pub mod config;
pub mod ffi;
pub mod loader;

pub use bridge::LuaHostBridge;
pub use config::LuaConfig;
pub use loader::LuaLoader;

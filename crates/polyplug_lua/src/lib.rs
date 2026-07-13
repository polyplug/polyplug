//! polyplug_lua: LuaJIT plugin loader for the polyplug runtime.

#[cfg(all(feature = "vendored-luajit", feature = "external-luajit"))]
compile_error!(
    "`vendored-luajit` and `external-luajit` are mutually exclusive; use \
     `--no-default-features --features external-luajit` when linking a target-provided LuaJIT"
);

#[cfg(not(any(feature = "vendored-luajit", feature = "external-luajit")))]
compile_error!("enable exactly one of `vendored-luajit` or `external-luajit`");

pub mod ffi;
pub mod host_bridge;
pub mod loader;

pub use loader::LuaLoader;

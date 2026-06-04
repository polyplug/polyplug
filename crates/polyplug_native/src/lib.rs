//! polyplug_native: Native (shared library) plugin loader for the polyplug runtime.

pub mod config;
pub mod ffi;
pub mod loader;

pub use config::NativeConfig;
pub use loader::NativeLoader;

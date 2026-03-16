//! polyplug — core plugin runtime for the polyplug platform.

pub mod error;
pub mod extensions;
pub mod graph;
pub mod loader;
pub mod registry;
pub mod reload;
pub mod runtime;
pub mod ffi;
pub mod version;

pub use reload::ReloadEvent;

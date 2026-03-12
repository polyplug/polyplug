//! polyplug_js_deno — V8 in-process JS adapter for polyplug.
//!
//! Implements BundleLoader for js-deno plugin bundles.
//! One V8 isolate per bundle, pinned to a dedicated thread.
//! No subprocess. No IPC.

pub mod config;
pub(crate) mod loader;

pub use config::JsDenoConfig;
pub use loader::JsDenoLoader;

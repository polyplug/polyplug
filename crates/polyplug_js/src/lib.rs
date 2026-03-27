//! polyplug_js — QuickJS in-process JS adapter for polyplug.
//!
//! Implements BundleLoader for js-quickjs plugin bundles.
//! One shared QuickJS VM per process. No subprocess. No IPC.

pub mod bridge;
pub mod config;
pub mod ffi;
pub(crate) mod loader;

pub use bridge::JsHostBridge;
pub use config::JsConfig;
pub use loader::JsLoader;

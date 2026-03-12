//! polyplug_js — QuickJS in-process JS adapter for polyplug.
//!
//! Implements BundleLoader for js-quickjs plugin bundles.
//! One shared QuickJS VM per process. No subprocess. No IPC.

pub mod config;
pub(crate) mod loader;

pub use config::JsConfig;
pub use loader::JsLoader;

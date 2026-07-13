//! polyplug_js — QuickJS JavaScript adapter for polyplug.
//!
//! Implements BundleLoader for js-quickjs plugin bundles.
//! Each bundle owns an isolated QuickJS VM. No subprocess. No IPC.

pub mod ffi;
pub(crate) mod loader;

pub use loader::JsLoader;

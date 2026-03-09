//! polyplug-js — JS/TS adapter for the polyplug runtime.
//!
//! Implements `BundleLoader` for `.node` plugin bundles (ts-node/js-node variants)
//! and stubs for ts-bun/js-bun and ts-deno/js-deno.
//!
//! # Usage
//! ```rust,ignore
//! use polyplug::runtime::RuntimeBuilder;
//! use polyplug_js::{JsConfig, JsLoader};
//!
//! let runtime = RuntimeBuilder::new()
//!     .loader(JsLoader::new("ts-node", JsConfig::node_only()))
//!     .loader(JsLoader::new("js-node", JsConfig::node_only()))
//!     .build()?;
//! ```

pub mod config;
pub(crate) mod loader;
pub use config::JsConfig;
pub use loader::JsLoader;

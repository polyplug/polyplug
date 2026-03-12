//! JsConfig — configuration for the QuickJS JS adapter.
//!
//! No fields — QuickJS is fully embedded, no system dependencies.

/// Configuration for the QuickJS JavaScript plugin loader.
///
/// No fields required — QuickJS is embedded in-process via rquickjs.
///
/// # Example
/// ```rust,ignore
/// use polyplug_js::{JsConfig, JsLoader};
/// let loader = JsLoader::new(JsConfig {});
/// ```
#[derive(Debug, Clone)]
pub struct JsConfig {}

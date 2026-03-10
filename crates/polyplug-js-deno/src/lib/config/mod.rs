//! JsDenoConfig — configuration for the V8/deno_core JS adapter.

/// Configuration for the V8/deno_core JavaScript plugin loader.
///
/// No fields required — V8 is embedded in-process via deno_core.
#[derive(Debug, Clone)]
pub struct JsDenoConfig {}

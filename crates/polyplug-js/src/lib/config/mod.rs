//! JsConfig — configuration for the JS/TS plugin loader.

use std::path::PathBuf;

/// Configuration for the JavaScript and TypeScript plugin loader.
///
/// At least one of `node`, `bun`, or `deno` must be `Some`.
/// Use `JsConfig::node_only()` for a quick node-only setup.
///
/// # Example
/// ```rust,ignore
/// use polyplug_js::JsConfig;
/// let config = JsConfig::node_only();
/// ```
#[derive(Debug, Clone)]
pub struct JsConfig {
    /// Configuration for the Node.js runtime. `None` = node loading disabled.
    pub node: Option<NodeConfig>,
    /// Configuration for the Bun runtime (stub in Epic 10). `None` = bun disabled.
    pub bun: Option<BunConfig>,
    /// Configuration for the Deno runtime (stub in Epic 10). `None` = deno disabled.
    pub deno: Option<DenoConfig>,
}

impl JsConfig {
    /// Create a node-only config with auto-discovered `node` binary.
    pub fn node_only() -> JsConfig {
        JsConfig {
            node: Some(NodeConfig { bin: None }),
            bun: None,
            deno: None,
        }
    }
}

/// Configuration for the Node.js sub-loader.
#[derive(Debug, Clone)]
pub struct NodeConfig {
    /// Path to the `node` binary. `None` = search PATH.
    pub bin: Option<PathBuf>,
}

/// Configuration for the Bun sub-loader (stub — not yet implemented).
#[derive(Debug, Clone)]
pub struct BunConfig {
    /// Path to the `bun` binary. `None` = search PATH.
    pub bin: Option<PathBuf>,
}

/// Configuration for the Deno sub-loader (stub — not yet implemented).
#[derive(Debug, Clone)]
pub struct DenoConfig {
    /// Path to the `deno` binary. `None` = search PATH.
    pub bin: Option<PathBuf>,
}

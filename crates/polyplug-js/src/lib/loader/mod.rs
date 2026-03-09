//! JsLoader — BundleLoader dispatcher for JS/TS plugin bundles.

pub(crate) mod bun;
pub(crate) mod deno;
pub(crate) mod node;

use std::path::Path;

use polyplug::abi::PluginRegistrar;
use polyplug::error::LoaderError;
use polyplug::error::PolyplugError;
use polyplug::loader::BundleLoader;

use crate::config::JsConfig;

/// Loader for JavaScript and TypeScript plugin bundles.
///
/// Dispatches to sub-loaders based on the `runtime` field in `manifest.toml`:
/// - `"ts-node"` / `"js-node"`: loads compiled `.node` shared library via `libloading`
/// - `"ts-bun"` / `"js-bun"`: stub (returns `RuntimeNotImplemented`)
/// - `"ts-deno"` / `"js-deno"`: stub (returns `RuntimeNotImplemented`)
pub struct JsLoader {
    pub(crate) runtime: &'static str,
    pub(crate) config: JsConfig,
}

impl JsLoader {
    /// Create a new `JsLoader` for the given runtime variant.
    ///
    /// Valid `runtime` values: `"ts-node"`, `"js-node"`, `"ts-bun"`, `"js-bun"`,
    /// `"ts-deno"`, `"js-deno"`.
    pub fn new(runtime: &'static str, config: JsConfig) -> JsLoader {
        JsLoader { runtime, config }
    }
}

impl BundleLoader for JsLoader {
    fn runtime_name(&self) -> &'static str {
        self.runtime
    }

    // runtime_names() is NOT overridden — the BundleLoader default returns vec![self.runtime_name().to_owned()].

    fn load(&self, path: &Path, registrar: &mut PluginRegistrar) -> Result<(), PolyplugError> {
        match self.runtime {
            "ts-node" | "js-node" => {
                let node_cfg: &crate::config::NodeConfig =
                    self.config.node.as_ref().ok_or_else(|| {
                        PolyplugError::Loader(LoaderError::JsBinaryNotConfigured {
                            runtime_name: self.runtime.to_owned(),
                            field_name: "node".to_owned(),
                            install_hint: "Node.js (https://nodejs.org)".to_owned(),
                        })
                    })?;
                node::load(path, registrar, node_cfg)
            }
            "ts-bun" | "js-bun" => bun::load(path, registrar, self.runtime),
            "ts-deno" | "js-deno" => deno::load(path, registrar, self.runtime),
            other => Err(PolyplugError::Loader(LoaderError::JsBinaryNotConfigured {
                runtime_name: other.to_owned(),
                field_name: "node/bun/deno".to_owned(),
                install_hint: "a supported JS runtime".to_owned(),
            })),
        }
    }
}

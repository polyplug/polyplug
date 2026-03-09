//! Node sub-loader for ts-node/js-node bundles.
//!
//! Loads `.node` shared libraries via libloading, resolves `polyplug_init` symbol,
//! and calls it. Full implementation in Task 7.

use std::path::Path;

use polyplug::abi::PluginRegistrar;
use polyplug::error::PolyplugError;

use crate::config::NodeConfig;

/// Load a ts-node/js-node `.node` bundle.
///
/// The `.node` file is a compiled C ABI shared library that exports `polyplug_init`.
/// It is loaded in-process via `libloading` — no Node.js subprocess is spawned.
pub(crate) fn load(
    path: &Path,
    registrar: &mut PluginRegistrar,
    _config: &NodeConfig,
) -> Result<(), PolyplugError> {
    // TODO(T7): implement full node .node dlopen loading
    let _ = (path, registrar);
    Err(PolyplugError::Loader(
        polyplug::error::LoaderError::RuntimeNotImplemented {
            runtime_name: "ts-node/js-node (T7 stub)".to_owned(),
        },
    ))
}

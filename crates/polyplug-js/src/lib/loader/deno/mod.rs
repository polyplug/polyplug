//! Deno sub-loader for ts-deno/js-deno bundles.
//!
//! STUB — Deno.dlopen-based loading is not yet implemented.
//! Returns `RuntimeNotImplemented` for all load attempts.

use std::path::Path;

use polyplug::abi::PluginRegistrar;
use polyplug::error::LoaderError;
use polyplug::error::PolyplugError;

/// Load a ts-deno/js-deno bundle (stub — not implemented).
pub(crate) fn load(
    _path: &Path,
    _registrar: &mut PluginRegistrar,
    runtime_name: &'static str,
) -> Result<(), PolyplugError> {
    Err(PolyplugError::Loader(LoaderError::RuntimeNotImplemented {
        runtime_name: runtime_name.to_owned(),
    }))
}

//! Bun sub-loader for ts-bun/js-bun bundles.
//!
//! STUB — bun:ffi-based loading is not yet implemented.
//! Returns `RuntimeNotImplemented` for all load attempts.

use std::path::Path;

use polyplug::abi::PluginRegistrar;
use polyplug::error::LoaderError;
use polyplug::error::PolyplugError;

/// Load a ts-bun/js-bun bundle (stub — not implemented).
pub(crate) fn load(
    _path: &Path,
    _registrar: &mut PluginRegistrar,
    runtime_name: &'static str,
) -> Result<(), PolyplugError> {
    Err(PolyplugError::Loader(LoaderError::RuntimeNotImplemented {
        runtime_name: runtime_name.to_owned(),
    }))
}

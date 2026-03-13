//! Native plugin loader stub — full loading logic to be added in a follow-up task.

use std::path::Path;

use crate::config::NativeConfig;
use polyplug::abi::PluginRegistrar;
use polyplug::error::LoaderError;
use polyplug::error::PolyplugError;
use polyplug::loader::BundleLoader;

/// Native plugin loader — loads compiled native bundles (.so / .dll / .dylib).
///
/// Full dlopen-based loading logic is implemented in a follow-up task.
/// This stub exists to satisfy the `BundleLoader` trait for registration purposes.
pub struct NativeLoader {
    /// Configuration for this loader instance.
    pub config: NativeConfig,
}

impl NativeLoader {
    /// Create a new `NativeLoader` with the given configuration.
    pub fn new(config: NativeConfig) -> NativeLoader {
        NativeLoader { config }
    }
}

impl BundleLoader for NativeLoader {
    fn runtime_name(&self) -> &'static str {
        "native"
    }

    fn load(&self, path: &Path, _registrar: &mut PluginRegistrar) -> Result<(), PolyplugError> {
        Err(PolyplugError::Loader(LoaderError::InitFailed {
            bundle: path.to_string_lossy().into_owned(),
            error: "native loader not yet implemented".to_owned(),
        }))
    }
}

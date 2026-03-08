//! polyplug-python — CPython adapter for the polyplug runtime.
//!
//! This crate teaches `polyplug` how to load Python plugin bundles
//! via CPython embedding.
//!
//! # Status
//! **Stub** — this crate is scaffolded infrastructure. The actual Python loading
//! logic is implemented in Epic 10. All `load()` calls currently return
//! `Err(LoaderError::RuntimeNotImplemented { runtime_name: "python" })`.
//!
//! # Usage
//! ```rust,ignore
//! use polyplug::runtime::RuntimeBuilder;
//! use polyplug_python::PythonLoader;
//!
//! let runtime = RuntimeBuilder::new()
//!     .loader(PythonLoader::new())
//!     .build()?;
//! ```

use std::path::Path;

use polyplug::abi::PluginRegistrar;
use polyplug::error::LoaderError;
use polyplug::error::PolyplugError;
use polyplug::loader::BundleLoader;

/// Stub loader for Python plugin bundles.
///
/// Returns `Err(LoaderError::RuntimeNotImplemented { runtime_name: "python" })`
/// until Epic 10 implements CPython embedding.
pub struct PythonLoader;

impl PythonLoader {
    /// Create a new `PythonLoader`.
    pub fn new() -> PythonLoader {
        PythonLoader
    }
}

impl Default for PythonLoader {
    fn default() -> PythonLoader {
        PythonLoader::new()
    }
}

impl BundleLoader for PythonLoader {
    fn runtime_name(&self) -> &'static str {
        "python"
    }

    fn load(&self, _path: &Path, _registrar: &mut PluginRegistrar) -> Result<(), PolyplugError> {
        Err(PolyplugError::Loader(LoaderError::RuntimeNotImplemented {
            runtime_name: "python".to_owned(),
        }))
    }
}

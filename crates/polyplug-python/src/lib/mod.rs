//! polyplug-python — CPython adapter for the polyplug runtime.
//!
//! Implements `BundleLoader` for `.py` plugin bundles using pyo3 0.28
//! CPython embedding. The interpreter is initialized exactly once per process.
//!
//! # Usage
//! ```rust,ignore
//! use polyplug::runtime::RuntimeBuilder;
//! use polyplug_python::{PythonConfig, PythonLoader};
//!
//! let runtime = RuntimeBuilder::new()
//!     .loader(PythonLoader::new(PythonConfig::default()))
//!     .build()?;
//! ```

pub mod config;
pub(crate) mod context;
pub use config::PythonConfig;

use std::path::Path;

use pyo3::Python;
use pyo3::types::PyAnyMethods;
use pyo3::types::PyModule;

use polyplug::abi::PluginRegistrar;
use polyplug::error::LoaderError;
use polyplug::error::PolyplugError;
use polyplug::loader::BundleLoader;

use crate::context::ensure_python_initialized;

/// Loader for Python plugin bundles (`.py` files).
///
/// Uses CPython embedding via pyo3 0.28. The interpreter is initialized
/// exactly once per process via a `std::sync::OnceLock`.
pub struct PythonLoader {
    config: PythonConfig,
}

impl PythonLoader {
    /// Create a new `PythonLoader` with the given configuration.
    pub fn new(config: PythonConfig) -> PythonLoader {
        PythonLoader { config }
    }
}

impl Default for PythonLoader {
    fn default() -> PythonLoader {
        PythonLoader::new(PythonConfig::default())
    }
}

impl BundleLoader for PythonLoader {
    fn runtime_name(&self) -> &'static str {
        "python"
    }

    fn load(&self, path: &Path, registrar: &mut PluginRegistrar) -> Result<(), PolyplugError> {
        // Step 1: Initialize (or verify already initialized) CPython.
        ensure_python_initialized(&self.config)?;

        // Step 2: Canonicalize the path.
        let abs_path: std::path::PathBuf = path.canonicalize().map_err(|_| {
            PolyplugError::Loader(LoaderError::PythonModuleImportFailed {
                path: path.to_string_lossy().into_owned(),
                reason: "path does not exist or is not accessible".to_owned(),
            })
        })?;

        let bundle_name: String = abs_path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();

        // Step 3: Load the Python module and call polyplug_init.
        Python::attach(|py| {
            // Step 3a: Import via importlib (no sys.path mutation).
            let importlib_util: pyo3::Bound<'_, PyModule> = PyModule::import(py, "importlib.util")
                .map_err(|e| {
                    PolyplugError::Loader(LoaderError::PythonModuleImportFailed {
                        path: abs_path.to_string_lossy().into_owned(),
                        reason: e.to_string(),
                    })
                })?;

            let spec: pyo3::Bound<'_, pyo3::PyAny> = importlib_util
                .getattr("spec_from_file_location")
                .map_err(|e| {
                    PolyplugError::Loader(LoaderError::PythonInitFailed {
                        reason: format!("spec_from_file_location not found: {e}"),
                    })
                })?
                .call1((&bundle_name, abs_path.to_string_lossy().as_ref()))
                .map_err(|e| {
                    PolyplugError::Loader(LoaderError::PythonModuleImportFailed {
                        path: abs_path.to_string_lossy().into_owned(),
                        reason: e.to_string(),
                    })
                })?;

            let module_from_spec: pyo3::Bound<'_, pyo3::PyAny> = importlib_util
                .getattr("module_from_spec")
                .map_err(|e| {
                    PolyplugError::Loader(LoaderError::PythonInitFailed {
                        reason: format!("module_from_spec not found: {e}"),
                    })
                })?
                .call1((&spec,))
                .map_err(|e| {
                    PolyplugError::Loader(LoaderError::PythonModuleImportFailed {
                        path: abs_path.to_string_lossy().into_owned(),
                        reason: e.to_string(),
                    })
                })?;

            // Step 3b: Execute the module.
            spec.getattr("loader")
                .and_then(|loader: pyo3::Bound<'_, pyo3::PyAny>| loader.getattr("exec_module"))
                .and_then(|exec_module: pyo3::Bound<'_, pyo3::PyAny>| {
                    exec_module.call1((&module_from_spec,))
                })
                .map_err(|e| {
                    PolyplugError::Loader(LoaderError::PythonModuleImportFailed {
                        path: abs_path.to_string_lossy().into_owned(),
                        reason: format!("exec_module failed: {e}"),
                    })
                })?;

            // Step 3c: Locate and call polyplug_init(registrar_ptr).
            let init_fn: pyo3::Bound<'_, pyo3::PyAny> =
                module_from_spec.getattr("polyplug_init").map_err(|_| {
                    PolyplugError::Loader(LoaderError::InitSymbolMissing {
                        bundle: bundle_name.clone(),
                    })
                })?;

            // SAFETY: registrar is a valid non-null mutable reference per BundleLoader contract.
            // We cast it to a raw pointer integer to pass across the Python/Rust boundary.
            // The Python plugin will cast it back to the correct ctypes pointer type.
            // The registrar lifetime extends for the duration of this call.
            let registrar_addr: usize = registrar as *mut PluginRegistrar as usize;

            init_fn.call1((registrar_addr,)).map_err(|e| {
                PolyplugError::Loader(LoaderError::PythonInitRaisedException {
                    bundle: bundle_name.clone(),
                    message: e.to_string(),
                })
            })?;

            Ok(())
        })
    }
}

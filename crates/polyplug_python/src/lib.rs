//! polyplug_python — CPython adapter for the polyplug runtime.
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
pub mod ffi;
pub use config::PythonConfig;

use std::path::Path;

use pyo3::Python;
use pyo3::types::PyAnyMethods;
use pyo3::types::PyModule;

use polyplug::error::LoaderError;
use polyplug::error::PolyplugError;
use polyplug::loader::BundleLoader;
use polyplug::loader::manifest::ManifestData;
use polyplug::runtime::HostContext;
use polyplug::runtime::Runtime;
use polyplug_abi::HostVTable;
use polyplug_abi::PluginContext;
use polyplug_abi::StringView;

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

    fn load(&self, manifest: &ManifestData, runtime: &Runtime) -> Result<(), PolyplugError> {
        let bundle_path: std::path::PathBuf = if !manifest.file.is_empty() {
            manifest.path.join(&manifest.file)
        } else {
            return Err(PolyplugError::Loader(LoaderError::ManifestMissingFile {
                bundle: manifest.name.clone(),
            }));
        };

        if !bundle_path.exists() {
            return Err(PolyplugError::Loader(
                LoaderError::PythonModuleImportFailed {
                    path: bundle_path.to_string_lossy().into_owned(),
                    reason: "file does not exist".to_owned(),
                },
            ));
        }

        ensure_python_initialized(&self.config)?;

        let abs_path: std::path::PathBuf = bundle_path.canonicalize().map_err(|_| {
            PolyplugError::Loader(LoaderError::PythonModuleImportFailed {
                path: bundle_path.to_string_lossy().into_owned(),
                reason: "path does not exist or is not accessible".to_owned(),
            })
        })?;

        let bundle_name: String = abs_path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();

        let bundle_dir: &Path = &manifest.path;
        let bundle_dir_str: String = bundle_dir.to_string_lossy().into_owned();
        let bundle_id: u64 = manifest.id;

        // Create HostContext for rt_ctx parameter.
        let host_ctx: HostContext = HostContext {
            runtime: runtime as *const Runtime as *mut Runtime,
            bundle_id,
        };
        let rt_ctx: *mut core::ffi::c_void =
            &host_ctx as *const HostContext as *mut core::ffi::c_void;

        // Get host_vtable from runtime.
        let host_vtable: &'static HostVTable = runtime.host_vtable();

        // Step 3: Load the Python module and call polyplug_init.
        Python::attach(|py| {
            // Step 3a: Prepend bundle directory (and site-packages) to sys.path.
            let sys_mod: pyo3::Bound<'_, PyModule> =
                PyModule::import(py, "sys").map_err(|e: pyo3::PyErr| {
                    PolyplugError::Loader(LoaderError::PythonInitRaisedException {
                        bundle: bundle_name.clone(),
                        message: e.to_string(),
                    })
                })?;
            let sys_path: pyo3::Bound<'_, pyo3::PyAny> =
                sys_mod.getattr("path").map_err(|e: pyo3::PyErr| {
                    PolyplugError::Loader(LoaderError::PythonInitRaisedException {
                        bundle: bundle_name.clone(),
                        message: e.to_string(),
                    })
                })?;
            sys_path
                .call_method1("insert", (0usize, bundle_dir_str.as_str()))
                .map_err(|e: pyo3::PyErr| {
                    PolyplugError::Loader(LoaderError::PythonInitRaisedException {
                        bundle: bundle_name.clone(),
                        message: e.to_string(),
                    })
                })?;
            let site_pkgs: std::path::PathBuf = bundle_dir.join("site-packages");
            if site_pkgs.exists() {
                let sp: String = site_pkgs.to_string_lossy().into_owned();
                sys_path
                    .call_method1("insert", (0usize, sp.as_str()))
                    .map_err(|e: pyo3::PyErr| {
                        PolyplugError::Loader(LoaderError::PythonInitRaisedException {
                            bundle: bundle_name.clone(),
                            message: e.to_string(),
                        })
                    })?;
            }
            // Step 3b: Import via importlib (no further sys.path mutation).
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

            // Step 3c: Locate and call polyplug_init(rt_ctx, host_vtable, ctx).
            let init_fn: pyo3::Bound<'_, pyo3::PyAny> =
                module_from_spec.getattr("polyplug_init").map_err(|_| {
                    PolyplugError::Loader(LoaderError::InitSymbolMissing {
                        bundle: bundle_name.clone(),
                    })
                })?;

            // NOTE: Intentionally leaked; bundle_path_static outlives this call.
            let bundle_path_static: &'static str =
                Box::leak(bundle_dir_str.clone().into_boxed_str());
            let ctx: PluginContext = PluginContext {
                bundle_path: StringView {
                    ptr: bundle_path_static.as_ptr(),
                    len: bundle_path_static.len(),
                },
                host_abi_version: polyplug_abi::POLYPLUG_ABI_VERSION,
                bundle_id,
            };

            // Pass pointers as i64 to preserve full 64-bit precision.
            let rt_ctx_i64: i64 = rt_ctx as usize as i64;
            let host_vtable_i64: i64 = host_vtable as *const HostVTable as usize as i64;
            let ctx_ptr: i64 = &ctx as *const PluginContext as i64;
            init_fn
                .call((rt_ctx_i64, host_vtable_i64, ctx_ptr), None)
                .map_err(|e: pyo3::PyErr| {
                    PolyplugError::Loader(LoaderError::PythonInitRaisedException {
                        bundle: bundle_name.clone(),
                        message: e.to_string(),
                    })
                })?;

            Ok(())
        })
    }
}

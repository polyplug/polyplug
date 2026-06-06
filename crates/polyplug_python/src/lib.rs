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

pub mod bridge;
pub mod config;
pub(crate) mod context;
pub mod ffi;
pub(crate) mod isolation;
pub use bridge::PythonHostBridge;
pub use config::PythonConfig;

use std::ffi::CString;
use std::path::Path;

use pyo3::Python;
use pyo3::types::PyAnyMethods;
use pyo3::types::PyModule;

use polyplug::error::LoaderError;
use polyplug::error::RuntimeError;
use polyplug::loader::BundleLoader;
use polyplug::loader::BundleSource;
use polyplug::loader::ManifestData;
use polyplug::runtime::Runtime;
use polyplug_abi::BundleInitContext;
use polyplug_abi::HostApi;
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

/// Derive a valid Python module identifier from a bundle name.
///
/// The bundle name is free-form (it may contain dots, dashes, or other
/// characters that are illegal in a Python module name), so every non
/// `[A-Za-z0-9_]` character is replaced with `_` and a leading digit is
/// prefixed with `_`. The result is used as the synthetic module name for an
/// in-memory (`Code`/`Bytes`) load, where there is no file path to derive a
/// module name from.
fn synthetic_module_name(bundle_name: &str) -> String {
    let mut name: String = String::with_capacity(bundle_name.len() + 1);
    for ch in bundle_name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            name.push(ch);
        } else {
            name.push('_');
        }
    }
    if name.is_empty() {
        name.push_str("polyplug_inline_bundle");
    } else if name
        .chars()
        .next()
        .is_some_and(|c: char| c.is_ascii_digit())
    {
        name.insert(0, '_');
    }
    name
}

impl PythonLoader {
    /// Load a Python plugin from in-memory source text (`Code` / `Bytes`).
    ///
    /// This mirrors the on-disk [`BundleLoader::load`] flow — execute the module,
    /// locate `polyplug_init`, call it with the self-passing `HostApi` pointer and
    /// the `BundleInitContext`, then isolate the bundle's freshly imported modules
    /// — but with two differences dictated by the absence of a bundle directory:
    ///
    /// - **No `sys.path` / `site-packages` provisioning.** In-memory sources are
    ///   single-file only (see [`BundleSource`]); there is no directory to make
    ///   importable. A plugin loaded this way may only `import` modules already
    ///   reachable on the interpreter's `sys.path` (the standard library and any
    ///   process-level site-packages) — for example `ctypes`, which is all a
    ///   self-contained guest needs to reach the ABI. It cannot `import` a
    ///   bundle-vendored generated SDK package.
    /// - **The module is compiled from source text** via `PyModule::from_code`
    ///   under a synthetic module name derived from `manifest.name`, instead of
    ///   being imported from a `.py` file.
    ///
    /// The module-isolation pass runs with an empty bundle directory: with no
    /// directory, no freshly imported module can be classified as "under the
    /// bundle", so the pass is a deliberate no-op here. That is correct for a
    /// single-file source — it imports no bundle-local package tree to isolate.
    ///
    /// [`BundleSource`]: polyplug::loader::BundleSource
    fn load_from_source_text(
        &self,
        manifest: &ManifestData,
        code: &str,
        runtime: &Runtime,
    ) -> Result<(), RuntimeError> {
        ensure_python_initialized(&self.config)?;

        let bundle_name: String = synthetic_module_name(&manifest.name);
        let bundle_id: u64 = manifest.id;

        let code_c: CString = CString::new(code).map_err(|e: std::ffi::NulError| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name.clone(),
                error: format!("Python source contained an interior nul byte: {}", e),
            })
        })?;
        let file_name_c: CString =
            CString::new(format!("{}.py", bundle_name)).map_err(|e: std::ffi::NulError| {
                RuntimeError::Loader(LoaderError::InitFailed {
                    bundle: bundle_name.clone(),
                    error: format!("synthetic file name contained an interior nul byte: {}", e),
                })
            })?;
        let module_name_c: CString =
            CString::new(bundle_name.as_str()).map_err(|e: std::ffi::NulError| {
                RuntimeError::Loader(LoaderError::InitFailed {
                    bundle: bundle_name.clone(),
                    error: format!(
                        "synthetic module name contained an interior nul byte: {}",
                        e
                    ),
                })
            })?;

        // Self-passing pattern: the interface already carries the runtime pointer.
        let host_interface: *const HostApi = runtime.as_context_ptr();

        // Per-thread init stack for dependency enforcement during init
        // (instance-owned; Rule 12 — no thread-locals).
        runtime.push_init_bundle_id(bundle_id);

        // Serialize the snapshot→exec→isolate critical section against other
        // concurrent Python loads sharing this process's interpreter (see the
        // path-based load for the full rationale).
        let _load_guard: std::sync::MutexGuard<'_, ()> = crate::context::acquire_load_lock();

        let result: Result<(), RuntimeError> = Python::attach(|py| {
            // Snapshot sys.modules before executing so the isolation pass can scope
            // exactly the modules this bundle introduced.
            let modules_before: std::collections::HashSet<String> =
                crate::isolation::snapshot_loaded_modules(py, &bundle_name)?;

            // Compile and execute the source text as a module.
            let module: pyo3::Bound<'_, PyModule> =
                PyModule::from_code(py, &code_c, &file_name_c, &module_name_c).map_err(
                    |e: pyo3::PyErr| {
                        RuntimeError::Loader(LoaderError::InitFailed {
                            bundle: bundle_name.clone(),
                            error: format!("inline module compile/exec failed: {}", e),
                        })
                    },
                )?;

            // Locate and call polyplug_init(host, ctx) — self-passing pattern.
            let init_fn: pyo3::Bound<'_, pyo3::PyAny> =
                module.getattr("polyplug_init").map_err(|_| {
                    RuntimeError::Loader(LoaderError::InitSymbolMissing {
                        bundle: bundle_name.clone(),
                    })
                })?;

            // The bundle path is empty for in-memory sources (no bundle directory).
            // NOTE: Intentionally leaked; bundle_path_static outlives this call.
            let bundle_path_static: &'static str = Box::leak(String::new().into_boxed_str());
            let ctx: BundleInitContext = BundleInitContext {
                bundle_id,
                bundle_path: StringView {
                    ptr: bundle_path_static.as_ptr(),
                    len: bundle_path_static.len(),
                },
            };

            let host_interface_i64: i64 = host_interface as usize as i64;
            let ctx_ptr: i64 = &ctx as *const BundleInitContext as i64;
            init_fn
                .call((host_interface_i64, ctx_ptr), None)
                .map_err(|e: pyo3::PyErr| {
                    RuntimeError::Loader(LoaderError::InitFailed {
                        bundle: bundle_name.clone(),
                        error: format!("polyplug_init call failed: {}", e),
                    })
                })?;

            // Isolate this bundle's freshly imported modules. With no bundle
            // directory ("") no imported module is classified as in-bundle, so this
            // is a no-op for single-file inline sources — by design.
            crate::isolation::isolate_bundle_modules(
                py,
                &bundle_name,
                bundle_id,
                "",
                &modules_before,
            )?;

            Ok::<(), RuntimeError>(())
        });

        runtime.pop_init_bundle_id();
        result
    }
}

impl BundleLoader for PythonLoader {
    fn runtime_name(&self) -> &'static str {
        "python"
    }

    fn load(
        &self,
        manifest: &ManifestData,
        source: &BundleSource,
        runtime: &Runtime,
    ) -> Result<(), RuntimeError> {
        // `Code` and `Bytes` carry the plugin's Python module source text directly,
        // with no bundle directory. `Bytes` is validated as UTF-8 first; both then
        // flow through the same in-memory exec path. `Path` keeps the original
        // on-disk import flow byte-for-byte.
        match source {
            BundleSource::Path(_) => {}
            BundleSource::Code(code) => {
                return self.load_from_source_text(manifest, code, runtime);
            }
            BundleSource::Bytes(bytes) => {
                let code: &str = core::str::from_utf8(bytes).map_err(|_| {
                    RuntimeError::Loader(LoaderError::InvalidSourceEncoding {
                        loader: "python",
                        source_kind: source.kind(),
                        bundle: manifest.name.clone(),
                    })
                })?;
                return self.load_from_source_text(manifest, code, runtime);
            }
        }

        let bundle_path: std::path::PathBuf = if !manifest.file.is_empty() {
            manifest.path.join(&manifest.file)
        } else {
            return Err(RuntimeError::Loader(LoaderError::ManifestMissingFile {
                bundle: manifest.name.clone(),
            }));
        };

        if !bundle_path.exists() {
            return Err(RuntimeError::Loader(LoaderError::InitFailed {
                bundle: manifest.name.clone(),
                error: format!(
                    "failed to import Python module at {}: file does not exist",
                    bundle_path.to_string_lossy()
                ),
            }));
        }

        ensure_python_initialized(&self.config)?;

        let abs_path: std::path::PathBuf = bundle_path.canonicalize().map_err(|_| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: manifest.name.clone(),
                error: format!(
                    "failed to canonicalize Python module path {}: path does not exist or is not accessible",
                    bundle_path.to_string_lossy()
                ),
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

        // Get HostApi pointer from runtime (self-passing pattern).
        // The interface already has the runtime pointer set internally.
        let host_interface: *const HostApi = runtime.as_context_ptr();

        // Push bundle_id onto the runtime's per-thread init stack for dependency
        // enforcement during init (instance-owned; Rule 12 — no thread-locals).
        runtime.push_init_bundle_id(bundle_id);

        // Serialize the snapshot→exec→isolate critical section against other
        // concurrent Python loads sharing this process's interpreter. CPython
        // releases the GIL during import I/O, so two parallel loads would
        // otherwise interleave their sys.modules mutations and corrupt each
        // other's per-bundle isolation. Held for the whole `Python::attach`.
        let _load_guard: std::sync::MutexGuard<'_, ()> = crate::context::acquire_load_lock();

        // Step 3: Load the Python module and call polyplug_init.
        Python::attach(|py| {
            // Step 3a: Prepend bundle directory (and site-packages) to sys.path.
            let sys_mod: pyo3::Bound<'_, PyModule> =
                PyModule::import(py, "sys").map_err(|e: pyo3::PyErr| {
                    RuntimeError::Loader(LoaderError::InitFailed {
                        bundle: bundle_name.clone(),
                        error: format!("Python sys import failed: {}", e),
                    })
                })?;
            let sys_path: pyo3::Bound<'_, pyo3::PyAny> =
                sys_mod.getattr("path").map_err(|e: pyo3::PyErr| {
                    RuntimeError::Loader(LoaderError::InitFailed {
                        bundle: bundle_name.clone(),
                        error: format!("Python sys.path get failed: {}", e),
                    })
                })?;
            sys_path
                .call_method1("insert", (0usize, bundle_dir_str.as_str()))
                .map_err(|e: pyo3::PyErr| {
                    RuntimeError::Loader(LoaderError::InitFailed {
                        bundle: bundle_name.clone(),
                        error: format!("sys.path insert failed: {}", e),
                    })
                })?;
            let site_pkgs: std::path::PathBuf = bundle_dir.join("site-packages");
            if site_pkgs.exists() {
                let sp: String = site_pkgs.to_string_lossy().into_owned();
                sys_path
                    .call_method1("insert", (0usize, sp.as_str()))
                    .map_err(|e: pyo3::PyErr| {
                        RuntimeError::Loader(LoaderError::InitFailed {
                            bundle: bundle_name.clone(),
                            error: format!("site-packages path insert failed: {}", e),
                        })
                    })?;
            }
            // Step 3b: Import via importlib (no further sys.path mutation).
            let importlib_util: pyo3::Bound<'_, PyModule> = PyModule::import(py, "importlib.util")
                .map_err(|e| {
                    RuntimeError::Loader(LoaderError::InitFailed {
                        bundle: bundle_name.clone(),
                        error: format!("importlib.util import failed: {}", e),
                    })
                })?;

            let spec: pyo3::Bound<'_, pyo3::PyAny> = importlib_util
                .getattr("spec_from_file_location")
                .map_err(|e| {
                    RuntimeError::Loader(LoaderError::InitFailed {
                        bundle: bundle_name.clone(),
                        error: format!("spec_from_file_location not found: {}", e),
                    })
                })?
                .call1((&bundle_name, abs_path.to_string_lossy().as_ref()))
                .map_err(|e| {
                    RuntimeError::Loader(LoaderError::InitFailed {
                        bundle: bundle_name.clone(),
                        error: format!(
                            "spec_from_file_location call failed for {}: {}",
                            abs_path.to_string_lossy(),
                            e
                        ),
                    })
                })?;

            let module_from_spec: pyo3::Bound<'_, pyo3::PyAny> = importlib_util
                .getattr("module_from_spec")
                .map_err(|e| {
                    RuntimeError::Loader(LoaderError::InitFailed {
                        bundle: bundle_name.clone(),
                        error: format!("module_from_spec not found: {}", e),
                    })
                })?
                .call1((&spec,))
                .map_err(|e| {
                    RuntimeError::Loader(LoaderError::InitFailed {
                        bundle: bundle_name.clone(),
                        error: format!(
                            "module_from_spec call failed for {}: {}",
                            abs_path.to_string_lossy(),
                            e
                        ),
                    })
                })?;

            // Snapshot sys.modules before executing the bundle so that, after
            // init, exactly the modules this bundle introduced can be isolated.
            let modules_before: std::collections::HashSet<String> =
                crate::isolation::snapshot_loaded_modules(py, &bundle_name)?;

            // Step 3b: Execute the module.
            spec.getattr("loader")
                .and_then(|loader: pyo3::Bound<'_, pyo3::PyAny>| loader.getattr("exec_module"))
                .and_then(|exec_module: pyo3::Bound<'_, pyo3::PyAny>| {
                    exec_module.call1((&module_from_spec,))
                })
                .map_err(|e| {
                    RuntimeError::Loader(LoaderError::InitFailed {
                        bundle: bundle_name.clone(),
                        error: format!(
                            "exec_module failed for {}: {}",
                            abs_path.to_string_lossy(),
                            e
                        ),
                    })
                })?;

            // Step 3c: Locate and call polyplug_init(host, ctx).
            // New signature: polyplug_init(host_interface, ctx) - self-passing pattern.
            let init_fn: pyo3::Bound<'_, pyo3::PyAny> =
                module_from_spec.getattr("polyplug_init").map_err(|_| {
                    RuntimeError::Loader(LoaderError::InitSymbolMissing {
                        bundle: bundle_name.clone(),
                    })
                })?;

            // NOTE: Intentionally leaked; bundle_path_static outlives this call.
            let bundle_path_static: &'static str =
                Box::leak(bundle_dir_str.clone().into_boxed_str());
            let ctx: BundleInitContext = BundleInitContext {
                bundle_id,
                bundle_path: StringView {
                    ptr: bundle_path_static.as_ptr(),
                    len: bundle_path_static.len(),
                },
            };

            // Pass HostApi pointer and BundleInitContext pointer to Python.
            // The HostApi uses self-passing pattern - Python guest code will pass it back
            // as the first parameter to each HostApi function call.
            let host_interface_i64: i64 = host_interface as usize as i64;
            let ctx_ptr: i64 = &ctx as *const BundleInitContext as i64;
            init_fn
                .call((host_interface_i64, ctx_ptr), None)
                .map_err(|e: pyo3::PyErr| {
                    RuntimeError::Loader(LoaderError::InitFailed {
                        bundle: bundle_name.clone(),
                        error: format!("polyplug_init call failed: {}", e),
                    })
                })?;

            // Isolate this bundle's freshly imported modules under a unique
            // per-bundle prefix so the next bundle imports its own generated
            // package instead of this one's cached copy. Re-keying keeps the
            // native dispatch CFUNCTYPE trampolines alive for the runtime's
            // lifetime while freeing the generic module names.
            crate::isolation::isolate_bundle_modules(
                py,
                &bundle_name,
                bundle_id,
                &bundle_dir_str,
                &modules_before,
            )?;

            Ok::<(), RuntimeError>(())
        })?;

        // Pop bundle_id from the runtime's per-thread init stack after init completes.
        runtime.pop_init_bundle_id();

        Ok(())
    }

    fn reload(
        &self,
        _manifest: &ManifestData,
        _runtime: &Runtime,
    ) -> Result<(), polyplug::error::RuntimeError> {
        Err(polyplug::error::RuntimeError::HotReloadDisabled)
    }
}

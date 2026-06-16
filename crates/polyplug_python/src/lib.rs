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
pub(crate) mod isolation;
pub mod loader;
pub use config::PythonConfig;
pub use loader::PythonLoaderData;

use std::collections::HashMap;
use std::ffi::CString;
use std::path::Path;
use std::sync::Mutex;
use std::sync::PoisonError;

use pyo3::Bound;
use pyo3::PyAny;
use pyo3::Python;
use pyo3::types::PyAnyMethods;
use pyo3::types::PyDict;
use pyo3::types::PyDictMethods;
use pyo3::types::PyModule;

use polyplug::error::LoaderError;
use polyplug::loader::BundleLoader;
use polyplug::loader::BundleSource;
use polyplug::loader::ManifestData;
use polyplug::runtime::Runtime;
use polyplug_abi::BundleInitContext;
use polyplug_abi::HostApi;
use polyplug_abi::StringView;
use polyplug_abi::SupportedLanguage;
use polyplug_utils::BundleId;

use crate::context::ensure_python_initialized;
use crate::loader::ContractRegistration;

/// Loader for Python plugin bundles (`.py` files).
///
/// Uses CPython embedding via pyo3 0.28. The interpreter is initialized
/// exactly once per process via a `std::sync::OnceLock`.
pub struct PythonLoader {
    config: PythonConfig,
    /// Per-bundle list of the `sys.modules` re-key prefixes produced by each load
    /// of that bundle (see [`isolation::isolate_bundle_modules`]).
    ///
    /// A bundle may be loaded more than once (reload re-runs `load`), each load
    /// minting a fresh prefix via the process-global nonce, so the prefixes
    /// accumulate in a `Vec`. On `unload` the loader drains this entry and purges
    /// every matching `sys.modules` key so a later load re-imports the fresh source
    /// instead of a cached module.
    module_prefixes: Mutex<HashMap<BundleId, Vec<String>>>,
}

impl PythonLoader {
    /// Create a new `PythonLoader` with the given configuration.
    pub fn new(config: PythonConfig) -> PythonLoader {
        PythonLoader {
            config,
            module_prefixes: Mutex::new(HashMap::new()),
        }
    }

    /// Record the `sys.modules` re-key `prefix` minted for `bundle_id` during a
    /// successful load, so [`PythonLoader::unload`] can purge those entries.
    fn track_module_prefix(&self, bundle_id: u64, prefix: String) {
        let mut map: std::sync::MutexGuard<'_, HashMap<BundleId, Vec<String>>> = self
            .module_prefixes
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        map.entry(BundleId::from_u64(bundle_id))
            .or_default()
            .push(prefix);
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
    /// From the `(registrations, AbiError)` tuple `polyplug_init` returned,
    /// collect the guest's registration data and register every contract with the
    /// runtime via the self-passing `HostApi` (which builds the per-bundle arena
    /// allocator threaded into each dispatch).
    ///
    /// Registrations flow through `polyplug_init`'s return value — nothing is read
    /// from any module namespace — so the split-module generated layout (whose
    /// entry file only imports `polyplug_init`) and the single-file layout register
    /// identically.
    ///
    /// Shared by the on-disk and in-memory load paths.
    fn collect_and_register(
        py: Python<'_>,
        init_ret: &Bound<'_, PyAny>,
        host_interface: *const HostApi,
        bundle_name: &str,
    ) -> Result<(), LoaderError> {
        let registrations: Vec<ContractRegistration> =
            loader::collect_registrations(py, init_ret, bundle_name)?;
        loader::register_contracts(registrations, host_interface, bundle_name)?;
        Ok(())
    }

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
    ) -> Result<(), LoaderError> {
        ensure_python_initialized(&self.config)?;

        let bundle_name: String = synthetic_module_name(&manifest.name);
        let bundle_id: u64 = manifest.id;

        let code_c: CString =
            CString::new(code).map_err(|e: std::ffi::NulError| LoaderError::InitFailed {
                bundle: bundle_name.clone(),
                error: format!("Python source contained an interior nul byte: {}", e),
            })?;
        let file_name_c: CString =
            CString::new(format!("{}.py", bundle_name)).map_err(|e: std::ffi::NulError| {
                LoaderError::InitFailed {
                    bundle: bundle_name.clone(),
                    error: format!("synthetic file name contained an interior nul byte: {}", e),
                }
            })?;
        let module_name_c: CString =
            CString::new(bundle_name.as_str()).map_err(|e: std::ffi::NulError| {
                LoaderError::InitFailed {
                    bundle: bundle_name.clone(),
                    error: format!(
                        "synthetic module name contained an interior nul byte: {}",
                        e
                    ),
                }
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

        let result: Result<String, LoaderError> = Python::attach(|py| {
            // Snapshot sys.modules before executing so the isolation pass can scope
            // exactly the modules this bundle introduced.
            let modules_before: std::collections::HashSet<String> =
                crate::isolation::snapshot_loaded_modules(py, &bundle_name)?;

            // Compile and execute the source text as a module.
            let module: pyo3::Bound<'_, PyModule> =
                PyModule::from_code(py, &code_c, &file_name_c, &module_name_c).map_err(
                    |e: pyo3::PyErr| LoaderError::InitFailed {
                        bundle: bundle_name.clone(),
                        error: format!("inline module compile/exec failed: {}", e),
                    },
                )?;

            // Locate polyplug_init(host, ctx) — self-passing pattern.
            let init_fn: pyo3::Bound<'_, pyo3::PyAny> =
                module
                    .getattr("polyplug_init")
                    .map_err(|_| LoaderError::InitSymbolMissing {
                        bundle: bundle_name.clone(),
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
            // polyplug_init RETURNS its (registrations, AbiError) tuple; the loader
            // reads that return value — nothing is deposited into the namespace.
            let init_ret: Bound<'_, PyAny> = init_fn
                .call((host_interface_i64, ctx_ptr), None)
                .map_err(|e: pyo3::PyErr| LoaderError::InitFailed {
                    bundle: bundle_name.clone(),
                    error: format!("polyplug_init call failed: {}", e),
                })?;

            // Collect the guest's registration data and register every contract
            // with VM dispatch.
            Self::collect_and_register(py, &init_ret, host_interface, &bundle_name)?;

            // Isolate this bundle's freshly imported modules. With no bundle
            // directory ("") no imported module is classified as in-bundle, so this
            // is a no-op for single-file inline sources — by design. The returned
            // prefix is still tracked for unload-purge symmetry, even though no
            // `sys.modules` entry carries it for inline sources.
            let prefix: String = crate::isolation::isolate_bundle_modules(
                py,
                &bundle_name,
                bundle_id,
                "",
                &modules_before,
            )?;

            Ok::<String, LoaderError>(prefix)
        });

        runtime.pop_init_bundle_id();
        let prefix: String = result?;
        self.track_module_prefix(bundle_id, prefix);
        Ok(())
    }
}

impl BundleLoader for PythonLoader {
    fn loader_name(&self) -> &'static str {
        "python"
    }

    fn loader_language(&self) -> SupportedLanguage {
        SupportedLanguage::Python
    }

    fn supports_hot_reload(&self) -> bool {
        false
    }

    fn load(
        &self,
        manifest: &ManifestData,
        source: &BundleSource,
        runtime: &Runtime,
    ) -> Result<(), LoaderError> {
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
                    LoaderError::InvalidSourceEncoding {
                        loader: "python",
                        source_kind: source.kind(),
                        bundle: manifest.name.clone(),
                    }
                })?;
                return self.load_from_source_text(manifest, code, runtime);
            }
        }

        let bundle_path: std::path::PathBuf = if !manifest.file.is_empty() {
            manifest.path.join(&manifest.file)
        } else {
            return Err(LoaderError::ManifestMissingFile {
                bundle: manifest.name.clone(),
            });
        };

        if !bundle_path.exists() {
            return Err(LoaderError::InitFailed {
                bundle: manifest.name.clone(),
                error: format!(
                    "failed to import Python module at {}: file does not exist",
                    bundle_path.to_string_lossy()
                ),
            });
        }

        ensure_python_initialized(&self.config)?;

        let abs_path: std::path::PathBuf = bundle_path.canonicalize().map_err(|_| {
            LoaderError::InitFailed {
                bundle: manifest.name.clone(),
                error: format!(
                    "failed to canonicalize Python module path {}: path does not exist or is not accessible",
                    bundle_path.to_string_lossy()
                ),
            }
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
        let prefix: String = Python::attach(|py| {
            // Step 3a: Prepend bundle directory (and site-packages) to sys.path.
            let sys_mod: pyo3::Bound<'_, PyModule> =
                PyModule::import(py, "sys").map_err(|e: pyo3::PyErr| LoaderError::InitFailed {
                    bundle: bundle_name.clone(),
                    error: format!("Python sys import failed: {}", e),
                })?;
            let sys_path: pyo3::Bound<'_, pyo3::PyAny> =
                sys_mod
                    .getattr("path")
                    .map_err(|e: pyo3::PyErr| LoaderError::InitFailed {
                        bundle: bundle_name.clone(),
                        error: format!("Python sys.path get failed: {}", e),
                    })?;
            sys_path
                .call_method1("insert", (0usize, bundle_dir_str.as_str()))
                .map_err(|e: pyo3::PyErr| LoaderError::InitFailed {
                    bundle: bundle_name.clone(),
                    error: format!("sys.path insert failed: {}", e),
                })?;
            let site_pkgs: std::path::PathBuf = bundle_dir.join("site-packages");
            if site_pkgs.exists() {
                let sp: String = site_pkgs.to_string_lossy().into_owned();
                sys_path
                    .call_method1("insert", (0usize, sp.as_str()))
                    .map_err(|e: pyo3::PyErr| LoaderError::InitFailed {
                        bundle: bundle_name.clone(),
                        error: format!("site-packages path insert failed: {}", e),
                    })?;
            }
            // Step 3b: Import via importlib (no further sys.path mutation).
            let importlib_util: pyo3::Bound<'_, PyModule> = PyModule::import(py, "importlib.util")
                .map_err(|e| LoaderError::InitFailed {
                    bundle: bundle_name.clone(),
                    error: format!("importlib.util import failed: {}", e),
                })?;

            let spec: pyo3::Bound<'_, pyo3::PyAny> = importlib_util
                .getattr("spec_from_file_location")
                .map_err(|e| LoaderError::InitFailed {
                    bundle: bundle_name.clone(),
                    error: format!("spec_from_file_location not found: {}", e),
                })?
                .call1((&bundle_name, abs_path.to_string_lossy().as_ref()))
                .map_err(|e| LoaderError::InitFailed {
                    bundle: bundle_name.clone(),
                    error: format!(
                        "spec_from_file_location call failed for {}: {}",
                        abs_path.to_string_lossy(),
                        e
                    ),
                })?;

            let module_from_spec: pyo3::Bound<'_, pyo3::PyAny> = importlib_util
                .getattr("module_from_spec")
                .map_err(|e| LoaderError::InitFailed {
                    bundle: bundle_name.clone(),
                    error: format!("module_from_spec not found: {}", e),
                })?
                .call1((&spec,))
                .map_err(|e| LoaderError::InitFailed {
                    bundle: bundle_name.clone(),
                    error: format!(
                        "module_from_spec call failed for {}: {}",
                        abs_path.to_string_lossy(),
                        e
                    ),
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
                .map_err(|e| LoaderError::InitFailed {
                    bundle: bundle_name.clone(),
                    error: format!(
                        "exec_module failed for {}: {}",
                        abs_path.to_string_lossy(),
                        e
                    ),
                })?;

            // Step 3c: Locate polyplug_init(host, ctx).
            // New signature: polyplug_init(host_interface, ctx) - self-passing pattern.
            let init_fn: pyo3::Bound<'_, pyo3::PyAny> =
                module_from_spec.getattr("polyplug_init").map_err(|_| {
                    LoaderError::InitSymbolMissing {
                        bundle: bundle_name.clone(),
                    }
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
            // polyplug_init RETURNS its (registrations, AbiError) tuple; the loader
            // reads that return value — nothing is deposited into the namespace.
            let init_ret: Bound<'_, PyAny> = init_fn
                .call((host_interface_i64, ctx_ptr), None)
                .map_err(|e: pyo3::PyErr| LoaderError::InitFailed {
                    bundle: bundle_name.clone(),
                    error: format!("polyplug_init call failed: {}", e),
                })?;

            // Collect the guest's registration data and register every contract
            // with VM dispatch.
            Self::collect_and_register(py, &init_ret, host_interface, &bundle_name)?;

            // Isolate this bundle's freshly imported modules under a unique
            // per-bundle prefix so the next bundle imports its own generated
            // package instead of this one's cached copy. Re-keying keeps the module
            // objects alive for the runtime's lifetime while freeing the generic
            // module names. The contract callables themselves are held by each
            // contract's `PythonLoaderData`, so they stay alive regardless.
            let prefix: String = crate::isolation::isolate_bundle_modules(
                py,
                &bundle_name,
                bundle_id,
                &bundle_dir_str,
                &modules_before,
            )?;

            Ok::<String, LoaderError>(prefix)
        })?;

        // Pop bundle_id from the runtime's per-thread init stack after init completes.
        runtime.pop_init_bundle_id();

        // Record the re-key prefix so `unload` can purge this bundle's `sys.modules`
        // entries and force a fresh re-import next load.
        self.track_module_prefix(bundle_id, prefix);

        Ok(())
    }

    fn reload(&self, _manifest: &ManifestData, _runtime: &Runtime) -> Result<(), LoaderError> {
        // Defensive: the runtime gates on `supports_hot_reload()` (false for python)
        // and never calls this. Honest impl that also protects any direct caller.
        Err(LoaderError::HotReloadUnsupported {
            loader_name: self.loader_name().to_owned(),
        })
    }

    // Unlike native `dlclose`, purging `sys.modules` is memory-safe regardless of any
    // in-flight call: CPython refcounts/GC keep every still-referenced module object
    // alive, so deleting a `sys.modules` entry only drops the import cache, never the
    // object a running dispatch is using. Python therefore ALWAYS purges this bundle's
    // module entries on unload — no deferred-reclaim branch, no quiescence hint, no
    // crossbeam-epoch (CPython owns object liveness, there is no raw resource for the
    // epoch to govern).
    fn unload(&self, bundle_id: BundleId, _runtime: &Runtime) -> Result<(), LoaderError> {
        // Drain this bundle's prefixes and delete every matching `sys.modules` key so
        // a subsequent load re-imports fresh source.
        let prefixes: Vec<String> = {
            let mut map: std::sync::MutexGuard<'_, HashMap<BundleId, Vec<String>>> = self
                .module_prefixes
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            match map.remove(&bundle_id) {
                Some(p) => p,
                None => return Ok(()),
            }
        };

        for prefix in prefixes {
            purge_prefix_from_sys_modules(&prefix)?;
        }

        Ok(())
    }
}

/// Delete every `sys.modules` entry whose key equals `prefix` or begins with
/// `prefix + "."` (the re-keyed module tree for one bundle load).
///
/// Holds the GIL via [`Python::attach`] (matching the crate's pyo3 usage). Keys
/// are collected first, then deleted, so the dict is never mutated mid-iteration;
/// an individual missing key is ignored (the entry may already be gone). A hard
/// interpreter error reaching `sys.modules` is mapped to a proper
/// [`LoaderError`].
fn purge_prefix_from_sys_modules(prefix: &str) -> Result<(), LoaderError> {
    let dotted: String = format!("{}.", prefix);

    Python::attach(|py: Python<'_>| -> Result<(), LoaderError> {
        let sys_mod: Bound<'_, PyModule> =
            PyModule::import(py, "sys").map_err(|e: pyo3::PyErr| LoaderError::InitFailed {
                bundle: "python".to_owned(),
                error: format!("Python sys import failed during unload purge: {}", e),
            })?;
        let modules: Bound<'_, PyAny> =
            sys_mod
                .getattr("modules")
                .map_err(|e: pyo3::PyErr| LoaderError::InitFailed {
                    bundle: "python".to_owned(),
                    error: format!("sys.modules access failed during unload purge: {}", e),
                })?;
        let dict: Bound<'_, PyDict> =
            modules
                .cast_into::<PyDict>()
                .map_err(|_| LoaderError::InitFailed {
                    bundle: "python".to_owned(),
                    error: "sys.modules is not a dict during unload purge".to_owned(),
                })?;

        let mut to_delete: Vec<Bound<'_, PyAny>> = Vec::new();
        for (key, _value) in dict.iter() {
            let key_str: String = match key.extract::<String>() {
                Ok(s) => s,
                Err(_) => continue,
            };
            if key_str == prefix || key_str.starts_with(&dotted) {
                to_delete.push(key);
            }
        }
        for key in to_delete {
            // A concurrently-removed key is fine; the goal is absence, not the act.
            let _ = dict.del_item(&key);
        }

        Ok(())
    })
}

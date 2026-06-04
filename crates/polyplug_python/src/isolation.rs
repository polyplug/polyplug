//! Per-bundle Python module isolation.
//!
//! # Why this exists
//!
//! Every polyplug Python bundle ships an identical generated package tree
//! (`generated`, `generated.guest`, `generated.guest.contracts`, …) and the
//! bundle entry module imports it with a fixed, generic name
//! (`from generated.guest.contracts import ...`). Because the CPython
//! interpreter is shared process-wide (documented Known Limitation), the first
//! bundle's `generated.*` modules get cached in `sys.modules`. Every subsequent
//! bundle then imports the **first** bundle's classes — registering the wrong
//! contracts.
//!
//! # The mechanism
//!
//! The interpreter itself must stay shared, but module *identity* must be
//! per-bundle. After a bundle's entry module has executed and `polyplug_init`
//! has registered its contracts:
//!
//! 1. Determine which `sys.modules` entries were newly added during this load
//!    and physically live under the bundle directory (the generated package,
//!    the entry module, and the bundle's vendored `site-packages`).
//! 2. Re-key each such module under a unique per-bundle prefix
//!    (`__polyplug_bundle_<id>__.<original_name>`).
//! 3. Delete the original generic-name entries from `sys.modules`.
//!
//! Re-keying (rather than deleting) keeps every module object — and crucially
//! the module-level `ctypes.CFUNCTYPE` trampolines that the registered native
//! dispatch pointers point into — permanently alive inside the interpreter.
//! Freeing the generic names lets the next bundle import a fresh, correct copy.
//!
//! This is surgical: only modules under the bundle directory are touched, never
//! a `sys.modules.clear()` hammer, and shared interpreter state is left intact.

use std::collections::HashSet;
use std::ffi::CString;

use pyo3::Bound;
use pyo3::Python;
use pyo3::types::PyAny;
use pyo3::types::PyAnyMethods;
use pyo3::types::PyDict;
use pyo3::types::PyDictMethods;
use pyo3::types::PyModule;

use polyplug::error::LoaderError;
use polyplug::error::RuntimeError;

/// Embedded Python helper performing the `sys.modules` surgery.
///
/// Implemented in Python because materializing a namespace package's
/// `__path__` (a lazily-recalculating `_NamespacePath`) and reasoning about
/// `__file__` / `__path__` membership is far cleaner and less error-prone in
/// Python than through raw pyo3 calls. The function takes the per-bundle prefix,
/// the bundle directory, and the snapshot of module names captured before the
/// bundle executed; it returns the list of re-keyed original names.
const ISOLATION_HELPER_PY: &str = r#"
import os
import sys


def isolate(prefix, bundle_dir, before):
    bundle_dir = os.path.realpath(bundle_dir)
    before = set(before)
    to_move = []
    for name in list(sys.modules.keys()):
        if name in before:
            continue
        module = sys.modules.get(name)
        if module is None:
            continue
        file = getattr(module, "__file__", None)
        try:
            search_paths = list(getattr(module, "__path__", []) or [])
        except Exception:
            # A namespace package whose parent was already moved can raise while
            # recalculating its path; treat it as in-bundle so it is re-keyed too.
            search_paths = [bundle_dir]
        under_bundle = False
        if file is not None and os.path.realpath(file).startswith(bundle_dir):
            under_bundle = True
        else:
            for search_path in search_paths:
                if os.path.realpath(str(search_path)).startswith(bundle_dir):
                    under_bundle = True
                    break
        if under_bundle:
            to_move.append(name)
    # Re-key children before parents so namespace-package path recalculation
    # never observes a half-moved tree.
    to_move.sort(key=len, reverse=True)
    moved = []
    for name in to_move:
        module = sys.modules[name]
        sys.modules[prefix + "." + name] = module
        del sys.modules[name]
        moved.append(name)
    return moved
"#;

/// Capture the set of `sys.modules` keys before a bundle's entry module runs.
///
/// The returned set is later subtracted from the post-load `sys.modules` to
/// identify modules introduced by this bundle.
pub(crate) fn snapshot_loaded_modules(
    py: Python<'_>,
    bundle_name: &str,
) -> Result<HashSet<String>, RuntimeError> {
    let sys_mod: Bound<'_, PyModule> = PyModule::import(py, "sys").map_err(|e: pyo3::PyErr| {
        RuntimeError::Loader(LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("Python sys import failed: {}", e),
        })
    })?;
    let modules: Bound<'_, PyAny> = sys_mod.getattr("modules").map_err(|e: pyo3::PyErr| {
        RuntimeError::Loader(LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("sys.modules access failed: {}", e),
        })
    })?;
    // `sys.modules.keys()` returns a `dict_keys` view, which is not a Sequence
    // and so cannot be extracted to `Vec<String>` directly; materialize it into
    // a `list` first.
    let keys_view: Bound<'_, PyAny> = modules.call_method0("keys").map_err(|e: pyo3::PyErr| {
        RuntimeError::Loader(LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("sys.modules.keys() failed: {}", e),
        })
    })?;
    let keys: Bound<'_, PyAny> = py
        .get_type::<pyo3::types::PyList>()
        .call1((keys_view,))
        .map_err(|e: pyo3::PyErr| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!("sys.modules keys materialization failed: {}", e),
            })
        })?;
    let names: Vec<String> = keys.extract().map_err(|e: pyo3::PyErr| {
        RuntimeError::Loader(LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("sys.modules keys extraction failed: {}", e),
        })
    })?;
    Ok(names.into_iter().collect())
}

/// Re-key all of a bundle's freshly imported, in-bundle modules under a unique
/// per-bundle prefix derived from `bundle_id`, freeing the generic names for the
/// next bundle. See the module-level documentation for the full rationale.
pub(crate) fn isolate_bundle_modules(
    py: Python<'_>,
    bundle_name: &str,
    bundle_id: u64,
    bundle_dir: &str,
    before: &HashSet<String>,
) -> Result<(), RuntimeError> {
    let helper_code: CString =
        CString::new(ISOLATION_HELPER_PY).map_err(|e: std::ffi::NulError| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!("isolation helper contained interior nul: {}", e),
            })
        })?;
    let file_name: CString =
        CString::new("polyplug_python_isolation.py").map_err(|e: std::ffi::NulError| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!("isolation file name contained interior nul: {}", e),
            })
        })?;
    let module_name: CString =
        CString::new("polyplug_python_isolation").map_err(|e: std::ffi::NulError| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!("isolation module name contained interior nul: {}", e),
            })
        })?;

    let helper: Bound<'_, PyModule> =
        PyModule::from_code(py, &helper_code, &file_name, &module_name).map_err(
            |e: pyo3::PyErr| {
                RuntimeError::Loader(LoaderError::InitFailed {
                    bundle: bundle_name.to_owned(),
                    error: format!("isolation helper compile failed: {}", e),
                })
            },
        )?;

    let prefix: String = format!("__polyplug_bundle_{:016X}__", bundle_id);
    let before_list: Vec<&str> = before.iter().map(|s: &String| s.as_str()).collect();

    helper
        .getattr("isolate")
        .and_then(|isolate: Bound<'_, PyAny>| {
            isolate.call1((prefix.as_str(), bundle_dir, before_list))
        })
        .map_err(|e: pyo3::PyErr| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!("module isolation failed: {}", e),
            })
        })?;

    // The helper module itself must not linger under its generic name where it
    // could collide with a future load; drop it from sys.modules now. Its code
    // object is no longer needed once `isolate` has run.
    let sys_mod: Bound<'_, PyModule> = PyModule::import(py, "sys").map_err(|e: pyo3::PyErr| {
        RuntimeError::Loader(LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("Python sys import failed: {}", e),
        })
    })?;
    let modules: Bound<'_, PyAny> = sys_mod.getattr("modules").map_err(|e: pyo3::PyErr| {
        RuntimeError::Loader(LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("sys.modules access failed: {}", e),
        })
    })?;
    if let Ok(dict) = modules.cast::<PyDict>() {
        let _ = dict.del_item("polyplug_python_isolation");
    }

    Ok(())
}

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
//!    (`__polyplug_bundle_<id>_<nonce>__.<original_name>`). The `<nonce>` is a
//!    process-global monotonic counter so two `Runtime` instances loading the
//!    same-named bundle (whose `<id>` name-hash is identical) never collide in
//!    the shared `sys.modules`.
//! 3. Delete the original generic-name entries from `sys.modules`.
//!
//! Re-keying (rather than deleting) keeps every module object permanently alive
//! inside the interpreter, and freeing the generic names lets the next bundle
//! import a fresh, correct copy. The contract callables the VM dispatcher invokes
//! are additionally held by each contract's `PythonLoaderData`, so they stay
//! alive independently of `sys.modules`.
//!
//! This is surgical: only modules under the bundle directory are touched, never
//! a `sys.modules.clear()` hammer, and shared interpreter state is left intact.

use core::sync::atomic::AtomicU64;
use core::sync::atomic::Ordering;
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
) -> Result<HashSet<String>, LoaderError> {
    let sys_mod: Bound<'_, PyModule> =
        PyModule::import(py, "sys").map_err(|e: pyo3::PyErr| LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("Python sys import failed: {}", e),
        })?;
    let modules: Bound<'_, PyAny> =
        sys_mod
            .getattr("modules")
            .map_err(|e: pyo3::PyErr| LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!("sys.modules access failed: {}", e),
            })?;
    // `sys.modules.keys()` returns a `dict_keys` view, which is not a Sequence
    // and so cannot be extracted to `Vec<String>` directly; materialize it into
    // a `list` first.
    let keys_view: Bound<'_, PyAny> =
        modules
            .call_method0("keys")
            .map_err(|e: pyo3::PyErr| LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!("sys.modules.keys() failed: {}", e),
            })?;
    let keys: Bound<'_, PyAny> = py
        .get_type::<pyo3::types::PyList>()
        .call1((keys_view,))
        .map_err(|e: pyo3::PyErr| LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("sys.modules keys materialization failed: {}", e),
        })?;
    let names: Vec<String> = keys
        .extract()
        .map_err(|e: pyo3::PyErr| LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("sys.modules keys extraction failed: {}", e),
        })?;
    Ok(names.into_iter().collect())
}

/// Monotonic counter that makes every isolation prefix globally unique within
/// the process.
///
/// `bundle_id` alone is a name hash, so two `Runtime` instances (or repeated
/// loads of the same-named bundle) would otherwise compute identical prefixes
/// and clobber each other's `sys.modules` entries. Because CPython is shared
/// process-wide (the documented Known Limitation that already sanctions
/// `PYTHON_INIT: OnceLock` — see CLAUDE.md Rule 12), `sys.modules` is a single
/// global namespace; the only way to guarantee non-colliding keys across
/// runtimes is a process-global nonce. This counter is exempt from the
/// no-globals rule under that same Known Limitation: it carries no runtime
/// state, only a uniqueness ticket, and does not couple runtime instances.
static ISOLATION_NONCE: AtomicU64 = AtomicU64::new(0);

/// Build the unique `sys.modules` re-key prefix for one bundle isolation pass.
///
/// Combines the bundle's name-hash id (for human-readable correlation) with a
/// process-global monotonic nonce (for guaranteed uniqueness across runtimes
/// and repeated loads).
fn next_isolation_prefix(bundle_id: u64) -> String {
    let nonce: u64 = ISOLATION_NONCE.fetch_add(1, Ordering::Relaxed);
    format!("__polyplug_bundle_{:016X}_{:016X}__", bundle_id, nonce)
}

/// Re-key all of a bundle's freshly imported, in-bundle modules under a unique
/// per-bundle prefix, freeing the generic names for the next bundle. The prefix
/// combines `bundle_id` with a process-global nonce so two runtimes loading the
/// same-named bundle never collide in the shared `sys.modules`. See the
/// module-level documentation for the full rationale.
pub(crate) fn isolate_bundle_modules(
    py: Python<'_>,
    bundle_name: &str,
    bundle_id: u64,
    bundle_dir: &str,
    before: &HashSet<String>,
) -> Result<String, LoaderError> {
    let helper_code: CString =
        CString::new(ISOLATION_HELPER_PY).map_err(|e: std::ffi::NulError| {
            LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!("isolation helper contained interior nul: {}", e),
            }
        })?;
    let file_name: CString =
        CString::new("polyplug_python_isolation.py").map_err(|e: std::ffi::NulError| {
            LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!("isolation file name contained interior nul: {}", e),
            }
        })?;
    let module_name: CString =
        CString::new("polyplug_python_isolation").map_err(|e: std::ffi::NulError| {
            LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!("isolation module name contained interior nul: {}", e),
            }
        })?;

    let helper: Bound<'_, PyModule> =
        PyModule::from_code(py, &helper_code, &file_name, &module_name).map_err(
            |e: pyo3::PyErr| LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!("isolation helper compile failed: {}", e),
            },
        )?;

    let prefix: String = next_isolation_prefix(bundle_id);
    let before_list: Vec<&str> = before.iter().map(|s: &String| s.as_str()).collect();

    helper
        .getattr("isolate")
        .and_then(|isolate: Bound<'_, PyAny>| {
            isolate.call1((prefix.as_str(), bundle_dir, before_list))
        })
        .map_err(|e: pyo3::PyErr| LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("module isolation failed: {}", e),
        })?;

    // The helper module itself must not linger under its generic name where it
    // could collide with a future load; drop it from sys.modules now. Its code
    // object is no longer needed once `isolate` has run.
    let sys_mod: Bound<'_, PyModule> =
        PyModule::import(py, "sys").map_err(|e: pyo3::PyErr| LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("Python sys import failed: {}", e),
        })?;
    let modules: Bound<'_, PyAny> =
        sys_mod
            .getattr("modules")
            .map_err(|e: pyo3::PyErr| LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!("sys.modules access failed: {}", e),
            })?;
    if let Ok(dict) = modules.cast::<PyDict>() {
        let _ = dict.del_item("polyplug_python_isolation");
    }

    // Return the prefix the bundle's modules were re-keyed under so the loader can
    // track it and purge those `sys.modules` entries on unload.
    Ok(prefix)
}

#[cfg(test)]
mod tests {
    use super::next_isolation_prefix;
    use std::collections::HashSet;

    /// Two prefixes built from the *same* `bundle_id` must still differ, because
    /// the process-global nonce advances on every call. This is what stops two
    /// runtimes loading the same-named bundle from clobbering each other's
    /// `sys.modules` entries.
    #[test]
    fn same_bundle_id_yields_distinct_prefixes() {
        let bundle_id: u64 = 0xDEAD_BEEF_CAFE_0001;
        let first: String = next_isolation_prefix(bundle_id);
        let second: String = next_isolation_prefix(bundle_id);
        assert_ne!(
            first, second,
            "identical bundle_id must still produce unique prefixes via the nonce"
        );
    }

    /// A batch of prefixes (mixing repeated and distinct bundle ids) must be
    /// pairwise unique across the whole process.
    #[test]
    fn prefixes_are_pairwise_unique() {
        let mut seen: HashSet<String> = HashSet::new();
        let ids: [u64; 4] = [1, 1, 2, 1];
        for &id in ids.iter() {
            for _ in 0..16 {
                let prefix: String = next_isolation_prefix(id);
                assert!(
                    seen.insert(prefix.clone()),
                    "prefix collision detected: {}",
                    prefix
                );
            }
        }
    }

    /// The prefix must embed the bundle id (for human-readable correlation) and
    /// follow the documented `__polyplug_bundle_<id>_<nonce>__` shape.
    #[test]
    fn prefix_embeds_bundle_id_and_has_expected_shape() {
        let bundle_id: u64 = 0x0123_4567_89AB_CDEF;
        let prefix: String = next_isolation_prefix(bundle_id);
        assert!(prefix.starts_with("__polyplug_bundle_0123456789ABCDEF_"));
        assert!(prefix.ends_with("__"));
    }
}

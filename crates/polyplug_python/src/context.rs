//! PythonContext — CPython interpreter singleton for polyplug_python.

use std::sync::OnceLock;

use pyo3::Python;

use polyplug::error::LoaderError;
use polyplug::error::RuntimeError;

use crate::config::PythonConfig;

/// Global one-time Python interpreter initialization sentinel.
/// `Python::initialize()` must be called exactly once per process.
static PYTHON_INIT: OnceLock<()> = OnceLock::new();

/// Initialize the CPython interpreter exactly once per process and verify
/// that the running Python version meets the minimum required by `config`.
///
/// Subsequent calls are no-ops (OnceLock is already set).
///
/// Returns `Err(LoaderError::InitFailed)` if the version is too old.
pub(crate) fn ensure_python_initialized(config: &PythonConfig) -> Result<(), RuntimeError> {
    // Step 1: Initialize CPython exactly once.
    // OnceLock::get_or_init is used (not get_or_try_init) because
    // Python::initialize() is infallible — it panics on failure,
    // which is acceptable at init time (same as dotnet's CLR init approach).
    PYTHON_INIT.get_or_init(|| {
        Python::initialize();
    });

    // Step 2: Verify version.
    Python::attach(|py| {
        let ver: pyo3::PythonVersionInfo<'_> = py.version_info();
        let (req_major, req_minor): (u32, u32) = config.min_version;
        if (ver.major as u32, ver.minor as u32) < (req_major, req_minor) {
            return Err(RuntimeError::Loader(LoaderError::InitFailed {
                bundle: "python".to_owned(),
                error: format!(
                    "runtime version mismatch: required {}.{}, found {}.{}",
                    req_major, req_minor, ver.major, ver.minor
                ),
            }));
        }
        Ok(())
    })
}

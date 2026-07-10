//! PythonContext — CPython interpreter singleton for polyplug_python.

use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::OnceLock;

use pyo3::Python;
use pyo3::PythonVersionInfo;

#[cfg(unix)]
use core::mem;
#[cfg(unix)]
use libloading::os::unix::Library;
#[cfg(unix)]
use libloading::os::unix::RTLD_GLOBAL;
#[cfg(unix)]
use libloading::os::unix::RTLD_LAZY;

use polyplug::error::LoaderError;

use crate::config::PythonConfig;

/// Global one-time Python interpreter initialization sentinel.
/// `Python::initialize()` must be called exactly once per process.
static PYTHON_INIT: OnceLock<()> = OnceLock::new();

/// Process-wide lock serializing the bundle-load critical section
/// (snapshot `sys.modules` → execute the bundle → isolate its modules).
///
/// The CPython interpreter — and therefore `sys.modules` — is a single shared
/// per-process resource (documented Known Limitation). The per-bundle module
/// isolation reads a `sys.modules` snapshot before executing a bundle and diffs
/// it afterward. CPython releases the GIL during import I/O, so without this
/// lock two concurrent loads on different threads interleave their `sys.modules`
/// mutations and one load's isolation pass would steal the other's freshly
/// imported modules. Serializing the whole snapshot→exec→isolate section makes
/// it atomic with respect to other Python loads. This guards interpreter-level
/// state only (like `PYTHON_INIT`), never per-`Runtime` state, so it does not
/// violate runtime isolation.
static PYTHON_LOAD_LOCK: Mutex<()> = Mutex::new(());

/// Acquire the process-wide Python bundle-load lock. The returned guard must be
/// held for the entire snapshot→exec→isolate critical section.
pub(crate) fn acquire_load_lock() -> MutexGuard<'static, ()> {
    PYTHON_LOAD_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(unix)]
/// The CPython link library name pyo3 was built against (e.g. `python3.14`),
/// captured at build time. Empty when the build config could not resolve it.
const PYTHON_LIB_NAME: &str = env!("POLYPLUG_PYTHON_LIB_NAME");

/// Promote libpython's symbols into the global symbol namespace.
///
/// When this loader is built as a `cdylib` and `dlopen`ed by a non-Python host
/// (Lua, JS, …), the dynamic loader brings libpython in as a transitive
/// dependency under `RTLD_LOCAL` semantics. CPython then fails to load its own
/// C extension modules (`_ctypes`, …) because their libpython symbols (e.g.
/// `PyUnicode_FromFormat`) are not globally visible. Re-`dlopen`ing libpython
/// with `RTLD_GLOBAL` promotes the already-resident library's symbols into the
/// global scope so extension modules resolve.
///
/// No-op on non-unix targets. Best-effort: failure to locate libpython is not
/// fatal here — the rust-host path links libpython directly and is unaffected.
#[cfg(unix)]
fn promote_libpython_symbols() {
    if PYTHON_LIB_NAME.is_empty() {
        return;
    }

    // Candidate sonames, most-specific first. The cdylib's DT_NEEDED entry is
    // the versioned `.so.1.0`; the unversioned name is the linker symlink.
    let candidates: [String; 2] = [
        format!("lib{PYTHON_LIB_NAME}.so.1.0"),
        format!("lib{PYTHON_LIB_NAME}.so"),
    ];

    for name in &candidates {
        // SAFETY: We only request RTLD_GLOBAL|RTLD_LAZY on a library that is
        // already resident in the process (linked via DT_NEEDED). dlopen on an
        // already-loaded library returns a handle to the existing mapping and
        // merges RTLD_GLOBAL into its symbol scope without re-initializing it.
        // The handle is intentionally leaked (via std::mem::forget) so the
        // promoted scope persists for the interpreter's lifetime.
        let opened: Result<Library, libloading::Error> =
            unsafe { Library::open(Some(name), RTLD_GLOBAL | RTLD_LAZY) };
        if let Ok(lib) = opened {
            mem::forget(lib);
            return;
        }
    }
}

#[cfg(not(unix))]
fn promote_libpython_symbols() {}

/// Initialize the CPython interpreter exactly once per process and verify
/// that the running Python version meets the minimum required by `config`.
///
/// Subsequent calls are no-ops (OnceLock is already set).
///
/// Returns `Err(LoaderError::InitFailed)` if the version is too old.
pub(crate) fn ensure_python_initialized(config: &PythonConfig) -> Result<(), LoaderError> {
    // Step 1: Initialize CPython exactly once.
    // OnceLock::get_or_init is used (not get_or_try_init) because
    // Python::initialize() is infallible — it panics on failure,
    // which is acceptable at init time (same as dotnet's CLR init approach).
    PYTHON_INIT.get_or_init(|| {
        // Ensure libpython's symbols are globally visible before the
        // interpreter loads its C extension modules. Required when this loader
        // is dlopened as a cdylib by a non-Python host.
        promote_libpython_symbols();
        Python::initialize();
    });

    // Step 2: Verify version.
    Python::attach(|py| {
        let ver: PythonVersionInfo = py.version_info();
        let (req_major, req_minor): (u32, u32) = config.min_version;
        if (ver.major as u32, ver.minor as u32) < (req_major, req_minor) {
            return Err(LoaderError::InitFailed {
                bundle: "python".to_owned(),
                error: format!(
                    "runtime version mismatch: required {}.{}, found {}.{}",
                    req_major, req_minor, ver.major, ver.minor
                ),
            });
        }
        Ok(())
    })
}

//! CPython interpreter bootstrap helpers for polyplug_python.
//!
//! The process-global once-per-process state (the "interpreter initialized"
//! flag and the snapshot→exec→isolate load serialization) lives in the
//! `polyplug` crate's `SharedState`, mutated only by
//! [`Runtime::with_python_load`](polyplug::runtime::Runtime::with_python_load).
//! This module supplies the two pieces that method drives: the one-time
//! interpreter init (run inside the guarded `init` closure) and the per-load
//! minimum-version check (run inside the guarded `body` closure).

use pyo3::Python;

use polyplug::error::LoaderError;

use crate::config::PythonConfig;

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
        let opened: Result<libloading::os::unix::Library, libloading::Error> = unsafe {
            libloading::os::unix::Library::open(
                Some(name),
                libloading::os::unix::RTLD_GLOBAL | libloading::os::unix::RTLD_LAZY,
            )
        };
        if let Ok(lib) = opened {
            core::mem::forget(lib);
            return;
        }
    }
}

#[cfg(not(unix))]
fn promote_libpython_symbols() {}

/// Initialize the CPython interpreter for this process.
///
/// Run exactly once, inside the `init` closure of
/// [`Runtime::with_python_load`](polyplug::runtime::Runtime::with_python_load):
/// the runtime holds the process-global `SharedState` lock and only invokes
/// this when `SupportedLanguage::Python` has not yet been marked initialized,
/// so the "exactly once per process" guarantee is owned by the runtime rather
/// than by a loader-side `OnceLock`.
///
/// `Python::initialize()` is infallible (it panics on hard failure, acceptable
/// at process init — same posture as the .NET CLR bootstrap), so this returns
/// `Ok` once the interpreter is up; the `Result` is part of the
/// `with_python_load` init-closure contract.
pub(crate) fn run_python_init() -> Result<(), LoaderError> {
    // Ensure libpython's symbols are globally visible before the interpreter
    // loads its C extension modules. Required when this loader is dlopened as a
    // cdylib by a non-Python host.
    promote_libpython_symbols();
    Python::initialize();
    Ok(())
}

/// Verify that the running CPython meets the minimum version required by
/// `config`. Run on **every** load (inside the guarded `body` closure), not
/// only at first init, so a runtime configured with a stricter minimum than an
/// earlier load still rejects an too-old interpreter.
///
/// Returns `Err(LoaderError::InitFailed)` if the running version is too old.
pub(crate) fn check_python_version(config: &PythonConfig) -> Result<(), LoaderError> {
    Python::attach(|py| {
        let ver: pyo3::PythonVersionInfo<'_> = py.version_info();
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

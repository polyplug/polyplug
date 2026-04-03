//! Python-specific error types.

use thiserror::Error;

/// Errors from the Python loader.
#[derive(Debug, Error)]
pub enum PythonLoaderError {
    #[error("Python interpreter initialization failed: {reason}")]
    PythonInitFailed { reason: String },

    #[error("failed to import Python module at `{path}`: {reason}")]
    PythonModuleImportFailed { path: String, reason: String },

    #[error("Python init function raised exception in bundle `{bundle}`: {message}")]
    PythonInitRaisedException { bundle: String, message: String },
}
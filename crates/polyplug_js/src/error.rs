//! JavaScript-specific error types.

use thiserror::Error;

/// Errors from the JS loader.
#[derive(Debug, Error)]
pub enum JsLoaderError {
    #[error("rolldown not found on PATH — js-quickjs pack requires rolldown. {hint}")]
    RolldownNotFound { hint: String },

    #[error("JS runtime \"{runtime}\" panicked during bundle load: {message}")]
    JsRuntimePanic { runtime: String, message: String },

    #[error("JS runtime initialization failed: {reason}")]
    JsRuntimeInitFailed { reason: String },

    #[error("module resolution failed: {reason}")]
    ModuleResolutionFailed { reason: String },

    #[error("failed to execute JS script: {reason}")]
    JsExecutionFailed { reason: String },
}
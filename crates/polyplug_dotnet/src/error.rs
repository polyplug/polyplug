//! .NET-specific error types.

use thiserror::Error;

/// Errors from the .NET loader.
#[derive(Debug, Error)]
pub enum DotnetLoaderError {
    #[error("hostfxr not found: searched DOTNET_ROOT, PATH, and well-known paths")]
    HostfxrNotFound,

    #[error("CLR initialization failed for runtime config `{path}`: {reason}")]
    ClrInitFailed { path: String, reason: String },

    #[error("assembly not found at path `{path}`")]
    AssemblyNotFound { path: String },

    #[error(".NET runtime version mismatch: required={required}, found={found}")]
    RuntimeVersionMismatch { required: String, found: String },

    #[error("invalid .NET framework version in TFM `{tfm}`: {reason}")]
    InvalidFrameworkVersion { tfm: String, reason: String },
}
//! Error — CodegenError type hierarchy for polyplugc.

use thiserror::Error;

/// Top-level error type for polyplugc code generation.
#[derive(Debug, Error)]
pub(crate) enum CodegenError {
    #[error("unknown type `{type_ref}` in contract `{contract}`")]
    UnknownType { type_ref: String, contract: String },

    #[allow(dead_code)]
    #[error("unsupported type `{type_name}` for language `{lang}`")]
    UnsupportedType { type_name: String, lang: String },

    #[error("failed to write generated file `{path}`: {source}")]
    WriteFailed {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("IR validation failed: {message}")]
    ValidationFailed { message: String },
}

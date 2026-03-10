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

    #[error("unsupported language `{lang}` for pack command")]
    UnsupportedLanguage { lang: String },

    #[error("failed to write generated file `{path}`: {source}")]
    WriteFailed {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("IR validation failed: {message}")]
    ValidationFailed { message: String },

    #[error(
        "bundle name \"{bundle_name}\" conflicts with contract name \"{bundle_name}\" \
         in api.toml. Bundle names and contract names must be unique across the \
         ecosystem. Rename the bundle in bundle.toml or the contract in api.toml."
    )]
    BundleNameConflict { bundle_name: String },

    #[error("failed to read cache file `{path}`: {source}")]
    CacheReadFailed {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to write cache file `{path}`: {source}")]
    CacheWriteFailed {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to deserialize cache file `{path}`: {source}")]
    CacheDeserializeFailed {
        path: String,
        #[source]
        source: toml::de::Error,
    },

    #[error("failed to serialize cache: {source}")]
    CacheSerializeFailed {
        #[source]
        source: toml::ser::Error,
    },
}

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

    #[error("failed to read file `{path}`: {source}")]
    ReadFailed {
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

    #[error("invalid repr `{repr}` for enum `{enum_name}`: must be u8 | u16 | u32 | u64")]
    EnumInvalidRepr { enum_name: String, repr: String },

    #[error(
        "invalid token in value expression `{expr}` for variant `{variant_name}` in enum `{enum_name}`"
    )]
    EnumInvalidValueExpr {
        enum_name: String,
        variant_name: String,
        expr: String,
    },

    #[error(
        "forward reference to variant `{ref_name}` in value expression for `{variant_name}` in enum `{enum_name}`: variant references must be backward-only"
    )]
    EnumForwardRef {
        enum_name: String,
        variant_name: String,
        ref_name: String,
    },

    #[error(
        "chained variant reference: `{variant_name}` references `{ref_name}` which itself references another variant in enum `{enum_name}`: only one level of variant reference is allowed"
    )]
    EnumChainedRef {
        enum_name: String,
        variant_name: String,
        ref_name: String,
    },

    #[error(
        "name `{name}` is used by both a [[type]] and an [[enum]]: names must be unique across both"
    )]
    EnumNameCollision { name: String },
}

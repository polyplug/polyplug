//! Error — PolyplugcError type hierarchy for polyplugc.

use thiserror::Error;

/// Top-level error type for polyplugc code generation.
#[derive(Debug, Error)]
pub enum PolyplugcError {
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

#[cfg(test)]
mod tests {
    use super::PolyplugcError;

    #[test]
    fn unknown_type_display() {
        let err: PolyplugcError = PolyplugcError::UnknownType {
            type_ref: "MySpecialType".to_owned(),
            contract: "audio_api".to_owned(),
        };
        let s: String = err.to_string();
        assert!(s.contains("unknown type"), "got: {s}");
        assert!(s.contains("MySpecialType"), "got: {s}");
        assert!(s.contains("audio_api"), "got: {s}");
    }

    #[test]
    fn unsupported_type_display() {
        let err: PolyplugcError = PolyplugcError::UnsupportedType {
            type_name: "SomeExoticType".to_owned(),
            lang: "cpp".to_owned(),
        };
        let s: String = err.to_string();
        assert!(s.contains("unsupported type"), "got: {s}");
        assert!(s.contains("SomeExoticType"), "got: {s}");
        assert!(s.contains("cpp"), "got: {s}");
    }

    #[test]
    fn unsupported_language_display() {
        let err: PolyplugcError = PolyplugcError::UnsupportedLanguage {
            lang: "cobol".to_owned(),
        };
        let s: String = err.to_string();
        assert!(s.contains("unsupported language"), "got: {s}");
        assert!(s.contains("cobol"), "got: {s}");
    }

    #[test]
    fn validation_failed_display() {
        let err: PolyplugcError = PolyplugcError::ValidationFailed {
            message: "field `foo` has invalid size".to_owned(),
        };
        let s: String = err.to_string();
        assert!(s.contains("IR validation failed"), "got: {s}");
        assert!(s.contains("foo"), "got: {s}");
    }

    #[test]
    fn bundle_name_conflict_display() {
        let err: PolyplugcError = PolyplugcError::BundleNameConflict {
            bundle_name: "shared_types".to_owned(),
        };
        let s: String = err.to_string();
        assert!(s.contains("conflict") || s.contains("conflicts"), "got: {s}");
        assert!(s.contains("shared_types"), "got: {s}");
    }

    #[test]
    fn enum_invalid_repr_display() {
        let err: PolyplugcError = PolyplugcError::EnumInvalidRepr {
            enum_name: "Color".to_owned(),
            repr: "i32".to_owned(),
        };
        let s: String = err.to_string();
        assert!(s.contains("invalid repr"), "got: {s}");
        assert!(s.contains("Color"), "got: {s}");
        assert!(s.contains("i32"), "got: {s}");
    }

    #[test]
    fn enum_invalid_value_expr_display() {
        let err: PolyplugcError = PolyplugcError::EnumInvalidValueExpr {
            enum_name: "Status".to_owned(),
            variant_name: "Pending".to_owned(),
            expr: "???".to_owned(),
        };
        let s: String = err.to_string();
        assert!(s.contains("invalid token") || s.contains("invalid"), "got: {s}");
        assert!(s.contains("Status"), "got: {s}");
        assert!(s.contains("Pending"), "got: {s}");
        assert!(s.contains("???"), "got: {s}");
    }

    #[test]
    fn enum_forward_ref_display() {
        let err: PolyplugcError = PolyplugcError::EnumForwardRef {
            enum_name: "Direction".to_owned(),
            variant_name: "Left".to_owned(),
            ref_name: "Right".to_owned(),
        };
        let s: String = err.to_string();
        assert!(s.contains("forward reference"), "got: {s}");
        assert!(s.contains("Direction"), "got: {s}");
        assert!(s.contains("Left"), "got: {s}");
        assert!(s.contains("Right"), "got: {s}");
    }

    #[test]
    fn enum_chained_ref_display() {
        let err: PolyplugcError = PolyplugcError::EnumChainedRef {
            enum_name: "Size".to_owned(),
            variant_name: "Large".to_owned(),
            ref_name: "Medium".to_owned(),
        };
        let s: String = err.to_string();
        assert!(s.contains("chained") || s.contains("chain"), "got: {s}");
        assert!(s.contains("Size"), "got: {s}");
        assert!(s.contains("Large"), "got: {s}");
        assert!(s.contains("Medium"), "got: {s}");
    }

    #[test]
    fn enum_name_collision_display() {
        let err: PolyplugcError = PolyplugcError::EnumNameCollision {
            name: "EventKind".to_owned(),
        };
        let s: String = err.to_string();
        assert!(s.contains("EventKind"), "got: {s}");
        // The message says both [[type]] and [[enum]] use the name
        assert!(s.contains("type") || s.contains("enum"), "got: {s}");
    }
}

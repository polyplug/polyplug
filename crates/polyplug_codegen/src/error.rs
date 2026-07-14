//! Error — PolyplugcError type hierarchy for polyplugc.

use core::error::Error;
use core::fmt;
use std::io;

/// Source location (file path, 1-based line, 1-based column) for diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLocation {
    /// Canonical path to the source file, or `"<input>"` for in-memory strings.
    pub file: String,
    /// 1-based line number.
    pub line: usize,
    /// 1-based column number (byte-offset within the line).
    pub col: usize,
}

impl fmt::Display for SourceLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}:{}", self.file, self.line, self.col)
    }
}

/// Top-level error type for polyplugc code generation.
#[derive(Debug)]
pub enum PolyplugcError {
    UnknownType {
        type_ref: String,
        contract: String,
        /// Optional source location of the offending type reference.
        location: Option<SourceLocation>,
        /// "did you mean ...?" hint when a close match exists among known type names.
        suggestion: Option<String>,
    },

    UnsupportedType {
        type_name: String,
        lang: String,
    },

    UnsupportedLanguage {
        lang: String,
    },

    WriteFailed {
        path: String,
        source: io::Error,
    },

    UnsafeOutputPath {
        path: String,
    },

    DuplicateOutputPath {
        path: String,
    },

    ReadFailed {
        path: String,
        source: io::Error,
    },

    ValidationFailed {
        message: String,
    },

    /// Documentation contains a control character that cannot be emitted safely.
    InvalidDocumentation {
        character: char,
        location: Option<SourceLocation>,
    },

    /// A per-language attribute is empty or spans more than one source line.
    InvalidLanguageAttribute {
        language: String,
        node: String,
        attribute: String,
        reason: String,
        location: Box<SourceLocation>,
    },

    /// TOML-level parse error with optional source location.
    TomlParseError {
        /// Human-readable parse error from the toml crate.
        message: String,
        /// Source location of the error, if the toml crate provided a byte span.
        location: Option<SourceLocation>,
    },

    BundleNameConflict {
        bundle_name: String,
    },

    EnumInvalidRepr {
        enum_name: String,
        repr: String,
        /// "did you mean ...?" hint for close repr spellings (e.g. "u33" → "u32").
        suggestion: Option<String>,
    },

    EnumInvalidValueExpr {
        enum_name: String,
        variant_name: String,
        expr: String,
    },

    EnumForwardRef {
        enum_name: String,
        variant_name: String,
        ref_name: String,
    },

    EnumChainedRef {
        enum_name: String,
        variant_name: String,
        ref_name: String,
    },

    EnumNameCollision {
        name: String,
        /// Optional contextual hint.
        suggestion: Option<String>,
    },

    GuestGenerationNotSupported {
        language: String,
        reason: String,
    },

    HostContractNameMissingPrefix {
        name: String,
    },

    DuplicateContractName {
        name: String,
        /// Optional pointer to where the name was first defined.
        first_defined_at: Option<SourceLocation>,
    },

    VersionOverflow {
        component: String,
        value: u32,
        version_str: String,
        /// Source location of the version field that overflowed, if known.
        location: Option<SourceLocation>,
        /// Optional actionable suggestion.
        suggestion: Option<String>,
    },

    InvalidIdentifier {
        kind: String,
        name: String,
        context: String,
        /// Source location of the invalid name, if known.
        location: Option<SourceLocation>,
    },

    DuplicateFunctionName {
        contract: String,
        function: String,
        /// Optional note pointing at where the function was first declared.
        first_defined_at: Option<SourceLocation>,
    },

    /// An identifier collides with a reserved keyword in one or more target
    /// languages (or a polyplug-reserved name). Such a name would flow verbatim
    /// into generated source and produce uncompilable output, so it is rejected
    /// at parse time rather than escaped/renamed in the generators.
    ReservedIdentifier {
        /// What kind of name this is: "function", "field", "contract", "enum",
        /// "enum variant", "type", "parameter".
        kind: String,
        /// The offending name.
        name: String,
        /// Where the name appeared (e.g. the contract/enum/type name).
        context: String,
        /// Human-readable list of the language(s) that reserve this name, e.g.
        /// "Python, C++" or "polyplug".
        languages: String,
        /// Source location of the reserved name, if known. Boxed to keep the
        /// `PolyplugcError` enum under clippy's `result_large_err` threshold.
        location: Option<Box<SourceLocation>>,
    },
}

impl fmt::Display for PolyplugcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PolyplugcError::UnknownType {
                type_ref,
                contract,
                location,
                suggestion,
            } => {
                if let Some(loc) = location {
                    write!(f, "{loc} — ")?;
                }
                write!(f, "unknown type `{type_ref}` in contract `{contract}`")?;
                if let Some(hint) = suggestion {
                    write!(f, " — did you mean `{hint}`?")?;
                }
                Ok(())
            }

            PolyplugcError::UnsupportedType { type_name, lang } => {
                write!(f, "unsupported type `{type_name}` for language `{lang}`")
            }

            PolyplugcError::UnsupportedLanguage { lang } => {
                write!(f, "unsupported language/runtime `{lang}`")
            }

            PolyplugcError::WriteFailed { path, source } => {
                write!(f, "failed to write generated file `{path}`: {source}")
            }

            PolyplugcError::UnsafeOutputPath { path } => {
                write!(
                    f,
                    "generated output path `{path}` must be relative and must not traverse parent directories"
                )
            }

            PolyplugcError::DuplicateOutputPath { path } => {
                write!(f, "generated output contains duplicate path `{path}`")
            }

            PolyplugcError::ReadFailed { path, source } => {
                write!(f, "failed to read file `{path}`: {source}")
            }

            PolyplugcError::ValidationFailed { message } => {
                write!(f, "IR validation failed: {message}")
            }

            PolyplugcError::InvalidDocumentation {
                character,
                location,
            } => {
                if let Some(loc) = location {
                    write!(f, "{loc} — ")?;
                }
                write!(
                    f,
                    "invalid documentation character U+{:04X}: documentation permits only tab, line breaks, and printable text",
                    *character as u32
                )
            }

            PolyplugcError::InvalidLanguageAttribute {
                language,
                node,
                attribute,
                reason,
                location,
            } => {
                write!(
                    f,
                    "{location} — invalid {language} attribute `{attribute}` on {node}: {reason}"
                )
            }

            PolyplugcError::TomlParseError { message, location } => {
                if let Some(loc) = location {
                    write!(f, "{loc} — ")?;
                }
                write!(f, "TOML parse error: {message}")
            }

            PolyplugcError::BundleNameConflict { bundle_name } => {
                write!(
                    f,
                    "bundle name \"{bundle_name}\" conflicts with contract name \"{bundle_name}\" \
                     in api.toml. Bundle names and contract names must be unique across the \
                     ecosystem. Rename the bundle in bundle.toml or the contract in api.toml."
                )
            }

            PolyplugcError::EnumInvalidRepr {
                enum_name,
                repr,
                suggestion,
            } => {
                write!(
                    f,
                    "invalid repr `{repr}` for enum `{enum_name}`: must be u8 | u16 | u32 | u64"
                )?;
                if let Some(hint) = suggestion {
                    write!(f, " — did you mean `{hint}`?")?;
                }
                Ok(())
            }

            PolyplugcError::EnumInvalidValueExpr {
                enum_name,
                variant_name,
                expr,
            } => {
                write!(
                    f,
                    "invalid token in value expression `{expr}` for variant `{variant_name}` in enum `{enum_name}`"
                )
            }

            PolyplugcError::EnumForwardRef {
                enum_name,
                variant_name,
                ref_name,
            } => {
                write!(
                    f,
                    "forward reference to variant `{ref_name}` in value expression for `{variant_name}` in enum `{enum_name}`: variant references must be backward-only"
                )
            }

            PolyplugcError::EnumChainedRef {
                enum_name,
                variant_name,
                ref_name,
            } => {
                write!(
                    f,
                    "chained variant reference: `{variant_name}` references `{ref_name}` which itself references another variant in enum `{enum_name}`: only one level of variant reference is allowed"
                )
            }

            PolyplugcError::EnumNameCollision { name, suggestion } => {
                write!(
                    f,
                    "name `{name}` is used by both a [[type]] and an [[enum]]: names must be unique across both"
                )?;
                if let Some(hint) = suggestion {
                    write!(f, " — {hint}")?;
                }
                Ok(())
            }

            PolyplugcError::GuestGenerationNotSupported { language, reason } => {
                write!(
                    f,
                    "guest generation not supported for `{language}`: {reason}"
                )
            }

            PolyplugcError::HostContractNameMissingPrefix { name } => {
                write!(
                    f,
                    "host contract name `{name}` must start with \"host.\" prefix (e.g., \"host.logger\")"
                )
            }

            PolyplugcError::DuplicateContractName {
                name,
                first_defined_at,
            } => {
                write!(
                    f,
                    "duplicate contract name `{name}`: contract names must be unique across both [[guest_contract]] and [[host_contract]]"
                )?;
                if let Some(loc) = first_defined_at {
                    write!(f, " (first defined at {loc})")?;
                }
                Ok(())
            }

            PolyplugcError::VersionOverflow {
                component,
                value,
                version_str,
                location,
                suggestion,
            } => {
                if let Some(loc) = location {
                    write!(f, "{loc} — ")?;
                }
                write!(
                    f,
                    "version overflow: {component}={value} exceeds maximum 65535 in version `{version_str}`"
                )?;
                if let Some(hint) = suggestion {
                    write!(f, " — did you mean `{hint}`?")?;
                }
                Ok(())
            }

            PolyplugcError::InvalidIdentifier {
                kind,
                name,
                context,
                location,
            } => {
                if let Some(loc) = location {
                    write!(f, "{loc} — ")?;
                }
                write!(
                    f,
                    "invalid {kind} name `{name}` in `{context}`: names must match [A-Za-z_][A-Za-z0-9_]* (a valid identifier)"
                )
            }

            PolyplugcError::DuplicateFunctionName {
                contract,
                function,
                first_defined_at,
            } => {
                write!(
                    f,
                    "duplicate function name `{function}` in contract `{contract}`"
                )?;
                if let Some(loc) = first_defined_at {
                    write!(f, " (first defined at {loc})")?;
                }
                Ok(())
            }

            PolyplugcError::ReservedIdentifier {
                kind,
                name,
                context,
                languages,
                location,
            } => {
                if let Some(loc) = location {
                    write!(f, "{loc} — ")?;
                }
                write!(
                    f,
                    "{kind} name `{name}` in `{context}` is a reserved keyword in: {languages} — \
                     it would produce uncompilable generated code; rename it"
                )
            }
        }
    }
}

impl Error for PolyplugcError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            PolyplugcError::WriteFailed { source, .. } => Some(source),
            PolyplugcError::ReadFailed { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::PolyplugcError;

    #[test]
    fn unknown_type_display() {
        let err: PolyplugcError = PolyplugcError::UnknownType {
            type_ref: "MySpecialType".to_owned(),
            contract: "audio_api".to_owned(),
            location: None,
            suggestion: None,
        };
        let s: String = err.to_string();
        assert!(s.contains("unknown type"), "got: {s}");
        assert!(s.contains("MySpecialType"), "got: {s}");
        assert!(s.contains("audio_api"), "got: {s}");
    }

    #[test]
    fn unknown_type_with_suggestion_display() {
        let err: PolyplugcError = PolyplugcError::UnknownType {
            type_ref: "Striing".to_owned(),
            contract: "audio_api".to_owned(),
            location: None,
            suggestion: Some("StringView".to_owned()),
        };
        let s: String = err.to_string();
        assert!(s.contains("unknown type"), "got: {s}");
        assert!(s.contains("Striing"), "got: {s}");
        assert!(s.contains("StringView"), "got: {s}");
        assert!(s.contains("did you mean"), "got: {s}");
    }

    #[test]
    fn unknown_type_with_location_display() {
        use super::SourceLocation;
        let err: PolyplugcError = PolyplugcError::UnknownType {
            type_ref: "Foo".to_owned(),
            contract: "bar".to_owned(),
            location: Some(SourceLocation {
                file: "api.toml".to_owned(),
                line: 5,
                col: 12,
            }),
            suggestion: None,
        };
        let s: String = err.to_string();
        assert!(s.contains("api.toml"), "got: {s}");
        assert!(s.contains("5"), "got: {s}");
        assert!(s.contains("12"), "got: {s}");
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
    fn toml_parse_error_without_location_display() {
        let err: PolyplugcError = PolyplugcError::TomlParseError {
            message: "expected `=` sign".to_owned(),
            location: None,
        };
        let s: String = err.to_string();
        assert!(s.contains("TOML parse error"), "got: {s}");
        assert!(s.contains("expected `=` sign"), "got: {s}");
    }

    #[test]
    fn toml_parse_error_with_location_display() {
        use super::SourceLocation;
        let err: PolyplugcError = PolyplugcError::TomlParseError {
            message: "expected `=` sign".to_owned(),
            location: Some(SourceLocation {
                file: "bundle.toml".to_owned(),
                line: 3,
                col: 7,
            }),
        };
        let s: String = err.to_string();
        assert!(s.contains("TOML parse error"), "got: {s}");
        assert!(s.contains("bundle.toml"), "got: {s}");
        assert!(s.contains("3"), "got: {s}");
        assert!(s.contains("7"), "got: {s}");
    }

    #[test]
    fn bundle_name_conflict_display() {
        let err: PolyplugcError = PolyplugcError::BundleNameConflict {
            bundle_name: "shared_types".to_owned(),
        };
        let s: String = err.to_string();
        assert!(
            s.contains("conflict") || s.contains("conflicts"),
            "got: {s}"
        );
        assert!(s.contains("shared_types"), "got: {s}");
    }

    #[test]
    fn enum_invalid_repr_display() {
        let err: PolyplugcError = PolyplugcError::EnumInvalidRepr {
            enum_name: "Color".to_owned(),
            repr: "i32".to_owned(),
            suggestion: None,
        };
        let s: String = err.to_string();
        assert!(s.contains("invalid repr"), "got: {s}");
        assert!(s.contains("Color"), "got: {s}");
        assert!(s.contains("i32"), "got: {s}");
    }

    #[test]
    fn enum_invalid_repr_with_suggestion_display() {
        let err: PolyplugcError = PolyplugcError::EnumInvalidRepr {
            enum_name: "Color".to_owned(),
            repr: "u33".to_owned(),
            suggestion: Some("u32".to_owned()),
        };
        let s: String = err.to_string();
        assert!(s.contains("invalid repr"), "got: {s}");
        assert!(s.contains("u33"), "got: {s}");
        assert!(s.contains("u32"), "got: {s}");
        assert!(s.contains("did you mean"), "got: {s}");
    }

    #[test]
    fn enum_invalid_value_expr_display() {
        let err: PolyplugcError = PolyplugcError::EnumInvalidValueExpr {
            enum_name: "Status".to_owned(),
            variant_name: "Pending".to_owned(),
            expr: "???".to_owned(),
        };
        let s: String = err.to_string();
        assert!(
            s.contains("invalid token") || s.contains("invalid"),
            "got: {s}"
        );
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
            suggestion: None,
        };
        let s: String = err.to_string();
        assert!(s.contains("EventKind"), "got: {s}");
        assert!(s.contains("type") || s.contains("enum"), "got: {s}");
    }

    #[test]
    fn version_overflow_display() {
        let err: PolyplugcError = PolyplugcError::VersionOverflow {
            component: "minor".to_owned(),
            value: 70000,
            version_str: "1.70000.0".to_owned(),
            location: None,
            suggestion: None,
        };
        let s: String = err.to_string();
        assert!(s.contains("version overflow"), "got: {s}");
        assert!(s.contains("minor"), "got: {s}");
        assert!(s.contains("70000"), "got: {s}");
        assert!(s.contains("65535"), "got: {s}");
    }

    #[test]
    fn version_overflow_with_location_display() {
        use super::SourceLocation;
        let err: PolyplugcError = PolyplugcError::VersionOverflow {
            component: "patch".to_owned(),
            value: 99999,
            version_str: "1.0.99999".to_owned(),
            location: Some(SourceLocation {
                file: "api.toml".to_owned(),
                line: 8,
                col: 11,
            }),
            suggestion: Some("use a value <= 65535".to_owned()),
        };
        let s: String = err.to_string();
        assert!(s.contains("version overflow"), "got: {s}");
        assert!(s.contains("api.toml"), "got: {s}");
        assert!(s.contains("8"), "got: {s}");
        assert!(s.contains("did you mean"), "got: {s}");
    }

    #[test]
    fn host_contract_name_missing_prefix_display() {
        let err: PolyplugcError = PolyplugcError::HostContractNameMissingPrefix {
            name: "logger".to_owned(),
        };
        let s: String = err.to_string();
        assert!(s.contains("logger"), "got: {s}");
        assert!(s.contains("host."), "got: {s}");
        assert!(s.contains("must start with"), "got: {s}");
    }

    #[test]
    fn duplicate_contract_name_display() {
        let err: PolyplugcError = PolyplugcError::DuplicateContractName {
            name: "shared.api".to_owned(),
            first_defined_at: None,
        };
        let s: String = err.to_string();
        assert!(s.contains("duplicate"), "got: {s}");
        assert!(s.contains("shared.api"), "got: {s}");
        assert!(s.contains("unique"), "got: {s}");
    }

    #[test]
    fn duplicate_function_name_display() {
        let err: PolyplugcError = PolyplugcError::DuplicateFunctionName {
            contract: "svc.api".to_owned(),
            function: "run".to_owned(),
            first_defined_at: None,
        };
        let s: String = err.to_string();
        assert!(s.contains("duplicate"), "got: {s}");
        assert!(s.contains("run"), "got: {s}");
        assert!(s.contains("svc.api"), "got: {s}");
    }
}

//! Error types for the SDK validator.

use std::path::PathBuf;

use thiserror::Error;

use crate::ast_grep::AstGrepError;

/// Top-level error type for configuration and validation failures.
///
/// Every variant is fatal: the CLI exits with code 2 when one is returned.
#[derive(Debug, Error)]
pub enum ValidatorError {
    /// The config file could not be read.
    #[error("failed to read config file {path}: {source}")]
    ConfigRead {
        /// Path to the config file.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// The config file is not valid YAML (or fields are malformed).
    #[error("failed to parse YAML config {path}: {source}")]
    ConfigParse {
        /// Path to the config file.
        path: PathBuf,
        /// The underlying YAML error.
        #[source]
        source: serde_yaml::Error,
    },

    /// The config declares a version other than 1.
    #[error("unsupported config version: {version} (only version 1 is supported)")]
    UnsupportedConfigVersion {
        /// The declared version.
        version: u32,
    },

    /// The same method is listed twice for one struct.
    #[error("duplicate method '{method}' found in struct '{struct_name}'")]
    DuplicateMethod {
        /// The struct containing the duplicate.
        struct_name: String,
        /// The duplicated method name.
        method: String,
    },

    /// A method name is not a plain identifier (it is interpolated into
    /// ast-grep rules, so anything else would corrupt the rule).
    #[error(
        "invalid method name '{method}' in struct '{struct_name}': must be a snake_case identifier"
    )]
    InvalidMethodName {
        /// The struct containing the method.
        struct_name: String,
        /// The offending method name.
        method: String,
    },

    /// A `targets:` or `naming:` key is not one of the known languages.
    #[error(
        "unknown language '{language}' in config section '{section}' (known: rust, python, csharp, cpp, js, lua)"
    )]
    UnknownLanguage {
        /// The config section containing the key.
        section: String,
        /// The unknown language key.
        language: String,
    },

    /// A language listed under `targets:` has no `naming:` entry.
    #[error(
        "no naming convention configured for language '{language}' (add it to the `naming:` section)"
    )]
    MissingNamingConvention {
        /// The language missing a naming entry.
        language: String,
    },

    /// A `naming:` entry is not a recognized convention.
    #[error(
        "invalid naming convention '{value}' for language '{language}' (expected snake_case, PascalCase, or camelCase)"
    )]
    InvalidNamingConvention {
        /// The language with the invalid entry.
        language: String,
        /// The invalid convention string.
        value: String,
    },

    /// An `enum_targets:` entry references an enum absent from `enums:`.
    #[error(
        "unknown enum '{enum_name}' in enum_targets for language '{language}' (add it to the `enums:` section)"
    )]
    UnknownEnum {
        /// The language whose targets reference the enum.
        language: String,
        /// The enum name missing from `enums:`.
        enum_name: String,
    },

    /// An enum name in `enums:` is not a plain identifier (it is
    /// interpolated into ast-grep rules, so anything else would corrupt the
    /// rule).
    #[error("invalid enum name '{enum_name}': must be a PascalCase identifier")]
    InvalidEnumName {
        /// The offending enum name.
        enum_name: String,
    },

    /// The same variant is listed twice for one enum (serde_yaml silently
    /// overwrites duplicate mapping keys, so this is detected explicitly).
    #[error("duplicate variant '{variant}' found in enum '{enum_name}'")]
    DuplicateVariant {
        /// The enum containing the duplicate.
        enum_name: String,
        /// The duplicated variant name.
        variant: String,
    },

    /// A variant name in `enums:` is not a plain identifier.
    #[error(
        "invalid variant name '{variant}' in enum '{enum_name}': must be a PascalCase identifier"
    )]
    InvalidVariantName {
        /// The enum containing the variant.
        enum_name: String,
        /// The offending variant name.
        variant: String,
    },

    /// A configured target file does not exist on disk.
    #[error("target file for language '{language}' does not exist: {path}")]
    TargetFileMissing {
        /// The language the file belongs to.
        language: String,
        /// The missing path.
        path: PathBuf,
    },

    /// A target file could not be read.
    #[error("failed to read {path}: {source}")]
    FileRead {
        /// The unreadable path.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// tree-sitter failed to produce a parse tree for a Lua file.
    #[error("failed to parse Lua file {path}")]
    LuaParse {
        /// The unparseable path.
        path: PathBuf,
    },

    /// The Lua tree-sitter parser could not be initialized.
    #[error("failed to initialize Lua parser: {message}")]
    LuaInit {
        /// The tree-sitter error message.
        message: String,
    },

    /// An ast-grep CLI failure (not found, execution failed, bad JSON).
    #[error(transparent)]
    AstGrep(#[from] AstGrepError),
}

//! CLI argument parsing for SDK validator.
//!
//! This module defines the command-line interface using clap derive macros.

use std::path::PathBuf;

use clap::Parser;

/// SDK Validator - Validates cross-language SDK consistency.
///
/// This tool validates that SDK implementations across different languages
/// (Rust, Python, C#, C++, TypeScript, JavaScript, Lua) are consistent with
/// the golden method set defined in the configuration file.
///
/// The tool uses ast-grep CLI for AST-based code analysis (except Lua, which
/// uses tree-sitter).
///
/// # Exit Codes
///
/// - 0: All methods are implemented in all languages (or no --fail-on-missing)
/// - 1: Some methods are missing (only with --fail-on-missing)
/// - 2: Configuration or runtime error
///
/// # Examples
///
/// Basic validation:
///   sdk-validator --config sdk-validator.yaml
///
/// JSON output for CI:
///   sdk-validator --config sdk-validator.yaml --json
///
/// Filter to specific struct:
///   sdk-validator --config sdk-validator.yaml --struct StringView
///
/// Fail on missing methods (for CI):
///   sdk-validator --config sdk-validator.yaml --fail-on-missing
#[derive(Debug, Parser)]
#[command(
    name = "sdk-validator",
    about = "Validates cross-language SDK consistency",
    version
)]
pub struct Args {
    /// Path to YAML configuration file.
    ///
    /// The configuration file defines:
    /// - The golden method set (authoritative, NOT extracted from code)
    /// - Naming conventions per language
    /// - Target SDK file paths for each language
    #[arg(short, long, value_name = "FILE")]
    pub config: PathBuf,

    /// Output as JSON instead of human-readable table.
    ///
    /// Useful for CI integration and programmatic consumption.
    #[arg(short, long)]
    pub json: bool,

    /// Filter validation to a specific struct.
    ///
    /// Only validate methods for the named struct.
    /// Example: --struct StringView
    #[arg(short, long = "struct", value_name = "NAME")]
    pub struct_name: Option<String>,

    /// Exit with code 1 if any methods are missing.
    ///
    /// Without this flag, the tool always exits with code 0 (unless there's
    /// an error). With this flag, it exits with code 1 if any required
    /// methods are not implemented in any language.
    ///
    /// Recommended for CI pipelines to block merges when SDKs are incomplete.
    #[arg(short = 'f', long)]
    pub fail_on_missing: bool,
}

impl Args {
    /// Parse command-line arguments.
    ///
    /// This is a convenience wrapper around `Args::parse()`.
    pub fn parse_args() -> Self {
        Self::parse()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_args_parse_config_required() {
        let args: Result<Args, clap::Error> = Args::try_parse_from(["sdk-validator"]);
        assert!(args.is_err());
    }

    #[test]
    fn test_args_parse_basic() {
        let args: Args = Args::parse_from(["sdk-validator", "--config", "test.yaml"]);
        assert_eq!(args.config, PathBuf::from("test.yaml"));
        assert!(!args.json);
        assert!(args.struct_name.is_none());
        assert!(!args.fail_on_missing);
    }

    #[test]
    fn test_args_parse_all_flags() {
        let args: Args = Args::parse_from([
            "sdk-validator",
            "--config",
            "test.yaml",
            "--json",
            "--struct",
            "StringView",
            "--fail-on-missing",
        ]);
        assert_eq!(args.config, PathBuf::from("test.yaml"));
        assert!(args.json);
        assert_eq!(args.struct_name, Some("StringView".to_string()));
        assert!(args.fail_on_missing);
    }

    #[test]
    fn test_args_parse_short_flags() {
        let args: Args = Args::parse_from([
            "sdk-validator",
            "-c",
            "test.yaml",
            "-j",
            "-s",
            "Buffer",
            "-f",
        ]);
        assert_eq!(args.config, PathBuf::from("test.yaml"));
        assert!(args.json);
        assert_eq!(args.struct_name, Some("Buffer".to_string()));
        assert!(args.fail_on_missing);
    }
}

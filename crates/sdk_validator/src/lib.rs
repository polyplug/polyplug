//! SDK Validator - Cross-language SDK consistency validation tool.
//!
//! Validates that the golden helper method set defined in
//! `sdk_validator.yaml` is implemented in every language SDK (Rust, Python,
//! C#, C++, JavaScript/TypeScript, and Lua). Detection is AST-based:
//! ast-grep for five languages, tree-sitter for Lua.

use std::process::ExitCode;

pub mod aggregator;
pub mod ast_grep;
pub mod cli;
pub mod config;
pub mod error;
pub mod languages;
pub mod reporter;

pub use aggregator::{
    LanguageReport, MethodStatus, MissingDetail, StructReport, ValidationReport, aggregate_results,
};
pub use ast_grep::{AstGrepError, AstGrepRunner, Match, NamingConvention, transform_name};
pub use config::{Config, filter_to_struct, parse_config};
pub use error::ValidatorError;
pub use reporter::Reporter;

/// Run the validator end to end and produce the process exit code.
///
/// Exit code semantics:
/// - `0`: validation ran; nothing missing (or `--fail-on-missing` not set)
/// - `1`: validation ran; methods are missing and `--fail-on-missing` is set
/// - `2` (via [`ValidatorError`] in `main`): configuration or tool error
///
/// # Errors
///
/// Returns a [`ValidatorError`] for config errors (unknown languages, missing
/// naming entries, missing target files) and tool failures (ast-grep not
/// installed or failing, Lua parser init failure).
pub fn run(args: cli::Args) -> Result<ExitCode, ValidatorError> {
    let config: Config = parse_config(&args.config)?;
    let filtered_config: Config = filter_to_struct(&config, args.struct_name.as_deref());
    let runner: AstGrepRunner = AstGrepRunner::detect()?;
    let report: ValidationReport = aggregate_results(&filtered_config, &runner)?;
    let reporter: Reporter = Reporter::new();

    let output: String = if args.json {
        reporter.generate_json(&report)
    } else {
        reporter.generate_table(&report)
    };

    println!("{output}");

    if args.fail_on_missing && !report.is_complete {
        Ok(ExitCode::from(1))
    } else {
        Ok(ExitCode::SUCCESS)
    }
}

//! ast-grep CLI orchestrator for SDK validation.
//!
//! Provides naming-convention transforms and a thin runner that executes the
//! ast-grep CLI with inline YAML rules and parses its JSON output.

use core::str::FromStr;
use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors that can occur during ast-grep operations.
#[derive(Debug, Error)]
pub enum AstGrepError {
    /// ast-grep CLI is not installed or not found in PATH.
    #[error(
        "ast-grep CLI not found (tried `ast-grep` and `sg`). Install it with `cargo install ast-grep --locked` or see https://ast-grep.github.io/guide/introduction.html"
    )]
    CliNotFound,

    /// ast-grep CLI execution failed.
    #[error("ast-grep execution failed: {message}")]
    ExecutionFailed {
        /// Error message from ast-grep.
        message: String,
    },

    /// Failed to parse ast-grep JSON output.
    #[error("failed to parse ast-grep output: {source}")]
    ParseError {
        /// The underlying JSON error.
        #[source]
        source: serde_json::Error,
    },

    /// Invalid naming convention specified.
    #[error("invalid naming convention: {convention}")]
    InvalidNamingConvention {
        /// The invalid convention string.
        convention: String,
    },
}

/// Supported naming conventions for method/function names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NamingConvention {
    /// snake_case: `to_str`, `get_user_by_id`
    Snake,
    /// PascalCase: `ToStr`, `GetUserById`
    Pascal,
    /// camelCase: `toStr`, `getUserById`
    Camel,
}

impl FromStr for NamingConvention {
    type Err = AstGrepError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "snake_case" | "snake" => Ok(Self::Snake),
            "pascalcase" | "pascal_case" | "pascal" => Ok(Self::Pascal),
            "camelcase" | "camel_case" | "camel" => Ok(Self::Camel),
            _ => Err(AstGrepError::InvalidNamingConvention {
                convention: s.to_string(),
            }),
        }
    }
}

/// Transform a name from one naming convention to another.
///
/// # Examples
///
/// ```
/// use sdk_validator::ast_grep::{transform_name, NamingConvention};
///
/// let result: String = transform_name("to_str", NamingConvention::Snake, NamingConvention::Pascal);
/// assert_eq!(result, "ToStr");
///
/// let result: String = transform_name("to_str", NamingConvention::Snake, NamingConvention::Camel);
/// assert_eq!(result, "toStr");
/// ```
pub fn transform_name(name: &str, from: NamingConvention, to: NamingConvention) -> String {
    if from == to {
        return name.to_string();
    }

    let words: Vec<String> = match from {
        NamingConvention::Snake => split_snake_case(name),
        NamingConvention::Pascal | NamingConvention::Camel => split_capitalized(name),
    };

    match to {
        NamingConvention::Snake => words.join("_").to_lowercase(),
        NamingConvention::Pascal => words
            .iter()
            .map(|w| capitalize_first(w))
            .collect::<Vec<String>>()
            .join(""),
        NamingConvention::Camel => {
            let mut result: String = String::new();
            for (i, word) in words.iter().enumerate() {
                if i == 0 {
                    result.push_str(&word.to_lowercase());
                } else {
                    result.push_str(&capitalize_first(word));
                }
            }
            result
        }
    }
}

/// Split a snake_case name into words.
fn split_snake_case(name: &str) -> Vec<String> {
    name.split('_').map(|s| s.to_string()).collect()
}

/// Split a PascalCase or camelCase name into words at uppercase boundaries.
fn split_capitalized(name: &str) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    let mut current_word: String = String::new();

    for ch in name.chars() {
        if ch.is_uppercase() && !current_word.is_empty() {
            words.push(current_word.clone());
            current_word.clear();
        }
        current_word.push(ch);
    }

    if !current_word.is_empty() {
        words.push(current_word);
    }

    words
}

/// Capitalize the first letter of a word.
fn capitalize_first(word: &str) -> String {
    let mut chars: core::str::Chars<'_> = word.chars();
    match chars.next() {
        Some(first) => {
            first.to_uppercase().collect::<String>() + chars.as_str().to_lowercase().as_str()
        }
        None => String::new(),
    }
}

/// A single match from ast-grep.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Match {
    /// The matched text.
    pub text: String,
    /// The range of the match in the file.
    pub range: Range,
    /// The file path where the match was found.
    pub file: String,
}

/// A range in a source file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Range {
    /// The byte offset range.
    #[serde(rename = "byteOffset")]
    pub byte_offset: ByteOffset,
}

/// Byte offset range.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ByteOffset {
    /// The start position (byte offset).
    pub start: usize,
    /// The end position (byte offset).
    pub end: usize,
}

/// Runner for executing ast-grep CLI commands.
///
/// Construct via [`AstGrepRunner::detect`], which resolves a real ast-grep
/// binary (preferring `ast-grep` over `sg`, and rejecting shadow-utils'
/// unrelated `sg(1)` by checking the `--version` output).
pub struct AstGrepRunner {
    /// Verified ast-grep binary name.
    binary: &'static str,
}

impl AstGrepRunner {
    /// Candidate binary names, in preference order.
    const CANDIDATES: [&'static str; 2] = ["ast-grep", "sg"];

    /// Locate a working ast-grep binary on PATH.
    ///
    /// # Errors
    ///
    /// Returns [`AstGrepError::CliNotFound`] if neither `ast-grep` nor `sg`
    /// resolves to a binary whose `--version` output identifies ast-grep.
    pub fn detect() -> Result<Self, AstGrepError> {
        for candidate in Self::CANDIDATES {
            if Self::is_ast_grep(candidate) {
                return Ok(Self { binary: candidate });
            }
        }
        Err(AstGrepError::CliNotFound)
    }

    /// Check that `binary --version` succeeds and reports ast-grep.
    ///
    /// Guards against shadow-utils' `sg(1)`, which shares the binary name.
    fn is_ast_grep(binary: &str) -> bool {
        Command::new(binary)
            .arg("--version")
            .output()
            .map(|output| {
                output.status.success()
                    && String::from_utf8_lossy(&output.stdout).contains("ast-grep")
            })
            .unwrap_or(false)
    }

    /// Run ast-grep with an inline YAML rule on a file.
    ///
    /// # Errors
    ///
    /// Returns an error if ast-grep cannot be spawned, exits with a failure
    /// status (e.g. malformed rule), or produces unparseable JSON.
    pub fn run_with_rule(&self, rule: &str, file_path: &Path) -> Result<Vec<Match>, AstGrepError> {
        let output: std::process::Output = Command::new(self.binary)
            .arg("scan")
            .arg("--inline-rules")
            .arg(rule)
            .arg("--json")
            .arg(file_path)
            .output()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    AstGrepError::CliNotFound
                } else {
                    AstGrepError::ExecutionFailed {
                        message: e.to_string(),
                    }
                }
            })?;

        if !output.status.success() {
            let stderr: std::borrow::Cow<'_, str> = String::from_utf8_lossy(&output.stderr);
            return Err(AstGrepError::ExecutionFailed {
                message: stderr.to_string(),
            });
        }

        let stdout: std::borrow::Cow<'_, str> = String::from_utf8_lossy(&output.stdout);
        if stdout.is_empty() || stdout.trim() == "null" {
            return Ok(Vec::new());
        }

        let matches: Vec<Match> =
            serde_json::from_str(&stdout).map_err(|source| AstGrepError::ParseError { source })?;

        Ok(matches)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transform_snake_to_pascal() {
        assert_eq!(
            transform_name("to_str", NamingConvention::Snake, NamingConvention::Pascal),
            "ToStr"
        );
        assert_eq!(
            transform_name(
                "get_user_by_id",
                NamingConvention::Snake,
                NamingConvention::Pascal
            ),
            "GetUserById"
        );
        assert_eq!(
            transform_name("parse", NamingConvention::Snake, NamingConvention::Pascal),
            "Parse"
        );
    }

    #[test]
    fn test_transform_snake_to_camel() {
        assert_eq!(
            transform_name("to_str", NamingConvention::Snake, NamingConvention::Camel),
            "toStr"
        );
        assert_eq!(
            transform_name(
                "get_user_by_id",
                NamingConvention::Snake,
                NamingConvention::Camel
            ),
            "getUserById"
        );
        assert_eq!(
            transform_name("parse", NamingConvention::Snake, NamingConvention::Camel),
            "parse"
        );
    }

    #[test]
    fn test_transform_pascal_to_snake() {
        assert_eq!(
            transform_name("ToStr", NamingConvention::Pascal, NamingConvention::Snake),
            "to_str"
        );
        assert_eq!(
            transform_name(
                "GetUserById",
                NamingConvention::Pascal,
                NamingConvention::Snake
            ),
            "get_user_by_id"
        );
    }

    #[test]
    fn test_transform_camel_to_pascal() {
        assert_eq!(
            transform_name("toStr", NamingConvention::Camel, NamingConvention::Pascal),
            "ToStr"
        );
        assert_eq!(
            transform_name(
                "getUserById",
                NamingConvention::Camel,
                NamingConvention::Pascal
            ),
            "GetUserById"
        );
    }

    #[test]
    fn test_transform_same_convention() {
        assert_eq!(
            transform_name("to_str", NamingConvention::Snake, NamingConvention::Snake),
            "to_str"
        );
        assert_eq!(
            transform_name("ToStr", NamingConvention::Pascal, NamingConvention::Pascal),
            "ToStr"
        );
        assert_eq!(
            transform_name("toStr", NamingConvention::Camel, NamingConvention::Camel),
            "toStr"
        );
    }

    #[test]
    fn test_transform_empty_string() {
        assert_eq!(
            transform_name("", NamingConvention::Snake, NamingConvention::Pascal),
            ""
        );
    }

    #[test]
    fn test_naming_convention_from_str() -> Result<(), AstGrepError> {
        assert_eq!(
            NamingConvention::from_str("snake_case")?,
            NamingConvention::Snake
        );
        assert_eq!(
            NamingConvention::from_str("PascalCase")?,
            NamingConvention::Pascal
        );
        assert_eq!(
            NamingConvention::from_str("camelCase")?,
            NamingConvention::Camel
        );
        Ok(())
    }

    #[test]
    fn test_naming_convention_from_str_invalid() {
        assert!(matches!(
            NamingConvention::from_str("invalid"),
            Err(AstGrepError::InvalidNamingConvention { .. })
        ));
    }

    fn test_runner() -> AstGrepRunner {
        match AstGrepRunner::detect() {
            Ok(runner) => runner,
            Err(_) => panic!(
                "ast-grep CLI not found. Please install ast-grep: https://ast-grep.github.io/guide/introduction.html"
            ),
        }
    }

    #[test]
    fn test_detect_finds_real_ast_grep() {
        // The test environment is expected to have ast-grep installed.
        let runner: AstGrepRunner = test_runner();
        assert!(AstGrepRunner::CANDIDATES.contains(&runner.binary));
    }

    #[test]
    fn test_is_ast_grep_rejects_non_ast_grep_binary() {
        // `true` exits 0 but prints nothing, so version verification must fail.
        assert!(!AstGrepRunner::is_ast_grep("true"));
        assert!(!AstGrepRunner::is_ast_grep("/nonexistent/binary"));
    }

    #[test]
    fn test_run_with_rule_bad_rule_is_error() {
        let runner: AstGrepRunner = test_runner();
        let result: Result<Vec<Match>, AstGrepError> =
            runner.run_with_rule("garbage: [not a rule", Path::new("Cargo.toml"));
        assert!(matches!(result, Err(AstGrepError::ExecutionFailed { .. })));
    }
}

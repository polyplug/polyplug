//! ast-grep CLI orchestrator for SDK validation.
//!
//! This module provides functionality to run ast-grep CLI as a subprocess
//! for validating method naming conventions across different languages.

use core::str::FromStr;
use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};
use thiserror::Error;

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur during ast-grep operations.
#[derive(Debug, Error)]
pub enum AstGrepError {
    /// ast-grep CLI is not installed or not found in PATH.
    #[error(
        "ast-grep CLI not found. Please install ast-grep: https://ast-grep.github.io/guide/introduction.html"
    )]
    CliNotFound,

    /// ast-grep CLI execution failed.
    #[error("ast-grep execution failed: {message}")]
    ExecutionFailed {
        /// Error message from ast-grep.
        message: String,
    },

    /// Failed to parse ast-grep JSON output.
    #[error("Failed to parse ast-grep output: {source}")]
    ParseError {
        /// The underlying JSON error.
        #[source]
        source: serde_json::Error,
    },

    /// Invalid language specified.
    #[error(
        "Invalid language: {language}. Supported languages: rust, python, csharp, cpp, typescript, javascript"
    )]
    InvalidLanguage {
        /// The invalid language string.
        language: String,
    },

    /// Invalid naming convention specified.
    #[error("Invalid naming convention: {convention}")]
    InvalidNamingConvention {
        /// The invalid convention string.
        convention: String,
    },
}

// ============================================================================
// Naming Convention Types
// ============================================================================

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

impl NamingConvention {
    /// Convert to a human-readable string.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Snake => "snake_case",
            Self::Pascal => "PascalCase",
            Self::Camel => "camelCase",
        }
    }
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

// ============================================================================
// Naming Convention Transformations
// ============================================================================

/// Transform a name from one naming convention to another.
///
/// # Arguments
///
/// * `name` - The name to transform.
/// * `from` - The source naming convention.
/// * `to` - The target naming convention.
///
/// # Returns
///
/// The transformed name in the target convention.
///
/// # Examples
///
/// ```
/// use sdk_validator::ast_grep::{transform_name, NamingConvention};
///
/// let result = transform_name("to_str", NamingConvention::Snake, NamingConvention::Pascal);
/// assert_eq!(result, "ToStr");
///
/// let result = transform_name("to_str", NamingConvention::Snake, NamingConvention::Camel);
/// assert_eq!(result, "toStr");
/// ```
pub fn transform_name(name: &str, from: NamingConvention, to: NamingConvention) -> String {
    // If same convention, return as-is
    if from == to {
        return name.to_string();
    }

    // First, split into words based on source convention
    let words: Vec<String> = match from {
        NamingConvention::Snake => split_snake_case(name),
        NamingConvention::Pascal => split_pascal_case(name),
        NamingConvention::Camel => split_camel_case(name),
    };

    // Then, join words in target convention
    match to {
        NamingConvention::Snake => words.join("_").to_lowercase(),
        NamingConvention::Pascal => words
            .iter()
            .map(|w| capitalize_first(w))
            .collect::<Vec<_>>()
            .join(""),
        NamingConvention::Camel => {
            let mut result = String::new();
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

/// Split a PascalCase name into words.
fn split_pascal_case(name: &str) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    let mut current_word = String::new();

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

/// Split a camelCase name into words.
fn split_camel_case(name: &str) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    let mut current_word = String::new();

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
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => {
            first.to_uppercase().collect::<String>() + chars.as_str().to_lowercase().as_str()
        }
        None => String::new(),
    }
}

// ============================================================================
// ast-grep Rule Generation
// ============================================================================

/// Supported languages for ast-grep pattern matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    /// Rust
    Rust,
    /// Python
    Python,
    /// C#
    CSharp,
    /// C++
    Cpp,
    /// TypeScript
    TypeScript,
    /// JavaScript
    JavaScript,
}

impl Language {
    /// Convert to ast-grep language string.
    pub fn as_ast_grep_str(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Python => "python",
            Self::CSharp => "csharp",
            Self::Cpp => "cpp",
            Self::TypeScript => "typescript",
            Self::JavaScript => "javascript",
        }
    }
}

impl FromStr for Language {
    type Err = AstGrepError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "rust" | "rs" => Ok(Self::Rust),
            "python" | "py" => Ok(Self::Python),
            "csharp" | "cs" | "c#" => Ok(Self::CSharp),
            "cpp" | "c++" | "cc" | "cxx" => Ok(Self::Cpp),
            "typescript" | "ts" => Ok(Self::TypeScript),
            "javascript" | "js" => Ok(Self::JavaScript),
            _ => Err(AstGrepError::InvalidLanguage {
                language: s.to_string(),
            }),
        }
    }
}

/// Generate an ast-grep pattern for finding a method/function by name.
///
/// # Arguments
///
/// * `language` - The target language.
/// * `method_name` - The method/function name to search for.
/// * `naming` - The naming convention used in the target language.
///
/// # Returns
///
/// An ast-grep pattern string that matches the method/function.
///
/// # Examples
///
/// ```
/// use sdk_validator::ast_grep::{generate_rule, Language, NamingConvention};
///
/// let pattern = generate_rule(Language::Rust, "to_str", NamingConvention::Snake);
/// assert!(pattern.contains("to_str"));
///
/// let pattern = generate_rule(Language::CSharp, "to_str", NamingConvention::Snake);
/// assert!(pattern.contains("ToStr"));
/// ```
pub fn generate_rule(language: Language, method_name: &str, naming: NamingConvention) -> String {
    match language {
        Language::Rust => generate_rust_pattern(method_name, naming),
        Language::Python => generate_python_pattern(method_name, naming),
        Language::CSharp => generate_csharp_pattern(method_name, naming),
        Language::Cpp => generate_cpp_pattern(method_name, naming),
        Language::TypeScript => generate_typescript_pattern(method_name, naming),
        Language::JavaScript => generate_javascript_pattern(method_name, naming),
    }
}

/// Generate a Rust function pattern.
fn generate_rust_pattern(method_name: &str, naming: NamingConvention) -> String {
    let name = transform_name(method_name, NamingConvention::Snake, naming);
    format!("fn {name}")
}

/// Generate a Python function pattern.
fn generate_python_pattern(method_name: &str, naming: NamingConvention) -> String {
    let name = transform_name(method_name, NamingConvention::Snake, naming);
    format!("def {name}")
}

/// Generate a C# method pattern.
fn generate_csharp_pattern(method_name: &str, naming: NamingConvention) -> String {
    let name = transform_name(method_name, NamingConvention::Snake, naming);
    let pascal_name = transform_name(&name, naming, NamingConvention::Pascal);
    format!("public static $RET {pascal_name}($$$)")
}

/// Generate a C++ function pattern.
fn generate_cpp_pattern(method_name: &str, naming: NamingConvention) -> String {
    transform_name(method_name, NamingConvention::Snake, naming)
}

/// Generate a TypeScript function pattern.
fn generate_typescript_pattern(method_name: &str, naming: NamingConvention) -> String {
    let name = transform_name(method_name, NamingConvention::Snake, naming);
    let camel_name = transform_name(&name, naming, NamingConvention::Camel);
    format!("function {camel_name}")
}

/// Generate a JavaScript function pattern.
fn generate_javascript_pattern(method_name: &str, naming: NamingConvention) -> String {
    let name = transform_name(method_name, NamingConvention::Snake, naming);
    let camel_name = transform_name(&name, naming, NamingConvention::Camel);
    format!("function {camel_name}")
}

// ============================================================================
// ast-grep Match Result Types
// ============================================================================

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

// ============================================================================
// AstGrepRunner
// ============================================================================

/// Runner for executing ast-grep CLI commands.
///
/// This struct manages the execution of ast-grep as a subprocess
/// and parses the JSON output.
pub struct AstGrepRunner {
    /// Path to the ast-grep binary. If None, uses "sg" from PATH.
    binary_path: Option<String>,
}

impl AstGrepRunner {
    /// Create a new AstGrepRunner using "sg" from PATH.
    pub fn new() -> Self {
        Self { binary_path: None }
    }

    /// Create a new AstGrepRunner with a specific binary path.
    pub fn with_binary_path(binary_path: String) -> Self {
        Self {
            binary_path: Some(binary_path),
        }
    }

    /// Check if ast-grep is available.
    pub fn is_available(&self) -> bool {
        let binary = self.binary_path.as_deref().unwrap_or("sg");
        Command::new(binary)
            .arg("--version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    /// Get the path to the ast-grep binary being used.
    pub fn binary_location(&self) -> &str {
        self.binary_path.as_deref().unwrap_or("sg")
    }

    /// Check if ast-grep can be found in PATH.
    pub fn can_find_in_path() -> bool {
        std::env::var("PATH")
            .map(|path| {
                path.split(':')
                    .any(|dir| std::path::Path::new(dir).join("sg").exists())
            })
            .unwrap_or(false)
    }

    /// Run ast-grep with a pattern on a file.
    ///
    /// # Arguments
    ///
    /// * `pattern` - The ast-grep pattern to search for.
    /// * `language` - The language to parse the file as.
    /// * `file_path` - The path to the file to search.
    ///
    /// # Returns
    ///
    /// A vector of matches found in the file.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - ast-grep is not installed
    /// - ast-grep execution fails
    /// - The JSON output cannot be parsed
    pub fn run_ast_grep(
        &self,
        pattern: &str,
        language: Language,
        file_path: &Path,
    ) -> Result<Vec<Match>, AstGrepError> {
        let binary = self.binary_path.as_deref().unwrap_or("sg");

        // Build the command
        let output = Command::new(binary)
            .arg("--pattern")
            .arg(pattern)
            .arg("--lang")
            .arg(language.as_ast_grep_str())
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

        // Check for execution failure
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AstGrepError::ExecutionFailed {
                message: stderr.to_string(),
            });
        }

        // Parse JSON output
        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.is_empty() || stdout == "null" {
            return Ok(Vec::new());
        }

        let matches: Vec<Match> =
            serde_json::from_str(&stdout).map_err(|source| AstGrepError::ParseError { source })?;

        Ok(matches)
    }

    /// Run ast-grep with an inline YAML rule.
    ///
    /// # Arguments
    ///
    /// * `rule` - The YAML rule string.
    /// * `file_path` - The path to the file to search.
    ///
    /// # Returns
    ///
    /// A vector of matches found in the file.
    pub fn run_with_rule(&self, rule: &str, file_path: &Path) -> Result<Vec<Match>, AstGrepError> {
        let binary = self.binary_path.as_deref().unwrap_or("sg");

        let output = Command::new(binary)
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
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AstGrepError::ExecutionFailed {
                message: stderr.to_string(),
            });
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.is_empty() || stdout == "null" {
            return Ok(Vec::new());
        }

        let matches: Vec<Match> =
            serde_json::from_str(&stdout).map_err(|source| AstGrepError::ParseError { source })?;

        Ok(matches)
    }
}

impl Default for AstGrepRunner {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // Naming Convention Transformation Tests
    // -------------------------------------------------------------------------

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
    fn test_transform_pascal_to_camel() {
        assert_eq!(
            transform_name("ToStr", NamingConvention::Pascal, NamingConvention::Camel),
            "toStr"
        );
        assert_eq!(
            transform_name(
                "GetUserById",
                NamingConvention::Pascal,
                NamingConvention::Camel
            ),
            "getUserById"
        );
    }

    #[test]
    fn test_transform_camel_to_snake() {
        assert_eq!(
            transform_name("toStr", NamingConvention::Camel, NamingConvention::Snake),
            "to_str"
        );
        assert_eq!(
            transform_name(
                "getUserById",
                NamingConvention::Camel,
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
        // Same convention should return unchanged
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
    fn test_transform_single_word() {
        assert_eq!(
            transform_name("parse", NamingConvention::Snake, NamingConvention::Pascal),
            "Parse"
        );
        assert_eq!(
            transform_name("Parse", NamingConvention::Pascal, NamingConvention::Snake),
            "parse"
        );
    }

    // -------------------------------------------------------------------------
    // NamingConvention Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_naming_convention_from_str() {
        assert_eq!(
            NamingConvention::from_str("snake_case").unwrap(),
            NamingConvention::Snake
        );
        assert_eq!(
            NamingConvention::from_str("snake").unwrap(),
            NamingConvention::Snake
        );
        assert_eq!(
            NamingConvention::from_str("PascalCase").unwrap(),
            NamingConvention::Pascal
        );
        assert_eq!(
            NamingConvention::from_str("pascal").unwrap(),
            NamingConvention::Pascal
        );
        assert_eq!(
            NamingConvention::from_str("camelCase").unwrap(),
            NamingConvention::Camel
        );
        assert_eq!(
            NamingConvention::from_str("camel").unwrap(),
            NamingConvention::Camel
        );
    }

    #[test]
    fn test_naming_convention_from_str_invalid() {
        assert!(matches!(
            NamingConvention::from_str("invalid"),
            Err(AstGrepError::InvalidNamingConvention { .. })
        ));
    }

    #[test]
    fn test_naming_convention_to_str() {
        assert_eq!(NamingConvention::Snake.as_str(), "snake_case");
        assert_eq!(NamingConvention::Pascal.as_str(), "PascalCase");
        assert_eq!(NamingConvention::Camel.as_str(), "camelCase");
    }

    // -------------------------------------------------------------------------
    // Language Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_language_from_str() {
        assert_eq!(Language::from_str("rust").unwrap(), Language::Rust);
        assert_eq!(Language::from_str("rs").unwrap(), Language::Rust);
        assert_eq!(Language::from_str("python").unwrap(), Language::Python);
        assert_eq!(Language::from_str("py").unwrap(), Language::Python);
        assert_eq!(Language::from_str("csharp").unwrap(), Language::CSharp);
        assert_eq!(Language::from_str("cs").unwrap(), Language::CSharp);
        assert_eq!(Language::from_str("c#").unwrap(), Language::CSharp);
        assert_eq!(Language::from_str("cpp").unwrap(), Language::Cpp);
        assert_eq!(Language::from_str("c++").unwrap(), Language::Cpp);
        assert_eq!(
            Language::from_str("typescript").unwrap(),
            Language::TypeScript
        );
        assert_eq!(Language::from_str("ts").unwrap(), Language::TypeScript);
        assert_eq!(
            Language::from_str("javascript").unwrap(),
            Language::JavaScript
        );
        assert_eq!(Language::from_str("js").unwrap(), Language::JavaScript);
    }

    #[test]
    fn test_language_from_str_invalid() {
        assert!(matches!(
            Language::from_str("invalid"),
            Err(AstGrepError::InvalidLanguage { .. })
        ));
    }

    #[test]
    fn test_language_to_ast_grep_str() {
        assert_eq!(Language::Rust.as_ast_grep_str(), "rust");
        assert_eq!(Language::Python.as_ast_grep_str(), "python");
        assert_eq!(Language::CSharp.as_ast_grep_str(), "csharp");
        assert_eq!(Language::Cpp.as_ast_grep_str(), "cpp");
        assert_eq!(Language::TypeScript.as_ast_grep_str(), "typescript");
        assert_eq!(Language::JavaScript.as_ast_grep_str(), "javascript");
    }

    // -------------------------------------------------------------------------
    // Rule Generation Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_generate_rust_rule() {
        let pattern = generate_rule(Language::Rust, "to_str", NamingConvention::Snake);
        assert!(pattern.contains("to_str"));
        assert!(pattern.contains("fn"));
    }

    #[test]
    fn test_generate_python_rule() {
        let pattern = generate_rule(Language::Python, "to_str", NamingConvention::Snake);
        assert!(pattern.contains("to_str"));
        assert!(pattern.contains("def"));
    }

    #[test]
    fn test_generate_csharp_rule() {
        let pattern = generate_rule(Language::CSharp, "to_str", NamingConvention::Snake);
        // C# should transform to PascalCase
        assert!(pattern.contains("ToStr"));
        assert!(pattern.contains("public static"));
    }

    #[test]
    fn test_generate_cpp_rule() {
        let pattern = generate_rule(Language::Cpp, "to_str", NamingConvention::Snake);
        assert!(pattern.contains("to_str"));
    }

    #[test]
    fn test_generate_typescript_rule() {
        let pattern = generate_rule(Language::TypeScript, "to_str", NamingConvention::Snake);
        // TypeScript should transform to camelCase
        assert!(pattern.contains("toStr"));
        assert!(pattern.contains("function"));
    }

    #[test]
    fn test_generate_javascript_rule() {
        let pattern = generate_rule(Language::JavaScript, "to_str", NamingConvention::Snake);
        // JavaScript should transform to camelCase
        assert!(pattern.contains("toStr"));
        assert!(pattern.contains("function"));
    }

    // -------------------------------------------------------------------------
    // AstGrepRunner Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_runner_new() {
        let runner = AstGrepRunner::new();
        assert!(runner.binary_path.is_none());
    }

    #[test]
    fn test_runner_with_binary_path() {
        let runner = AstGrepRunner::with_binary_path("/usr/local/bin/sg".to_string());
        assert_eq!(runner.binary_path, Some("/usr/local/bin/sg".to_string()));
    }

    #[test]
    fn test_runner_default() {
        let runner = AstGrepRunner::default();
        assert!(runner.binary_path.is_none());
    }
}

//! Language-specific validation modules.
//!
//! Each language has its own validator that uses the ast-grep CLI to detect
//! method/function definitions in SDK files. Lua uses tree-sitter instead
//! since ast-grep doesn't support Lua.
//!
//! Validation is per-file: every target file listed for a language must
//! independently implement every golden method.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::ast_grep::{AstGrepRunner, NamingConvention, transform_name};
use crate::error::ValidatorError;

pub mod cpp;
pub mod csharp;
pub mod js;
pub mod lua;
pub mod python;
pub mod rust;

pub use cpp::CppValidator;
pub use csharp::CSharpValidator;
pub use js::JsValidator;
pub use lua::LuaValidator;
pub use python::PythonValidator;
pub use rust::RustValidator;

/// A method missing from a language SDK, with the files it is missing from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MissingMethod {
    /// The canonical (snake_case) method name.
    pub method: String,
    /// Target files that do not implement the method. Empty when the
    /// language has no target files configured at all.
    pub missing_files: Vec<String>,
}

/// Result of validating a single struct's methods in a language SDK.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ValidationResult {
    /// Name of the struct being validated (e.g., "StringView").
    pub struct_name: String,
    /// Name of the language (e.g., "csharp").
    pub language: String,
    /// Methods found in every target file of the SDK.
    pub found_methods: Vec<String>,
    /// Methods missing from at least one target file.
    pub missing_methods: Vec<MissingMethod>,
}

impl ValidationResult {
    /// Create a new validation result.
    pub fn new(struct_name: String, language: String) -> Self {
        Self {
            struct_name,
            language,
            found_methods: Vec::new(),
            missing_methods: Vec::new(),
        }
    }

    /// Check if all required methods are present in all target files.
    pub fn is_complete(&self) -> bool {
        self.missing_methods.is_empty()
    }
}

/// Trait for language-specific SDK validators.
///
/// Implementations answer a single question: does `native_name` have a real
/// definition (not a call site or comment) in `file`?
pub trait LanguageValidator {
    /// Get the language name for this validator.
    fn language_name(&self) -> &'static str;

    /// Check whether a method definition exists in a single file.
    ///
    /// `native_name` is already transformed to the language's configured
    /// naming convention.
    ///
    /// # Errors
    ///
    /// Returns a [`ValidatorError`] if the detection tool fails; tool
    /// failures are never silently treated as "missing".
    fn method_in_file(
        &mut self,
        runner: &AstGrepRunner,
        native_name: &str,
        file: &Path,
    ) -> Result<bool, ValidatorError>;
}

/// Validate a language SDK against the golden method set, per file.
///
/// A method counts as found only if every target file implements it. The
/// configured naming convention transforms each canonical snake_case name
/// into the language's native spelling before probing.
///
/// # Errors
///
/// Returns a [`ValidatorError`] if a target file does not exist or the
/// underlying detection tool fails.
pub fn validate_language(
    validator: &mut dyn LanguageValidator,
    runner: &AstGrepRunner,
    naming: NamingConvention,
    struct_name: &str,
    required_methods: &[String],
    target_files: &[PathBuf],
) -> Result<ValidationResult, ValidatorError> {
    let language: &'static str = validator.language_name();
    let mut result: ValidationResult =
        ValidationResult::new(struct_name.to_string(), language.to_string());

    for file in target_files {
        if !file.exists() {
            return Err(ValidatorError::TargetFileMissing {
                language: language.to_string(),
                path: file.clone(),
            });
        }
    }

    for method_name in required_methods {
        let native_name: String = transform_name(method_name, NamingConvention::Snake, naming);

        if target_files.is_empty() {
            result.missing_methods.push(MissingMethod {
                method: method_name.clone(),
                missing_files: Vec::new(),
            });
            continue;
        }

        let mut missing_files: Vec<String> = Vec::new();
        for file in target_files {
            if !validator.method_in_file(runner, &native_name, file)? {
                missing_files.push(file.display().to_string());
            }
        }

        if missing_files.is_empty() {
            result.found_methods.push(method_name.clone());
        } else {
            result.missing_methods.push(MissingMethod {
                method: method_name.clone(),
                missing_files,
            });
        }
    }

    result.found_methods.sort();
    result
        .missing_methods
        .sort_by(|a, b| a.method.cmp(&b.method));

    Ok(result)
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::path::PathBuf;

    use crate::ast_grep::AstGrepRunner;

    /// Resolve an ast-grep runner or panic with the install hint.
    pub(crate) fn runner() -> AstGrepRunner {
        match AstGrepRunner::detect() {
            Ok(runner) => runner,
            Err(_) => panic!(
                "ast-grep CLI not found. Please install ast-grep: https://ast-grep.github.io/guide/introduction.html"
            ),
        }
    }

    /// Resolve a path under the repo root (two levels above this crate).
    pub(crate) fn repo_path(relative: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(relative)
    }

    /// The golden StringView method set.
    pub(crate) fn golden_methods() -> Vec<String> {
        vec![
            "to_str".to_string(),
            "starts_with".to_string(),
            "ends_with".to_string(),
            "strip_prefix".to_string(),
            "split".to_string(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validation_result_new() {
        let result: ValidationResult =
            ValidationResult::new("StringView".to_string(), "rust".to_string());
        assert_eq!(result.struct_name, "StringView");
        assert_eq!(result.language, "rust");
        assert!(result.found_methods.is_empty());
        assert!(result.missing_methods.is_empty());
        assert!(result.is_complete());
    }

    #[test]
    fn test_validate_language_empty_targets_marks_all_missing()
    -> Result<(), Box<dyn core::error::Error>> {
        let runner: AstGrepRunner = test_support::runner();
        let mut validator: RustValidator = RustValidator::new();

        let result: ValidationResult = validate_language(
            &mut validator,
            &runner,
            NamingConvention::Snake,
            "StringView",
            &["to_str".to_string()],
            &[],
        )?;

        assert!(result.found_methods.is_empty());
        assert_eq!(result.missing_methods.len(), 1);
        assert_eq!(result.missing_methods[0].method, "to_str");
        assert!(result.missing_methods[0].missing_files.is_empty());
        Ok(())
    }

    #[test]
    fn test_validate_language_missing_target_file_is_fatal() {
        let runner: AstGrepRunner = test_support::runner();
        let mut validator: RustValidator = RustValidator::new();

        let result: Result<ValidationResult, ValidatorError> = validate_language(
            &mut validator,
            &runner,
            NamingConvention::Snake,
            "StringView",
            &["to_str".to_string()],
            &[PathBuf::from("/nonexistent/file.rs")],
        );

        match result {
            Err(ValidatorError::TargetFileMissing { language, path }) => {
                assert_eq!(language, "rust");
                assert_eq!(path, PathBuf::from("/nonexistent/file.rs"));
            }
            other => panic!("expected TargetFileMissing error, got {other:?}"),
        }
    }
}

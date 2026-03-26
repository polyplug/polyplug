//! Python SDK validator using ast-grep CLI.
//!
//! This module provides validation for Python SDK files, detecting
//! standalone function definitions that match the required method names.

use std::path::Path;

use crate::ast_grep::{AstGrepRunner, Language, NamingConvention, generate_rule};
use crate::languages::{LanguageValidator, ValidationResult};

/// Validator for Python SDK files.
///
/// Detects standalone function definitions (e.g., `def to_str(...):`) in
/// Python source files using ast-grep CLI.
pub struct PythonValidator;

impl PythonValidator {
    /// Create a new Python validator.
    pub fn new() -> Self {
        Self
    }
}

impl Default for PythonValidator {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageValidator for PythonValidator {
    fn language_name(&self) -> &'static str {
        "python"
    }

    fn ast_grep_language(&self) -> Language {
        Language::Python
    }

    fn naming_convention(&self) -> NamingConvention {
        NamingConvention::Snake
    }

    fn validate(
        &self,
        runner: &AstGrepRunner,
        struct_name: &str,
        required_methods: &[String],
        target_files: &[String],
    ) -> ValidationResult {
        let mut result: ValidationResult =
            ValidationResult::new(struct_name.to_string(), self.language_name().to_string());

        let naming: NamingConvention = self.naming_convention();
        let language: Language = self.ast_grep_language();

        for method_name in required_methods {
            let pattern: String = generate_rule(language, method_name, naming);
            let mut found: bool = false;

            for file_path_str in target_files {
                let file_path: &Path = Path::new(file_path_str);

                if !file_path.exists() {
                    continue;
                }

                match runner.run_ast_grep(&pattern, language, file_path) {
                    Ok(matches) => {
                        if !matches.is_empty() {
                            found = true;
                            break;
                        }
                    }
                    Err(_) => {
                        continue;
                    }
                }
            }

            if found {
                result.found_methods.push(method_name.clone());
            } else {
                result.missing_methods.push(method_name.clone());
            }
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_temp_python_file(content: &str) -> NamedTempFile {
        let mut file: NamedTempFile =
            NamedTempFile::with_suffix(".py").expect("Failed to create temp file");
        file.write_all(content.as_bytes())
            .expect("Failed to write temp file");
        file.flush().expect("Failed to flush temp file");
        file
    }

    #[test]
    fn test_python_validator_new() {
        let validator: PythonValidator = PythonValidator::new();
        assert_eq!(validator.language_name(), "python");
        assert_eq!(validator.ast_grep_language(), Language::Python);
        assert_eq!(validator.naming_convention(), NamingConvention::Snake);
    }

    #[test]
    fn test_python_validator_default() {
        let validator: PythonValidator = PythonValidator::default();
        assert_eq!(validator.language_name(), "python");
    }

    #[test]
    fn test_python_validator_detects_to_str() {
        let python_code: &str = r#"
def to_str(sv: StringView) -> str:
    """Convert StringView to Python str."""
    return ""
"#;
        let file: NamedTempFile = create_temp_python_file(python_code);
        let runner: AstGrepRunner = AstGrepRunner::new();
        let validator: PythonValidator = PythonValidator::new();

        let result: ValidationResult = validator.validate(
            &runner,
            "StringView",
            &["to_str".to_string()],
            &[file.path().to_string_lossy().to_string()],
        );

        assert!(result.found_methods.contains(&"to_str".to_string()));
        assert!(!result.missing_methods.contains(&"to_str".to_string()));
    }

    #[test]
    fn test_python_validator_detects_multiple_methods() {
        let python_code: &str = r#"
def to_str(sv: StringView) -> str:
    return ""

def starts_with(sv: StringView, prefix: str) -> bool:
    return True

def strip_prefix(sv: StringView, prefix: str) -> str:
    return ""

def split(sv: StringView, delimiter: str) -> list[str]:
    return []
"#;
        let file: NamedTempFile = create_temp_python_file(python_code);
        let runner: AstGrepRunner = AstGrepRunner::new();
        let validator: PythonValidator = PythonValidator::new();

        let result: ValidationResult = validator.validate(
            &runner,
            "StringView",
            &[
                "to_str".to_string(),
                "starts_with".to_string(),
                "ends_with".to_string(),
                "strip_prefix".to_string(),
                "split".to_string(),
            ],
            &[file.path().to_string_lossy().to_string()],
        );

        // Should find these methods
        assert!(result.found_methods.contains(&"to_str".to_string()));
        assert!(result.found_methods.contains(&"starts_with".to_string()));
        assert!(result.found_methods.contains(&"strip_prefix".to_string()));
        assert!(result.found_methods.contains(&"split".to_string()));

        // Should report ends_with as missing
        assert!(result.missing_methods.contains(&"ends_with".to_string()));
        assert!(!result.is_complete());
    }

    #[test]
    fn test_python_validator_handles_type_annotated_functions() {
        let python_code: &str = r#"
def to_str(sv: StringView) -> str:
    """Function with type annotations."""
    return ""
"#;
        let file: NamedTempFile = create_temp_python_file(python_code);
        let runner: AstGrepRunner = AstGrepRunner::new();
        let validator: PythonValidator = PythonValidator::new();

        let result: ValidationResult = validator.validate(
            &runner,
            "StringView",
            &["to_str".to_string()],
            &[file.path().to_string_lossy().to_string()],
        );

        assert!(result.found_methods.contains(&"to_str".to_string()));
    }

    #[test]
    fn test_python_validator_handles_functions_without_annotations() {
        let python_code: &str = r#"
def to_str(sv):
    return ""
"#;
        let file: NamedTempFile = create_temp_python_file(python_code);
        let runner: AstGrepRunner = AstGrepRunner::new();
        let validator: PythonValidator = PythonValidator::new();

        let result: ValidationResult = validator.validate(
            &runner,
            "StringView",
            &["to_str".to_string()],
            &[file.path().to_string_lossy().to_string()],
        );

        assert!(result.found_methods.contains(&"to_str".to_string()));
    }

    #[test]
    fn test_python_validator_missing_method() {
        let python_code: &str = r#"
def to_str(sv):
    return ""
"#;
        let file: NamedTempFile = create_temp_python_file(python_code);
        let runner: AstGrepRunner = AstGrepRunner::new();
        let validator: PythonValidator = PythonValidator::new();

        let result: ValidationResult = validator.validate(
            &runner,
            "StringView",
            &["ends_with".to_string()],
            &[file.path().to_string_lossy().to_string()],
        );

        assert!(result.missing_methods.contains(&"ends_with".to_string()));
        assert!(!result.is_complete());
    }

    #[test]
    fn test_python_validator_nonexistent_file() {
        let runner: AstGrepRunner = AstGrepRunner::new();
        let validator: PythonValidator = PythonValidator::new();

        let result: ValidationResult = validator.validate(
            &runner,
            "StringView",
            &["to_str".to_string()],
            &["/nonexistent/file.py".to_string()],
        );

        // Should report method as missing when file doesn't exist
        assert!(result.missing_methods.contains(&"to_str".to_string()));
    }

    #[test]
    fn test_python_validator_completion_percentage() {
        let python_code: &str = r#"
def to_str(sv):
    return ""

def starts_with(sv, prefix):
    return True
"#;
        let file: NamedTempFile = create_temp_python_file(python_code);
        let runner: AstGrepRunner = AstGrepRunner::new();
        let validator: PythonValidator = PythonValidator::new();

        let result: ValidationResult = validator.validate(
            &runner,
            "StringView",
            &[
                "to_str".to_string(),
                "starts_with".to_string(),
                "ends_with".to_string(),
            ],
            &[file.path().to_string_lossy().to_string()],
        );

        // 2 out of 3 methods found = 66%
        assert_eq!(result.completion_percentage(), 66);
    }

    #[test]
    fn test_python_validator_all_methods_found() {
        let python_code: &str = r#"
def to_str(sv):
    return ""

def starts_with(sv, prefix):
    return True
"#;
        let file: NamedTempFile = create_temp_python_file(python_code);
        let runner: AstGrepRunner = AstGrepRunner::new();
        let validator: PythonValidator = PythonValidator::new();

        let result: ValidationResult = validator.validate(
            &runner,
            "StringView",
            &["to_str".to_string(), "starts_with".to_string()],
            &[file.path().to_string_lossy().to_string()],
        );

        assert!(result.is_complete());
        assert_eq!(result.completion_percentage(), 100);
    }
}

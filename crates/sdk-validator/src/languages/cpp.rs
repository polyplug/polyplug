//! C++ SDK validator using ast-grep CLI.
//!
//! This module provides validation for C++ SDK files, detecting inline functions
//! and namespace-qualified functions in header files.

use std::path::Path;

use crate::ast_grep::{AstGrepRunner, Language, NamingConvention, generate_rule};

use super::{LanguageValidator, ValidationResult};

/// Validator for C++ SDK files.
///
/// Detects inline functions and namespace-qualified functions in C++ header files.
/// C++ uses snake_case for method names.
pub struct CppValidator;

impl CppValidator {
    /// Create a new C++ validator.
    pub fn new() -> Self {
        Self
    }
}

impl Default for CppValidator {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageValidator for CppValidator {
    fn language_name(&self) -> &'static str {
        "cpp"
    }

    fn ast_grep_language(&self) -> Language {
        Language::Cpp
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

        for method in required_methods {
            let found: bool = self.method_exists(runner, method, target_files);
            if found {
                result.found_methods.push(method.clone());
            } else {
                result.missing_methods.push(method.clone());
            }
        }

        result
    }
}

impl CppValidator {
    /// Check if a method exists in the given files.
    ///
    /// # Arguments
    ///
    /// * `runner` - The ast-grep runner to use.
    /// * `method_name` - The method name in snake_case.
    /// * `files` - The list of file paths to search.
    ///
    /// # Returns
    ///
    /// `true` if the method was found in any of the files.
    fn method_exists(&self, runner: &AstGrepRunner, method_name: &str, files: &[String]) -> bool {
        let pattern: String = self.generate_pattern(method_name);

        for file in files {
            let path: &Path = Path::new(file);
            if !path.exists() {
                continue;
            }

            match runner.run_ast_grep(&pattern, self.ast_grep_language(), path) {
                Ok(matches) => {
                    if !matches.is_empty() {
                        return true;
                    }
                }
                Err(_) => {
                    // If ast-grep fails, continue to next file
                    continue;
                }
            }
        }

        false
    }

    /// Generate an ast-grep pattern for finding a C++ function by name.
    ///
    /// # Arguments
    ///
    /// * `method_name` - The method name in snake_case.
    ///
    /// # Returns
    ///
    /// An ast-grep pattern string that matches the function in C++.
    fn generate_pattern(&self, method_name: &str) -> String {
        generate_rule(
            self.ast_grep_language(),
            method_name,
            self.naming_convention(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cpp_validator_new() {
        let validator: CppValidator = CppValidator::new();
        assert_eq!(validator.language_name(), "cpp");
        assert_eq!(validator.ast_grep_language(), Language::Cpp);
        assert_eq!(validator.naming_convention(), NamingConvention::Snake);
    }

    #[test]
    fn test_cpp_validator_default() {
        let validator: CppValidator = CppValidator::default();
        assert_eq!(validator.language_name(), "cpp");
    }

    #[test]
    fn test_cpp_validator_generate_pattern() {
        let validator: CppValidator = CppValidator::new();

        // Test snake_case method names
        let pattern: String = validator.generate_pattern("to_string");
        assert!(pattern.contains("to_string"));

        let pattern: String = validator.generate_pattern("starts_with");
        assert!(pattern.contains("starts_with"));

        let pattern: String = validator.generate_pattern("ends_with");
        assert!(pattern.contains("ends_with"));
    }

    #[test]
    fn test_validation_result_new() {
        let result: ValidationResult =
            ValidationResult::new("StringView".to_string(), "cpp".to_string());

        assert_eq!(result.struct_name, "StringView");
        assert_eq!(result.language, "cpp");
        assert!(result.found_methods.is_empty());
        assert!(result.missing_methods.is_empty());
        assert!(result.is_complete());
        assert_eq!(result.completion_percentage(), 100);
    }

    #[test]
    fn test_validation_result_with_methods() {
        let mut result: ValidationResult =
            ValidationResult::new("StringView".to_string(), "cpp".to_string());

        result.found_methods.push("to_string".to_string());
        result.found_methods.push("starts_with".to_string());
        result.missing_methods.push("ends_with".to_string());

        assert_eq!(result.found_methods.len(), 2);
        assert_eq!(result.missing_methods.len(), 1);
        assert!(!result.is_complete());
        assert_eq!(result.completion_percentage(), 66);
    }

    #[test]
    fn test_validation_result_completion_percentage() {
        let mut result: ValidationResult =
            ValidationResult::new("StringView".to_string(), "cpp".to_string());

        // 0 methods = 100%
        assert_eq!(result.completion_percentage(), 100);

        // 1 found, 0 missing = 100%
        result.found_methods.push("to_string".to_string());
        assert_eq!(result.completion_percentage(), 100);

        // 1 found, 1 missing = 50%
        result.missing_methods.push("ends_with".to_string());
        assert_eq!(result.completion_percentage(), 50);

        // 1 found, 3 missing = 25%
        result.missing_methods.push("starts_with".to_string());
        result.missing_methods.push("strip_prefix".to_string());
        assert_eq!(result.completion_percentage(), 25);
    }

    // Integration test that requires ast-grep CLI
    // This test will be skipped if ast-grep is not installed
    #[test]
    fn test_cpp_validator_with_real_sdk() {
        let runner: AstGrepRunner = AstGrepRunner::new();

        // Skip test if ast-grep is not available
        if !runner.is_available() {
            panic!(
                "ast-grep CLI not found. Please install ast-grep: https://ast-grep.github.io/guide/introduction.html"
            );
        }

        let validator: CppValidator = CppValidator::new();
        let sdk_path: String = "sdks/cpp/abi/polyplug/helpers.hpp".to_string();

        // Skip test if SDK file doesn't exist
        if !Path::new(&sdk_path).exists() {
            eprintln!("Skipping test: C++ SDK file not found");
            return;
        }

        let required_methods: Vec<String> = vec![
            "to_string".to_string(),
            "to_string_view".to_string(),
            "starts_with".to_string(),
            "ends_with".to_string(), // This should be missing
            "strip_prefix".to_string(),
            "split".to_string(),
        ];

        let result: ValidationResult =
            validator.validate(&runner, "StringView", &required_methods, &[sdk_path]);

        // Verify found methods
        assert!(
            result.found_methods.contains(&"to_string".to_string()),
            "to_string should be found"
        );
        assert!(
            result.found_methods.contains(&"to_string_view".to_string()),
            "to_string_view should be found"
        );
        assert!(
            result.found_methods.contains(&"starts_with".to_string()),
            "starts_with should be found"
        );
        assert!(
            result.found_methods.contains(&"strip_prefix".to_string()),
            "strip_prefix should be found"
        );
        assert!(
            result.found_methods.contains(&"split".to_string()),
            "split should be found"
        );

        // Verify missing methods
        assert!(
            result.missing_methods.contains(&"ends_with".to_string()),
            "ends_with should be missing"
        );

        // Verify completion percentage (5/6 = 83%)
        assert_eq!(result.completion_percentage(), 83);
    }
}

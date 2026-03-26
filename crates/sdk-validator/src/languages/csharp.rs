//! C# SDK validator using ast-grep CLI.

use std::path::PathBuf;

use crate::ast_grep::{transform_name, AstGrepRunner, Language, NamingConvention};
use crate::languages::{LanguageValidator, ValidationResult};

/// Validator for C# SDK files.
///
/// Detects methods in C# SDK files using ast-grep CLI. Handles:
/// - Extension methods (`public static string ToString(this StringView sv)`)
/// - Static class methods (`public static StringView FromPtr(...)`)
/// - Expression-bodied methods (`public static StringView FromPtr(...) => ...`)
///
/// Uses the class pattern `public static class $CLASS { $$$ }` to get the entire
/// class body, then checks if method names appear in the text.
pub struct CSharpValidator;

impl CSharpValidator {
    /// Create a new C# validator.
    pub fn new() -> Self {
        Self
    }

    fn generate_class_pattern() -> String {
        "public static class $CLASS { $$$ }".to_string()
    }

    fn text_contains_method(class_text: &str, method_name: &str) -> bool {
        let pascal_name: String = transform_name(
            method_name,
            NamingConvention::Snake,
            NamingConvention::Pascal,
        );
        // Check for method declarations like "public static ... MethodName("
        // or "public ... MethodName(" for extension methods
        class_text.contains(&format!(" {pascal_name}("))
            || class_text.contains(&format!(" {pascal_name}<"))
    }
}

impl Default for CSharpValidator {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageValidator for CSharpValidator {
    fn language_name(&self) -> &'static str {
        "csharp"
    }

    fn ast_grep_language(&self) -> Language {
        Language::CSharp
    }

    fn naming_convention(&self) -> NamingConvention {
        NamingConvention::Pascal
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

        let pattern: String = Self::generate_class_pattern();

        let mut all_class_texts: Vec<String> = Vec::new();
        for file_path in target_files {
            let path: PathBuf = PathBuf::from(file_path);
            if !path.exists() {
                continue;
            }

            match runner.run_ast_grep(&pattern, self.ast_grep_language(), &path) {
                Ok(matches) => {
                    for m in matches {
                        all_class_texts.push(m.text);
                    }
                }
                Err(_) => {
                    continue;
                }
            }
        }

        for method_name in required_methods {
            let found: bool = all_class_texts
                .iter()
                .any(|text| Self::text_contains_method(text, method_name));

            // Store snake_case name for aggregator compatibility
            if found {
                result.found_methods.push(method_name.clone());
            } else {
                result.missing_methods.push(method_name.clone());
            }
        }

        result.found_methods.sort();
        result.missing_methods.sort();

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_temp_csharp_file(content: &str) -> NamedTempFile {
        let mut file: NamedTempFile =
            NamedTempFile::with_suffix(".cs").expect("Failed to create temp file");
        file.write_all(content.as_bytes())
            .expect("Failed to write temp file");
        file.flush().expect("Failed to flush temp file");
        file
    }

    #[test]
    fn test_csharp_validator_new() {
        let validator: CSharpValidator = CSharpValidator::new();
        assert_eq!(validator.language_name(), "csharp");
        assert_eq!(validator.ast_grep_language(), Language::CSharp);
        assert_eq!(validator.naming_convention(), NamingConvention::Pascal);
    }

    #[test]
    fn test_csharp_validator_default() {
        let validator: CSharpValidator = CSharpValidator::default();
        assert_eq!(validator.language_name(), "csharp");
    }

    #[test]
    fn test_generate_class_pattern() {
        let pattern: String = CSharpValidator::generate_class_pattern();
        assert!(pattern.contains("public static class"));
    }

    #[test]
    fn test_text_contains_method() {
        let class_text = "public static string ToString(this StringView sv) { return \"test\"; }";
        assert!(CSharpValidator::text_contains_method(
            class_text,
            "to_string"
        ));

        let class_text =
            "public static StringView FromPtr(IntPtr ptr, int length) => new StringView();";
        assert!(CSharpValidator::text_contains_method(
            class_text, "from_ptr"
        ));

        assert!(!CSharpValidator::text_contains_method(
            class_text,
            "to_string"
        ));
    }

    #[test]
    fn test_validation_result_new() {
        let result: ValidationResult =
            ValidationResult::new("StringView".to_string(), "csharp".to_string());
        assert_eq!(result.struct_name, "StringView");
        assert_eq!(result.language, "csharp");
        assert!(result.found_methods.is_empty());
        assert!(result.missing_methods.is_empty());
        assert!(result.is_complete());
        assert_eq!(result.completion_percentage(), 100);
    }

    #[test]
    fn test_validation_result_with_methods() {
        let mut result: ValidationResult =
            ValidationResult::new("StringView".to_string(), "csharp".to_string());
        result.found_methods.push("ToString".to_string());
        result.missing_methods.push("StartsWith".to_string());

        assert!(!result.is_complete());
        assert_eq!(result.completion_percentage(), 50);
    }

    #[test]
    fn test_csharp_validator_detects_to_string() {
        let csharp_code: &str = r#"
using System;

namespace Polyplug.Abi {
    public static class StringViewHelper {
        public static string ToString(this StringView sv) {
            return "test";
        }
    }
}
"#;

        let file: NamedTempFile = create_temp_csharp_file(csharp_code);
        let runner: AstGrepRunner = AstGrepRunner::new();

        if !runner.is_available() {
            eprintln!("Skipping test: ast-grep not installed");
            return;
        }

        let validator: CSharpValidator = CSharpValidator::new();
        let required_methods: Vec<String> = vec!["to_string".to_string()];
        let target_files: Vec<String> = vec![file.path().to_string_lossy().to_string()];

        let result: ValidationResult =
            validator.validate(&runner, "StringView", &required_methods, &target_files);

        assert!(result.found_methods.contains(&"to_string".to_string()));
        assert!(result.missing_methods.is_empty());
    }

    #[test]
    fn test_csharp_validator_detects_expression_bodied() {
        let csharp_code: &str = r#"
using System;

namespace Polyplug.Abi {
    public static class StringViewHelper {
        public static StringView FromPtr(IntPtr ptr, int length) =>
            new StringView { Ptr = ptr, Len = (nuint)length };
    }
}
"#;

        let file: NamedTempFile = create_temp_csharp_file(csharp_code);
        let runner: AstGrepRunner = AstGrepRunner::new();

        if !runner.is_available() {
            eprintln!("Skipping test: ast-grep not installed");
            return;
        }

        let validator: CSharpValidator = CSharpValidator::new();
        let required_methods: Vec<String> = vec!["from_ptr".to_string()];
        let target_files: Vec<String> = vec![file.path().to_string_lossy().to_string()];

        let result: ValidationResult =
            validator.validate(&runner, "StringView", &required_methods, &target_files);

        assert!(result.found_methods.contains(&"from_ptr".to_string()));
    }

    #[test]
    fn test_csharp_validator_detects_static_method() {
        let csharp_code: &str = r#"
using System;

namespace Polyplug.Abi {
    public static class StringViewHelper {
        public static StringView FromPtr(IntPtr ptr, int length) {
            return new StringView();
        }
    }
}
"#;

        let file: NamedTempFile = create_temp_csharp_file(csharp_code);
        let runner: AstGrepRunner = AstGrepRunner::new();

        if !runner.is_available() {
            eprintln!("Skipping test: ast-grep not installed");
            return;
        }

        let validator: CSharpValidator = CSharpValidator::new();
        let required_methods: Vec<String> = vec!["from_ptr".to_string()];
        let target_files: Vec<String> = vec![file.path().to_string_lossy().to_string()];

        let result: ValidationResult =
            validator.validate(&runner, "StringView", &required_methods, &target_files);

        assert!(result.found_methods.contains(&"from_ptr".to_string()));
    }

    #[test]
    fn test_csharp_validator_reports_missing() {
        let csharp_code: &str = r#"
using System;

namespace Polyplug.Abi {
    public static class StringViewHelper {
        public static string ToString(this StringView sv) {
            return "test";
        }
    }
}
"#;

        let file: NamedTempFile = create_temp_csharp_file(csharp_code);
        let runner: AstGrepRunner = AstGrepRunner::new();

        if !runner.is_available() {
            eprintln!("Skipping test: ast-grep not installed");
            return;
        }

        let validator: CSharpValidator = CSharpValidator::new();
        let required_methods: Vec<String> = vec![
            "to_string".to_string(),
            "starts_with".to_string(),
            "ends_with".to_string(),
        ];
        let target_files: Vec<String> = vec![file.path().to_string_lossy().to_string()];

        let result: ValidationResult =
            validator.validate(&runner, "StringView", &required_methods, &target_files);

        assert!(result.found_methods.contains(&"to_string".to_string()));
        assert!(result.missing_methods.contains(&"starts_with".to_string()));
        assert!(result.missing_methods.contains(&"ends_with".to_string()));
        assert!(!result.is_complete());
    }

    #[test]
    fn test_csharp_validator_missing_file() {
        let runner: AstGrepRunner = AstGrepRunner::new();

        if !runner.is_available() {
            eprintln!("Skipping test: ast-grep not installed");
            return;
        }

        let validator: CSharpValidator = CSharpValidator::new();
        let required_methods: Vec<String> = vec!["to_string".to_string()];
        let target_files: Vec<String> = vec!["/nonexistent/file.cs".to_string()];

        let result: ValidationResult =
            validator.validate(&runner, "StringView", &required_methods, &target_files);

        assert!(result.found_methods.is_empty());
        assert!(result.missing_methods.contains(&"to_string".to_string()));
    }

    #[test]
    fn test_csharp_validator_real_sdk() {
        let sdk_path: &str = "sdks/csharp/abi/StringViewHelper.cs";
        if !std::path::Path::new(sdk_path).exists() {
            eprintln!("Skipping test: SDK file not found");
            return;
        }

        let runner: AstGrepRunner = AstGrepRunner::new();
        if !runner.is_available() {
            eprintln!("Skipping test: ast-grep not installed");
            return;
        }

        let validator: CSharpValidator = CSharpValidator::new();
        let required_methods: Vec<String> = vec![
            "to_string".to_string(),
            "starts_with".to_string(),
            "ends_with".to_string(),
            "strip_prefix".to_string(),
            "split".to_string(),
        ];
        let target_files: Vec<String> = vec![sdk_path.to_string()];

        let result: ValidationResult =
            validator.validate(&runner, "StringView", &required_methods, &target_files);

        assert!(result.found_methods.contains(&"to_string".to_string()));
        assert!(result.found_methods.contains(&"starts_with".to_string()));
        assert!(result.found_methods.contains(&"ends_with".to_string()));
        assert!(result.found_methods.contains(&"strip_prefix".to_string()));
        assert!(result.found_methods.contains(&"split".to_string()));
        assert!(result.missing_methods.is_empty());
    }
}

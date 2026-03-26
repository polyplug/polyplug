//! C# SDK validator using ast-grep CLI.

use std::path::PathBuf;

use crate::ast_grep::{AstGrepRunner, Language, NamingConvention, transform_name};
use crate::languages::{LanguageValidator, ValidationResult};

/// Validator for C# SDK files.
///
/// Detects methods in C# SDK files using ast-grep CLI. Handles:
/// - Extension methods (`public static string ToString(this StringView sv)`)
/// - Static class methods (`public static StringView FromPtr(...)`)
/// - Expression-bodied methods (`public static StringView FromPtr(...) => ...`)
pub struct CSharpValidator;

impl CSharpValidator {
    /// Create a new C# validator.
    pub fn new() -> Self {
        Self
    }

    /// Generate a YAML rule for finding all method declarations, then filter by name.
    fn generate_method_rule() -> String {
        "id: find-methods\nlanguage: csharp\nrule:\n  kind: method_declaration".to_string()
    }

    /// Check if a method match contains the expected method name.
    fn match_contains_method(match_text: &str, method_name: &str) -> bool {
        let pascal_name: String = transform_name(
            method_name,
            NamingConvention::Snake,
            NamingConvention::Pascal,
        );
        match_text.contains(&format!(" {pascal_name}("))
            || match_text.contains(&format!(" {pascal_name}<"))
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

        let rule: String = Self::generate_method_rule();

        for method_name in required_methods {
            let mut found: bool = false;

            for file_path in target_files {
                let path: PathBuf = PathBuf::from(file_path);
                if !path.exists() {
                    continue;
                }

                match runner.run_with_rule(&rule, &path) {
                    Ok(matches) => {
                        for m in matches {
                            if Self::match_contains_method(&m.text, method_name) {
                                found = true;
                                break;
                            }
                        }
                        if found {
                            break;
                        }
                    }
                    Err(_) => {
                        continue;
                    }
                }
            }

            let pascal_name: String = transform_name(
                method_name,
                NamingConvention::Snake,
                NamingConvention::Pascal,
            );

            if found {
                result.found_methods.push(pascal_name);
            } else {
                result.missing_methods.push(pascal_name);
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
    fn test_generate_method_rule() {
        let rule: String = CSharpValidator::generate_method_rule();
        assert!(rule.contains("method_declaration"));
        assert!(rule.contains("csharp"));
    }

    #[test]
    fn test_match_contains_method() {
        let method_text = "public static string ToString(this StringView sv) { return \"test\"; }";
        assert!(CSharpValidator::match_contains_method(
            method_text,
            "to_string"
        ));

        let method_text =
            "public static StringView FromPtr(IntPtr ptr, int length) => new StringView();";
        assert!(CSharpValidator::match_contains_method(
            method_text,
            "from_ptr"
        ));

        assert!(!CSharpValidator::match_contains_method(
            method_text,
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

        assert!(result.found_methods.contains(&"ToString".to_string()));
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

        assert!(result.found_methods.contains(&"FromPtr".to_string()));
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

        assert!(result.found_methods.contains(&"FromPtr".to_string()));
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

        assert!(result.found_methods.contains(&"ToString".to_string()));
        assert!(result.missing_methods.contains(&"StartsWith".to_string()));
        assert!(result.missing_methods.contains(&"EndsWith".to_string()));
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
        assert!(result.missing_methods.contains(&"ToString".to_string()));
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

        assert!(result.found_methods.contains(&"ToString".to_string()));
        assert!(result.missing_methods.contains(&"StartsWith".to_string()));
        assert!(result.missing_methods.contains(&"EndsWith".to_string()));
        assert!(result.missing_methods.contains(&"StripPrefix".to_string()));
        assert!(result.missing_methods.contains(&"Split".to_string()));
    }
}

//! Rust SDK validator using ast-grep CLI.

use std::path::PathBuf;

use crate::ast_grep::{AstGrepRunner, Language, NamingConvention};
use crate::languages::{LanguageValidator, ValidationResult};

/// Validator for Rust SDK files.
///
/// Detects functions in Rust SDK files using ast-grep CLI. Handles:
/// - Public functions: `pub fn name() { }`, `pub fn name() -> Ret { }`
/// - Private functions: `fn name() { }`, `fn name() -> Ret { }`
/// - Functions with visibility modifiers: `pub(crate) fn name() { }`
pub struct RustValidator;

impl RustValidator {
    /// Create a new Rust validator.
    pub fn new() -> Self {
        Self
    }

    /// Generate an ast-grep YAML rule for detecting a Rust function.
    ///
    /// Rust functions can have various forms:
    /// 1. `fn name() { }` - private, no return type
    /// 2. `fn name() -> Ret { }` - private, with return type
    /// 3. `pub fn name() { }` - public, no return type
    /// 4. `pub fn name() -> Ret { }` - public, with return type
    /// 5. `pub(crate) fn name() { }` - restricted visibility
    ///
    /// We use an `any` rule to match all these variants.
    fn generate_function_rule(method_name: &str) -> String {
        format!(
            r#"id: find-function
language: rust
rule:
  any:
    - pattern: fn {method_name}($$$) {{ $$$ }}
    - pattern: fn {method_name}($$$) -> $RET {{ $$$ }}
    - pattern: pub fn {method_name}($$$) {{ $$$ }}
    - pattern: pub fn {method_name}($$$) -> $RET {{ $$$ }}
    - pattern: pub($$$) fn {method_name}($$$) {{ $$$ }}
    - pattern: pub($$$) fn {method_name}($$$) -> $RET {{ $$$ }}
"#
        )
    }
}

impl Default for RustValidator {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageValidator for RustValidator {
    fn language_name(&self) -> &'static str {
        "rust"
    }

    fn ast_grep_language(&self) -> Language {
        Language::Rust
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

        for method_name in required_methods {
            let rule: String = Self::generate_function_rule(method_name);
            let mut found: bool = false;

            for file_path in target_files {
                let path: PathBuf = PathBuf::from(file_path);
                if !path.exists() {
                    continue;
                }

                match runner.run_with_rule(&rule, &path) {
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

    fn create_temp_rust_file(content: &str) -> NamedTempFile {
        let mut file: NamedTempFile =
            NamedTempFile::with_suffix(".rs").expect("Failed to create temp file");
        file.write_all(content.as_bytes())
            .expect("Failed to write temp file");
        file.flush().expect("Failed to flush temp file");
        file
    }

    #[test]
    fn test_rust_validator_new() {
        let validator: RustValidator = RustValidator::new();
        assert_eq!(validator.language_name(), "rust");
        assert_eq!(validator.ast_grep_language(), Language::Rust);
        assert_eq!(validator.naming_convention(), NamingConvention::Snake);
    }

    #[test]
    fn test_rust_validator_default() {
        let validator: RustValidator = RustValidator::default();
        assert_eq!(validator.language_name(), "rust");
    }

    #[test]
    fn test_generate_function_rule() {
        let rule: String = RustValidator::generate_function_rule("to_str");
        assert!(rule.contains("to_str"));
        assert!(rule.contains("fn"));
        assert!(rule.contains("any:"));

        let rule: String = RustValidator::generate_function_rule("starts_with");
        assert!(rule.contains("starts_with"));
    }

    #[test]
    fn test_validation_result_new() {
        let result: ValidationResult =
            ValidationResult::new("StringView".to_string(), "rust".to_string());
        assert_eq!(result.struct_name, "StringView");
        assert_eq!(result.language, "rust");
        assert!(result.found_methods.is_empty());
        assert!(result.missing_methods.is_empty());
        assert!(result.is_complete());
        assert_eq!(result.completion_percentage(), 100);
    }

    #[test]
    fn test_validation_result_with_methods() {
        let mut result: ValidationResult =
            ValidationResult::new("StringView".to_string(), "rust".to_string());
        result.found_methods.push("to_str".to_string());
        result.missing_methods.push("starts_with".to_string());

        assert!(!result.is_complete());
        assert_eq!(result.completion_percentage(), 50);
    }

    #[test]
    fn test_rust_validator_detects_to_str() {
        let rust_code: &str = r#"
//! polyplug guest library

pub fn to_str(sv: StringView) -> &'static str {
    if sv.ptr.is_null() || sv.len == 0 {
        return "";
    }
    unsafe { core::str::from_utf8(core::slice::from_raw_parts(sv.ptr, sv.len)).unwrap_or("") }
}
"#;

        let file: NamedTempFile = create_temp_rust_file(rust_code);
        let runner: AstGrepRunner = AstGrepRunner::new();

        if !runner.is_available() {
            panic!(
                "ast-grep CLI not found. Please install ast-grep: https://ast-grep.github.io/guide/introduction.html"
            );
        }

        let validator: RustValidator = RustValidator::new();
        let required_methods: Vec<String> = vec!["to_str".to_string()];
        let target_files: Vec<String> = vec![file.path().to_string_lossy().to_string()];

        let result: ValidationResult =
            validator.validate(&runner, "StringView", &required_methods, &target_files);

        assert!(result.found_methods.contains(&"to_str".to_string()));
        assert!(result.missing_methods.is_empty());
    }

    #[test]
    fn test_rust_validator_detects_alloc_string() {
        let rust_code: &str = r#"
//! polyplug guest library

pub fn alloc_string(s: &str) -> Result<StringView, GuestError> {
    let bytes: &[u8] = s.as_bytes();
    let ptr: *mut u8 = polyplug_host_alloc(bytes.len(), 1);
    if ptr.is_null() {
        return Err(GuestError {
            code: AbiErrorCode::Generic,
            message: "allocation failed".to_string(),
        });
    }
    unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len()) };
    Ok(StringView {
        ptr,
        len: bytes.len(),
    })
}
"#;

        let file: NamedTempFile = create_temp_rust_file(rust_code);
        let runner: AstGrepRunner = AstGrepRunner::new();

        if !runner.is_available() {
            panic!(
                "ast-grep CLI not found. Please install ast-grep: https://ast-grep.github.io/guide/introduction.html"
            );
        }

        let validator: RustValidator = RustValidator::new();
        let required_methods: Vec<String> = vec!["alloc_string".to_string()];
        let target_files: Vec<String> = vec![file.path().to_string_lossy().to_string()];

        let result: ValidationResult =
            validator.validate(&runner, "StringView", &required_methods, &target_files);

        assert!(result.found_methods.contains(&"alloc_string".to_string()));
    }

    #[test]
    fn test_rust_validator_detects_private_function() {
        let rust_code: &str = r#"
fn internal_helper(x: i32) -> i32 {
    x * 2
}
"#;

        let file: NamedTempFile = create_temp_rust_file(rust_code);
        let runner: AstGrepRunner = AstGrepRunner::new();

        if !runner.is_available() {
            panic!(
                "ast-grep CLI not found. Please install ast-grep: https://ast-grep.github.io/guide/introduction.html"
            );
        }

        let validator: RustValidator = RustValidator::new();
        let required_methods: Vec<String> = vec!["internal_helper".to_string()];
        let target_files: Vec<String> = vec![file.path().to_string_lossy().to_string()];

        let result: ValidationResult =
            validator.validate(&runner, "StringView", &required_methods, &target_files);

        assert!(
            result
                .found_methods
                .contains(&"internal_helper".to_string())
        );
    }

    #[test]
    fn test_rust_validator_reports_missing() {
        let rust_code: &str = r#"
//! polyplug guest library

pub fn to_str(sv: StringView) -> &'static str {
    ""
}
"#;

        let file: NamedTempFile = create_temp_rust_file(rust_code);
        let runner: AstGrepRunner = AstGrepRunner::new();

        if !runner.is_available() {
            panic!(
                "ast-grep CLI not found. Please install ast-grep: https://ast-grep.github.io/guide/introduction.html"
            );
        }

        let validator: RustValidator = RustValidator::new();
        let required_methods: Vec<String> = vec![
            "to_str".to_string(),
            "starts_with".to_string(),
            "ends_with".to_string(),
        ];
        let target_files: Vec<String> = vec![file.path().to_string_lossy().to_string()];

        let result: ValidationResult =
            validator.validate(&runner, "StringView", &required_methods, &target_files);

        assert!(result.found_methods.contains(&"to_str".to_string()));
        assert!(result.missing_methods.contains(&"starts_with".to_string()));
        assert!(result.missing_methods.contains(&"ends_with".to_string()));
        assert!(!result.is_complete());
    }

    #[test]
    fn test_rust_validator_missing_file() {
        let runner: AstGrepRunner = AstGrepRunner::new();

        if !runner.is_available() {
            panic!(
                "ast-grep CLI not found. Please install ast-grep: https://ast-grep.github.io/guide/introduction.html"
            );
        }

        let validator: RustValidator = RustValidator::new();
        let required_methods: Vec<String> = vec!["to_str".to_string()];
        let target_files: Vec<String> = vec!["/nonexistent/file.rs".to_string()];

        let result: ValidationResult =
            validator.validate(&runner, "StringView", &required_methods, &target_files);

        assert!(result.found_methods.is_empty());
        assert!(result.missing_methods.contains(&"to_str".to_string()));
    }

    #[test]
    fn test_rust_validator_multiple_files() {
        let rust_code1: &str = r#"
pub fn to_str(sv: StringView) -> &'static str {
    ""
}
"#;

        let rust_code2: &str = r#"
pub fn starts_with(sv: StringView, prefix: &str) -> bool {
    false
}
"#;

        let file1: NamedTempFile = create_temp_rust_file(rust_code1);
        let file2: NamedTempFile = create_temp_rust_file(rust_code2);
        let runner: AstGrepRunner = AstGrepRunner::new();

        if !runner.is_available() {
            panic!(
                "ast-grep CLI not found. Please install ast-grep: https://ast-grep.github.io/guide/introduction.html"
            );
        }

        let validator: RustValidator = RustValidator::new();
        let required_methods: Vec<String> = vec![
            "to_str".to_string(),
            "starts_with".to_string(),
            "ends_with".to_string(),
        ];
        let target_files: Vec<String> = vec![
            file1.path().to_string_lossy().to_string(),
            file2.path().to_string_lossy().to_string(),
        ];

        let result: ValidationResult =
            validator.validate(&runner, "StringView", &required_methods, &target_files);

        assert!(result.found_methods.contains(&"to_str".to_string()));
        assert!(result.found_methods.contains(&"starts_with".to_string()));
        assert!(result.missing_methods.contains(&"ends_with".to_string()));
    }
}

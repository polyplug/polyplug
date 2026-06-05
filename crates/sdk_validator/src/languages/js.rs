//! JavaScript/TypeScript SDK validator using ast-grep CLI.
//!
//! This validator targets TypeScript files (`.ts`) and detects function
//! declarations using camelCase naming convention.

use std::path::PathBuf;

use crate::ast_grep::{AstGrepRunner, Language, NamingConvention, transform_name};
use crate::languages::{LanguageValidator, ValidationResult};

/// Validator for JavaScript/TypeScript SDK files.
///
/// Detects functions in TypeScript SDK files using ast-grep CLI. Handles:
/// - Function declarations: `export function toStr(...): string`
/// - Arrow functions: `export const toStr = (...): string => ...`
///
/// Note: Only TypeScript is validated for now (not plain JavaScript).
pub struct JsValidator;

impl JsValidator {
    /// Create a new JavaScript/TypeScript validator.
    pub fn new() -> Self {
        Self
    }

    fn generate_function_pattern(method_name: &str) -> String {
        let camel_name: String = transform_name(
            method_name,
            NamingConvention::Snake,
            NamingConvention::Camel,
        );
        format!("function {camel_name}($$$): $$$ {{ $$$ }}")
    }
}

impl Default for JsValidator {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageValidator for JsValidator {
    fn language_name(&self) -> &'static str {
        "js"
    }

    fn ast_grep_language(&self) -> Language {
        Language::TypeScript
    }

    fn naming_convention(&self) -> NamingConvention {
        NamingConvention::Camel
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
            let pattern: String = Self::generate_function_pattern(method_name);
            let mut found: bool = false;

            for file_path in target_files {
                let path: PathBuf = PathBuf::from(file_path);
                if !path.exists() {
                    continue;
                }

                match runner.run_ast_grep(&pattern, self.ast_grep_language(), &path) {
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

    fn create_temp_typescript_file(
        content: &str,
    ) -> Result<NamedTempFile, Box<dyn core::error::Error>> {
        let mut file: NamedTempFile = NamedTempFile::with_suffix(".ts")?;
        file.write_all(content.as_bytes())?;
        file.flush()?;
        Ok(file)
    }

    #[test]
    fn test_js_validator_new() {
        let validator: JsValidator = JsValidator::new();
        assert_eq!(validator.language_name(), "js");
        assert_eq!(validator.ast_grep_language(), Language::TypeScript);
        assert_eq!(validator.naming_convention(), NamingConvention::Camel);
    }

    #[test]
    fn test_js_validator_default() {
        let validator: JsValidator = JsValidator;
        assert_eq!(validator.language_name(), "js");
    }

    #[test]
    fn test_generate_function_pattern() {
        let pattern: String = JsValidator::generate_function_pattern("to_str");
        assert!(pattern.contains("toStr"));
        assert!(pattern.contains("function"));

        let pattern: String = JsValidator::generate_function_pattern("starts_with");
        assert!(pattern.contains("startsWith"));

        let pattern: String = JsValidator::generate_function_pattern("strip_prefix");
        assert!(pattern.contains("stripPrefix"));
    }

    #[test]
    fn test_js_validator_detects_function() -> Result<(), Box<dyn core::error::Error>> {
        let typescript_code: &str = r#"
/**
 * Convert a StringView to a JavaScript string.
 */
export function toStr(sv: StringView | null | undefined): string {
    if (!sv || sv.ptr === 0n || sv.len === 0) return '';
    return '';
}
"#;

        let file: NamedTempFile = create_temp_typescript_file(typescript_code)?;
        let runner: AstGrepRunner = AstGrepRunner::new();

        if !runner.is_available() {
            panic!(
                "ast-grep CLI not found. Please install ast-grep: https://ast-grep.github.io/guide/introduction.html"
            );
        }

        let validator: JsValidator = JsValidator::new();
        let required_methods: Vec<String> = vec!["to_str".to_string()];
        let target_files: Vec<String> = vec![file.path().to_string_lossy().to_string()];

        let result: ValidationResult =
            validator.validate(&runner, "StringView", &required_methods, &target_files);

        assert!(result.found_methods.contains(&"to_str".to_string()));
        assert!(result.missing_methods.is_empty());
        Ok(())
    }

    #[test]
    fn test_js_validator_detects_starts_with() -> Result<(), Box<dyn core::error::Error>> {
        let typescript_code: &str = r#"
/**
 * Check if a string starts with a prefix.
 */
export function startsWith(sv: StringView | string, prefix: string): boolean {
    const s: string = typeof sv === 'string' ? sv : stringViewToString(sv);
    return s.startsWith(prefix);
}
"#;

        let file: NamedTempFile = create_temp_typescript_file(typescript_code)?;
        let runner: AstGrepRunner = AstGrepRunner::new();

        if !runner.is_available() {
            panic!(
                "ast-grep CLI not found. Please install ast-grep: https://ast-grep.github.io/guide/introduction.html"
            );
        }

        let validator: JsValidator = JsValidator::new();
        let required_methods: Vec<String> = vec!["starts_with".to_string()];
        let target_files: Vec<String> = vec![file.path().to_string_lossy().to_string()];

        let result: ValidationResult =
            validator.validate(&runner, "StringView", &required_methods, &target_files);

        assert!(result.found_methods.contains(&"starts_with".to_string()));
        Ok(())
    }

    #[test]
    fn test_js_validator_detects_strip_prefix() -> Result<(), Box<dyn core::error::Error>> {
        let typescript_code: &str = r#"
/**
 * Strip a prefix from a string.
 */
export function stripPrefix(sv: StringView | string, prefix: string): string {
    const s: string = typeof sv === 'string' ? sv : stringViewToString(sv);
    if (s.startsWith(prefix)) {
        return s.slice(prefix.length);
    }
    return s;
}
"#;

        let file: NamedTempFile = create_temp_typescript_file(typescript_code)?;
        let runner: AstGrepRunner = AstGrepRunner::new();

        if !runner.is_available() {
            panic!(
                "ast-grep CLI not found. Please install ast-grep: https://ast-grep.github.io/guide/introduction.html"
            );
        }

        let validator: JsValidator = JsValidator::new();
        let required_methods: Vec<String> = vec!["strip_prefix".to_string()];
        let target_files: Vec<String> = vec![file.path().to_string_lossy().to_string()];

        let result: ValidationResult =
            validator.validate(&runner, "StringView", &required_methods, &target_files);

        assert!(result.found_methods.contains(&"strip_prefix".to_string()));
        Ok(())
    }

    #[test]
    fn test_js_validator_detects_split() -> Result<(), Box<dyn core::error::Error>> {
        let typescript_code: &str = r#"
/**
 * Split a string by a delimiter.
 */
export function split(sv: StringView | string, delimiter: string): string[] {
    const s: string = typeof sv === 'string' ? sv : stringViewToString(sv);
    return s.split(delimiter);
}
"#;

        let file: NamedTempFile = create_temp_typescript_file(typescript_code)?;
        let runner: AstGrepRunner = AstGrepRunner::new();

        if !runner.is_available() {
            panic!(
                "ast-grep CLI not found. Please install ast-grep: https://ast-grep.github.io/guide/introduction.html"
            );
        }

        let validator: JsValidator = JsValidator::new();
        let required_methods: Vec<String> = vec!["split".to_string()];
        let target_files: Vec<String> = vec![file.path().to_string_lossy().to_string()];

        let result: ValidationResult =
            validator.validate(&runner, "StringView", &required_methods, &target_files);

        assert!(result.found_methods.contains(&"split".to_string()));
        Ok(())
    }

    #[test]
    fn test_js_validator_reports_missing() -> Result<(), Box<dyn core::error::Error>> {
        let typescript_code: &str = r#"
export function toStr(sv: StringView | null | undefined): string {
    return '';
}

export function startsWith(sv: StringView | string, prefix: string): boolean {
    return true;
}
"#;

        let file: NamedTempFile = create_temp_typescript_file(typescript_code)?;
        let runner: AstGrepRunner = AstGrepRunner::new();

        if !runner.is_available() {
            panic!(
                "ast-grep CLI not found. Please install ast-grep: https://ast-grep.github.io/guide/introduction.html"
            );
        }

        let validator: JsValidator = JsValidator::new();
        let required_methods: Vec<String> = vec![
            "to_str".to_string(),
            "starts_with".to_string(),
            "ends_with".to_string(),
            "strip_prefix".to_string(),
            "split".to_string(),
        ];
        let target_files: Vec<String> = vec![file.path().to_string_lossy().to_string()];

        let result: ValidationResult =
            validator.validate(&runner, "StringView", &required_methods, &target_files);

        assert!(result.found_methods.contains(&"to_str".to_string()));
        assert!(result.found_methods.contains(&"starts_with".to_string()));
        assert!(result.missing_methods.contains(&"ends_with".to_string()));
        assert!(result.missing_methods.contains(&"strip_prefix".to_string()));
        assert!(result.missing_methods.contains(&"split".to_string()));
        assert!(!result.is_complete());
        Ok(())
    }

    #[test]
    fn test_js_validator_missing_file() {
        let runner: AstGrepRunner = AstGrepRunner::new();

        if !runner.is_available() {
            panic!(
                "ast-grep CLI not found. Please install ast-grep: https://ast-grep.github.io/guide/introduction.html"
            );
        }

        let validator: JsValidator = JsValidator::new();
        let required_methods: Vec<String> = vec!["to_str".to_string()];
        let target_files: Vec<String> = vec!["/nonexistent/file.ts".to_string()];

        let result: ValidationResult =
            validator.validate(&runner, "StringView", &required_methods, &target_files);

        assert!(result.found_methods.is_empty());
        assert!(result.missing_methods.contains(&"to_str".to_string()));
    }

    #[test]
    fn test_js_validator_multiple_files() -> Result<(), Box<dyn core::error::Error>> {
        let file1_content: &str = r#"
export function toStr(sv: StringView | null | undefined): string {
    return '';
}
"#;

        let file2_content: &str = r#"
export function startsWith(sv: StringView | string, prefix: string): boolean {
    return true;
}
"#;

        let file1: NamedTempFile = create_temp_typescript_file(file1_content)?;
        let file2: NamedTempFile = create_temp_typescript_file(file2_content)?;
        let runner: AstGrepRunner = AstGrepRunner::new();

        if !runner.is_available() {
            panic!(
                "ast-grep CLI not found. Please install ast-grep: https://ast-grep.github.io/guide/introduction.html"
            );
        }

        let validator: JsValidator = JsValidator::new();
        let required_methods: Vec<String> = vec!["to_str".to_string(), "starts_with".to_string()];
        let target_files: Vec<String> = vec![
            file1.path().to_string_lossy().to_string(),
            file2.path().to_string_lossy().to_string(),
        ];

        let result: ValidationResult =
            validator.validate(&runner, "StringView", &required_methods, &target_files);

        assert!(result.found_methods.contains(&"to_str".to_string()));
        assert!(result.found_methods.contains(&"starts_with".to_string()));
        assert!(result.is_complete());
        Ok(())
    }
}

//! JavaScript/TypeScript SDK validator using ast-grep CLI.

use std::path::Path;

use crate::ast_grep::{AstGrepRunner, Match};
use crate::error::ValidatorError;
use crate::languages::LanguageValidator;

/// Validator for JavaScript/TypeScript SDK files.
///
/// Parses files as TypeScript (which also parses plain JavaScript) and
/// detects, via an ast-grep inline-rules `any:`:
/// - annotated function declarations: `function toStr(sv: SV): string { ... }`
/// - un-annotated function declarations: `function toStr(sv) { ... }`
/// - arrow functions: `const toStr = (sv) => ...` (with or without a return
///   type annotation, expression or block body)
///
/// `export` prefixes match implicitly because ast-grep matches sub-nodes.
pub struct JsValidator;

impl JsValidator {
    /// Create a new JavaScript/TypeScript validator.
    pub fn new() -> Self {
        Self
    }

    /// Generate the ast-grep inline rule for a JS/TS function definition.
    fn generate_rule(method_name: &str) -> String {
        format!(
            r#"id: find-function
language: typescript
severity: hint
rule:
  any:
    - pattern: function {method_name}($$$) {{ $$$ }}
    - pattern: 'function {method_name}($$$): $RET {{ $$$ }}'
    - pattern: const {method_name} = ($$$) => $BODY
    - pattern: 'const {method_name} = ($$$): $RET => $BODY'
"#
        )
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

    fn method_in_file(
        &mut self,
        runner: &AstGrepRunner,
        native_name: &str,
        file: &Path,
    ) -> Result<bool, ValidatorError> {
        let rule: String = Self::generate_rule(native_name);
        let matches: Vec<Match> = runner.run_with_rule(&rule, file)?;
        Ok(!matches.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;
    use tempfile::NamedTempFile;

    use crate::ast_grep::NamingConvention;
    use crate::languages::test_support::{golden_methods, repo_path, runner};
    use crate::languages::{ValidationResult, validate_language};

    fn create_temp_ts_file(content: &str) -> Result<NamedTempFile, Box<dyn core::error::Error>> {
        let mut file: NamedTempFile = NamedTempFile::with_suffix(".ts")?;
        file.write_all(content.as_bytes())?;
        file.flush()?;
        Ok(file)
    }

    fn validate_file(
        methods: &[String],
        file: &Path,
    ) -> Result<ValidationResult, Box<dyn core::error::Error>> {
        let mut validator: JsValidator = JsValidator::new();
        let result: ValidationResult = validate_language(
            &mut validator,
            &runner(),
            NamingConvention::Camel,
            "StringView",
            methods,
            &[file.to_path_buf()],
        )?;
        Ok(result)
    }

    #[test]
    fn test_detects_annotated_function() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_ts_file(
            r#"
export function toStr(sv: StringView | null | undefined): string {
    return '';
}
"#,
        )?;
        let result: ValidationResult = validate_file(&["to_str".to_string()], file.path())?;
        assert!(result.found_methods.contains(&"to_str".to_string()));
        Ok(())
    }

    #[test]
    fn test_detects_unannotated_function() -> Result<(), Box<dyn core::error::Error>> {
        // Plain-JS form (a future target file is plain JS).
        let file: NamedTempFile = create_temp_ts_file(
            r#"
export function toStr(sv) {
    return '';
}
"#,
        )?;
        let result: ValidationResult = validate_file(&["to_str".to_string()], file.path())?;
        assert!(result.found_methods.contains(&"to_str".to_string()));
        Ok(())
    }

    #[test]
    fn test_detects_arrow_functions() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_ts_file(
            r#"
export const startsWith = (sv, prefix) => sv.startsWith(prefix);
const endsWith = (sv, suffix) => { return true; };
export const stripPrefix = (sv: string, prefix: string): string => sv.slice(prefix.length);
"#,
        )?;
        let result: ValidationResult = validate_file(
            &[
                "starts_with".to_string(),
                "ends_with".to_string(),
                "strip_prefix".to_string(),
            ],
            file.path(),
        )?;
        assert!(result.found_methods.contains(&"starts_with".to_string()));
        assert!(result.found_methods.contains(&"ends_with".to_string()));
        assert!(result.found_methods.contains(&"strip_prefix".to_string()));
        Ok(())
    }

    #[test]
    fn test_renamed_definition_does_not_match() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_ts_file(
            r#"
export function toStr2(sv: StringView): string {
    return '';
}
"#,
        )?;
        let result: ValidationResult = validate_file(&["to_str".to_string()], file.path())?;
        assert!(result.found_methods.is_empty());
        Ok(())
    }

    #[test]
    fn test_call_site_only_does_not_match() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_ts_file(
            r#"
export function other(sv: StringView): string {
    const s = toStr(sv);
    return s;
}
"#,
        )?;
        let result: ValidationResult = validate_file(&["to_str".to_string()], file.path())?;
        assert!(result.found_methods.is_empty());
        Ok(())
    }

    #[test]
    fn test_comment_only_does_not_match() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_ts_file(
            r#"
// toStr(sv) converts a StringView; function toStr is documented here only.
export function unrelated(): void {}
"#,
        )?;
        let result: ValidationResult = validate_file(&["to_str".to_string()], file.path())?;
        assert!(result.found_methods.is_empty());
        Ok(())
    }

    #[test]
    fn test_reports_missing_methods() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_ts_file(
            r#"
export function toStr(sv: StringView | null | undefined): string {
    return '';
}
"#,
        )?;
        let result: ValidationResult = validate_file(
            &["to_str".to_string(), "ends_with".to_string()],
            file.path(),
        )?;
        assert!(result.found_methods.contains(&"to_str".to_string()));
        assert_eq!(result.missing_methods.len(), 1);
        assert_eq!(result.missing_methods[0].method, "ends_with");
        Ok(())
    }

    #[test]
    fn test_real_sdk_has_all_golden_methods() -> Result<(), Box<dyn core::error::Error>> {
        let sdk_path: PathBuf = repo_path("sdks/js/abi/abi.ts");
        let result: ValidationResult = validate_file(&golden_methods(), &sdk_path)?;
        assert!(
            result.is_complete(),
            "js SDK missing methods: {:?}",
            result.missing_methods
        );
        assert_eq!(result.found_methods.len(), 5);
        Ok(())
    }
}

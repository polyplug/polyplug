//! Rust SDK validator using ast-grep CLI.

use std::path::Path;

use crate::ast_grep::{AstGrepRunner, Match};
use crate::error::ValidatorError;
use crate::languages::LanguageValidator;

/// Validator for Rust SDK files.
///
/// Detects function definitions via an ast-grep inline-rules `any:` covering
/// private/`pub`/`pub(crate)` visibility, `unsafe`, `const`, generics
/// (including lifetime-only generics like `strip_prefix<'a>`), and optional
/// return types. Call sites and comments do not match.
pub struct RustValidator;

impl RustValidator {
    /// Create a new Rust validator.
    pub fn new() -> Self {
        Self
    }

    /// Generate the ast-grep inline rule for a Rust function definition.
    fn generate_rule(method_name: &str) -> String {
        format!(
            r#"id: find-function
language: rust
severity: hint
rule:
  any:
    - pattern: fn {method_name}($$$) {{ $$$ }}
    - pattern: fn {method_name}($$$) -> $RET {{ $$$ }}
    - pattern: fn {method_name}<$$$>($$$) {{ $$$ }}
    - pattern: fn {method_name}<$$$>($$$) -> $RET {{ $$$ }}
    - pattern: pub fn {method_name}($$$) {{ $$$ }}
    - pattern: pub fn {method_name}($$$) -> $RET {{ $$$ }}
    - pattern: pub fn {method_name}<$$$>($$$) {{ $$$ }}
    - pattern: pub fn {method_name}<$$$>($$$) -> $RET {{ $$$ }}
    - pattern: pub($$$) fn {method_name}($$$) {{ $$$ }}
    - pattern: pub($$$) fn {method_name}($$$) -> $RET {{ $$$ }}
    - pattern: unsafe fn {method_name}($$$) {{ $$$ }}
    - pattern: unsafe fn {method_name}($$$) -> $RET {{ $$$ }}
    - pattern: pub unsafe fn {method_name}($$$) {{ $$$ }}
    - pattern: pub unsafe fn {method_name}($$$) -> $RET {{ $$$ }}
    - pattern: pub unsafe fn {method_name}<$$$>($$$) -> $RET {{ $$$ }}
    - pattern: const fn {method_name}($$$) {{ $$$ }}
    - pattern: const fn {method_name}($$$) -> $RET {{ $$$ }}
    - pattern: pub const fn {method_name}($$$) {{ $$$ }}
    - pattern: pub const fn {method_name}($$$) -> $RET {{ $$$ }}
    - pattern: pub const unsafe fn {method_name}($$$) -> $RET {{ $$$ }}
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

    fn create_temp_rust_file(content: &str) -> Result<NamedTempFile, Box<dyn core::error::Error>> {
        let mut file: NamedTempFile = NamedTempFile::with_suffix(".rs")?;
        file.write_all(content.as_bytes())?;
        file.flush()?;
        Ok(file)
    }

    fn validate_file(
        methods: &[String],
        file: &Path,
    ) -> Result<ValidationResult, Box<dyn core::error::Error>> {
        let mut validator: RustValidator = RustValidator::new();
        let result: ValidationResult = validate_language(
            &mut validator,
            &runner(),
            NamingConvention::Snake,
            "StringView",
            methods,
            &[file.to_path_buf()],
        )?;
        Ok(result)
    }

    #[test]
    fn test_detects_pub_fn() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_rust_file(
            r#"
pub fn to_str(sv: StringView) -> &'static str {
    ""
}
"#,
        )?;
        let result: ValidationResult = validate_file(&["to_str".to_string()], file.path())?;
        assert!(result.found_methods.contains(&"to_str".to_string()));
        Ok(())
    }

    #[test]
    fn test_detects_private_fn() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_rust_file(
            r#"
fn internal_helper(x: i32) -> i32 {
    x * 2
}
"#,
        )?;
        let result: ValidationResult =
            validate_file(&["internal_helper".to_string()], file.path())?;
        assert!(
            result
                .found_methods
                .contains(&"internal_helper".to_string())
        );
        Ok(())
    }

    #[test]
    fn test_detects_lifetime_generic_unsafe_fn() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_rust_file(
            r#"
pub unsafe fn strip_prefix<'a>(sv: &'a StringView, prefix: &str) -> &'a str {
    ""
}
"#,
        )?;
        let result: ValidationResult = validate_file(&["strip_prefix".to_string()], file.path())?;
        assert!(result.found_methods.contains(&"strip_prefix".to_string()));
        Ok(())
    }

    #[test]
    fn test_detects_const_fn() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_rust_file(
            r#"
pub const fn to_str(sv: &StringView) -> &str {
    ""
}
const fn starts_with(sv: &StringView) -> bool {
    true
}
"#,
        )?;
        let result: ValidationResult = validate_file(
            &["to_str".to_string(), "starts_with".to_string()],
            file.path(),
        )?;
        assert!(result.found_methods.contains(&"to_str".to_string()));
        assert!(result.found_methods.contains(&"starts_with".to_string()));
        Ok(())
    }

    #[test]
    fn test_renamed_definition_does_not_match() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_rust_file(
            r#"
pub unsafe fn to_str2(sv: &StringView) -> &str {
    ""
}
"#,
        )?;
        let result: ValidationResult = validate_file(&["to_str".to_string()], file.path())?;
        assert!(result.found_methods.is_empty());
        assert_eq!(result.missing_methods[0].method, "to_str");
        Ok(())
    }

    #[test]
    fn test_call_site_only_does_not_match() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_rust_file(
            r#"
pub fn other(sv: &StringView) -> bool {
    let s = to_str(sv);
    s.is_empty()
}
"#,
        )?;
        let result: ValidationResult = validate_file(&["to_str".to_string()], file.path())?;
        assert!(result.found_methods.is_empty());
        Ok(())
    }

    #[test]
    fn test_comment_only_does_not_match() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_rust_file(
            r#"
// to_str(sv) is documented here but not defined
// pub fn to_str(sv: &StringView) -> &str { "" }
"#,
        )?;
        let result: ValidationResult = validate_file(&["to_str".to_string()], file.path())?;
        assert!(result.found_methods.is_empty());
        Ok(())
    }

    #[test]
    fn test_per_file_semantics_method_must_be_in_all_files()
    -> Result<(), Box<dyn core::error::Error>> {
        let file1: NamedTempFile = create_temp_rust_file(
            r#"
pub fn to_str(sv: &StringView) -> &str { "" }
"#,
        )?;
        let file2: NamedTempFile = create_temp_rust_file(
            r#"
pub fn starts_with(sv: &StringView, prefix: &str) -> bool { false }
"#,
        )?;

        let mut validator: RustValidator = RustValidator::new();
        let result: ValidationResult = validate_language(
            &mut validator,
            &runner(),
            NamingConvention::Snake,
            "StringView",
            &["to_str".to_string()],
            &[file1.path().to_path_buf(), file2.path().to_path_buf()],
        )?;

        // to_str is only in file1, so it must be reported missing in file2.
        assert!(result.found_methods.is_empty());
        assert_eq!(result.missing_methods.len(), 1);
        assert_eq!(result.missing_methods[0].method, "to_str");
        assert_eq!(
            result.missing_methods[0].missing_files,
            vec![file2.path().display().to_string()]
        );
        Ok(())
    }

    #[test]
    fn test_real_sdk_has_all_golden_methods() -> Result<(), Box<dyn core::error::Error>> {
        let sdk_path: PathBuf = repo_path("sdks/rust/guest/src/lib.rs");
        let result: ValidationResult = validate_file(&golden_methods(), &sdk_path)?;
        assert!(
            result.is_complete(),
            "rust SDK missing methods: {:?}",
            result.missing_methods
        );
        assert_eq!(result.found_methods.len(), 5);
        Ok(())
    }
}

//! Python SDK validator using ast-grep CLI.

use std::path::Path;

use crate::ast_grep::{AstGrepRunner, Match};
use crate::error::ValidatorError;
use crate::languages::LanguageValidator;

/// Validator for Python SDK files.
///
/// Detects standalone function definitions via an ast-grep inline-rules
/// `any:` of full `def` patterns (with and without a return annotation).
/// Unlike the bare `def name` snippet the validator previously used, these
/// patterns parse cleanly (no "Pattern contains an ERROR node" warning) and
/// still match by exact identifier only.
pub struct PythonValidator;

impl PythonValidator {
    /// Create a new Python validator.
    pub fn new() -> Self {
        Self
    }

    /// Generate the ast-grep inline rule for a Python function definition.
    fn generate_rule(method_name: &str) -> String {
        format!(
            r#"id: find-function
language: python
severity: hint
rule:
  any:
    - pattern: 'def {method_name}($$$): $$$'
    - pattern: 'def {method_name}($$$) -> $RET: $$$'
"#
        )
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

    fn create_temp_python_file(
        content: &str,
    ) -> Result<NamedTempFile, Box<dyn core::error::Error>> {
        let mut file: NamedTempFile = NamedTempFile::with_suffix(".py")?;
        file.write_all(content.as_bytes())?;
        file.flush()?;
        Ok(file)
    }

    fn validate_file(
        methods: &[String],
        file: &Path,
    ) -> Result<ValidationResult, Box<dyn core::error::Error>> {
        let mut validator: PythonValidator = PythonValidator::new();
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
    fn test_detects_annotated_function() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_python_file(
            r#"
def to_str(sv: StringView) -> str:
    """Convert StringView to Python str."""
    return ""
"#,
        )?;
        let result: ValidationResult = validate_file(&["to_str".to_string()], file.path())?;
        assert!(result.found_methods.contains(&"to_str".to_string()));
        Ok(())
    }

    #[test]
    fn test_detects_unannotated_function() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_python_file(
            r#"
def to_str(sv):
    return ""
"#,
        )?;
        let result: ValidationResult = validate_file(&["to_str".to_string()], file.path())?;
        assert!(result.found_methods.contains(&"to_str".to_string()));
        Ok(())
    }

    #[test]
    fn test_renamed_definition_does_not_match() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_python_file(
            r#"
def to_str2(sv):
    return ""
"#,
        )?;
        let result: ValidationResult = validate_file(&["to_str".to_string()], file.path())?;
        assert!(result.found_methods.is_empty());
        Ok(())
    }

    #[test]
    fn test_call_site_only_does_not_match() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_python_file(
            r#"
def other(sv):
    s = to_str(sv)
    return s
"#,
        )?;
        let result: ValidationResult = validate_file(&["to_str".to_string()], file.path())?;
        assert!(result.found_methods.is_empty());
        Ok(())
    }

    #[test]
    fn test_comment_only_does_not_match() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_python_file(
            r#"
# to_str(sv) converts a StringView
# def to_str(sv): documented but not defined
def unrelated():
    pass
"#,
        )?;
        let result: ValidationResult = validate_file(&["to_str".to_string()], file.path())?;
        assert!(result.found_methods.is_empty());
        Ok(())
    }

    #[test]
    fn test_reports_missing_methods() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_python_file(
            r#"
def to_str(sv):
    return ""

def starts_with(sv, prefix):
    return True
"#,
        )?;
        let result: ValidationResult = validate_file(
            &[
                "to_str".to_string(),
                "starts_with".to_string(),
                "ends_with".to_string(),
            ],
            file.path(),
        )?;
        assert!(result.found_methods.contains(&"to_str".to_string()));
        assert!(result.found_methods.contains(&"starts_with".to_string()));
        assert_eq!(result.missing_methods.len(), 1);
        assert_eq!(result.missing_methods[0].method, "ends_with");
        Ok(())
    }

    #[test]
    fn test_real_sdk_has_all_golden_methods() -> Result<(), Box<dyn core::error::Error>> {
        let sdk_path: PathBuf =
            repo_path("sdks/python/polyplug_abi/polyplug_abi/string_view_helper.py");
        let result: ValidationResult = validate_file(&golden_methods(), &sdk_path)?;
        assert!(
            result.is_complete(),
            "python SDK missing methods: {:?}",
            result.missing_methods
        );
        assert_eq!(result.found_methods.len(), 5);
        Ok(())
    }
}

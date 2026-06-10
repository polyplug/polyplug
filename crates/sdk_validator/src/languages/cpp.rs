//! C++ SDK validator using ast-grep CLI.

use std::path::Path;

use crate::ast_grep::{AstGrepRunner, Match};
use crate::error::ValidatorError;
use crate::languages::LanguageValidator;

/// Validator for C++ SDK files.
///
/// Detects real function definitions only. A pattern like
/// `inline $RET name($$$) { $$$ }` cannot work here: tree-sitter-cpp parses
/// the `{ $$$ }` body of such a pattern as an initializer list, so it never
/// matches a `function_definition` node (verified empirically against
/// ast-grep 0.42). Instead the rule matches by node kind: a
/// `function_definition` whose declarator chain contains a
/// `function_declarator` named exactly `name` (anchored regex). This covers
/// `inline`/non-`inline`, `noexcept`, templated return types, and
/// pointer/reference returns — and cannot match call sites or comments.
pub struct CppValidator;

impl CppValidator {
    /// Create a new C++ validator.
    pub fn new() -> Self {
        Self
    }

    /// Generate the ast-grep inline rule for a C++ function definition.
    fn generate_rule(method_name: &str) -> String {
        format!(
            r#"id: find-function
language: cpp
severity: hint
rule:
  kind: function_definition
  has:
    field: declarator
    any:
      - kind: function_declarator
        has:
          field: declarator
          kind: identifier
          regex: ^{method_name}$
      - has:
          stopBy: end
          kind: function_declarator
          has:
            field: declarator
            kind: identifier
            regex: ^{method_name}$
"#
        )
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

    fn create_temp_cpp_file(content: &str) -> Result<NamedTempFile, Box<dyn core::error::Error>> {
        let mut file: NamedTempFile = NamedTempFile::with_suffix(".hpp")?;
        file.write_all(content.as_bytes())?;
        file.flush()?;
        Ok(file)
    }

    fn validate_file(
        methods: &[String],
        file: &Path,
    ) -> Result<ValidationResult, Box<dyn core::error::Error>> {
        let mut validator: CppValidator = CppValidator::new();
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
    fn test_detects_inline_function() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_cpp_file(
            r#"
inline std::string to_str(StringView sv) {
    return to_string(sv);
}
"#,
        )?;
        let result: ValidationResult = validate_file(&["to_str".to_string()], file.path())?;
        assert!(result.found_methods.contains(&"to_str".to_string()));
        Ok(())
    }

    #[test]
    fn test_detects_noexcept_function() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_cpp_file(
            r#"
inline bool starts_with(StringView sv, std::string_view prefix) noexcept {
    return true;
}
"#,
        )?;
        let result: ValidationResult = validate_file(&["starts_with".to_string()], file.path())?;
        assert!(result.found_methods.contains(&"starts_with".to_string()));
        Ok(())
    }

    #[test]
    fn test_detects_templated_return_type() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_cpp_file(
            r#"
inline std::vector<std::string_view> split(StringView sv, char delimiter) {
    return {};
}
"#,
        )?;
        let result: ValidationResult = validate_file(&["split".to_string()], file.path())?;
        assert!(result.found_methods.contains(&"split".to_string()));
        Ok(())
    }

    #[test]
    fn test_detects_non_inline_function() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_cpp_file(
            r#"
bool ends_with(StringView sv, std::string_view suffix) noexcept {
    return false;
}
"#,
        )?;
        let result: ValidationResult = validate_file(&["ends_with".to_string()], file.path())?;
        assert!(result.found_methods.contains(&"ends_with".to_string()));
        Ok(())
    }

    #[test]
    fn test_renamed_definition_does_not_match() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_cpp_file(
            r#"
inline std::string to_str2(StringView sv) {
    return to_string(sv);
}
"#,
        )?;
        let result: ValidationResult = validate_file(&["to_str".to_string()], file.path())?;
        assert!(result.found_methods.is_empty());
        Ok(())
    }

    #[test]
    fn test_call_site_only_does_not_match() -> Result<(), Box<dyn core::error::Error>> {
        // The pre-rework validator matched the bare identifier `to_str`,
        // so this file falsely passed. It must not match now.
        let file: NamedTempFile = create_temp_cpp_file(
            r#"
inline void other(StringView sv) {
    auto s = to_str(sv);
}
"#,
        )?;
        let result: ValidationResult = validate_file(&["to_str".to_string()], file.path())?;
        assert!(result.found_methods.is_empty());
        assert_eq!(result.missing_methods[0].method, "to_str");
        Ok(())
    }

    #[test]
    fn test_comment_only_does_not_match() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_cpp_file(
            r#"
// to_str(sv) converts a StringView; see also to_str overloads.
inline void unrelated() {}
"#,
        )?;
        let result: ValidationResult = validate_file(&["to_str".to_string()], file.path())?;
        assert!(result.found_methods.is_empty());
        Ok(())
    }

    #[test]
    fn test_real_sdk_has_all_golden_methods() -> Result<(), Box<dyn core::error::Error>> {
        let sdk_path: PathBuf = repo_path("sdks/cpp/abi/polyplug/abi.hpp");
        let result: ValidationResult = validate_file(&golden_methods(), &sdk_path)?;
        assert!(
            result.is_complete(),
            "cpp SDK missing methods: {:?}",
            result.missing_methods
        );
        assert_eq!(result.found_methods.len(), 5);
        Ok(())
    }
}

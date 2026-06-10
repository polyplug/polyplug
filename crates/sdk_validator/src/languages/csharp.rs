//! C# SDK validator using ast-grep CLI.

use std::path::Path;

use crate::ast_grep::{AstGrepRunner, Match};
use crate::error::ValidatorError;
use crate::languages::LanguageValidator;

/// Validator for C# SDK files.
///
/// Detects real `method_declaration` nodes via ast-grep inline rules with
/// `context`/`selector` (a bare method declaration is not valid top-level C#,
/// so plain patterns parse as `local_function_statement` and never match).
/// Covered shapes:
/// - block-bodied: `public static bool StartsWith(StringView sv, string p) { ... }`
/// - expression-bodied: `public static string ToStr(StringView sv) => ToString(sv);`
/// - extension methods: `public static string ToString(this StringView sv) { ... }`
///   (the `this` modifier is part of the parameter list, covered by `$$$`)
/// - `unsafe` methods: `public static unsafe string ToString(...)` — ast-grep
///   modifier matching is strict, so the unsafe shapes need their own variants
///
/// The name match is exact (an AST identifier, not a substring), so call
/// sites, comments, and renamed methods like `ToStr2` do not match.
pub struct CSharpValidator;

impl CSharpValidator {
    /// Create a new C# validator.
    pub fn new() -> Self {
        Self
    }

    /// Generate the ast-grep inline rule for a C# method declaration.
    fn generate_rule(method_name: &str) -> String {
        format!(
            r#"id: find-method
language: csharp
severity: hint
rule:
  any:
    - pattern:
        context: 'class _C {{ public static $RET {method_name}($$$) {{ $$$ }} }}'
        selector: method_declaration
    - pattern:
        context: 'class _C {{ public static $RET {method_name}($$$) => $EXPR; }}'
        selector: method_declaration
    - pattern:
        context: 'class _C {{ static $RET {method_name}($$$) {{ $$$ }} }}'
        selector: method_declaration
    - pattern:
        context: 'class _C {{ static $RET {method_name}($$$) => $EXPR; }}'
        selector: method_declaration
    - pattern:
        context: 'class _C {{ public static unsafe $RET {method_name}($$$) {{ $$$ }} }}'
        selector: method_declaration
    - pattern:
        context: 'class _C {{ public static unsafe $RET {method_name}($$$) => $EXPR; }}'
        selector: method_declaration
    - pattern:
        context: 'class _C {{ static unsafe $RET {method_name}($$$) {{ $$$ }} }}'
        selector: method_declaration
    - pattern:
        context: 'class _C {{ static unsafe $RET {method_name}($$$) => $EXPR; }}'
        selector: method_declaration
"#
        )
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

    fn create_temp_csharp_file(
        content: &str,
    ) -> Result<NamedTempFile, Box<dyn core::error::Error>> {
        let mut file: NamedTempFile = NamedTempFile::with_suffix(".cs")?;
        file.write_all(content.as_bytes())?;
        file.flush()?;
        Ok(file)
    }

    fn validate_file(
        methods: &[String],
        file: &Path,
    ) -> Result<ValidationResult, Box<dyn core::error::Error>> {
        let mut validator: CSharpValidator = CSharpValidator::new();
        let result: ValidationResult = validate_language(
            &mut validator,
            &runner(),
            NamingConvention::Pascal,
            "StringView",
            methods,
            &[file.to_path_buf()],
        )?;
        Ok(result)
    }

    #[test]
    fn test_detects_block_bodied_method() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_csharp_file(
            r#"
namespace Polyplug.Abi {
    public static class StringViewHelper {
        public static bool StartsWith(StringView sv, string prefix) {
            return true;
        }
    }
}
"#,
        )?;
        let result: ValidationResult = validate_file(&["starts_with".to_string()], file.path())?;
        assert!(result.found_methods.contains(&"starts_with".to_string()));
        Ok(())
    }

    #[test]
    fn test_detects_expression_bodied_method() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_csharp_file(
            r#"
namespace Polyplug.Abi {
    public static class StringViewHelper {
        public static string ToStr(StringView sv) => ToString(sv);
    }
}
"#,
        )?;
        let result: ValidationResult = validate_file(&["to_str".to_string()], file.path())?;
        assert!(result.found_methods.contains(&"to_str".to_string()));
        Ok(())
    }

    #[test]
    fn test_detects_extension_method() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_csharp_file(
            r#"
namespace Polyplug.Abi {
    public static class StringViewHelper {
        public static string ToString(this StringView sv) {
            return "";
        }
    }
}
"#,
        )?;
        let result: ValidationResult = validate_file(&["to_string".to_string()], file.path())?;
        assert!(result.found_methods.contains(&"to_string".to_string()));
        Ok(())
    }

    #[test]
    fn test_detects_unsafe_method() -> Result<(), Box<dyn core::error::Error>> {
        // The consolidated StringViewHelper.ToString is method-level unsafe;
        // ast-grep modifier matching is strict, so without dedicated unsafe
        // rule variants this shape reports missing.
        let file: NamedTempFile = create_temp_csharp_file(
            r#"
namespace Polyplug.Abi {
    public static class StringViewHelper {
        public static unsafe string ToStr(StringView sv) {
            return "";
        }
    }
}
"#,
        )?;
        let result: ValidationResult = validate_file(&["to_str".to_string()], file.path())?;
        assert!(result.found_methods.contains(&"to_str".to_string()));
        Ok(())
    }

    #[test]
    fn test_renamed_definition_does_not_match() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_csharp_file(
            r#"
public static class Helper {
    public static string ToStr2(StringView sv) => "";
}
"#,
        )?;
        let result: ValidationResult = validate_file(&["to_str".to_string()], file.path())?;
        assert!(result.found_methods.is_empty());
        Ok(())
    }

    #[test]
    fn test_call_site_only_does_not_match() -> Result<(), Box<dyn core::error::Error>> {
        // The pre-rework validator did `class_text.contains(" ToStr(")`,
        // so this call site falsely passed. It must not match now.
        let file: NamedTempFile = create_temp_csharp_file(
            r#"
public static class Helper {
    public static void Other(StringView sv) {
        var s = ToStr(sv);
    }
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
        // A comment containing " ToStr(" inside a static class falsely
        // passed the pre-rework substring check.
        let file: NamedTempFile = create_temp_csharp_file(
            r#"
public static class Helper {
    // call ToStr( here to convert a StringView
    public static void Unrelated() { }
}
"#,
        )?;
        let result: ValidationResult = validate_file(&["to_str".to_string()], file.path())?;
        assert!(result.found_methods.is_empty());
        Ok(())
    }

    #[test]
    fn test_real_sdk_has_all_golden_methods() -> Result<(), Box<dyn core::error::Error>> {
        let sdk_path: PathBuf = repo_path("sdks/csharp/abi/Abi.cs");
        let result: ValidationResult = validate_file(&golden_methods(), &sdk_path)?;
        assert!(
            result.is_complete(),
            "csharp SDK missing methods: {:?}",
            result.missing_methods
        );
        assert_eq!(result.found_methods.len(), 5);
        Ok(())
    }
}

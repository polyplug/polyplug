//! C# SDK validator using ast-grep CLI.

use std::path::Path;

use crate::ast_grep::{AstGrepRunner, Match};
use crate::error::ValidatorError;
use crate::languages::{LanguageValidator, parse_field_name_trailing, parse_variant_text};

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

    /// Generate the ast-grep inline rule matching every `field_declaration`
    /// inside the `struct_declaration` named `struct_name`
    /// (e.g. `public struct StringView {{ public IntPtr Ptr; ... }}`).
    fn generate_struct_rule(struct_name: &str) -> String {
        format!(
            r#"id: struct-fields
language: csharp
severity: hint
rule:
  kind: field_declaration
  inside:
    stopBy: end
    kind: struct_declaration
    has:
      field: name
      regex: ^{struct_name}$
"#
        )
    }

    /// Generate the ast-grep inline rule matching every
    /// `enum_member_declaration` inside the `enum_declaration` named
    /// `enum_name` (e.g. `public enum AbiErrorCode : uint {{ Ok = 0, ... }}`).
    fn generate_enum_rule(enum_name: &str) -> String {
        format!(
            r#"id: enum-variants
language: csharp
severity: hint
rule:
  kind: enum_member_declaration
  inside:
    stopBy: end
    kind: enum_declaration
    has:
      field: name
      regex: ^{enum_name}$
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

    fn enum_variants_in_file(
        &mut self,
        runner: &AstGrepRunner,
        enum_name: &str,
        file: &Path,
    ) -> Result<Vec<(String, Option<i64>)>, ValidatorError> {
        let rule: String = Self::generate_enum_rule(enum_name);
        let matches: Vec<Match> = runner.run_with_rule(&rule, file)?;
        Ok(matches
            .iter()
            .map(|m| parse_variant_text(&m.text))
            .collect())
    }

    fn struct_fields_in_file(
        &mut self,
        runner: &AstGrepRunner,
        struct_name: &str,
        file: &Path,
    ) -> Result<Vec<String>, ValidatorError> {
        let rule: String = Self::generate_struct_rule(struct_name);
        let matches: Vec<Match> = runner.run_with_rule(&rule, file)?;
        Ok(matches
            .iter()
            .map(|m| parse_field_name_trailing(&m.text))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;
    use tempfile::NamedTempFile;

    use crate::ast_grep::NamingConvention;
    use crate::languages::test_support::{
        golden_enum, golden_methods, golden_struct, repo_path, runner,
    };
    use crate::languages::{
        EnumValidationResult, FieldCheck, FieldOutcome, StructFieldValidationResult,
        ValidationResult, VariantCheck, VariantOutcome, validate_language, validate_language_enum,
        validate_language_struct,
    };

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

    fn validate_enum_file(
        enum_name: &str,
        file: &Path,
    ) -> Result<EnumValidationResult, Box<dyn core::error::Error>> {
        let mut validator: CSharpValidator = CSharpValidator::new();
        let result: EnumValidationResult = validate_language_enum(
            &mut validator,
            &runner(),
            enum_name,
            &golden_enum(enum_name),
            &[file.to_path_buf()],
        )?;
        Ok(result)
    }

    #[test]
    fn test_enum_exact_match_passes() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_csharp_file(
            r#"
namespace Polyplug.Abi;
public enum DispatchType : uint
{
    Native = 0,
    VirtualMachine = 1,
}
"#,
        )?;
        let result: EnumValidationResult = validate_enum_file("DispatchType", file.path())?;
        assert!(result.is_complete(), "unexpected drift: {result:?}");
        Ok(())
    }

    #[test]
    fn test_enum_wrong_value_fails_with_expected_vs_found()
    -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_csharp_file(
            r#"
public enum DispatchType : uint
{
    Native = 0,
    VirtualMachine = 9,
}
"#,
        )?;
        let result: EnumValidationResult = validate_enum_file("DispatchType", file.path())?;
        let check: &VariantCheck = result
            .checks
            .iter()
            .find(|c| c.variant == "VirtualMachine")
            .ok_or("missing VirtualMachine check")?;
        assert_eq!(check.expected, 1);
        assert_eq!(check.outcome, VariantOutcome::WrongValue { found: 9 });
        Ok(())
    }

    #[test]
    fn test_enum_missing_and_extra_variants_fail() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_csharp_file(
            r#"
public enum DispatchType : uint
{
    Native = 0,
    Stale = 2,
}
"#,
        )?;
        let result: EnumValidationResult = validate_enum_file("DispatchType", file.path())?;
        let check: &VariantCheck = result
            .checks
            .iter()
            .find(|c| c.variant == "VirtualMachine")
            .ok_or("missing VirtualMachine check")?;
        assert_eq!(check.outcome, VariantOutcome::Missing);
        assert_eq!(result.extra_variants.len(), 1);
        assert_eq!(result.extra_variants[0].variant, "Stale");
        Ok(())
    }

    #[test]
    fn test_enum_commented_out_variant_does_not_count() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_csharp_file(
            r#"
// DispatchType has VirtualMachine = 1 per the ABI.
public enum DispatchType : uint
{
    Native = 0,
    // VirtualMachine = 1,
}
"#,
        )?;
        let result: EnumValidationResult = validate_enum_file("DispatchType", file.path())?;
        let check: &VariantCheck = result
            .checks
            .iter()
            .find(|c| c.variant == "VirtualMachine")
            .ok_or("missing VirtualMachine check")?;
        assert_eq!(check.outcome, VariantOutcome::Missing);
        assert!(result.extra_variants.is_empty());
        Ok(())
    }

    #[test]
    fn test_enum_value_in_string_does_not_count() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_csharp_file(
            r#"
public static class Doc
{
    public const string Hint = "VirtualMachine = 1";
}
public enum DispatchType : uint
{
    Native = 0,
}
"#,
        )?;
        let result: EnumValidationResult = validate_enum_file("DispatchType", file.path())?;
        let check: &VariantCheck = result
            .checks
            .iter()
            .find(|c| c.variant == "VirtualMachine")
            .ok_or("missing VirtualMachine check")?;
        assert_eq!(check.outcome, VariantOutcome::Missing);
        Ok(())
    }

    #[test]
    fn test_real_abi_mirror_matches_golden_enums() -> Result<(), Box<dyn core::error::Error>> {
        let path: PathBuf = repo_path("sdks/csharp/abi/Abi.cs");
        for enum_name in [
            "AbiErrorCode",
            "LogLevel",
            "DispatchType",
            "ReloadPhaseType",
        ] {
            let result: EnumValidationResult = validate_enum_file(enum_name, &path)?;
            assert!(result.is_complete(), "{enum_name} drift: {result:?}");
        }
        Ok(())
    }

    #[test]
    fn test_real_host_reload_phase_matches_golden_enum() -> Result<(), Box<dyn core::error::Error>>
    {
        let path: PathBuf = repo_path("sdks/csharp/host/ReloadPhase.cs");
        let result: EnumValidationResult = validate_enum_file("ReloadPhaseType", &path)?;
        assert!(result.is_complete(), "ReloadPhaseType drift: {result:?}");
        Ok(())
    }

    fn validate_struct_file(
        struct_name: &str,
        golden_fields: &[String],
        file: &Path,
    ) -> Result<StructFieldValidationResult, Box<dyn core::error::Error>> {
        let mut validator: CSharpValidator = CSharpValidator::new();
        let result: StructFieldValidationResult = validate_language_struct(
            &mut validator,
            &runner(),
            NamingConvention::Pascal,
            struct_name,
            golden_fields,
            &[file.to_path_buf()],
        )?;
        Ok(result)
    }

    #[test]
    fn test_struct_exact_match_passes() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_csharp_file(
            r#"
public struct StringView
{
    public IntPtr Ptr;
    public nuint Len;
}
"#,
        )?;
        let result: StructFieldValidationResult =
            validate_struct_file("StringView", &golden_struct("StringView"), file.path())?;
        assert!(result.is_complete(), "unexpected drift: {result:?}");
        Ok(())
    }

    #[test]
    fn test_struct_renamed_field_is_missing_and_extra() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_csharp_file(
            r#"
public struct StringView
{
    public IntPtr Pointer;
    public nuint Len;
}
"#,
        )?;
        let result: StructFieldValidationResult =
            validate_struct_file("StringView", &golden_struct("StringView"), file.path())?;
        assert!(!result.is_complete());
        let check: &FieldCheck = result
            .checks
            .iter()
            .find(|c| c.field == "ptr")
            .ok_or("missing ptr check")?;
        assert_eq!(check.outcome, FieldOutcome::Missing);
        assert_eq!(result.extra_fields.len(), 1);
        assert_eq!(result.extra_fields[0].field, "Pointer");
        Ok(())
    }

    #[test]
    fn test_struct_comment_only_does_not_match() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_csharp_file(
            r#"
public static class Doc
{
    // public struct StringView { public IntPtr Ptr; public nuint Len; }
}
"#,
        )?;
        let result: StructFieldValidationResult =
            validate_struct_file("StringView", &golden_struct("StringView"), file.path())?;
        assert!(
            result
                .checks
                .iter()
                .all(|c| c.outcome == FieldOutcome::Missing)
        );
        Ok(())
    }

    #[test]
    fn test_struct_other_struct_in_same_file_not_confused()
    -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_csharp_file(
            r#"
public struct Other
{
    public uint A;
    public uint B;
}
public struct StringView
{
    public IntPtr Ptr;
    public nuint Len;
}
"#,
        )?;
        let result: StructFieldValidationResult =
            validate_struct_file("StringView", &golden_struct("StringView"), file.path())?;
        assert!(result.is_complete(), "unexpected drift: {result:?}");
        Ok(())
    }

    #[test]
    fn test_real_abi_mirror_structs_match_golden() -> Result<(), Box<dyn core::error::Error>> {
        let path: PathBuf = repo_path("sdks/csharp/abi/Abi.cs");
        for struct_name in ["StringView", "AbiError"] {
            let result: StructFieldValidationResult =
                validate_struct_file(struct_name, &golden_struct(struct_name), &path)?;
            assert!(result.is_complete(), "{struct_name} drift: {result:?}");
        }
        Ok(())
    }
}

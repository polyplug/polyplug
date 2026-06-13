//! Python SDK validator using ast-grep CLI.

use std::path::Path;

use crate::ast_grep::{AstGrepRunner, Match};
use crate::error::ValidatorError;
use crate::languages::{LanguageValidator, parse_variant_text};

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

    /// Generate the ast-grep inline rule matching every `string` literal that
    /// is the first element of a `tuple` inside the `_fields_` list of the
    /// ctypes `class_definition` named `struct_name`
    /// (e.g. `_fields_ = [("ptr", ctypes.c_void_p), ...]`). The ctypes type is
    /// never a bare string literal, so only the field-name strings match.
    fn generate_struct_rule(struct_name: &str) -> String {
        format!(
            r#"id: struct-fields
language: python
severity: hint
rule:
  kind: string
  inside:
    stopBy: end
    kind: tuple
    inside:
      stopBy: end
      kind: list
      inside:
        stopBy: end
        kind: assignment
        has:
          field: left
          regex: ^_fields_$
        inside:
          stopBy: end
          kind: class_definition
          has:
            field: name
            regex: ^{struct_name}$
"#
        )
    }

    /// Generate the ast-grep inline rule matching class-body-level
    /// assignments inside the `class_definition` named `enum_name`
    /// (e.g. `class AbiErrorCode(enum.IntEnum):` with `Ok = 0` members).
    ///
    /// The neighbor-only `inside` chain (assignment -> expression_statement
    /// -> block -> class_definition) excludes assignments inside method
    /// bodies of the class.
    fn generate_enum_rule(enum_name: &str) -> String {
        format!(
            r#"id: enum-variants
language: python
severity: hint
rule:
  kind: assignment
  inside:
    kind: expression_statement
    inside:
      kind: block
      inside:
        kind: class_definition
        has:
          field: name
          regex: ^{enum_name}$
"#
        )
    }
}

impl Default for PythonValidator {
    fn default() -> Self {
        Self::new()
    }
}

/// Strip the surrounding quotes from a ctypes `_fields_` field-name string
/// literal (`"ptr"` -> `ptr`, `'ptr'` -> `ptr`).
fn parse_field_string(text: &str) -> String {
    text.trim()
        .trim_matches(|c| c == '"' || c == '\'')
        .to_string()
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
            .map(|m| parse_field_string(&m.text))
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

    fn validate_enum_file(
        enum_name: &str,
        file: &Path,
    ) -> Result<EnumValidationResult, Box<dyn core::error::Error>> {
        let mut validator: PythonValidator = PythonValidator::new();
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
        let file: NamedTempFile = create_temp_python_file(
            r#"
import enum


class DispatchType(enum.IntEnum):
    """Dispatch mechanism type."""
    Native = 0
    VirtualMachine = 1
"#,
        )?;
        let result: EnumValidationResult = validate_enum_file("DispatchType", file.path())?;
        assert!(result.is_complete(), "unexpected drift: {result:?}");
        Ok(())
    }

    #[test]
    fn test_enum_wrong_value_fails_with_expected_vs_found()
    -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_python_file(
            r#"
class DispatchType(enum.IntEnum):
    Native = 0
    VirtualMachine = 4
"#,
        )?;
        let result: EnumValidationResult = validate_enum_file("DispatchType", file.path())?;
        let check: &VariantCheck = result
            .checks
            .iter()
            .find(|c| c.variant == "VirtualMachine")
            .ok_or("missing VirtualMachine check")?;
        assert_eq!(check.expected, 1);
        assert_eq!(check.outcome, VariantOutcome::WrongValue { found: 4 });
        Ok(())
    }

    #[test]
    fn test_enum_missing_and_extra_variants_fail() -> Result<(), Box<dyn core::error::Error>> {
        // SCREAMING_CASE members are drift: golden names are PascalCase, so
        // NATIVE both leaves Native missing and surfaces as a stale extra.
        let file: NamedTempFile = create_temp_python_file(
            r#"
class DispatchType(enum.IntEnum):
    NATIVE = 0
    VirtualMachine = 1
"#,
        )?;
        let result: EnumValidationResult = validate_enum_file("DispatchType", file.path())?;
        let check: &VariantCheck = result
            .checks
            .iter()
            .find(|c| c.variant == "Native")
            .ok_or("missing Native check")?;
        assert_eq!(check.outcome, VariantOutcome::Missing);
        assert_eq!(result.extra_variants.len(), 1);
        assert_eq!(result.extra_variants[0].variant, "NATIVE");
        Ok(())
    }

    #[test]
    fn test_enum_commented_out_variant_does_not_count() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_python_file(
            r#"
# DispatchType has VirtualMachine = 1 per the ABI.
class DispatchType(enum.IntEnum):
    """VirtualMachine = 1 is documented here only."""
    Native = 0
    # VirtualMachine = 1
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
    fn test_enum_method_body_assignments_do_not_count() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_python_file(
            r#"
class DispatchType(enum.IntEnum):
    Native = 0
    VirtualMachine = 1


class Other:
    def __init__(self):
        self.Stale = 9
"#,
        )?;
        let result: EnumValidationResult = validate_enum_file("DispatchType", file.path())?;
        assert!(result.is_complete(), "unexpected drift: {result:?}");
        Ok(())
    }

    #[test]
    fn test_real_abi_mirror_matches_golden_enums() -> Result<(), Box<dyn core::error::Error>> {
        let path: PathBuf = repo_path("sdks/python/abi/abi.py");
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
    fn test_real_package_reload_phase_matches_golden_enum()
    -> Result<(), Box<dyn core::error::Error>> {
        let path: PathBuf = repo_path("sdks/python/polyplug_abi/polyplug_abi/__init__.py");
        let result: EnumValidationResult = validate_enum_file("ReloadPhaseType", &path)?;
        assert!(result.is_complete(), "ReloadPhaseType drift: {result:?}");
        Ok(())
    }

    fn validate_struct_file(
        struct_name: &str,
        golden_fields: &[String],
        file: &Path,
    ) -> Result<StructFieldValidationResult, Box<dyn core::error::Error>> {
        let mut validator: PythonValidator = PythonValidator::new();
        let result: StructFieldValidationResult = validate_language_struct(
            &mut validator,
            &runner(),
            struct_name,
            golden_fields,
            &[file.to_path_buf()],
        )?;
        Ok(result)
    }

    #[test]
    fn test_struct_exact_match_passes() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_python_file(
            r#"
import ctypes
class StringView(ctypes.Structure):
    _fields_ = [
        ("ptr", ctypes.c_void_p),
        ("len", ctypes.c_size_t),
    ]
"#,
        )?;
        let result: StructFieldValidationResult =
            validate_struct_file("StringView", &golden_struct("StringView"), file.path())?;
        assert!(result.is_complete(), "unexpected drift: {result:?}");
        Ok(())
    }

    #[test]
    fn test_struct_renamed_field_is_missing_and_extra() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_python_file(
            r#"
import ctypes
class StringView(ctypes.Structure):
    _fields_ = [
        ("pointer", ctypes.c_void_p),
        ("len", ctypes.c_size_t),
    ]
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
        assert_eq!(result.extra_fields[0].field, "pointer");
        Ok(())
    }

    #[test]
    fn test_struct_comment_only_does_not_match() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_python_file(
            r#"
import ctypes
# class StringView(ctypes.Structure): _fields_ = [("ptr", c_void_p), ("len", c_size_t)]
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
    fn test_struct_other_class_not_confused() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_python_file(
            r#"
import ctypes
class Other(ctypes.Structure):
    _fields_ = [
        ("a", ctypes.c_uint32),
        ("b", ctypes.c_uint32),
    ]
class StringView(ctypes.Structure):
    _fields_ = [
        ("ptr", ctypes.c_void_p),
        ("len", ctypes.c_size_t),
    ]
"#,
        )?;
        let result: StructFieldValidationResult =
            validate_struct_file("StringView", &golden_struct("StringView"), file.path())?;
        assert!(result.is_complete(), "unexpected drift: {result:?}");
        Ok(())
    }

    #[test]
    fn test_real_abi_mirror_structs_match_golden() -> Result<(), Box<dyn core::error::Error>> {
        let path: PathBuf = repo_path("sdks/python/abi/abi.py");
        for struct_name in ["StringView", "AbiError"] {
            let result: StructFieldValidationResult =
                validate_struct_file(struct_name, &golden_struct(struct_name), &path)?;
            assert!(result.is_complete(), "{struct_name} drift: {result:?}");
        }
        Ok(())
    }
}

//! Rust SDK validator using ast-grep CLI.

use std::path::Path;

use crate::ast_grep::{AstGrepRunner, Match};
use crate::error::ValidatorError;
use crate::languages::{LanguageValidator, parse_field_name_typed, parse_variant_text};

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

    /// Generate the ast-grep inline rule matching every `field_declaration`
    /// node inside the `struct_item` named `struct_name`.
    fn generate_struct_rule(struct_name: &str) -> String {
        format!(
            r#"id: struct-fields
language: rust
severity: hint
rule:
  kind: field_declaration
  inside:
    stopBy: end
    kind: struct_item
    has:
      field: name
      regex: ^{struct_name}$
"#
        )
    }

    /// Generate the ast-grep inline rule matching every `enum_variant` node
    /// inside the `enum_item` named `enum_name`.
    fn generate_enum_rule(enum_name: &str) -> String {
        format!(
            r#"id: enum-variants
language: rust
severity: hint
rule:
  kind: enum_variant
  inside:
    stopBy: end
    kind: enum_item
    has:
      field: name
      regex: ^{enum_name}$
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
            .map(|m| parse_field_name_typed(&m.text))
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
        EnumValidationResult, StructFieldValidationResult, ValidationResult, VariantCheck,
        VariantOutcome, validate_language, validate_language_enum, validate_language_struct,
    };

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

    fn validate_enum_file(
        enum_name: &str,
        file: &Path,
    ) -> Result<EnumValidationResult, Box<dyn core::error::Error>> {
        let mut validator: RustValidator = RustValidator::new();
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
        let file: NamedTempFile = create_temp_rust_file(
            r#"
#[repr(u32)]
pub enum DispatchType {
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
        let file: NamedTempFile = create_temp_rust_file(
            r#"
pub enum DispatchType {
    Native = 0,
    VirtualMachine = 2,
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
        assert_eq!(check.outcome, VariantOutcome::WrongValue { found: 2 });
        Ok(())
    }

    #[test]
    fn test_enum_missing_variant_fails() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_rust_file(
            r#"
pub enum DispatchType {
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
    fn test_enum_extra_variant_fails() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_rust_file(
            r#"
pub enum DispatchType {
    Native = 0,
    VirtualMachine = 1,
    Stale = 2,
}
"#,
        )?;
        let result: EnumValidationResult = validate_enum_file("DispatchType", file.path())?;
        assert!(!result.is_complete());
        assert_eq!(result.extra_variants.len(), 1);
        assert_eq!(result.extra_variants[0].variant, "Stale");
        assert_eq!(result.extra_variants[0].value, Some(2));
        Ok(())
    }

    #[test]
    fn test_enum_commented_out_variant_does_not_count() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_rust_file(
            r#"
pub enum DispatchType {
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
        // The commented-out line must not surface as an extra variant either.
        assert!(result.extra_variants.is_empty());
        Ok(())
    }

    #[test]
    fn test_enum_value_in_comment_or_string_does_not_count()
    -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_rust_file(
            r#"
// DispatchType has VirtualMachine = 1 per the ABI.
pub const DOC: &str = "VirtualMachine = 1";
pub enum DispatchType {
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
    fn test_enum_other_enum_in_same_file_is_not_confused() -> Result<(), Box<dyn core::error::Error>>
    {
        let file: NamedTempFile = create_temp_rust_file(
            r#"
pub enum Other {
    Native = 7,
    VirtualMachine = 8,
}
pub enum DispatchType {
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
    fn test_real_abi_sources_match_golden_enums() -> Result<(), Box<dyn core::error::Error>> {
        let targets: [(&str, &str); 4] = [
            (
                "AbiErrorCode",
                "crates/polyplug_abi/src/types/error_code.rs",
            ),
            ("LogLevel", "crates/polyplug_abi/src/types/log_level.rs"),
            (
                "DispatchType",
                "crates/polyplug_abi/src/dispatch/dispatch_type.rs",
            ),
            (
                "ReloadPhaseType",
                "crates/polyplug_abi/src/runtime/reload_phase.rs",
            ),
        ];
        for (enum_name, relative) in targets {
            let path: PathBuf = repo_path(relative);
            let result: EnumValidationResult = validate_enum_file(enum_name, &path)?;
            assert!(result.is_complete(), "{enum_name} drift: {result:?}");
        }
        Ok(())
    }

    fn validate_struct_file(
        struct_name: &str,
        golden_fields: &[String],
        file: &Path,
    ) -> Result<StructFieldValidationResult, Box<dyn core::error::Error>> {
        let mut validator: RustValidator = RustValidator::new();
        let result: StructFieldValidationResult = validate_language_struct(
            &mut validator,
            &runner(),
            NamingConvention::Snake,
            struct_name,
            golden_fields,
            &[file.to_path_buf()],
        )?;
        Ok(result)
    }

    #[test]
    fn test_struct_exact_match_passes() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_rust_file(
            r#"
#[repr(C)]
pub struct StringView {
    pub ptr: *const u8,
    pub len: usize,
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
        let file: NamedTempFile = create_temp_rust_file(
            r#"
#[repr(C)]
pub struct StringView {
    pub pointer: *const u8,
    pub len: usize,
}
"#,
        )?;
        let result: StructFieldValidationResult =
            validate_struct_file("StringView", &golden_struct("StringView"), file.path())?;
        assert!(!result.is_complete());
        let check: &crate::languages::FieldCheck = result
            .checks
            .iter()
            .find(|c| c.field == "ptr")
            .ok_or("missing ptr check")?;
        assert_eq!(check.outcome, crate::languages::FieldOutcome::Missing);
        assert_eq!(result.extra_fields.len(), 1);
        assert_eq!(result.extra_fields[0].field, "pointer");
        Ok(())
    }

    #[test]
    fn test_struct_underscore_marker_is_skipped() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_rust_file(
            r#"
#[repr(C)]
pub struct Array<T: Sized> {
    pub items: *mut T,
    pub len: usize,
    pub align: usize,
    _marker: PhantomData<T>,
}
"#,
        )?;
        let result: StructFieldValidationResult =
            validate_struct_file("Array", &golden_struct("Array"), file.path())?;
        assert!(result.is_complete(), "unexpected drift: {result:?}");
        assert!(result.extra_fields.is_empty());
        Ok(())
    }

    #[test]
    fn test_struct_comment_only_does_not_match() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_rust_file(
            r#"
// pub struct StringView { pub ptr: *const u8, pub len: usize }
"#,
        )?;
        let result: StructFieldValidationResult =
            validate_struct_file("StringView", &golden_struct("StringView"), file.path())?;
        // No real definition, so every golden field is missing.
        assert!(
            result
                .checks
                .iter()
                .all(|c| c.outcome == crate::languages::FieldOutcome::Missing)
        );
        Ok(())
    }

    #[test]
    fn test_struct_other_struct_in_same_file_not_confused()
    -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_rust_file(
            r#"
#[repr(C)]
pub struct Other {
    pub a: u32,
    pub b: u32,
}
#[repr(C)]
pub struct StringView {
    pub ptr: *const u8,
    pub len: usize,
}
"#,
        )?;
        let result: StructFieldValidationResult =
            validate_struct_file("StringView", &golden_struct("StringView"), file.path())?;
        assert!(result.is_complete(), "unexpected drift: {result:?}");
        Ok(())
    }

    #[test]
    fn test_real_abi_structs_match_golden() -> Result<(), Box<dyn core::error::Error>> {
        let targets: [(&str, &str); 2] = [
            ("StringView", "crates/polyplug_abi/src/types/string_view.rs"),
            ("AbiError", "crates/polyplug_abi/src/types/abi_error.rs"),
        ];
        for (struct_name, relative) in targets {
            let path: PathBuf = repo_path(relative);
            let result: StructFieldValidationResult =
                validate_struct_file(struct_name, &golden_struct(struct_name), &path)?;
            assert!(result.is_complete(), "{struct_name} drift: {result:?}");
        }
        Ok(())
    }
}

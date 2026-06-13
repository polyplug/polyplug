//! C++ SDK validator using ast-grep CLI.

use std::path::Path;

use crate::ast_grep::{AstGrepRunner, Match};
use crate::error::ValidatorError;
use crate::languages::{LanguageValidator, parse_field_name_trailing, parse_variant_text};

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

    /// Generate the ast-grep inline rule matching every `field_declaration`
    /// inside the `struct_specifier` named `struct_name`. A bodyless forward
    /// declaration (`struct StringView;`) has no `field_declaration` children
    /// and contributes nothing.
    fn generate_struct_rule(struct_name: &str) -> String {
        format!(
            r#"id: struct-fields
language: cpp
severity: hint
rule:
  kind: field_declaration
  inside:
    stopBy: end
    kind: struct_specifier
    has:
      field: name
      regex: ^{struct_name}$
"#
        )
    }

    /// Generate the ast-grep inline rule matching every `enumerator` inside
    /// the `enum_specifier` named `enum_name`. Covers both `enum class X`
    /// and plain `enum X`; a bodyless forward declaration has no enumerators
    /// and contributes nothing.
    fn generate_enum_rule(enum_name: &str) -> String {
        format!(
            r#"id: enum-variants
language: cpp
severity: hint
rule:
  kind: enumerator
  inside:
    stopBy: end
    kind: enum_specifier
    has:
      field: name
      regex: ^{enum_name}$
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
inline std::vector<std::string_view> split(StringView sv, std::string_view delimiter) {
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

    fn validate_enum_file(
        enum_name: &str,
        file: &Path,
    ) -> Result<EnumValidationResult, Box<dyn core::error::Error>> {
        let mut validator: CppValidator = CppValidator::new();
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
    fn test_enum_exact_match_passes_with_forward_declaration()
    -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_cpp_file(
            r#"
enum class DispatchType : uint32_t;
enum class DispatchType : uint32_t {
    Native = 0,
    VirtualMachine = 1,
};
"#,
        )?;
        let result: EnumValidationResult = validate_enum_file("DispatchType", file.path())?;
        assert!(result.is_complete(), "unexpected drift: {result:?}");
        Ok(())
    }

    #[test]
    fn test_enum_wrong_value_fails_with_expected_vs_found()
    -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_cpp_file(
            r#"
enum class DispatchType : uint32_t {
    Native = 3,
    VirtualMachine = 1,
};
"#,
        )?;
        let result: EnumValidationResult = validate_enum_file("DispatchType", file.path())?;
        let check: &VariantCheck = result
            .checks
            .iter()
            .find(|c| c.variant == "Native")
            .ok_or("missing Native check")?;
        assert_eq!(check.expected, 0);
        assert_eq!(check.outcome, VariantOutcome::WrongValue { found: 3 });
        Ok(())
    }

    #[test]
    fn test_enum_missing_and_extra_variants_fail() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_cpp_file(
            r#"
enum class DispatchType : uint32_t {
    Native = 0,
    Stale = 2,
};
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
        let file: NamedTempFile = create_temp_cpp_file(
            r#"
// DispatchType has VirtualMachine = 1 per the ABI.
enum class DispatchType : uint32_t {
    Native = 0,
    // VirtualMachine = 1,
};
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
        let file: NamedTempFile = create_temp_cpp_file(
            r#"
inline const char* hint() { return "VirtualMachine = 1"; }
enum class DispatchType : uint32_t {
    Native = 0,
};
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
        let path: PathBuf = repo_path("sdks/cpp/abi/polyplug/abi.hpp");
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

    fn validate_struct_file(
        struct_name: &str,
        golden_fields: &[String],
        file: &Path,
    ) -> Result<StructFieldValidationResult, Box<dyn core::error::Error>> {
        let mut validator: CppValidator = CppValidator::new();
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
        let file: NamedTempFile = create_temp_cpp_file(
            r#"
struct StringView {
    const uint8_t* ptr;
    size_t len;
};
"#,
        )?;
        let result: StructFieldValidationResult =
            validate_struct_file("StringView", &golden_struct("StringView"), file.path())?;
        assert!(result.is_complete(), "unexpected drift: {result:?}");
        Ok(())
    }

    #[test]
    fn test_struct_renamed_field_is_missing_and_extra() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_cpp_file(
            r#"
struct StringView {
    const uint8_t* pointer;
    size_t len;
};
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
    fn test_struct_forward_declaration_does_not_match() -> Result<(), Box<dyn core::error::Error>> {
        // A bodyless forward declaration must yield no fields.
        let file: NamedTempFile = create_temp_cpp_file(
            r#"
struct StringView;
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
    fn test_struct_comment_only_does_not_match() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_cpp_file(
            r#"
// struct StringView { const uint8_t* ptr; size_t len; };
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
        let file: NamedTempFile = create_temp_cpp_file(
            r#"
struct Other {
    uint32_t a;
    uint32_t b;
};
struct StringView {
    const uint8_t* ptr;
    size_t len;
};
"#,
        )?;
        let result: StructFieldValidationResult =
            validate_struct_file("StringView", &golden_struct("StringView"), file.path())?;
        assert!(result.is_complete(), "unexpected drift: {result:?}");
        Ok(())
    }

    #[test]
    fn test_real_abi_mirror_structs_match_golden() -> Result<(), Box<dyn core::error::Error>> {
        let path: PathBuf = repo_path("sdks/cpp/abi/polyplug/abi.hpp");
        for struct_name in ["StringView", "AbiError"] {
            let result: StructFieldValidationResult =
                validate_struct_file(struct_name, &golden_struct(struct_name), &path)?;
            assert!(result.is_complete(), "{struct_name} drift: {result:?}");
        }
        Ok(())
    }
}

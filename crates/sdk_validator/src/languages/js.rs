//! JavaScript/TypeScript SDK validator using ast-grep CLI.

use std::path::Path;

#[cfg(test)]
use core::error::Error;

use crate::ast_grep::{AstGrepRunner, Match};
use crate::error::ValidatorError;
use crate::languages::{LanguageValidator, parse_field_name_typed, parse_variant_text};

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

    /// Determine the ast-grep language name from the file extension.
    ///
    /// ast-grep uses the file extension to select the parser, so `.js` files
    /// must declare `language: javascript` while `.ts` / `.tsx` files use
    /// `language: typescript`.  Using the wrong language causes ast-grep to
    /// silently return no matches.
    fn ast_grep_language(file: &Path) -> &'static str {
        match file.extension().and_then(|e| e.to_str()) {
            Some("ts") | Some("tsx") => "typescript",
            _ => "javascript",
        }
    }

    /// Generate the ast-grep inline rule for a JS/TS function definition.
    fn generate_rule(method_name: &str, language: &str) -> String {
        format!(
            r#"id: find-function
language: {language}
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

    /// Generate the ast-grep inline rule for a TypeScript enum declaration
    /// (`export const enum X {{ Ok = 0, ... }}`). Matches both
    /// `enum_assignment` members and bare valueless members, so stale
    /// variants without a value are still detected.
    fn generate_ts_enum_rule(enum_name: &str) -> String {
        format!(
            r#"id: enum-variants
language: typescript
severity: hint
rule:
  any:
    - kind: enum_assignment
    - kind: property_identifier
      inside:
        kind: enum_body
  inside:
    stopBy: end
    kind: enum_declaration
    has:
      field: name
      regex: ^{enum_name}$
"#
        )
    }

    /// Generate the ast-grep inline rule matching every `property_signature`
    /// inside the TypeScript `interface_declaration` named `struct_name`
    /// (`export interface StringView {{ ptr: bigint; len: number; }}`). The
    /// `_OFFSET`/`_SIZE` const mirrors are not interface members and never
    /// match.
    fn generate_struct_rule(struct_name: &str) -> String {
        format!(
            r#"id: struct-fields
language: typescript
severity: hint
rule:
  kind: property_signature
  inside:
    stopBy: end
    kind: interface_declaration
    has:
      field: name
      regex: ^{struct_name}$
"#
        )
    }

    /// Generate the ast-grep inline rule for a plain-JS object-literal enum
    /// mirror (`export const X = {{ Ok: 0, ... }};`). Matches every `pair`
    /// inside the object assigned to the const named `enum_name`.
    fn generate_js_enum_rule(enum_name: &str) -> String {
        format!(
            r#"id: enum-variants
language: javascript
severity: hint
rule:
  kind: pair
  inside:
    kind: object
    inside:
      kind: variable_declarator
      has:
        field: name
        regex: ^{enum_name}$
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
        let language: &str = Self::ast_grep_language(file);
        let rule: String = Self::generate_rule(native_name, language);
        let matches: Vec<Match> = runner.run_with_rule(&rule, file)?;
        Ok(!matches.is_empty())
    }

    fn enum_variants_in_file(
        &mut self,
        runner: &AstGrepRunner,
        enum_name: &str,
        file: &Path,
    ) -> Result<Vec<(String, Option<i64>)>, ValidatorError> {
        let rule: String = match Self::ast_grep_language(file) {
            "typescript" => Self::generate_ts_enum_rule(enum_name),
            _ => Self::generate_js_enum_rule(enum_name),
        };
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
        // Struct mirrors are TypeScript `interface` declarations, which only
        // exist in `.ts` sources — the rule always parses as typescript.
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
        EnumValidationResult, FieldCheck, FieldOutcome, StructFieldValidationResult,
        ValidationResult, VariantCheck, VariantOutcome, validate_language, validate_language_enum,
        validate_language_struct,
    };

    fn create_temp_ts_file(content: &str) -> Result<NamedTempFile, Box<dyn Error>> {
        let mut file: NamedTempFile = NamedTempFile::with_suffix(".ts")?;
        file.write_all(content.as_bytes())?;
        file.flush()?;
        Ok(file)
    }

    fn create_temp_js_file(content: &str) -> Result<NamedTempFile, Box<dyn Error>> {
        let mut file: NamedTempFile = NamedTempFile::with_suffix(".js")?;
        file.write_all(content.as_bytes())?;
        file.flush()?;
        Ok(file)
    }

    fn validate_file(methods: &[String], file: &Path) -> Result<ValidationResult, Box<dyn Error>> {
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
    fn test_detects_annotated_function() -> Result<(), Box<dyn Error>> {
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
    fn test_detects_unannotated_function() -> Result<(), Box<dyn Error>> {
        // Plain-JS form — no type annotations, `.ts` extension used here.
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
    fn test_detects_arrow_functions() -> Result<(), Box<dyn Error>> {
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
    fn test_renamed_definition_does_not_match() -> Result<(), Box<dyn Error>> {
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
    fn test_call_site_only_does_not_match() -> Result<(), Box<dyn Error>> {
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
    fn test_comment_only_does_not_match() -> Result<(), Box<dyn Error>> {
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
    fn test_reports_missing_methods() -> Result<(), Box<dyn Error>> {
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
    fn test_detects_plain_js_function() -> Result<(), Box<dyn Error>> {
        // Validates that `.js` files are parsed with `language: javascript`, not
        // `language: typescript` — the two parsers are distinct in ast-grep and
        // the wrong choice silently produces zero matches.
        let file: NamedTempFile = create_temp_js_file(
            r#"
export function toStr(sv) {
    return '';
}
export function startsWith(sv, prefix) {
    return sv.startsWith(prefix);
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
    fn test_real_sdk_has_all_golden_methods() -> Result<(), Box<dyn Error>> {
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

    #[test]
    fn test_real_guest_js_has_all_golden_methods() -> Result<(), Box<dyn Error>> {
        let sdk_path: PathBuf = repo_path("sdks/js/guest/polyplug_guest.js");
        let result: ValidationResult = validate_file(&golden_methods(), &sdk_path)?;
        assert!(
            result.is_complete(),
            "js guest SDK missing methods: {:?}",
            result.missing_methods
        );
        assert_eq!(result.found_methods.len(), 5);
        Ok(())
    }

    fn validate_enum_file(
        enum_name: &str,
        file: &Path,
    ) -> Result<EnumValidationResult, Box<dyn Error>> {
        let mut validator: JsValidator = JsValidator::new();
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
    fn test_ts_enum_exact_match_passes() -> Result<(), Box<dyn Error>> {
        let file: NamedTempFile = create_temp_ts_file(
            r#"
export const enum DispatchType {
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
    fn test_ts_enum_wrong_value_fails_with_expected_vs_found() -> Result<(), Box<dyn Error>> {
        let file: NamedTempFile = create_temp_ts_file(
            r#"
export const enum DispatchType {
    Native = 0,
    VirtualMachine = 5,
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
        assert_eq!(check.outcome, VariantOutcome::WrongValue { found: 5 });
        Ok(())
    }

    #[test]
    fn test_ts_enum_missing_and_extra_variants_fail() -> Result<(), Box<dyn Error>> {
        let file: NamedTempFile = create_temp_ts_file(
            r#"
export const enum DispatchType {
    Native = 0,
    Stale,
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
        // A valueless stale member is still detected as extra.
        assert_eq!(result.extra_variants.len(), 1);
        assert_eq!(result.extra_variants[0].variant, "Stale");
        assert_eq!(result.extra_variants[0].value, None);
        Ok(())
    }

    #[test]
    fn test_ts_enum_commented_out_variant_does_not_count() -> Result<(), Box<dyn Error>> {
        let file: NamedTempFile = create_temp_ts_file(
            r#"
// DispatchType has VirtualMachine = 1 per the ABI.
export const enum DispatchType {
    Native = 0,
    // VirtualMachine = 1,
}
const doc: string = "VirtualMachine = 1";
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
    fn test_js_object_literal_exact_match_passes() -> Result<(), Box<dyn Error>> {
        let file: NamedTempFile = create_temp_js_file(
            r#"
export const DispatchType = {
    Native: 0,
    VirtualMachine: 1,
};
"#,
        )?;
        let result: EnumValidationResult = validate_enum_file("DispatchType", file.path())?;
        assert!(result.is_complete(), "unexpected drift: {result:?}");
        Ok(())
    }

    #[test]
    fn test_js_object_literal_wrong_value_fails_with_expected_vs_found()
    -> Result<(), Box<dyn Error>> {
        let file: NamedTempFile = create_temp_js_file(
            r#"
export const DispatchType = {
    Native: 0,
    VirtualMachine: 3,
};
"#,
        )?;
        let result: EnumValidationResult = validate_enum_file("DispatchType", file.path())?;
        let check: &VariantCheck = result
            .checks
            .iter()
            .find(|c| c.variant == "VirtualMachine")
            .ok_or("missing VirtualMachine check")?;
        assert_eq!(check.expected, 1);
        assert_eq!(check.outcome, VariantOutcome::WrongValue { found: 3 });
        Ok(())
    }

    #[test]
    fn test_js_object_literal_missing_and_extra_variants_fail() -> Result<(), Box<dyn Error>> {
        let file: NamedTempFile = create_temp_js_file(
            r#"
export const DispatchType = {
    Native: 0,
    Stale: 2,
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
    fn test_js_object_literal_comment_and_other_objects_do_not_count() -> Result<(), Box<dyn Error>>
    {
        let file: NamedTempFile = create_temp_js_file(
            r#"
// DispatchType has VirtualMachine: 1 per the ABI.
export const Other = {
    VirtualMachine: 1,
};
export const DispatchType = {
    Native: 0,
    // VirtualMachine: 1,
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
    fn test_real_abi_mirror_matches_golden_enums() -> Result<(), Box<dyn Error>> {
        let path: PathBuf = repo_path("sdks/js/abi/abi.ts");
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
    fn test_real_guest_js_matches_golden_enums() -> Result<(), Box<dyn Error>> {
        let path: PathBuf = repo_path("sdks/js/guest/polyplug_guest.js");
        for enum_name in ["AbiErrorCode", "LogLevel"] {
            let result: EnumValidationResult = validate_enum_file(enum_name, &path)?;
            assert!(result.is_complete(), "{enum_name} drift: {result:?}");
        }
        Ok(())
    }

    fn validate_struct_file(
        struct_name: &str,
        golden_fields: &[String],
        file: &Path,
    ) -> Result<StructFieldValidationResult, Box<dyn Error>> {
        let mut validator: JsValidator = JsValidator::new();
        // TypeScript interface field names are snake_case (unlike JS methods,
        // which are camelCase), and the comparator normalizes either way back
        // to the golden snake spelling — no naming argument needed.
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
    fn test_struct_exact_match_passes() -> Result<(), Box<dyn Error>> {
        let file: NamedTempFile = create_temp_ts_file(
            r#"
export interface StringView {
    ptr: bigint;
    len: number;
}
"#,
        )?;
        let result: StructFieldValidationResult =
            validate_struct_file("StringView", &golden_struct("StringView"), file.path())?;
        assert!(result.is_complete(), "unexpected drift: {result:?}");
        Ok(())
    }

    #[test]
    fn test_struct_renamed_field_is_missing_and_extra() -> Result<(), Box<dyn Error>> {
        let file: NamedTempFile = create_temp_ts_file(
            r#"
export interface StringView {
    pointer: bigint;
    len: number;
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
        assert_eq!(result.extra_fields[0].field, "pointer");
        Ok(())
    }

    #[test]
    fn test_struct_comment_only_does_not_match() -> Result<(), Box<dyn Error>> {
        let file: NamedTempFile = create_temp_ts_file(
            r#"
// export interface StringView { ptr: bigint; len: number; }
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
    fn test_struct_other_interface_not_confused() -> Result<(), Box<dyn Error>> {
        let file: NamedTempFile = create_temp_ts_file(
            r#"
export interface Other {
    a: number;
    b: number;
}
export interface StringView {
    ptr: bigint;
    len: number;
}
"#,
        )?;
        let result: StructFieldValidationResult =
            validate_struct_file("StringView", &golden_struct("StringView"), file.path())?;
        assert!(result.is_complete(), "unexpected drift: {result:?}");
        Ok(())
    }

    #[test]
    fn test_real_abi_mirror_structs_match_golden() -> Result<(), Box<dyn Error>> {
        let path: PathBuf = repo_path("sdks/js/abi/abi.ts");
        for struct_name in ["StringView", "AbiError"] {
            let result: StructFieldValidationResult =
                validate_struct_file(struct_name, &golden_struct(struct_name), &path)?;
            assert!(result.is_complete(), "{struct_name} drift: {result:?}");
        }
        Ok(())
    }
}

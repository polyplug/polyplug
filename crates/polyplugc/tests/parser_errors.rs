//! Parser error-handling integration tests for polyplugc.
//!
//! Covers: malformed TOML, missing required fields, duplicate names, invalid
//! type references, circular/chained enum expressions, enum validation, and
//! overflow / range detection in version parsing.
//!
//! Run with:
//!   cargo test --test parser_errors --package polyplugc

#![allow(clippy::expect_used)]

use polyplug_codegen::error::PolyplugcError;
use polyplugc::parser::{parse_api_str, parse_bundle_str};

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Assert that `parse_api_str` fails and the error matches the predicate.
fn api_err(toml: &str) -> PolyplugcError {
    parse_api_str(toml).expect_err("expected parse error but got Ok")
}

/// Assert that `parse_bundle_str` fails and the error matches the predicate.
fn bundle_err(toml: &str) -> PolyplugcError {
    parse_bundle_str(toml).expect_err("expected parse error but got Ok")
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. Malformed TOML
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn malformed_toml_missing_equals() {
    let err: PolyplugcError = api_err("[[contract]]\nname \"no-equals\"");
    assert!(
        matches!(err, PolyplugcError::ValidationFailed { .. }),
        "expected ValidationFailed for malformed TOML, got {err:?}",
    );
}

#[test]
fn malformed_toml_unclosed_bracket() {
    let err: PolyplugcError = api_err("[[contract\nname = \"x\"");
    assert!(
        matches!(err, PolyplugcError::ValidationFailed { .. }),
        "expected ValidationFailed for unclosed bracket, got {err:?}",
    );
}

#[test]
fn malformed_toml_invalid_string_literal() {
    // Unquoted string value is not valid TOML.
    let err: PolyplugcError = api_err("[[contract]]\nname = bad value");
    assert!(
        matches!(err, PolyplugcError::ValidationFailed { .. }),
        "expected ValidationFailed for invalid string literal, got {err:?}",
    );
}

#[test]
fn malformed_toml_duplicate_key_in_table() {
    let err: PolyplugcError = api_err("[bundle]\nname = \"x\"\nname = \"y\"\nversion = \"1.0\"");
    // TOML spec disallows duplicate keys — the TOML parser returns an error.
    assert!(
        matches!(err, PolyplugcError::ValidationFailed { .. }),
        "expected ValidationFailed for duplicate TOML key, got {err:?}",
    );
}

#[test]
fn malformed_bundle_toml_missing_bundle_section() {
    // A bundle.toml MUST have a [bundle] header.
    let err: PolyplugcError = bundle_err("name = \"x\"\nversion = \"1.0\"");
    assert!(
        matches!(err, PolyplugcError::ValidationFailed { .. }),
        "expected ValidationFailed for missing [bundle] section, got {err:?}",
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Missing required fields
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn bundle_missing_name_field() {
    // `name` is required under [bundle].
    let err: PolyplugcError = bundle_err("[bundle]\nversion = \"1.0\"");
    assert!(
        matches!(err, PolyplugcError::ValidationFailed { .. }),
        "expected ValidationFailed for missing bundle name, got {err:?}",
    );
}

#[test]
fn bundle_missing_version_field() {
    // `version` is required under [bundle].
    let err: PolyplugcError = bundle_err("[bundle]\nname = \"my-bundle\"");
    assert!(
        matches!(err, PolyplugcError::ValidationFailed { .. }),
        "expected ValidationFailed for missing bundle version, got {err:?}",
    );
}

#[test]
fn contract_missing_version_field() {
    // `version` is required inside [[contract]].
    let err: PolyplugcError = api_err("[[contract]]\nname = \"my.contract\"");
    assert!(
        matches!(err, PolyplugcError::ValidationFailed { .. }),
        "expected ValidationFailed for missing contract version, got {err:?}",
    );
}

#[test]
fn contract_missing_name_field() {
    // `name` is required inside [[contract]].
    let err: PolyplugcError = api_err("[[contract]]\nversion = \"1.0\"");
    assert!(
        matches!(err, PolyplugcError::ValidationFailed { .. }),
        "expected ValidationFailed for missing contract name, got {err:?}",
    );
}

#[test]
fn enum_missing_repr_field() {
    // `repr` is required inside [[enum]].
    let err: PolyplugcError =
        api_err("[[enum]]\nname = \"Status\"\n\n[[enum.variants]]\nname = \"Ok\"\nvalue = \"0\"");
    assert!(
        matches!(err, PolyplugcError::ValidationFailed { .. }),
        "expected ValidationFailed for missing enum repr, got {err:?}",
    );
}

#[test]
fn enum_variant_missing_value_field() {
    // `value` is required inside [[enum.variants]].
    let err: PolyplugcError =
        api_err("[[enum]]\nname = \"Status\"\nrepr = \"u32\"\n\n[[enum.variants]]\nname = \"Ok\"");
    assert!(
        matches!(err, PolyplugcError::ValidationFailed { .. }),
        "expected ValidationFailed for missing variant value, got {err:?}",
    );
}

#[test]
fn enum_variant_missing_name_field() {
    // `name` is required inside [[enum.variants]].
    let err: PolyplugcError =
        api_err("[[enum]]\nname = \"Status\"\nrepr = \"u32\"\n\n[[enum.variants]]\nvalue = \"0\"");
    assert!(
        matches!(err, PolyplugcError::ValidationFailed { .. }),
        "expected ValidationFailed for missing variant name, got {err:?}",
    );
}

#[test]
fn type_field_missing_type_key() {
    // `type` (renamed from `ty`) is required inside [[types.fields]].
    let err: PolyplugcError =
        api_err("[[types]]\nname = \"Point\"\n\n[[types.fields]]\nname = \"x\"");
    assert!(
        matches!(err, PolyplugcError::ValidationFailed { .. }),
        "expected ValidationFailed for missing field type, got {err:?}",
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Duplicate names (type vs enum collision)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn duplicate_type_and_enum_name_rejected() {
    // A [[types]] and [[enum]] share the name "Status".
    let toml: &str = concat!(
        "[[types]]\nname = \"Status\"\n\n",
        "[[enum]]\nname = \"Status\"\nrepr = \"u32\"\n\n",
        "[[enum.variants]]\nname = \"Ok\"\nvalue = \"0\"\n",
    );
    let err: PolyplugcError = api_err(toml);
    assert!(
        matches!(err, PolyplugcError::EnumNameCollision { ref name } if name == "Status"),
        "expected EnumNameCollision for \"Status\", got {err:?}",
    );
}

#[test]
fn duplicate_type_and_enum_different_names_ok() {
    // Names are distinct — should succeed.
    let toml: &str = concat!(
        "[[types]]\nname = \"Point\"\n",
        "[[types.fields]]\nname = \"x\"\ntype = \"u32\"\n\n",
        "[[enum]]\nname = \"Status\"\nrepr = \"u32\"\n\n",
        "[[enum.variants]]\nname = \"Ok\"\nvalue = \"0\"\n",
    );
    let result: Result<polyplugc::ir::ValidatedIr, PolyplugcError> = parse_api_str(toml);
    assert!(
        result.is_ok(),
        "expected Ok for distinct names, got {result:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Invalid type references
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn unknown_type_in_function_param_rejected() {
    let toml: &str = concat!(
        "[[contract]]\nname = \"test.math\"\nversion = \"1.0\"\n\n",
        "[[contract.functions]]\nname = \"add\"\n\n",
        "[[contract.functions.params]]\nname = \"a\"\ntype = \"NonExistentType\"\n",
    );
    let err: PolyplugcError = api_err(toml);
    assert!(
        matches!(
            err,
            PolyplugcError::UnknownType { ref type_ref, .. } if type_ref == "NonExistentType"
        ),
        "expected UnknownType for \"NonExistentType\", got {err:?}",
    );
}

#[test]
fn unknown_type_in_function_return_rejected() {
    let toml: &str = concat!(
        "[[contract]]\nname = \"test.math\"\nversion = \"1.0\"\n\n",
        "[[contract.functions]]\nname = \"compute\"\nreturn = \"GhostType\"\n",
    );
    let err: PolyplugcError = api_err(toml);
    assert!(
        matches!(
            err,
            PolyplugcError::UnknownType { ref type_ref, .. } if type_ref == "GhostType"
        ),
        "expected UnknownType for \"GhostType\", got {err:?}",
    );
}

#[test]
fn unknown_type_in_struct_field_rejected() {
    let toml: &str = concat!(
        "[[types]]\nname = \"Wrapper\"\n\n",
        "[[types.fields]]\nname = \"inner\"\ntype = \"UndeclaredStruct\"\n",
    );
    let err: PolyplugcError = api_err(toml);
    assert!(
        matches!(
            err,
            PolyplugcError::UnknownType { ref type_ref, .. } if type_ref == "UndeclaredStruct"
        ),
        "expected UnknownType for \"UndeclaredStruct\", got {err:?}",
    );
}

#[test]
fn known_primitive_types_accepted() {
    // All primitives should resolve without error.
    let toml: &str = concat!(
        "[[contract]]\nname = \"prims\"\nversion = \"1.0\"\n\n",
        "[[contract.functions]]\nname = \"a\"\n",
        "[[contract.functions.params]]\nname = \"p\"\ntype = \"u8\"\n\n",
        "[[contract.functions]]\nname = \"b\"\n",
        "[[contract.functions.params]]\nname = \"p\"\ntype = \"u16\"\n\n",
        "[[contract.functions]]\nname = \"c\"\n",
        "[[contract.functions.params]]\nname = \"p\"\ntype = \"u32\"\n\n",
        "[[contract.functions]]\nname = \"d\"\n",
        "[[contract.functions.params]]\nname = \"p\"\ntype = \"u64\"\n\n",
        "[[contract.functions]]\nname = \"e\"\n",
        "[[contract.functions.params]]\nname = \"p\"\ntype = \"i8\"\n\n",
        "[[contract.functions]]\nname = \"f\"\n",
        "[[contract.functions.params]]\nname = \"p\"\ntype = \"i16\"\n\n",
        "[[contract.functions]]\nname = \"g\"\n",
        "[[contract.functions.params]]\nname = \"p\"\ntype = \"i32\"\n\n",
        "[[contract.functions]]\nname = \"h\"\n",
        "[[contract.functions.params]]\nname = \"p\"\ntype = \"i64\"\n\n",
        "[[contract.functions]]\nname = \"i\"\n",
        "[[contract.functions.params]]\nname = \"p\"\ntype = \"f32\"\n\n",
        "[[contract.functions]]\nname = \"j\"\n",
        "[[contract.functions.params]]\nname = \"p\"\ntype = \"f64\"\n\n",
        "[[contract.functions]]\nname = \"k\"\n",
        "[[contract.functions.params]]\nname = \"p\"\ntype = \"bool\"\n",
    );
    let result: Result<polyplugc::ir::ValidatedIr, PolyplugcError> = parse_api_str(toml);
    assert!(
        result.is_ok(),
        "expected all primitives to resolve, got {result:?}"
    );
}

#[test]
fn abi_builtins_accepted_as_type_refs() {
    // StringView, Buffer, Ptr, Void are all valid ABI types.
    let toml: &str = concat!(
        "[[contract]]\nname = \"builtins\"\nversion = \"1.0\"\n\n",
        "[[contract.functions]]\nname = \"a\"\n",
        "[[contract.functions.params]]\nname = \"s\"\ntype = \"StringView\"\n\n",
        "[[contract.functions]]\nname = \"b\"\nreturn = \"Buffer\"\n\n",
        "[[contract.functions]]\nname = \"c\"\nreturn = \"Void\"\n\n",
        "[[contract.functions]]\nname = \"d\"\nreturn = \"ptr\"\n",
    );
    let result: Result<polyplugc::ir::ValidatedIr, PolyplugcError> = parse_api_str(toml);
    assert!(
        result.is_ok(),
        "expected ABI builtins to resolve, got {result:?}"
    );
}

#[test]
fn user_defined_type_resolves_when_declared() {
    // A [[types]] struct used as a param type should resolve.
    let toml: &str = concat!(
        "[[types]]\nname = \"Point\"\n",
        "[[types.fields]]\nname = \"x\"\ntype = \"f32\"\n",
        "[[types.fields]]\nname = \"y\"\ntype = \"f32\"\n\n",
        "[[contract]]\nname = \"geo\"\nversion = \"1.0\"\n\n",
        "[[contract.functions]]\nname = \"distance\"\n",
        "[[contract.functions.params]]\nname = \"p\"\ntype = \"Point\"\n",
    );
    let result: Result<polyplugc::ir::ValidatedIr, PolyplugcError> = parse_api_str(toml);
    assert!(
        result.is_ok(),
        "expected user-defined type to resolve, got {result:?}"
    );
}

#[test]
fn enum_type_resolves_as_param_type() {
    // An [[enum]] name used as a param type should resolve.
    let toml: &str = concat!(
        "[[enum]]\nname = \"Status\"\nrepr = \"u32\"\n",
        "[[enum.variants]]\nname = \"Ok\"\nvalue = \"0\"\n\n",
        "[[contract]]\nname = \"svc\"\nversion = \"1.0\"\n\n",
        "[[contract.functions]]\nname = \"get_status\"\nreturn = \"Status\"\n",
    );
    let result: Result<polyplugc::ir::ValidatedIr, PolyplugcError> = parse_api_str(toml);
    assert!(
        result.is_ok(),
        "expected enum type to resolve as return, got {result:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Circular / chained enum variant references
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn enum_forward_reference_rejected() {
    // Variant B references C which is declared after B.
    let toml: &str = concat!(
        "[[enum]]\nname = \"Flags\"\nrepr = \"u32\"\n\n",
        "[[enum.variants]]\nname = \"A\"\nvalue = \"1\"\n",
        "[[enum.variants]]\nname = \"B\"\nvalue = \"C | 1\"\n",
        "[[enum.variants]]\nname = \"C\"\nvalue = \"2\"\n",
    );
    let err: PolyplugcError = api_err(toml);
    assert!(
        matches!(
            err,
            PolyplugcError::EnumForwardRef { ref ref_name, ref variant_name, .. }
                if ref_name == "C" && variant_name == "B"
        ),
        "expected EnumForwardRef C in B, got {err:?}",
    );
}

#[test]
fn enum_chained_reference_rejected() {
    // A = 1, B = A | 1 (ok), C = B | 2 (chained: C→B→A)
    let toml: &str = concat!(
        "[[enum]]\nname = \"Flags\"\nrepr = \"u32\"\n\n",
        "[[enum.variants]]\nname = \"A\"\nvalue = \"1\"\n",
        "[[enum.variants]]\nname = \"B\"\nvalue = \"A | 1\"\n",
        "[[enum.variants]]\nname = \"C\"\nvalue = \"B | 2\"\n",
    );
    let err: PolyplugcError = api_err(toml);
    assert!(
        matches!(
            err,
            PolyplugcError::EnumChainedRef { ref variant_name, ref ref_name, .. }
                if variant_name == "C" && ref_name == "B"
        ),
        "expected EnumChainedRef C→B, got {err:?}",
    );
}

#[test]
fn enum_single_level_ref_accepted() {
    // B = A | 1 is fine (one level deep, backward reference).
    let toml: &str = concat!(
        "[[enum]]\nname = \"Flags\"\nrepr = \"u32\"\n\n",
        "[[enum.variants]]\nname = \"A\"\nvalue = \"1\"\n",
        "[[enum.variants]]\nname = \"B\"\nvalue = \"A | 1\"\n",
    );
    let result: Result<polyplugc::ir::ValidatedIr, PolyplugcError> = parse_api_str(toml);
    assert!(
        result.is_ok(),
        "expected single-level backward ref to succeed, got {result:?}"
    );
}

#[test]
fn enum_multiple_back_refs_in_one_value_accepted() {
    // D = A | B | C  — all backward refs, none of A/B/C ref any variant.
    let toml: &str = concat!(
        "[[enum]]\nname = \"Flags\"\nrepr = \"u32\"\n\n",
        "[[enum.variants]]\nname = \"A\"\nvalue = \"1\"\n",
        "[[enum.variants]]\nname = \"B\"\nvalue = \"2\"\n",
        "[[enum.variants]]\nname = \"C\"\nvalue = \"4\"\n",
        "[[enum.variants]]\nname = \"D\"\nvalue = \"A | B | C\"\n",
    );
    let result: Result<polyplugc::ir::ValidatedIr, PolyplugcError> = parse_api_str(toml);
    assert!(
        result.is_ok(),
        "expected multi-back-ref expression to succeed, got {result:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Enum validation: invalid repr, invalid expression tokens
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn enum_invalid_repr_i32_rejected() {
    let toml: &str = concat!(
        "[[enum]]\nname = \"Status\"\nrepr = \"i32\"\n\n",
        "[[enum.variants]]\nname = \"Ok\"\nvalue = \"0\"\n",
    );
    let err: PolyplugcError = api_err(toml);
    assert!(
        matches!(
            err,
            PolyplugcError::EnumInvalidRepr { ref enum_name, ref repr }
                if enum_name == "Status" && repr == "i32"
        ),
        "expected EnumInvalidRepr for i32, got {err:?}",
    );
}

#[test]
fn enum_invalid_repr_f32_rejected() {
    let toml: &str = concat!(
        "[[enum]]\nname = \"Mode\"\nrepr = \"f32\"\n\n",
        "[[enum.variants]]\nname = \"Fast\"\nvalue = \"0\"\n",
    );
    let err: PolyplugcError = api_err(toml);
    assert!(
        matches!(
            err,
            PolyplugcError::EnumInvalidRepr { ref enum_name, ref repr }
                if enum_name == "Mode" && repr == "f32"
        ),
        "expected EnumInvalidRepr for f32, got {err:?}",
    );
}

#[test]
fn enum_invalid_repr_empty_string_rejected() {
    let toml: &str = concat!(
        "[[enum]]\nname = \"E\"\nrepr = \"\"\n\n",
        "[[enum.variants]]\nname = \"X\"\nvalue = \"0\"\n",
    );
    let err: PolyplugcError = api_err(toml);
    assert!(
        matches!(err, PolyplugcError::EnumInvalidRepr { .. }),
        "expected EnumInvalidRepr for empty repr, got {err:?}",
    );
}

#[test]
fn enum_all_valid_reprs_accepted() {
    for repr in &["u8", "u16", "u32", "u64"] {
        let toml: String = format!(
            "[[enum]]\nname = \"E\"\nrepr = \"{repr}\"\n\n[[enum.variants]]\nname = \"X\"\nvalue = \"0\"\n"
        );
        let result: Result<polyplugc::ir::ValidatedIr, PolyplugcError> = parse_api_str(&toml);
        assert!(
            result.is_ok(),
            "expected repr={repr} to be accepted, got {result:?}",
        );
    }
}

#[test]
fn enum_invalid_value_expr_ampersand_rejected() {
    // `&` is not a supported operator.
    let toml: &str = concat!(
        "[[enum]]\nname = \"Flags\"\nrepr = \"u32\"\n\n",
        "[[enum.variants]]\nname = \"A\"\nvalue = \"1 & 2\"\n",
    );
    let err: PolyplugcError = api_err(toml);
    assert!(
        matches!(
            err,
            PolyplugcError::EnumInvalidValueExpr { ref variant_name, .. }
                if variant_name == "A"
        ),
        "expected EnumInvalidValueExpr for `&` operator, got {err:?}",
    );
}

#[test]
fn enum_invalid_value_expr_single_lt_rejected() {
    // `<` alone (not `<<`) is invalid.
    let toml: &str = concat!(
        "[[enum]]\nname = \"Flags\"\nrepr = \"u32\"\n\n",
        "[[enum.variants]]\nname = \"A\"\nvalue = \"1 < 2\"\n",
    );
    let err: PolyplugcError = api_err(toml);
    assert!(
        matches!(
            err,
            PolyplugcError::EnumInvalidValueExpr { ref variant_name, .. }
                if variant_name == "A"
        ),
        "expected EnumInvalidValueExpr for single `<`, got {err:?}",
    );
}

#[test]
fn enum_valid_hex_literal_accepted() {
    let toml: &str = concat!(
        "[[enum]]\nname = \"Flags\"\nrepr = \"u32\"\n\n",
        "[[enum.variants]]\nname = \"A\"\nvalue = \"0xFF\"\n",
        "[[enum.variants]]\nname = \"B\"\nvalue = \"0x0A\"\n",
    );
    let result: Result<polyplugc::ir::ValidatedIr, PolyplugcError> = parse_api_str(toml);
    assert!(
        result.is_ok(),
        "expected hex literals to be accepted, got {result:?}"
    );
}

#[test]
fn enum_valid_binary_literal_accepted() {
    let toml: &str = concat!(
        "[[enum]]\nname = \"Flags\"\nrepr = \"u32\"\n\n",
        "[[enum.variants]]\nname = \"A\"\nvalue = \"0b1010\"\n",
    );
    let result: Result<polyplugc::ir::ValidatedIr, PolyplugcError> = parse_api_str(toml);
    assert!(
        result.is_ok(),
        "expected binary literals to be accepted, got {result:?}"
    );
}

#[test]
fn enum_valid_tilde_operator_accepted() {
    let toml: &str = concat!(
        "[[enum]]\nname = \"Flags\"\nrepr = \"u32\"\n\n",
        "[[enum.variants]]\nname = \"All\"\nvalue = \"~0\"\n",
    );
    let result: Result<polyplugc::ir::ValidatedIr, PolyplugcError> = parse_api_str(toml);
    assert!(
        result.is_ok(),
        "expected ~ operator to be accepted, got {result:?}"
    );
}

#[test]
fn enum_valid_grouped_expr_accepted() {
    let toml: &str = concat!(
        "[[enum]]\nname = \"Flags\"\nrepr = \"u32\"\n\n",
        "[[enum.variants]]\nname = \"A\"\nvalue = \"(1 << 2)\"\n",
    );
    let result: Result<polyplugc::ir::ValidatedIr, PolyplugcError> = parse_api_str(toml);
    assert!(
        result.is_ok(),
        "expected grouped expression to be accepted, got {result:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Version parsing — overflow / invalid formats
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn version_non_numeric_major_rejected() {
    let err: PolyplugcError = api_err("[[contract]]\nname = \"x\"\nversion = \"abc.0\"");
    assert!(
        matches!(err, PolyplugcError::ValidationFailed { .. }),
        "expected ValidationFailed for non-numeric major, got {err:?}",
    );
}

#[test]
fn version_negative_component_rejected() {
    // u32::parse rejects negative numbers.
    let err: PolyplugcError = api_err("[[contract]]\nname = \"x\"\nversion = \"-1.0\"");
    assert!(
        matches!(err, PolyplugcError::ValidationFailed { .. }),
        "expected ValidationFailed for negative version component, got {err:?}",
    );
}

#[test]
fn version_overflow_rejected() {
    // 4294967296 > u32::MAX
    let err: PolyplugcError = api_err("[[contract]]\nname = \"x\"\nversion = \"4294967296.0\"");
    assert!(
        matches!(err, PolyplugcError::ValidationFailed { .. }),
        "expected ValidationFailed for overflowing version, got {err:?}",
    );
}

#[test]
fn version_minor_overflow_rejected() {
    let err: PolyplugcError = api_err("[[contract]]\nname = \"x\"\nversion = \"1.4294967296\"");
    assert!(
        matches!(err, PolyplugcError::ValidationFailed { .. }),
        "expected ValidationFailed for overflowing minor version, got {err:?}",
    );
}

#[test]
fn version_patch_overflow_rejected() {
    let err: PolyplugcError = api_err("[[contract]]\nname = \"x\"\nversion = \"1.0.4294967296\"");
    assert!(
        matches!(err, PolyplugcError::ValidationFailed { .. }),
        "expected ValidationFailed for overflowing patch version, got {err:?}",
    );
}

#[test]
fn version_empty_string_rejected() {
    let err: PolyplugcError = api_err("[[contract]]\nname = \"x\"\nversion = \"\"");
    // Empty string: split('.') yields [""], parse_u32("") will fail.
    assert!(
        matches!(err, PolyplugcError::ValidationFailed { .. }),
        "expected ValidationFailed for empty version string, got {err:?}",
    );
}

#[test]
fn version_single_component_accepted() {
    // "1" → major=1, minor=0, patch=0 — should succeed.
    let result: Result<polyplugc::ir::ValidatedIr, PolyplugcError> =
        parse_api_str("[[contract]]\nname = \"x\"\nversion = \"1\"");
    assert!(
        result.is_ok(),
        "expected single component version to succeed, got {result:?}"
    );
}

#[test]
fn version_major_minor_accepted() {
    // "2.3" → major=2, minor=3, patch=0 — should succeed.
    let result: Result<polyplugc::ir::ValidatedIr, PolyplugcError> =
        parse_api_str("[[contract]]\nname = \"x\"\nversion = \"2.3\"");
    assert!(
        result.is_ok(),
        "expected major.minor version to succeed, got {result:?}"
    );
}

#[test]
fn bundle_version_overflow_rejected() {
    let err: PolyplugcError = bundle_err("[bundle]\nname = \"b\"\nversion = \"4294967296.0\"");
    assert!(
        matches!(err, PolyplugcError::ValidationFailed { .. }),
        "expected ValidationFailed for overflowing bundle version, got {err:?}",
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Bundle-specific validation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn bundle_with_no_plugins_accepted() {
    // plugins are optional.
    let toml: &str = "[bundle]\nname = \"empty-bundle\"\nversion = \"1.0\"\nfile = \"test.so\"";
    let result: Result<polyplugc::ir::ValidatedIr, PolyplugcError> = parse_bundle_str(toml);
    assert!(
        result.is_ok(),
        "expected bundle with no plugins to succeed, got {result:?}"
    );
}

#[test]
fn bundle_plugin_missing_version_rejected() {
    let err: PolyplugcError = bundle_err(concat!(
        "[bundle]\nname = \"b\"\nversion = \"1.0\"\n\n",
        "[[plugin]]\nname = \"my-plugin\"\n",
    ));
    assert!(
        matches!(err, PolyplugcError::ValidationFailed { .. }),
        "expected ValidationFailed for plugin missing version, got {err:?}",
    );
}

#[test]
fn bundle_plugin_invalid_version_rejected() {
    let err: PolyplugcError = bundle_err(concat!(
        "[bundle]\nname = \"b\"\nversion = \"1.0\"\n\n",
        "[[plugin]]\nname = \"my-plugin\"\nversion = \"bad.ver\"\n",
    ));
    assert!(
        matches!(err, PolyplugcError::ValidationFailed { .. }),
        "expected ValidationFailed for plugin invalid version, got {err:?}",
    );
}

#[test]
fn bundle_well_formed_succeeds() {
    let toml: &str = concat!(
        "[bundle]\nname = \"codec-bundle\"\nversion = \"2.0.1\"\nfile = \"test.so\"\n\n",
        "[[plugin]]\nname = \"jpeg-encoder\"\nversion = \"1.0.0\"\n",
        "implements = [\"image.encode@1.0\"]\n\n",
        "[[dependency]]\nkind = \"contract\"\ncontract = \"image.encode\"\nmin_version = \"1.0\"\n",
    );
    let result: Result<polyplugc::ir::ValidatedIr, PolyplugcError> = parse_bundle_str(toml);
    assert!(
        result.is_ok(),
        "expected well-formed bundle to succeed, got {result:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Edge cases — empty inputs, whitespace-only, mixed valid/invalid
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn empty_api_toml_accepted() {
    // An empty file is a valid (empty) API schema.
    let result: Result<polyplugc::ir::ValidatedIr, PolyplugcError> = parse_api_str("");
    assert!(
        result.is_ok(),
        "expected empty API TOML to succeed, got {result:?}"
    );
}

#[test]
fn whitespace_only_api_toml_accepted() {
    let result: Result<polyplugc::ir::ValidatedIr, PolyplugcError> = parse_api_str("   \n\n  ");
    assert!(
        result.is_ok(),
        "expected whitespace-only API TOML to succeed, got {result:?}"
    );
}

#[test]
fn comments_only_api_toml_accepted() {
    let result: Result<polyplugc::ir::ValidatedIr, PolyplugcError> =
        parse_api_str("# This is just a comment\n# Another comment\n");
    assert!(
        result.is_ok(),
        "expected comments-only API TOML to succeed, got {result:?}"
    );
}

#[test]
fn multiple_contracts_all_parse() {
    let toml: &str = concat!(
        "[[contract]]\nname = \"svc.alpha\"\nversion = \"1.0\"\n\n",
        "[[contract.functions]]\nname = \"ping\"\n\n",
        "[[contract]]\nname = \"svc.beta\"\nversion = \"2.1\"\n\n",
        "[[contract.functions]]\nname = \"pong\"\n",
    );
    let ir: polyplugc::ir::ValidatedIr =
        parse_api_str(toml).expect("expected multiple contracts to parse");
    assert_eq!(ir.contracts.len(), 2, "expected 2 contracts");
    assert_eq!(ir.contracts[0].name, "svc.alpha");
    assert_eq!(ir.contracts[1].name, "svc.beta");
}

#[test]
fn second_contract_has_bad_type_rejected() {
    // First contract is fine; second has an unknown type reference.
    let toml: &str = concat!(
        "[[contract]]\nname = \"svc.good\"\nversion = \"1.0\"\n\n",
        "[[contract.functions]]\nname = \"ok\"\n\n",
        "[[contract]]\nname = \"svc.bad\"\nversion = \"1.0\"\n\n",
        "[[contract.functions]]\nname = \"broken\"\n\n",
        "[[contract.functions.params]]\nname = \"x\"\ntype = \"Phantom\"\n",
    );
    let err: PolyplugcError = api_err(toml);
    assert!(
        matches!(
            err,
            PolyplugcError::UnknownType { ref type_ref, .. } if type_ref == "Phantom"
        ),
        "expected UnknownType for Phantom in second contract, got {err:?}",
    );
}

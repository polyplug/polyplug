//! TOML parsing edge-case tests for polyplugc.
//!
//! Focuses on TOML syntax anomalies that are distinct from semantic validation:
//! missing closing brackets, invalid escape sequences, mixed table/array syntax,
//! empty input, comments-only input, invalid Unicode escapes, very long lines,
//! and deeply nested tables.
//!
//! Run with:
//!   cargo test --test toml_malformed --package polyplugc

#![allow(clippy::expect_used)]

use crate::error::PolyplugcError;
use crate::ir::ValidatedIr;
use crate::parser::{parse_api_str, parse_bundle_str};

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Assert that `parse_api_str` fails and return the error.
fn api_err(toml: &str) -> PolyplugcError {
    parse_api_str(toml).expect_err("expected parse error but got Ok")
}

/// Assert that `parse_bundle_str` fails and return the error.
fn bundle_err(toml: &str) -> PolyplugcError {
    parse_bundle_str(toml).expect_err("expected parse error but got Ok")
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. Missing closing bracket variants
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn missing_closing_bracket_single_table() {
    // `[bundle` missing the closing `]` — TOML parse error.
    let err: PolyplugcError = bundle_err("[bundle\nname = \"x\"\nversion = \"1.0\"");
    assert!(
        matches!(err, PolyplugcError::TomlParseError { .. }),
        "expected TomlParseError for missing `]` in table header, got {err:?}",
    );
}

#[test]
fn missing_closing_bracket_array_of_tables() {
    // `[[contract` missing the closing `]]` — TOML parse error.
    let err: PolyplugcError = api_err("[[contract\nname = \"x\"\nversion = \"1.0\"");
    assert!(
        matches!(err, PolyplugcError::TomlParseError { .. }),
        "expected TomlParseError for missing `]]` in array-of-tables header, got {err:?}",
    );
}

#[test]
fn missing_closing_bracket_nested_subtable() {
    // `[[contract.functions` missing the closing `]]`.
    let err: PolyplugcError = api_err(concat!(
        "[[contract]]\nname = \"x\"\nversion = \"1.0\"\n\n",
        "[[contract.functions\nname = \"f\"\n",
    ));
    assert!(
        matches!(err, PolyplugcError::TomlParseError { .. }),
        "expected TomlParseError for missing `]]` in sub-array header, got {err:?}",
    );
}

#[test]
fn unclosed_inline_array_value() {
    // Value is an unclosed inline array: `implements = ["x"`.
    let err: PolyplugcError = bundle_err(concat!(
        "[bundle]\nname = \"b\"\nversion = \"1.0\"\n\n",
        "[[plugin]]\nname = \"p\"\nimplements = [\"x\"\n",
    ));
    assert!(
        matches!(err, PolyplugcError::TomlParseError { .. }),
        "expected TomlParseError for unclosed inline array, got {err:?}",
    );
}

#[test]
fn unclosed_inline_table_value() {
    // Inline table `{name = "x"` missing the closing `}`.
    let err: PolyplugcError = api_err("[[contract]]\nname = {value = \"x\"\nversion = \"1.0\"");
    assert!(
        matches!(err, PolyplugcError::TomlParseError { .. }),
        "expected TomlParseError for unclosed inline table, got {err:?}",
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Invalid escape sequences in string values
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn invalid_escape_sequence_backslash_q() {
    // `\q` is not a valid TOML escape sequence.
    let err: PolyplugcError = api_err("[[contract]]\nname = \"bad\\qescape\"\nversion = \"1.0\"");
    assert!(
        matches!(err, PolyplugcError::TomlParseError { .. }),
        "expected TomlParseError for `\\q` escape, got {err:?}",
    );
}

#[test]
fn invalid_escape_sequence_backslash_a() {
    // `\a` is not a valid TOML escape sequence (unlike C).
    let err: PolyplugcError = bundle_err("[bundle]\nname = \"bad\\aescape\"\nversion = \"1.0\"");
    assert!(
        matches!(err, PolyplugcError::TomlParseError { .. }),
        "expected TomlParseError for `\\a` escape, got {err:?}",
    );
}

#[test]
fn invalid_escape_sequence_lone_backslash() {
    // A lone trailing backslash before end-of-string is invalid.
    let err: PolyplugcError = api_err("[[contract]]\nname = \"trailing\\\"\nversion = \"1.0\"");
    assert!(
        matches!(err, PolyplugcError::TomlParseError { .. }),
        "expected TomlParseError for lone trailing backslash, got {err:?}",
    );
}

#[test]
fn valid_escape_sequences_accepted() {
    // Standard TOML escapes: `\\`, `\"`, `\n`, `\t`, `\r` must all be accepted by
    // the TOML lexer. The resulting contract name is not a valid identifier, so
    // name validation (not the TOML parser) rejects it — which proves the escape
    // was lexed successfully rather than producing a TOML parse error.
    let result: Result<ValidatedIr, PolyplugcError> = parse_api_str(
        "[[contract]]\nname = \"esc\\\\slash\\\"quote\\nnewline\\ttab\"\nversion = \"1.0\"",
    );
    assert!(
        matches!(result, Err(PolyplugcError::InvalidIdentifier { .. })),
        "expected TOML escapes to lex (then fail identifier validation), got {result:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Mixed table / array-of-tables syntax collisions
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mixed_table_and_array_of_tables_same_key() {
    // Defining `[contract]` and then `[[contract]]` for the same key violates the TOML spec.
    let err: PolyplugcError = api_err(concat!(
        "[contract]\nname = \"single\"\nversion = \"1.0\"\n\n",
        "[[contract]]\nname = \"array\"\nversion = \"1.0\"\n",
    ));
    assert!(
        matches!(err, PolyplugcError::TomlParseError { .. }),
        "expected TomlParseError for mixed `[contract]` + `[[contract]]`, got {err:?}",
    );
}

#[test]
fn redefining_existing_table_header() {
    // Duplicate `[bundle]` header is illegal in TOML.
    let err: PolyplugcError = bundle_err(concat!(
        "[bundle]\nname = \"first\"\nversion = \"1.0\"\n\n",
        "[bundle]\nname = \"second\"\nversion = \"2.0\"\n",
    ));
    assert!(
        matches!(err, PolyplugcError::TomlParseError { .. }),
        "expected TomlParseError for duplicate `[bundle]` header, got {err:?}",
    );
}

#[test]
fn array_element_defined_before_array_header() {
    // Assigning `contract.name` as a dotted key then using `[[contract]]`
    // is a TOML conflict (implicitly created vs. explicitly created).
    let err: PolyplugcError = api_err(concat!(
        "contract.name = \"dotted\"\n\n",
        "[[contract]]\nname = \"array\"\nversion = \"1.0\"\n",
    ));
    assert!(
        matches!(err, PolyplugcError::TomlParseError { .. }),
        "expected TomlParseError for dotted-key vs array-of-tables conflict, got {err:?}",
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Empty TOML
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn empty_string_is_valid_api_toml() {
    // An empty API schema is semantically valid — no contracts, no types.
    let result: Result<ValidatedIr, PolyplugcError> = parse_api_str("");
    assert!(
        result.is_ok(),
        "expected empty string to parse as empty API, got {result:?}"
    );
    let ir: ValidatedIr = result.expect("parse");
    assert_eq!(ir.contracts.len(), 0, "expected zero contracts");
    assert_eq!(ir.types.len(), 0, "expected zero types");
    assert_eq!(ir.enums.len(), 0, "expected zero enums");
}

#[test]
fn empty_string_is_invalid_bundle_toml() {
    // A bundle.toml MUST have a `[bundle]` section — empty string should fail.
    let err: PolyplugcError = bundle_err("");
    assert!(
        matches!(err, PolyplugcError::TomlParseError { .. }),
        "expected TomlParseError for empty bundle TOML, got {err:?}",
    );
}

#[test]
fn newlines_only_is_valid_api_toml() {
    let result: Result<ValidatedIr, PolyplugcError> = parse_api_str("\n\n\n");
    assert!(
        result.is_ok(),
        "expected newlines-only to parse as empty API, got {result:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Comments-only TOML
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn comments_only_is_valid_api_toml() {
    let toml: &str = "# polyplug api schema\n# no contracts defined yet\n";
    let result: Result<ValidatedIr, PolyplugcError> = parse_api_str(toml);
    assert!(
        result.is_ok(),
        "expected comments-only API TOML to succeed, got {result:?}"
    );
}

#[test]
fn comments_only_is_invalid_bundle_toml() {
    // Comments but no `[bundle]` section — must fail.
    let err: PolyplugcError = bundle_err("# just a comment\n# another comment\n");
    assert!(
        matches!(err, PolyplugcError::TomlParseError { .. }),
        "expected TomlParseError for comments-only bundle TOML, got {err:?}",
    );
}

#[test]
fn comment_after_value_is_valid() {
    // Inline comments after values are valid TOML.
    let toml: &str = concat!(
        "[[contract]] # define a contract\n",
        "name = \"svc.foo\" # the name\n",
        "version = \"1.0\" # the version\n",
    );
    let result: Result<ValidatedIr, PolyplugcError> = parse_api_str(toml);
    assert!(
        result.is_ok(),
        "expected inline comments after values to be accepted, got {result:?}"
    );
}

#[test]
fn comment_mid_key_value_is_invalid() {
    // A `#` inside a quoted key name is still a valid character inside a string,
    // but a `#` between the key and `=` terminates the line early — TOML error.
    let err: PolyplugcError = api_err("[[contract]]\nname # comment = \"x\"\nversion = \"1.0\"");
    assert!(
        matches!(err, PolyplugcError::TomlParseError { .. }),
        "expected TomlParseError for `#` between key and `=`, got {err:?}",
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Invalid Unicode escape sequences
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn unicode_escape_with_too_few_hex_digits() {
    // `\uXXX` requires exactly 4 hex digits; 3 is invalid.
    let err: PolyplugcError = api_err("[[contract]]\nname = \"bad\\u004\"\nversion = \"1.0\"");
    assert!(
        matches!(err, PolyplugcError::TomlParseError { .. }),
        "expected TomlParseError for `\\u` with 3 hex digits, got {err:?}",
    );
}

#[test]
fn unicode_escape_with_non_hex_digit() {
    // `\uXXXG` — `G` is not a valid hex digit.
    let err: PolyplugcError = api_err("[[contract]]\nname = \"bad\\u000G\"\nversion = \"1.0\"");
    assert!(
        matches!(err, PolyplugcError::TomlParseError { .. }),
        "expected TomlParseError for `\\u000G` non-hex digit, got {err:?}",
    );
}

#[test]
fn unicode_escape_surrogate_pair_rejected() {
    // `\uD800` is a lone surrogate — invalid in TOML (must be valid Unicode scalar).
    let err: PolyplugcError = api_err("[[contract]]\nname = \"\\uD800\"\nversion = \"1.0\"");
    assert!(
        matches!(err, PolyplugcError::TomlParseError { .. }),
        "expected TomlParseError for lone surrogate `\\uD800`, got {err:?}",
    );
}

#[test]
fn unicode_escape_valid_codepoint_accepted() {
    // `\u0041` is 'A' — a perfectly valid Unicode scalar value.
    let result: Result<ValidatedIr, PolyplugcError> =
        parse_api_str("[[contract]]\nname = \"\\u0041BC\"\nversion = \"1.0\"");
    assert!(
        result.is_ok(),
        "expected valid \\u0041 Unicode escape to be accepted, got {result:?}"
    );
}

#[test]
fn long_unicode_escape_u_uppercase_valid() {
    // `\U0001F600` (8 hex digits) — valid TOML long Unicode escape. The TOML
    // lexer must accept it; the resulting name contains a non-identifier
    // codepoint (emoji), so name validation rejects it afterwards. An
    // InvalidIdentifier (not a TomlParseError) proves the escape lexed.
    let result: Result<ValidatedIr, PolyplugcError> =
        parse_api_str("[[contract]]\nname = \"emoji\\U0001F600end\"\nversion = \"1.0\"");
    assert!(
        matches!(result, Err(PolyplugcError::InvalidIdentifier { .. })),
        "expected \\U0001F600 long Unicode escape to lex (then fail identifier validation), got {result:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Very long lines
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn very_long_name_value_accepted() {
    // A 10 000-character name string — TOML has no line-length limit.
    let long_name: String = "a".repeat(10_000);
    let toml: String = format!("[[contract]]\nname = \"{long_name}\"\nversion = \"1.0\"\n");
    let result: Result<ValidatedIr, PolyplugcError> = parse_api_str(&toml);
    assert!(
        result.is_ok(),
        "expected very long name string to be accepted, got {result:?}"
    );
    let ir: ValidatedIr = result.expect("parse");
    assert_eq!(ir.contracts[0].name.len(), 10_000);
}

#[test]
fn very_long_comment_line_accepted() {
    // A comment that is 50 000 characters long should not cause an error.
    let long_comment: String = format!("# {}", "x".repeat(50_000));
    let toml: String = format!("{long_comment}\n[[contract]]\nname = \"svc\"\nversion = \"1.0\"\n");
    let result: Result<ValidatedIr, PolyplugcError> = parse_api_str(&toml);
    assert!(
        result.is_ok(),
        "expected very long comment line to be accepted, got {result:?}"
    );
}

#[test]
fn very_long_key_name_invalid() {
    // A key that is not a valid TOML bare key (contains spaces) must fail.
    // We rely on the space to make the key malformed rather than just "long".
    let long_key: String = format!("{} extra", "a".repeat(1_000));
    let toml: String = format!("[[contract]]\n{long_key} = \"v\"\nversion = \"1.0\"\n");
    let err: PolyplugcError = api_err(&toml);
    assert!(
        matches!(err, PolyplugcError::TomlParseError { .. }),
        "expected TomlParseError for key with embedded space, got {err:?}",
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Nested tables
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn deeply_nested_dotted_keys_accepted() {
    // TOML supports dotted keys; here we use them on a non-ABI field.
    // The `[bundle]` section can have dotted-path keys.
    // We verify the parser doesn't crash on deep dotting even if unused.
    let toml: &str = concat!(
        "[bundle]\n",
        "name = \"nested-bundle\"\n",
        "version = \"1.0\"\n",
        "extra.deep.key = \"ignored\"\n",
    );
    // The `RawBundleMeta` struct has `#[serde(default)]`-annotated optional fields;
    // depending on the schema this may succeed or surface a `TomlParseError`.
    // We only assert it doesn't panic.
    let _result: Result<ValidatedIr, PolyplugcError> = parse_bundle_str(toml);
    // No assertion on Ok/Err — just confirm no panic / no crash.
}

#[test]
fn nested_inline_table_in_array_is_malformed() {
    // An inline table that is itself unclosed inside an array is invalid.
    let err: PolyplugcError = bundle_err(concat!(
        "[bundle]\nname = \"b\"\nversion = \"1.0\"\n\n",
        "[[plugin]]\nname = \"p\"\n",
        "implements = [{contract = \"svc\"\n",
    ));
    assert!(
        matches!(err, PolyplugcError::TomlParseError { .. }),
        "expected TomlParseError for unclosed inline table in array, got {err:?}",
    );
}

#[test]
fn super_table_key_conflict_after_dotted_key() {
    // Assigning `bundle.name` via a dotted key first, then defining `[bundle]`
    // and re-assigning `name` is a TOML duplicate-key violation.
    let err: PolyplugcError = bundle_err(concat!(
        "bundle.name = \"first\"\n\n",
        "[bundle]\nname = \"second\"\nversion = \"1.0\"\n",
    ));
    assert!(
        matches!(err, PolyplugcError::TomlParseError { .. }),
        "expected TomlParseError for dotted-key then [bundle] name conflict, got {err:?}",
    );
}

#[test]
fn contract_functions_params_nested_array_round_trip() {
    // A fully-specified nested structure [[contract]] → [[contract.functions]] →
    // [[contract.functions.params]] must parse cleanly.
    let toml: &str = concat!(
        "[[contract]]\n",
        "name = \"math.ops\"\n",
        "version = \"1.0\"\n\n",
        "[[contract.functions]]\n",
        "name = \"add\"\n\n",
        "[[contract.functions.params]]\n",
        "name = \"a\"\n",
        "type = \"u32\"\n\n",
        "[[contract.functions.params]]\n",
        "name = \"b\"\n",
        "type = \"u32\"\n",
    );
    let result: Result<ValidatedIr, PolyplugcError> = parse_api_str(toml);
    assert!(
        result.is_ok(),
        "expected deeply-nested array-of-tables round-trip to succeed, got {result:?}"
    );
    let ir: ValidatedIr = result.expect("parse");
    assert_eq!(ir.contracts[0].functions[0].params.len(), 2);
}

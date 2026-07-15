#[path = "support/failure.rs"]
mod failure;
#[path = "support/success.rs"]
mod success;

use failure::PanicOnFailure;
use success::PanicOnSuccess;

use std::fs;

use polyplug_codegen::{InternalRustGenerateConfig, OutputLayout, generate_internal_rust};

use polyplug_codegen::PolyplugcError;
use polyplug_codegen::parser::parse_api_str;

const API: &str = r#"
[[enum]]
name = "Kind"
repr = "u32"
langs = { rust = { serde = "human-name-binary-discriminant", derives = ["Ord", "Ord"], attributes = ["allow(dead_code)"] } }
[[enum.variants]]
name = "Empty"
value = "17"
langs = { rust = { primary_name = "none", aliases = ["empty"], default = true, attributes = ["allow(non_camel_case_types)"] } }
[[enum.variants]]
name = "Boolean"
value = "305419896"
[[enum.variants]]
name = "Integer"
value = "2596069104"
[[enum.variants]]
name = "Decimal"
value = "3735928559"
[[enum.variants]]
name = "Text"
value = "4294967294"

[[enum]]
name = "SmallKind"
repr = "u8"
langs = { rust = { serde = "human-name-binary-discriminant" } }
[[enum.variants]]
name = "Tiny"
value = "7"
langs = { rust = { default = true } }
[[enum.variants]]
name = "Large"
value = "251"

[[enum]]
name = "MediumKind"
repr = "u16"
langs = { rust = { serde = "human-name-binary-discriminant" } }
[[enum.variants]]
name = "Medium"
value = "513"

[[enum]]
name = "LargeKind"
repr = "u64"
langs = { rust = { serde = "human-name-binary-discriminant" } }
[[enum.variants]]
name = "Large"
value = "4294967297"

[[types]]
name = "Value"
langs = { rust = { derives = ["Serialize", "Deserialize", "Serialize"], tagged_enum = { tag_field = "kind", variants = [{ tag = "Empty", name = "None", default = true }, { tag = "Boolean", name = "Bool", payload = "bool_value" }, { tag = "Integer", name = "Int", payload = "int_value" }, { tag = "Decimal", name = "Float", payload = "float_value" }, { tag = "Text", name = "String", payload = "string_value" }] } } }
[[types.fields]]
name = "kind"
type = "Kind"
[[types.fields]]
name = "bool_value"
type = "bool"
[[types.fields]]
name = "int_value"
type = "i64"
[[types.fields]]
name = "float_value"
type = "f64"
[[types.fields]]
name = "string_value"
type = "StringView"
[[types.fields]]
name = "values"
type = "Array<bool>"
langs = { rust = { empty_sequence_as_null = true } }
"#;

#[test]
fn lowers_deduplicated_typed_rust_rules() {
    let ir = parse_api_str(API).or_panic("valid Rust semantic rules");
    let enum_rules = ir.enums[0].langs.rust().or_panic("enum Rust rules");
    assert_eq!(enum_rules.derives, ["Ord"]);
    assert!(enum_rules.serde.is_some());
    let variant = ir.enums[0].variants[0]
        .langs
        .rust()
        .or_panic("variant rules");
    assert_eq!(variant.primary_name.as_deref(), Some("none"));
    assert_eq!(variant.aliases, ["empty"]);
    assert!(variant.default);
    let rules = ir
        .types
        .iter()
        .find(|ty| ty.name == "Value")
        .or_panic("Value type")
        .langs
        .rust()
        .or_panic("type Rust rules");
    assert_eq!(rules.derives, ["Serialize", "Deserialize"]);
    let projection = rules.tagged_enum.as_ref().or_panic("projection");
    assert_eq!(projection.tag_field, "kind");
    assert_eq!(
        projection.variants[1].payload.as_deref(),
        Some("bool_value")
    );
}

#[test]
fn rejects_incomplete_or_reused_tagged_enum_mappings() {
    let mapping = "{ tag = \"Empty\", name = \"None\", default = true }, { tag = \"Boolean\", name = \"Bool\", payload = \"bool_value\" }, { tag = \"Integer\", name = \"Int\", payload = \"int_value\" }, { tag = \"Decimal\", name = \"Float\", payload = \"float_value\" }, { tag = \"Text\", name = \"String\", payload = \"string_value\" }";
    for (replacement, expected) in [
        (
            "{ tag = \"Empty\", name = \"None\" }",
            "map every tag variant",
        ),
        (
            "{ tag = \"Empty\", name = \"None\" }, { tag = \"Boolean\", name = \"Bool\", payload = \"bool_value\" }, { tag = \"Integer\", name = \"Int\", payload = \"bool_value\" }, { tag = \"Decimal\", name = \"Float\", payload = \"float_value\" }, { tag = \"Text\", name = \"String\", payload = \"string_value\" }",
            "reuses field",
        ),
    ] {
        let api = API.replace(mapping, replacement);
        let error = parse_api_str(&api).err_or_panic("invalid tagged projection");
        match error {
            PolyplugcError::ValidationFailed { message } => {
                assert!(message.contains(expected), "{message}")
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }
}

#[test]
fn rejects_ambiguous_rust_enum_and_projection_names() {
    let missing_enum_rules = r#"
[[enum]]
name = "Kind"
repr = "u8"
[[enum.variants]]
name = "First"
value = "1"
langs = { rust = { default = true } }
[[enum.variants]]
name = "Second"
value = "2"
langs = { rust = { default = true } }
"#;
    let duplicate_human_names = r#"
[[enum]]
name = "Kind"
repr = "u8"
langs = { rust = { serde = "human-name-binary-discriminant" } }
[[enum.variants]]
name = "First"
value = "1"
langs = { rust = { primary_name = "shared" } }
[[enum.variants]]
name = "Second"
value = "2"
langs = { rust = { aliases = ["shared"] } }
"#;
    for (api, expected) in [
        (missing_enum_rules, "more than one default variant"),
        (duplicate_human_names, "reuses serialized name `shared`"),
    ] {
        let error = parse_api_str(api).err_or_panic("ambiguous Rust enum names must fail");
        assert!(
            matches!(error, PolyplugcError::ValidationFailed { ref message } if message.contains(expected)),
            "expected `{expected}`, got {error:?}"
        );
    }

    let invalid_projection = API.replace("name = \"None\"", "name = \"not-valid\"");
    assert!(
        matches!(
            parse_api_str(&invalid_projection),
            Err(PolyplugcError::InvalidIdentifier { .. })
        ),
        "projected Rust variants must be identifiers"
    );
    let keyword_projection = API.replace("name = \"None\"", "name = \"match\"");
    assert!(
        matches!(
            parse_api_str(&keyword_projection),
            Err(PolyplugcError::ReservedIdentifier { .. })
        ),
        "projected Rust variants must not use Rust keywords"
    );
    let duplicate_projection = API.replace(
        "tag = \"Boolean\", name = \"Bool\", payload = \"bool_value\"",
        "tag = \"Boolean\", name = \"None\", payload = \"bool_value\"",
    );
    let error = parse_api_str(&duplicate_projection)
        .err_or_panic("duplicate projected Rust variants must fail");
    assert!(
        matches!(error, PolyplugcError::ValidationFailed { ref message } if message.contains("reuses projected variant name")),
        "unexpected duplicate projection error: {error:?}"
    );
}

#[test]
fn rejects_misplaced_semantic_rules() {
    let error = parse_api_str("[langs.rust]\nempty_sequence_as_null = true\n")
        .err_or_panic("root field rule must fail");
    assert!(matches!(error, PolyplugcError::ValidationFailed { .. }));
    let invalid = parse_api_str(
        "[[types]]\nname = \"Invalid\"\n[[types.fields]]\nname = \"flag\"\ntype = \"bool\"\nlangs = { rust = { empty_sequence_as_null = true } }\n",
    )
    .err_or_panic("non-array null helper must fail");
    assert!(matches!(invalid, PolyplugcError::ValidationFailed { .. }));
}

#[test]
fn emits_flat_abi_and_tagged_enum_domain_projection() {
    let temp = tempfile::tempdir().or_panic("temporary API");
    let api = temp.path().join("api.toml");
    let bundle = temp.path().join("bundle.toml");
    fs::write(&api, API).or_panic("write API");
    fs::write(
        &bundle,
        "[bundle]\nname = \"tagged_projection\"\nversion = \"1.0\"\napi = \"api.toml\"\n\n[[plugin]]\nname = \"provider\"\nimplements = [\"projection.value@1.0\"]\n",
    )
    .or_panic("write bundle");
    let mut api_text = fs::read_to_string(&api).or_panic("read API");
    api_text.push_str(
        "\n[[guest_contract]]\nname = \"projection.value\"\nversion = \"1.0\"\n[[guest_contract.functions]]\nname = \"roundtrip\"\nparams = [{ name = \"value\", type = \"Value\" }]\nreturn = \"Value\"\n",
    );
    fs::write(&api, api_text).or_panic("extend API");
    let output = generate_internal_rust(InternalRustGenerateConfig {
        bundle_toml: bundle,
        layout: OutputLayout::unified(),
    })
    .or_panic("generate tagged projection");
    let domain = output
        .files
        .iter()
        .find(|file| file.path.ends_with("guest/domain.rs"))
        .or_panic("domain output")
        .content
        .as_str();
    let interfaces = output
        .files
        .iter()
        .find(|file| file.path.ends_with("guest/interfaces.rs"))
        .or_panic("adapter output")
        .content
        .as_str();
    assert!(
        domain.contains("#[allow(dead_code)]")
            && domain.contains("#[allow(non_camel_case_types)]\n    #[default]\n    Empty,")
            && domain.contains("#[default]\n    None,")
            && domain.contains("Deserialize, Default)"),
        "domain enums must preserve authored attributes and defaults: {domain}"
    );
    assert!(
        domain.contains("serializer.serialize_u32(")
            && domain.contains("<u32 as serde::Deserialize>::deserialize")
            && domain.contains("const __POLYPLUG_DISCRIMINANT_0: u32 = 17;")
            && !domain.contains("super::types::"),
        "u32 dual serde must use local ABI discriminants: {domain}"
    );
    assert!(
        domain.contains("serializer.serialize_u8(")
            && domain.contains("<u8 as serde::Deserialize>::deserialize")
            && domain.contains("const __POLYPLUG_DISCRIMINANT_0: u8 = 7;")
            && domain.contains("serializer.serialize_u16(")
            && domain.contains("<u16 as serde::Deserialize>::deserialize")
            && domain.contains("const __POLYPLUG_DISCRIMINANT_0: u16 = 513;")
            && domain.contains("serializer.serialize_u64(")
            && domain.contains("<u64 as serde::Deserialize>::deserialize")
            && domain.contains("const __POLYPLUG_DISCRIMINANT_0: u64 = 4294967297;"),
        "all non-u32 dual-serde representations must preserve their authored ABI widths: {domain}"
    );
    assert!(
        !domain.contains("derive(Debug, Clone, PartialEq, Eq")
            && !domain.contains("derive(Debug, Clone, PartialEq, Hash"),
        "f64 tagged projection must not acquire invalid Eq or Hash derives: {domain}"
    );
    assert!(
        domain.contains("<Option<Vec<T>> as serde::Deserialize>::deserialize")
            && domain.contains("value: &[T]"),
        "empty sequence serde helpers must use import-free UFCS and slice inputs: {domain}"
    );
    assert!(
        !interfaces.contains("core::mem::zeroed()")
            && interfaces.contains("kind: super::types::Kind::Boolean")
            && interfaces.contains("int_value: 0"),
        "tagged ABI values must initialize every field without invalid enum states: {interfaces}"
    );
    assert!(interfaces.contains("super::types::Kind::Boolean => super::domain::Value::Bool"));
}

#[test]
fn internal_fingerprint_includes_api_root_language_rules() {
    let temp = tempfile::tempdir().or_panic("temporary fingerprint API");
    let api = temp.path().join("api.toml");
    let bundle = temp.path().join("bundle.toml");
    let api_source = "[[guest_contract]]\nname = \"projection.value\"\nversion = \"1.0\"\n";
    fs::write(&api, api_source).or_panic("write API");
    fs::write(
        &bundle,
        "[bundle]\nname = \"root_rules_fingerprint\"\nversion = \"1.0\"\napi = \"api.toml\"\n\n[[plugin]]\nname = \"provider\"\nimplements = [\"projection.value@1.0\"]\n",
    )
    .or_panic("write bundle");
    let fingerprint = |output: &polyplug_codegen::GenerateOutput| {
        output
            .files
            .iter()
            .find(|file| file.path.ends_with("guest/domain.rs"))
            .and_then(|file| {
                file.content.lines().find(|line| {
                    line.starts_with("pub const _POLYPLUG_INTERNAL_GENERATION_FINGERPRINT:")
                })
            })
            .map(str::to_owned)
            .or_panic("domain fingerprint")
    };
    let original = fingerprint(
        &generate_internal_rust(InternalRustGenerateConfig {
            bundle_toml: bundle.clone(),
            layout: OutputLayout::unified(),
        })
        .or_panic("generate original profile"),
    );
    fs::write(
        &api,
        format!("{api_source}\n[langs.rust]\nattributes = [\"allow(dead_code)\"]\n"),
    )
    .or_panic("write root Rust language rule");
    let changed = fingerprint(
        &generate_internal_rust(InternalRustGenerateConfig {
            bundle_toml: bundle,
            layout: OutputLayout::unified(),
        })
        .or_panic("generate root-rule profile"),
    );
    assert_ne!(
        original, changed,
        "API-root language rules must invalidate internal generated bindings"
    );
}

#[test]
fn explicit_empty_rules_are_byte_identical_to_absent_node_rules() {
    let temp = tempfile::tempdir().or_panic("temporary normalization API");
    let api = temp.path().join("api.toml");
    let bundle = temp.path().join("bundle.toml");
    let base_api = "[[types]]\nname = \"Payload\"\nfields = [{ name = \"flag\", type = \"bool\" }]\n\n[[guest_contract]]\nname = \"projection.value\"\nversion = \"1.0\"\n";
    fs::write(&api, base_api).or_panic("write baseline API");
    fs::write(
        &bundle,
        "[bundle]\nname = \"empty_rules_identity\"\nversion = \"1.0\"\napi = \"api.toml\"\n\n[[plugin]]\nname = \"provider\"\nimplements = [\"projection.value@1.0\"]\n",
    )
    .or_panic("write bundle");
    let output_bytes = |output: polyplug_codegen::GenerateOutput| {
        let mut files = output
            .files
            .into_iter()
            .map(|file| (file.path, file.content))
            .collect::<Vec<_>>();
        files.sort_by(|left, right| left.0.cmp(&right.0));
        files
    };
    let absent = output_bytes(
        generate_internal_rust(InternalRustGenerateConfig {
            bundle_toml: bundle.clone(),
            layout: OutputLayout::unified(),
        })
        .or_panic("generate absent rules"),
    );
    fs::write(
        &api,
        r#"[langs.rust]
attributes = []

[[types]]
name = "Payload"
fields = [{ name = "flag", type = "bool" }]
langs = { rust = {} }

[[guest_contract]]
name = "projection.value"
version = "1.0"
"#,
    )
    .or_panic("write explicit empty rules");
    let explicit = output_bytes(
        generate_internal_rust(InternalRustGenerateConfig {
            bundle_toml: bundle,
            layout: OutputLayout::unified(),
        })
        .or_panic("generate explicit empty rules"),
    );
    assert_eq!(
        absent, explicit,
        "empty language attributes and all-default Rust rules must not perturb internal output"
    );
}

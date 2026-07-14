#[path = "support/failure.rs"]
mod failure;
#[path = "support/success.rs"]
mod success;

use failure::PanicOnFailure;
use success::PanicOnSuccess;

use polyplug_codegen::Lang;
use polyplug_codegen::PolyplugcError;
use polyplug_codegen::parser::parse_api_str;

#[test]
fn lowers_all_languages_at_all_authored_node_locations() {
    let api: &str = r#"
[langs.rust]
attributes = ["derive(Clone)"]
[langs.cpp]
attributes = ["nodiscard"]
[langs.csharp]
attributes = ["Serializable"]
[langs.python]
attributes = ["dataclass"]
[langs.lua]
attributes = ["metatable"]
[langs.javascript]
attributes = ["sealed"]

[[types]]
name = "Packet"
langs = { cpp = { attributes = ["alignas(8)"] } }
[[types.fields]]
name = "code"
type = "u32"
langs = { csharp = { attributes = ["JsonPropertyName(\"code\")"] } }

[[enum]]
name = "Mode"
repr = "u32"
langs = { python = { attributes = ["enum.unique"] } }
[[enum.variants]]
name = "Fast"
value = "1"
langs = { lua = { attributes = ["fast"] } }

[[guest_contract]]
name = "pipeline.Decoder"
version = "1.0.0"
langs = { javascript = { attributes = ["public"] } }
[[guest_contract.functions]]
name = "decode"
langs = { rust = { attributes = ["inline"] } }
[guest_contract.functions.return]
type = "u32"
[guest_contract.functions.return.langs.csharp]
attributes = ["return:MarshalAs(UnmanagedType.U4)"]
[[guest_contract.functions.params]]
name = "input"
type = "StringView"
langs = { cpp = { attributes = ["const"] } }

[[host_contract]]
name = "host.logger"
version = "1.0.0"
langs = { python = { attributes = ["protocol"] } }
[[host_contract.functions]]
name = "level"
langs = { lua = { attributes = ["method"] } }
[host_contract.functions.return]
type = "u32"
[host_contract.functions.return.langs.rust]
attributes = ["must_use"]
[[host_contract.functions.params]]
name = "message"
type = "StringView"
langs = { javascript = { attributes = ["readonly"] } }
"#;

    let ir = parse_api_str(api).or_panic("all language rules parse");
    assert_eq!(
        ir.langs
            .for_lang(Lang::Rust)
            .or_panic("root rust rules")
            .attributes,
        ["derive(Clone)"]
    );
    assert_eq!(
        ir.langs
            .for_lang(Lang::JsQuickJs)
            .or_panic("root javascript rules")
            .attributes,
        ["sealed"]
    );
    assert_eq!(
        ir.types[0]
            .langs
            .for_lang(Lang::Cpp)
            .or_panic("type cpp rules")
            .attributes,
        ["alignas(8)"]
    );
    assert_eq!(
        ir.types[0].fields[0]
            .langs
            .for_lang(Lang::CSharp)
            .or_panic("field csharp rules")
            .attributes,
        ["JsonPropertyName(\"code\")"]
    );
    assert_eq!(
        ir.enums[0]
            .langs
            .for_lang(Lang::Python)
            .or_panic("enum python rules")
            .attributes,
        ["enum.unique"]
    );
    assert_eq!(
        ir.enums[0].variants[0]
            .langs
            .for_lang(Lang::Lua)
            .or_panic("variant lua rules")
            .attributes,
        ["fast"]
    );

    let guest = &ir.contracts[0];
    assert_eq!(
        guest
            .langs
            .for_lang(Lang::JsQuickJs)
            .or_panic("guest contract javascript rules")
            .attributes,
        ["public"]
    );
    assert_eq!(
        guest.functions[0]
            .langs
            .for_lang(Lang::Rust)
            .or_panic("guest function rust rules")
            .attributes,
        ["inline"]
    );
    assert_eq!(
        guest.functions[0].params[0]
            .langs
            .for_lang(Lang::Cpp)
            .or_panic("guest parameter cpp rules")
            .attributes,
        ["const"]
    );
    assert_eq!(
        guest.functions[0]
            .return_langs
            .for_lang(Lang::CSharp)
            .or_panic("guest return csharp rules")
            .attributes,
        ["return:MarshalAs(UnmanagedType.U4)"]
    );

    let host = &ir.host_contracts[0];
    assert_eq!(
        host.langs
            .for_lang(Lang::Python)
            .or_panic("host contract python rules")
            .attributes,
        ["protocol"]
    );
    assert_eq!(
        host.functions[0]
            .langs
            .for_lang(Lang::Lua)
            .or_panic("host function lua rules")
            .attributes,
        ["method"]
    );
    assert_eq!(
        host.functions[0].params[0]
            .langs
            .for_lang(Lang::JsQuickJs)
            .or_panic("host parameter javascript rules")
            .attributes,
        ["readonly"]
    );
    assert_eq!(
        host.functions[0]
            .return_langs
            .for_lang(Lang::Rust)
            .or_panic("host return rust rules")
            .attributes,
        ["must_use"]
    );
}

#[test]
fn allows_optional_subsets_and_unchanged_apis() {
    let subset = parse_api_str(
        r#"
[[types]]
name = "Packet"
[types.langs.rust]
attributes = ["repr(C)"]
"#,
    )
    .or_panic("optional subset parses");
    assert!(subset.langs.for_lang(Lang::Rust).is_none());
    assert!(subset.types[0].langs.for_lang(Lang::Cpp).is_none());
    assert_eq!(
        subset.types[0]
            .langs
            .for_lang(Lang::Rust)
            .or_panic("rust subset")
            .attributes,
        ["repr(C)"]
    );

    let unchanged = parse_api_str(
        r#"
[[guest_contract]]
name = "pipeline.Decoder"
version = "1.0.0"
[[guest_contract.functions]]
name = "decode"
return = "u32"
"#,
    )
    .or_panic("existing API without langs parses");
    assert!(unchanged.langs.for_lang(Lang::Rust).is_none());
    assert!(unchanged.contracts[0].langs.for_lang(Lang::Cpp).is_none());
    assert!(
        unchanged.contracts[0].functions[0]
            .return_langs
            .for_lang(Lang::Lua)
            .is_none()
    );
}

#[test]
fn rejects_unknown_and_misplaced_language_rules_with_locations() {
    let unknown = parse_api_str("[langs.go]\nattributes = [\"tag\"]\n")
        .err_or_panic("unknown language must fail");
    match unknown {
        PolyplugcError::TomlParseError {
            message,
            location: Some(location),
        } => {
            assert!(message.contains("go"), "unexpected diagnostic: {message}");
            assert_eq!(location.line, 1);
        }
        other => panic!("expected located unknown-language rejection, got {other:?}"),
    }

    let unknown_nested = parse_api_str("[langs.rust]\nattribute = [\"tag\"]\n")
        .err_or_panic("unknown nested language key must fail");
    match unknown_nested {
        PolyplugcError::TomlParseError {
            message,
            location: Some(location),
        } => {
            assert!(
                message.contains("attribute"),
                "unexpected diagnostic: {message}"
            );
            assert_eq!(location.line, 2);
        }
        other => panic!("expected located nested-key rejection, got {other:?}"),
    }

    let misplaced = parse_api_str(
        r#"
[[guest_contract]]
name = "pipeline.Decoder"
version = "1.0.0"

[[guest_contract.functions]]
name = "decode"
[guest_contract.functions.params.langs.rust]
attributes = ["inline"]
"#,
    )
    .err_or_panic("misplaced langs must fail");
    match misplaced {
        PolyplugcError::TomlParseError {
            location: Some(location),
            ..
        } => assert_eq!(location.line, 8),
        other => panic!("expected located invalid-placement rejection, got {other:?}"),
    }
}

#[test]
fn rejects_empty_and_multiline_attributes_with_locations() {
    for (attribute, reason) in [("\"   \"", "empty"), ("\"first\\nsecond\"", "single line")] {
        let api = format!("[langs.rust]\nattributes = [{attribute}]\n");
        let err = parse_api_str(&api).err_or_panic("invalid attribute contents must fail");
        match err {
            PolyplugcError::InvalidLanguageAttribute {
                language,
                node,
                reason: actual_reason,
                location,
                ..
            } => {
                assert_eq!(language, "rust");
                assert_eq!(node, "API root");
                assert!(
                    actual_reason.contains(reason),
                    "unexpected reason: {actual_reason}"
                );
                assert_eq!(location.line, 2);
            }
            other => panic!("expected invalid language attribute, got {other:?}"),
        }
    }
}

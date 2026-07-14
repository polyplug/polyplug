//! Documentation coverage for the public `api.toml` schema reference.
//!
//! Keep the canonical schema list and the reference headings together: adding a
//! schema node or removing a documented section must update this contract.

const API_TOML_REFERENCE: &str = include_str!("../../../docs/API_TOML.md");
const SUMMARY: &str = include_str!("../../../docs/SUMMARY.md");
const CODE_GENERATION: &str = include_str!("../../../docs/CODE_GENERATION.md");
const QUICK_START: &str = include_str!("../../../docs/QUICKSTART.md");
const RUST_GUIDE: &str = include_str!("../../../docs/languages/rust.md");
const CPP_GUIDE: &str = include_str!("../../../docs/languages/cpp.md");
const CSHARP_GUIDE: &str = include_str!("../../../docs/languages/csharp.md");
const PYTHON_GUIDE: &str = include_str!("../../../docs/languages/python.md");
const LUA_GUIDE: &str = include_str!("../../../docs/languages/lua.md");
const JAVASCRIPT_GUIDE: &str = include_str!("../../../docs/languages/js.md");

struct SchemaSection {
    schema: &'static str,
    heading: &'static str,
    anchor: &'static str,
}

const CANONICAL_SCHEMA_SECTIONS: &[SchemaSection] = &[
    SchemaSection {
        schema: "[[types]]",
        heading: "Struct types",
        anchor: "struct-types",
    },
    SchemaSection {
        schema: "[[types.fields]]",
        heading: "Fields",
        anchor: "fields",
    },
    SchemaSection {
        schema: "[[enum]]",
        heading: "Enums and variants",
        anchor: "enums-and-variants",
    },
    SchemaSection {
        schema: "[[enum.variants]]",
        heading: "Enums and variants",
        anchor: "enums-and-variants",
    },
    SchemaSection {
        schema: "[[guest_contract]]",
        heading: "Guest contracts",
        anchor: "guest-contracts",
    },
    SchemaSection {
        schema: "[[host_contract]]",
        heading: "Host contracts",
        anchor: "host-contracts",
    },
    SchemaSection {
        schema: "[[guest_contract.functions]]",
        heading: "Functions, parameters, and returns",
        anchor: "functions-parameters-and-returns",
    },
    SchemaSection {
        schema: "[[host_contract.functions]]",
        heading: "Functions, parameters, and returns",
        anchor: "functions-parameters-and-returns",
    },
    SchemaSection {
        schema: "[[guest_contract.functions.params]]",
        heading: "Functions, parameters, and returns",
        anchor: "functions-parameters-and-returns",
    },
    SchemaSection {
        schema: "[[host_contract.functions.params]]",
        heading: "Functions, parameters, and returns",
        anchor: "functions-parameters-and-returns",
    },
    SchemaSection {
        schema: "[guest_contract.functions.return]",
        heading: "Functions, parameters, and returns",
        anchor: "functions-parameters-and-returns",
    },
    SchemaSection {
        schema: "[host_contract.functions.return]",
        heading: "Functions, parameters, and returns",
        anchor: "functions-parameters-and-returns",
    },
    SchemaSection {
        schema: "docs",
        heading: "Documentation",
        anchor: "documentation",
    },
    SchemaSection {
        schema: "version",
        heading: "Defaults and validation",
        anchor: "defaults-and-validation",
    },
    SchemaSection {
        schema: "singleton",
        heading: "Host contracts",
        anchor: "host-contracts",
    },
    SchemaSection {
        schema: "repr",
        heading: "Enums and variants",
        anchor: "enums-and-variants",
    },
    SchemaSection {
        schema: "bitflag",
        heading: "Enums and variants",
        anchor: "enums-and-variants",
    },
    SchemaSection {
        schema: "[langs.<language>]",
        heading: "Language customization",
        anchor: "language-customization",
    },
    SchemaSection {
        schema: "attributes",
        heading: "Language customization",
        anchor: "language-customization",
    },
    SchemaSection {
        schema: "[...functions.return.langs.<language>]",
        heading: "Attachment paths for every customizable node",
        anchor: "attachment-paths-for-every-customizable-node",
    },
    SchemaSection {
        schema: "[...langs.rust]",
        heading: "Rust semantic rules",
        anchor: "rust-semantic-rules",
    },
];

fn mdbook_anchor(heading: &str) -> String {
    let mut anchor = String::new();
    let mut needs_separator = false;

    for character in heading.chars() {
        if character.is_ascii_alphanumeric() {
            if needs_separator && !anchor.is_empty() {
                anchor.push('-');
            }
            anchor.push(character.to_ascii_lowercase());
            needs_separator = false;
        } else if !anchor.is_empty() {
            needs_separator = true;
        }
    }

    anchor
}

fn has_heading(markdown: &str, heading: &str) -> bool {
    markdown.lines().any(|line| {
        line.strip_prefix('#')
            .map(|line| line.trim_start_matches('#').trim_start())
            == Some(heading)
    })
}

#[test]
fn api_toml_reference_covers_every_canonical_schema_section() {
    for section in CANONICAL_SCHEMA_SECTIONS {
        assert!(
            API_TOML_REFERENCE.contains(section.schema),
            "API_TOML.md is missing canonical schema syntax `{}`",
            section.schema,
        );
        assert!(
            has_heading(API_TOML_REFERENCE, section.heading),
            "API_TOML.md is missing the `{}` section for `{}`",
            section.heading,
            section.schema,
        );
        assert_eq!(
            mdbook_anchor(section.heading),
            section.anchor,
            "the canonical `{}` section must retain its stable mdBook anchor",
            section.heading,
        );
    }

    for language in ["rust", "cpp", "csharp", "python", "lua", "javascript"] {
        assert!(
            API_TOML_REFERENCE.contains(language),
            "API_TOML.md is missing the `{language}` language-rule key",
        );
    }

    assert!(
        !API_TOML_REFERENCE.contains("Each language entry contains exactly one optional key"),
        "API_TOML.md must distinguish the shared `attributes` key from Rust semantic keys",
    );

    for wrapper in [
        "#![example(value)]",
        "// [[langprint::root(example(value))]]",
        "[assembly: example(value)]",
        "# @langprint Root: example(value)",
        "---@langprint Root: example(value)",
        "/** @langprint Root: example(value) */",
    ] {
        assert!(
            API_TOML_REFERENCE.contains(wrapper),
            "API_TOML.md is missing the exact LangPrint wrapper `{wrapper}`",
        );
    }
}

#[test]
fn api_toml_reference_covers_rust_semantic_rule_keys_and_representations() {
    for key in [
        "derives",
        "serde",
        "primary_name",
        "aliases",
        "default",
        "empty_sequence_as_null",
        "tagged_enum",
        "tag_field",
        "payload",
    ] {
        assert!(
            API_TOML_REFERENCE.contains(key),
            "API_TOML.md is missing Rust semantic rule key `{key}`",
        );
    }

    for heading in [
        "Enum names, defaults, and dual serde",
        "Tagged enum domain projection",
        "Empty sequence as null",
        "Rust semantic validation errors",
    ] {
        assert!(
            has_heading(API_TOML_REFERENCE, heading),
            "API_TOML.md is missing Rust semantic section `{heading}`",
        );
    }

    for representation in [
        "human-name-binary-discriminant",
        "Human-readable",
        "Non-human-readable",
        "ABI-flat",
        "DomainTypes",
    ] {
        assert!(
            API_TOML_REFERENCE.contains(representation),
            "API_TOML.md is missing Rust semantic representation `{representation}`",
        );
    }
}

#[test]
fn api_toml_reference_is_linked_from_authoring_guides() {
    for guide in [
        SUMMARY,
        CODE_GENERATION,
        QUICK_START,
        RUST_GUIDE,
        CPP_GUIDE,
        CSHARP_GUIDE,
        PYTHON_GUIDE,
        LUA_GUIDE,
        JAVASCRIPT_GUIDE,
    ] {
        assert!(
            guide.contains("API_TOML.md"),
            "an authoring guide must link to API_TOML.md",
        );
    }
}

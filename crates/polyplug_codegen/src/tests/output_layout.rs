use std::path::PathBuf;

use crate::{
    GeneratedFile, Lang, OutputDestination, OutputLayout, OutputPartition, ValidatedImport,
};

fn generated_file(partition: OutputPartition, references: Vec<OutputPartition>) -> GeneratedFile {
    GeneratedFile {
        path: PathBuf::from("generated.rs"),
        content: String::new(),
        force_regenerate: false,
        partition,
        references,
    }
}

#[test]
fn validated_imports_use_language_specific_grammars() {
    let valid_cases = [
        (Lang::Rust, "shared::domain"),
        (Lang::Cpp, "shared/domain-types.hpp"),
        (Lang::CSharp, "Shared.DomainTypes"),
        (Lang::Python, "shared.domain_types"),
        (Lang::Lua, "shared.domain_types"),
        (Lang::JsQuickJs, "@shared/domain-types"),
    ];

    for (language, value) in valid_cases {
        let import = ValidatedImport::parse(language, value)
            .unwrap_or_else(|error| panic!("{language:?} must accept `{value}`: {error}"));
        assert_eq!(import.language(), language);
        assert_eq!(import.as_str(), value);
    }

    for value in [
        "package/subpath",
        "./shared/domain.ts",
        "file:///tmp/shared/domain.ts",
        "file:///C:/shared/domain.ts",
    ] {
        assert!(
            ValidatedImport::parse(Lang::JsQuickJs, value).is_ok(),
            "JavaScript must accept `{value}`"
        );
    }
}

#[test]
fn malformed_imports_are_rejected_for_every_language() {
    let cases = [
        (
            Lang::Rust,
            &[
                "shared::not-valid",
                "shared::fn",
                "shared::domain;evil",
                "shared::domain path",
                "shared::\"domain\"",
            ][..],
        ),
        (
            Lang::Cpp,
            &[
                "guest/../domain.hpp",
                "guest//domain.hpp",
                "guest/domain path.hpp",
                "guest/\"domain.hpp\"",
                "guest/<domain.hpp>",
            ][..],
        ),
        (
            Lang::CSharp,
            &[
                "Shared..Domain",
                "Shared.class",
                "Shared/Domain",
                "Shared.Domain;System",
                "Shared.\"Domain\"",
            ][..],
        ),
        (
            Lang::Python,
            &[
                "shared..domain",
                "shared.import",
                ".shared",
                "shared/domain",
                "shared.domain;os",
            ][..],
        ),
        (
            Lang::Lua,
            &[
                "shared..domain",
                "shared/domain",
                "shared domain",
                "shared;domain",
                "shared.\"domain\"",
            ][..],
        ),
        (
            Lang::JsQuickJs,
            &[
                "@scope",
                "../domain.ts",
                "./../domain.ts",
                "package//subpath",
                "package;globalThis",
                "\"package\"",
                "package name",
                "https://example.test/domain.ts",
            ][..],
        ),
    ];

    for (language, values) in cases {
        for value in values {
            assert!(
                ValidatedImport::parse(language, *value).is_err(),
                "{language:?} must reject `{value}`"
            );
        }
    }
}

#[test]
fn import_only_sources_cannot_reference_inline_partitions() {
    let layout = OutputLayout {
        bindings: OutputDestination::Inline,
        domain_types: OutputDestination::ImportOnly {
            import: ValidatedImport::parse(Lang::Rust, "shared::domain")
                .unwrap_or_else(|error| panic!("valid domain import: {error}")),
        },
        guest_contracts: OutputDestination::Omit,
    };
    let files = [generated_file(
        OutputPartition::DomainTypes,
        vec![OutputPartition::Bindings],
    )];

    assert!(
        layout.validate(Lang::Rust, &files).is_err(),
        "an external source partition cannot resolve an inline dependency"
    );
}

#[test]
fn import_only_sources_accept_referenced_canonical_imports() {
    let layout = OutputLayout {
        bindings: OutputDestination::ImportOnly {
            import: ValidatedImport::parse(Lang::Rust, "shared::bindings")
                .unwrap_or_else(|error| panic!("valid bindings import: {error}")),
        },
        domain_types: OutputDestination::ImportOnly {
            import: ValidatedImport::parse(Lang::Rust, "shared::domain")
                .unwrap_or_else(|error| panic!("valid domain import: {error}")),
        },
        guest_contracts: OutputDestination::Omit,
    };
    let files = [generated_file(
        OutputPartition::DomainTypes,
        vec![OutputPartition::Bindings],
    )];

    assert!(
        layout.validate(Lang::Rust, &files).is_ok(),
        "canonical external dependency must validate"
    );
}

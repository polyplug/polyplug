//! Contract documentation parsing and code generation coverage.

#![allow(clippy::expect_used)]

use crate::Lang;
use crate::ResolvedBundleFile;
use crate::Side;
use crate::generate::generate_ir;
use crate::ir::ResolvedBundle;
use crate::ir::ResolvedPlugin;
use crate::ir::ValidatedIr;
use crate::ir::Version;
use crate::parser::parse_api_str;
use crate::{GenerateOutput, PolyplugcError};
use std::fs;
use std::process::Command;

use tempfile::tempdir;

const PLAIN_API: &str = r#"
[[types]]
name = "Record"
fields = [{ name = "value", type = "u32" }]

[[enum]]
name = "State"
repr = "u32"
variants = [{ name = "Ready", value = "1" }]

[[guest_contract]]
name = "sample.Docs"
version = "1.0"

[[guest_contract.functions]]
name = "query"
params = [{ name = "address", type = "u64" }]
return = "Record"

[[host_contract]]
name = "host.docs"
version = "1.0"

[[host_contract.functions]]
name = "report"
params = [{ name = "state", type = "State" }]
return = "void"
"#;

const DOCUMENTED_API: &str = r#"
[[types]]
name = "Record"
docs = '''Type documentation <&> used by callers.
Type documentation continues on a second line.'''
fields = [{ name = "value", type = "u32", docs = "Field documentation." }]

[[enum]]
name = "State"
repr = "u32"
docs = "Enum documentation."
variants = [{ name = "Ready", value = "1", docs = "Variant documentation." }]

[[guest_contract]]
name = "sample.Docs"
version = "1.0"
docs = '''Plugin contract documentation. */
Plugin contract documentation continues.'''

[[guest_contract.functions]]
name = "query"
docs = '''Plugin function documentation with """ and C:\docs.'''
params = [{ name = "address", type = "u64", docs = '''Plugin parameter documentation.
Plugin parameter documentation continues.'''}]
return = { type = "Record", docs = "Plugin return documentation." }

[[host_contract]]
name = "host.docs"
version = "1.0"
docs = "Host contract documentation."

[[host_contract.functions]]
name = "report"
docs = "Host function documentation."
params = [{ name = "state", type = "State", docs = "Host parameter documentation." }]
return = { type = "void", docs = "Host return documentation." }
"#;

fn attach_docs_bundle(ir: &mut ValidatedIr) {
    ir.bundle = Some(ResolvedBundle {
        name: "docs-bundle".to_owned(),
        version: Version {
            major: 1,
            minor: 0,
            patch: 0,
        },
        loader: "lua".to_owned(),
        file: ResolvedBundleFile::Single("docs.so".to_owned()),
        plugins: vec![ResolvedPlugin {
            name: "docs_plugin".to_owned(),
            implements: vec!["sample.Docs@1.0".to_owned()],
            optional: vec![],
        }],
        bundle_id: 0xD0C5,
        dependencies: vec![],
        needs_reinit_on_dep_reload: false,
    });
}

fn documented_ir() -> ValidatedIr {
    let mut ir: ValidatedIr = parse_api_str(DOCUMENTED_API).expect("parse documented API");
    attach_docs_bundle(&mut ir);
    ir
}

fn generated_file<'a>(output: &'a GenerateOutput, suffix: &str) -> &'a str {
    output
        .files
        .iter()
        .find(|file| file.path.to_string_lossy().ends_with(suffix))
        .map(|file| file.content.as_str())
        .unwrap_or_else(|| panic!("missing generated file ending in {suffix}"))
}

struct DocumentationPaths {
    lang: Lang,
    host_callers: &'static str,
    host_callers_stub: Option<&'static str>,
    host_contracts: &'static str,
    host_contracts_stub: Option<&'static str>,
    guest_contracts: &'static str,
    guest_contracts_stub: Option<&'static str>,
    guest_host_contracts: &'static str,
    guest_host_contracts_stub: Option<&'static str>,
    host_types: &'static str,
    guest_types: &'static str,
    escaped_type_docs: &'static str,
}

fn assert_documented_file(output: &GenerateOutput, suffix: &str, phrase: &str, scope: &str) {
    assert!(
        generated_file(output, suffix).contains(phrase),
        "{scope} must contain `{phrase}`"
    );
}

#[test]
fn documentation_reaches_validated_ir_and_normalizes_line_endings() {
    let api: String = DOCUMENTED_API.replace(
        "Plugin function documentation with \"\"\" and C:\\docs.",
        "Plugin function documentation.\r\nSecond line.\rThird line.",
    );
    let ir: ValidatedIr = parse_api_str(&api).expect("parse documented API");

    assert_eq!(
        ir.types[0].docs.as_deref(),
        Some(
            "Type documentation <&> used by callers.\nType documentation continues on a second line."
        )
    );
    assert_eq!(
        ir.types[0].fields[0].docs.as_deref(),
        Some("Field documentation.")
    );
    assert_eq!(ir.enums[0].docs.as_deref(), Some("Enum documentation."));
    assert_eq!(
        ir.enums[0].variants[0].docs.as_deref(),
        Some("Variant documentation.")
    );
    assert_eq!(
        ir.contracts[0].docs.as_deref(),
        Some("Plugin contract documentation. */\nPlugin contract documentation continues.")
    );
    assert_eq!(
        ir.contracts[0].functions[0].docs.as_deref(),
        Some("Plugin function documentation.\nSecond line.\nThird line.")
    );
    assert_eq!(
        ir.contracts[0].functions[0].params[0].docs.as_deref(),
        Some("Plugin parameter documentation.\nPlugin parameter documentation continues.")
    );
    assert_eq!(
        ir.contracts[0].functions[0].return_docs.as_deref(),
        Some("Plugin return documentation.")
    );
    assert_eq!(
        ir.host_contracts[0].docs.as_deref(),
        Some("Host contract documentation.")
    );
    assert_eq!(
        ir.host_contracts[0].functions[0].return_docs.as_deref(),
        Some("Host return documentation.")
    );
}

#[test]
fn both_return_syntaxes_preserve_type_and_documented_return() {
    let plain: ValidatedIr = parse_api_str(PLAIN_API).expect("parse legacy return string");
    let documented: ValidatedIr =
        parse_api_str(DOCUMENTED_API).expect("parse documented return table");

    assert!(plain.contracts[0].functions[0].return_docs.is_none());
    assert!(documented.contracts[0].functions[0].returns.is_some());
    assert_eq!(
        documented.contracts[0].functions[0].return_docs.as_deref(),
        Some("Plugin return documentation.")
    );
    assert!(documented.host_contracts[0].functions[0].returns.is_none());
    assert_eq!(
        documented.host_contracts[0].functions[0]
            .return_docs
            .as_deref(),
        Some("Host return documentation.")
    );
}

#[test]
fn documentation_controls_are_rejected_with_a_location() {
    let invalid: String = DOCUMENTED_API.replace("Field documentation.", "bad\\u0000docs");
    let err: PolyplugcError = parse_api_str(&invalid).expect_err("NUL docs must fail");
    assert!(matches!(
        err,
        PolyplugcError::InvalidDocumentation {
            character: '\0',
            location: Some(_)
        }
    ));

    let invalid: String = DOCUMENTED_API.replace("Field documentation.", "bad\\u0085docs");
    let err: PolyplugcError = parse_api_str(&invalid).expect_err("C1 docs must fail");
    assert!(matches!(
        err,
        PolyplugcError::InvalidDocumentation {
            character: '\u{0085}',
            location: Some(_)
        }
    ));
}

#[test]
fn docs_only_changes_leave_contract_identity_and_manifests_unchanged() {
    let plain: ValidatedIr = parse_api_str(PLAIN_API).expect("parse plain API");
    let documented: ValidatedIr = documented_ir();

    assert_eq!(
        plain.contracts[0].contract_id,
        documented.contracts[0].contract_id
    );
    assert_eq!(
        plain.host_contracts[0].contract_id,
        documented.host_contracts[0].contract_id
    );
    assert_eq!(
        plain.contracts[0].functions[0].function_id,
        documented.contracts[0].functions[0].function_id
    );
    assert_eq!(
        plain.host_contracts[0].functions[0].function_id,
        documented.host_contracts[0].functions[0].function_id
    );

    let mut plain_with_bundle: ValidatedIr = parse_api_str(PLAIN_API).expect("parse plain API");
    attach_docs_bundle(&mut plain_with_bundle);
    for lang in [
        Lang::Rust,
        Lang::Cpp,
        Lang::CSharp,
        Lang::Python,
        Lang::Lua,
        Lang::JsQuickJs,
    ] {
        let plain_output: GenerateOutput =
            generate_ir(&plain_with_bundle, lang, Side::Guest).expect("generate plain guest");
        let docs_output: GenerateOutput =
            generate_ir(&documented, lang, Side::Guest).expect("generate documented guest");
        let plain_manifest: Option<&str> = plain_output
            .files
            .iter()
            .find(|file| file.path.to_string_lossy() == "manifest.toml")
            .map(|file| file.content.as_str());
        let docs_manifest: Option<&str> = docs_output
            .files
            .iter()
            .find(|file| file.path.to_string_lossy() == "manifest.toml")
            .map(|file| file.content.as_str());
        assert_eq!(
            plain_manifest,
            docs_manifest,
            "manifest changed for {}",
            lang.as_str()
        );
    }
}

#[test]
fn no_docs_output_remains_on_the_existing_surface() {
    let plain: ValidatedIr = parse_api_str(PLAIN_API).expect("parse plain API");
    let rust: GenerateOutput =
        generate_ir(&plain, Lang::Rust, Side::Guest).expect("generate plain Rust");
    let contracts: &str = generated_file(&rust, "guest/contracts.rs");
    assert!(contracts.contains("Guest trait for contract `sample.Docs`"));
    assert!(!contracts.contains("Plugin function documentation."));

    let cpp: GenerateOutput =
        generate_ir(&plain, Lang::Cpp, Side::Guest).expect("generate plain C++");
    assert!(
        !generated_file(&cpp, "guest/contracts.hpp").contains("Plugin function documentation.")
    );
}

#[test]
fn every_language_emits_documentation_on_host_and_guest_surfaces() {
    let ir: ValidatedIr = documented_ir();
    let languages: [DocumentationPaths; 6] = [
        DocumentationPaths {
            lang: Lang::Rust,
            host_callers: "host/host_callers.rs",
            host_callers_stub: None,
            host_contracts: "host/host_contracts.rs",
            host_contracts_stub: None,
            guest_contracts: "guest/contracts.rs",
            guest_contracts_stub: None,
            guest_host_contracts: "guest/host_contract_callers.rs",
            guest_host_contracts_stub: None,
            host_types: "host/types.rs",
            guest_types: "guest/types.rs",
            escaped_type_docs: "Type documentation <&> used by callers.",
        },
        DocumentationPaths {
            lang: Lang::Cpp,
            host_callers: "host/host_callers.hpp",
            host_callers_stub: None,
            host_contracts: "host/host_contracts.hpp",
            host_contracts_stub: None,
            guest_contracts: "guest/contracts.hpp",
            guest_contracts_stub: None,
            guest_host_contracts: "guest/host_contracts.hpp",
            guest_host_contracts_stub: None,
            host_types: "host/types.hpp",
            guest_types: "guest/types.hpp",
            escaped_type_docs: "Type documentation <&> used by callers.",
        },
        DocumentationPaths {
            lang: Lang::CSharp,
            host_callers: "host/Callers.cs",
            host_callers_stub: None,
            host_contracts: "host/Contracts.cs",
            host_contracts_stub: None,
            guest_contracts: "guest/Contracts.cs",
            guest_contracts_stub: None,
            guest_host_contracts: "guest/HostContracts.cs",
            guest_host_contracts_stub: None,
            host_types: "host/Types.cs",
            guest_types: "guest/Types.cs",
            escaped_type_docs: "Type documentation &lt;&amp;&gt; used by callers.",
        },
        DocumentationPaths {
            lang: Lang::Python,
            host_callers: "host/callers.py",
            host_callers_stub: Some("host/callers.pyi"),
            host_contracts: "host/contracts.py",
            host_contracts_stub: Some("host/contracts.pyi"),
            guest_contracts: "guest/contracts.py",
            guest_contracts_stub: Some("guest/contracts.pyi"),
            guest_host_contracts: "guest/host_contracts.py",
            guest_host_contracts_stub: Some("guest/host_contracts.pyi"),
            host_types: "host/types.py",
            guest_types: "guest/types.py",
            escaped_type_docs: "Type documentation <&> used by callers.",
        },
        DocumentationPaths {
            lang: Lang::Lua,
            host_callers: "host/callers.lua",
            host_callers_stub: None,
            host_contracts: "host/contracts.lua",
            host_contracts_stub: None,
            guest_contracts: "guest/contracts.lua",
            guest_contracts_stub: None,
            guest_host_contracts: "guest/host_contracts.lua",
            guest_host_contracts_stub: None,
            host_types: "host/types.lua",
            guest_types: "guest/types.lua",
            escaped_type_docs: "Type documentation <&> used by callers.",
        },
        DocumentationPaths {
            lang: Lang::JsQuickJs,
            host_callers: "host/callers.ts",
            host_callers_stub: None,
            host_contracts: "host/contracts.ts",
            host_contracts_stub: None,
            guest_contracts: "guest/contracts.ts",
            guest_contracts_stub: None,
            guest_host_contracts: "guest/host_contracts.ts",
            guest_host_contracts_stub: None,
            host_types: "host/types.ts",
            guest_types: "guest/types.ts",
            escaped_type_docs: "Type documentation <&> used by callers.",
        },
    ];

    for paths in &languages {
        let host: GenerateOutput =
            generate_ir(&ir, paths.lang, Side::Host).expect("generate documented host");
        for phrase in [
            "Plugin contract documentation.",
            "Plugin function documentation",
            "Plugin parameter documentation.",
            "Plugin return documentation.",
        ] {
            assert_documented_file(&host, paths.host_callers, phrase, "host caller");
        }
        if let Some(stub) = paths.host_callers_stub {
            for phrase in [
                "Plugin contract documentation.",
                "Plugin function documentation",
                "Plugin parameter documentation.",
                "Plugin return documentation.",
            ] {
                assert_documented_file(&host, stub, phrase, "host caller stub");
            }
        }
        for phrase in [
            "Host contract documentation.",
            "Host function documentation.",
            "Host parameter documentation.",
            "Host return documentation.",
        ] {
            assert_documented_file(
                &host,
                paths.host_contracts,
                phrase,
                "host contract interface",
            );
        }
        if let Some(stub) = paths.host_contracts_stub {
            for phrase in [
                "Host contract documentation.",
                "Host function documentation.",
                "Host parameter documentation.",
                "Host return documentation.",
            ] {
                assert_documented_file(&host, stub, phrase, "host contract interface stub");
            }
        }
        for phrase in [
            paths.escaped_type_docs,
            "Field documentation.",
            "Enum documentation.",
            "Variant documentation.",
        ] {
            assert_documented_file(&host, paths.host_types, phrase, "host type output");
        }

        let guest: GenerateOutput =
            generate_ir(&ir, paths.lang, Side::Guest).expect("generate documented guest");
        for phrase in [
            "Plugin contract documentation.",
            "Plugin function documentation",
            "Plugin parameter documentation.",
            "Plugin return documentation.",
        ] {
            assert_documented_file(
                &guest,
                paths.guest_contracts,
                phrase,
                "guest plugin interface",
            );
        }
        if let Some(stub) = paths.guest_contracts_stub {
            for phrase in [
                "Plugin contract documentation.",
                "Plugin function documentation",
                "Plugin parameter documentation.",
                "Plugin return documentation.",
            ] {
                assert_documented_file(&guest, stub, phrase, "guest plugin stub");
            }
        }
        for phrase in [
            "Host contract documentation.",
            "Host function documentation.",
            "Host parameter documentation.",
            "Host return documentation.",
        ] {
            assert_documented_file(
                &guest,
                paths.guest_host_contracts,
                phrase,
                "guest host-contract caller",
            );
        }
        if let Some(stub) = paths.guest_host_contracts_stub {
            for phrase in [
                "Host contract documentation.",
                "Host function documentation.",
                "Host parameter documentation.",
                "Host return documentation.",
            ] {
                assert_documented_file(&guest, stub, phrase, "guest host-contract caller stub");
            }
        }
        for phrase in [
            paths.escaped_type_docs,
            "Field documentation.",
            "Enum documentation.",
            "Variant documentation.",
        ] {
            assert_documented_file(&guest, paths.guest_types, phrase, "guest type output");
        }
    }

    let cpp: GenerateOutput = generate_ir(&ir, Lang::Cpp, Side::Guest).expect("generate C++");
    assert_documented_file(
        &cpp,
        "guest/contracts.hpp",
        "Plugin contract documentation. */",
        "C++ line documentation",
    );

    let csharp: GenerateOutput = generate_ir(&ir, Lang::CSharp, Side::Guest).expect("generate C#");
    assert_documented_file(
        &csharp,
        "guest/Types.cs",
        "&lt;&amp;&gt;",
        "C# XML documentation",
    );

    let python: GenerateOutput =
        generate_ir(&ir, Lang::Python, Side::Guest).expect("generate Python");
    assert_documented_file(
        &python,
        "guest/contracts.py",
        "\\\"\"\"",
        "Python triple-quote escaping",
    );
    assert_documented_file(
        &python,
        "guest/contracts.py",
        "C:\\\\docs.",
        "Python backslash escaping",
    );

    let lua: GenerateOutput = generate_ir(&ir, Lang::Lua, Side::Guest).expect("generate Lua");
    assert_documented_file(
        &lua,
        "guest/contracts.lua",
        "--- Plugin parameter documentation.\n--- Plugin parameter documentation continues.",
        "Lua multiline annotations",
    );

    let js: GenerateOutput =
        generate_ir(&ir, Lang::JsQuickJs, Side::Guest).expect("generate QuickJS");
    assert_documented_file(
        &js,
        "guest/contracts.ts",
        "*\\/",
        "TypeScript JSDoc terminator escaping",
    );
}

#[test]
fn documented_python_outputs_compile() {
    let ir: ValidatedIr = documented_ir();
    let tmp = tempdir().expect("create temporary output directory");

    for side in [Side::Host, Side::Guest] {
        let output: GenerateOutput =
            generate_ir(&ir, Lang::Python, side).expect("generate documented Python");
        for file in output
            .files
            .iter()
            .filter(|file| file.path.extension().is_some_and(|ext| ext == "py"))
        {
            let path = tmp.path().join(&file.path);
            fs::create_dir_all(path.parent().expect("generated file has a parent"))
                .expect("create generated directory");
            fs::write(&path, &file.content).expect("write generated Python");
            let result = Command::new("python3")
                .args(["-m", "py_compile"])
                .arg(&path)
                .output()
                .expect("run python3 -m py_compile");
            assert!(
                result.status.success(),
                "documented Python output did not compile: {}\n{}",
                path.display(),
                String::from_utf8_lossy(&result.stderr)
            );
        }
    }
}

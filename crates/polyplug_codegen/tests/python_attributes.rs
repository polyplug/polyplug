#[path = "support/failure.rs"]
mod failure;

use failure::PanicOnFailure;

use std::fs;
use std::path::Path;
use std::process::Command;

use polyplug_codegen::{
    GenerateConfig, Lang, OutputDestination, OutputLayout, Side, ValidatedImport, generate,
};
use tempfile::tempdir;

fn generated<'a>(output: &'a polyplug_codegen::GenerateOutput, path: &str) -> &'a str {
    output
        .files
        .iter()
        .find(|file| file.path == Path::new(path))
        .unwrap_or_else(|| panic!("missing generated {path}"))
        .content
        .as_str()
}

const SENTINEL_API: &str = r#"
[langs.python]
attributes = ["root_first", "root_second"]

[[types]]
name = "Packet"
langs = { python = { attributes = ["staticmethod"] } }
[[types.fields]]
name = "code"
type = "u32"
langs = { python = { attributes = ["field_marker"] } }

[[enum]]
name = "Mode"
repr = "u32"
langs = { python = { attributes = ["staticmethod"] } }
[[enum.variants]]
name = "Fast"
value = "1"
langs = { python = { attributes = ["variant_marker"] } }

[[guest_contract]]
name = "sentinel.Guest"
version = "1.0.0"
langs = { python = { attributes = ["staticmethod"] } }
[[guest_contract.functions]]
name = "invoke"
langs = { python = { attributes = ["staticmethod"] } }
[guest_contract.functions.return]
type = "u32"
langs = { python = { attributes = ["guest_return"] } }
[[guest_contract.functions.params]]
name = "value"
type = "u32"
langs = { python = { attributes = ["guest_param_first", "guest_param_second"] } }

[[host_contract]]
name = "host.Sentinel"
version = "1.0.0"
langs = { python = { attributes = ["staticmethod"] } }
[[host_contract.functions]]
name = "report"
langs = { python = { attributes = ["staticmethod"] } }
[host_contract.functions.return]
type = "u32"
langs = { python = { attributes = ["host_return"] } }
[[host_contract.functions.params]]
name = "level"
type = "u32"
langs = { python = { attributes = ["host_param_first", "host_param_second"] } }
"#;

fn assert_in_order(text: &str, expected: &[&str]) {
    let mut offset = 0;
    for item in expected {
        let found = text[offset..]
            .find(item)
            .unwrap_or_else(|| panic!("missing `{item}` in:\n{text}"));
        offset += found + item.len();
    }
}

fn assert_root_once(text: &str) {
    assert_eq!(text.matches("# @langprint Root: root_first").count(), 1);
    assert_eq!(text.matches("# @langprint Root: root_second").count(), 1);
    assert_in_order(
        text,
        &[
            "# @langprint Root: root_first",
            "# @langprint Root: root_second",
        ],
    );
}

fn assert_guest_surface(text: &str) {
    assert_in_order(
        text,
        &[
            "@staticmethod\nclass SentinelGuest",
            "# @langprint Parameter: guest_param_first",
            "# @langprint Parameter: guest_param_second",
            "# @langprint Return: guest_return",
            "@staticmethod\n    def invoke",
        ],
    );
}

fn assert_host_surface(text: &str) {
    assert_in_order(
        text,
        &[
            "@staticmethod\nclass HostSentinel",
            "# @langprint Parameter: host_param_first",
            "# @langprint Parameter: host_param_second",
            "# @langprint Return: host_return",
            "@staticmethod",
        ],
    );
}

fn write_output(output: &polyplug_codegen::GenerateOutput, root: &Path) {
    for file in &output.files {
        let path = root.join(&file.path);
        fs::create_dir_all(path.parent().or_panic("generated file parent"))
            .or_panic("create generated file parent");
        fs::write(path, &file.content).or_panic("write generated file");
    }
}

#[test]
fn python_attributes_cover_every_public_semantic_surface_in_unified_and_split_output() {
    let temp = tempdir().or_panic("temporary api directory");
    let api = temp.path().join("sentinel-api.toml");
    fs::write(&api, SENTINEL_API).or_panic("write sentinel API");

    let host = generate(GenerateConfig {
        api_toml: api.clone(),
        lang: Lang::Python,
        side: Side::Host,
        layout: OutputLayout::unified(),
    })
    .or_panic("generate unified Python host");
    let guest = generate(GenerateConfig {
        api_toml: api.clone(),
        lang: Lang::Python,
        side: Side::Guest,
        layout: OutputLayout::unified(),
    })
    .or_panic("generate unified Python guest");

    for text in [
        generated(&host, "host/types.py"),
        generated(&host, "host/types.pyi"),
        generated(&host, "host/callers.py"),
        generated(&host, "host/callers.pyi"),
        generated(&host, "host/contracts.py"),
        generated(&host, "host/contracts.pyi"),
        generated(&guest, "guest/types.py"),
        generated(&guest, "guest/types.pyi"),
        generated(&guest, "guest/contracts.py"),
        generated(&guest, "guest/contracts.pyi"),
        generated(&guest, "guest/host_contracts.py"),
        generated(&guest, "guest/host_contracts.pyi"),
    ] {
        assert_root_once(text);
    }

    for types in [
        generated(&host, "host/types.py"),
        generated(&guest, "guest/types.py"),
    ] {
        assert_in_order(
            types,
            &[
                "@staticmethod\nclass Mode",
                "# @langprint Variant: variant_marker\n    FAST = 1",
                "@staticmethod\nclass Packet",
                "# @langprint Field: field_marker\n        (\"code\", ctypes.c_uint32)",
            ],
        );
    }
    for types_stub in [
        generated(&host, "host/types.pyi"),
        generated(&guest, "guest/types.pyi"),
    ] {
        assert!(types_stub.contains("# @langprint Field: field_marker\n    code: ctypes.c_uint32"));
    }

    assert_guest_surface(generated(&guest, "guest/contracts.py"));
    assert_guest_surface(generated(&guest, "guest/contracts.pyi"));
    assert_guest_surface(generated(&host, "host/callers.py"));
    assert_guest_surface(generated(&host, "host/callers.pyi"));
    assert_host_surface(generated(&host, "host/contracts.py"));
    assert_host_surface(generated(&host, "host/contracts.pyi"));
    assert_host_surface(generated(&guest, "guest/host_contracts.py"));
    assert_host_surface(generated(&guest, "guest/host_contracts.pyi"));

    let split = generate(GenerateConfig {
        api_toml: api.clone(),
        lang: Lang::Python,
        side: Side::Guest,
        layout: OutputLayout {
            bindings: OutputDestination::Inline,
            domain_types: OutputDestination::Emit {
                root: temp.path().join("domain"),
                import: ValidatedImport::parse(Lang::Python, "sentinel_domain")
                    .or_panic("valid Python domain import"),
            },
            guest_contracts: OutputDestination::Emit {
                root: temp.path().join("contracts"),
                import: ValidatedImport::parse(Lang::Python, "sentinel_contracts")
                    .or_panic("valid Python contract import"),
            },
        },
    })
    .or_panic("generate split Python guest");
    for path in [
        "domain.py",
        "domain.pyi",
        "guest_contracts.py",
        "guest_contracts.pyi",
        "guest/types.py",
        "guest/types.pyi",
        "guest/contracts.py",
        "guest/contracts.pyi",
        "guest/host_contracts.py",
        "guest/host_contracts.pyi",
    ] {
        assert_root_once(generated(&split, path));
    }
    assert_guest_surface(generated(&split, "guest_contracts.py"));
    assert_guest_surface(generated(&split, "guest_contracts.pyi"));
    assert_in_order(
        generated(&split, "domain.py"),
        &[
            "@staticmethod\nclass Mode",
            "# @langprint Variant: variant_marker\n    FAST = 1",
            "@staticmethod\nclass Packet",
            "# @langprint Field: field_marker\n        (\"code\", ctypes.c_uint32)",
        ],
    );

    let output_root = temp.path().join("output");
    write_output(&host, &output_root);
    write_output(&guest, &output_root);
    let python_files: Vec<_> = host
        .files
        .iter()
        .chain(&guest.files)
        .filter(|file| {
            matches!(
                file.path.extension().and_then(|ext| ext.to_str()),
                Some("py") | Some("pyi")
            )
        })
        .map(|file| output_root.join(&file.path))
        .collect();
    let compile = Command::new("python3")
        .arg("-m")
        .arg("py_compile")
        .args(&python_files)
        .output()
        .or_panic("run python3 -m py_compile");
    assert!(
        compile.status.success(),
        "generated Python did not compile:\n{}",
        String::from_utf8_lossy(&compile.stderr)
    );

    write_output(&split, &output_root);
    fs::copy(
        output_root.join("domain.py"),
        output_root.join("sentinel_domain.py"),
    )
    .or_panic("install split domain import target");
    fs::copy(
        output_root.join("guest_contracts.py"),
        output_root.join("sentinel_contracts.py"),
    )
    .or_panic("install split contract import target");
    let split_python_files: Vec<_> = split
        .files
        .iter()
        .filter(|file| {
            matches!(
                file.path.extension().and_then(|ext| ext.to_str()),
                Some("py") | Some("pyi")
            )
        })
        .map(|file| output_root.join(&file.path))
        .collect();
    let split_compile = Command::new("python3")
        .arg("-m")
        .arg("py_compile")
        .args(&split_python_files)
        .output()
        .or_panic("run split python3 -m py_compile");
    assert!(
        split_compile.status.success(),
        "split generated Python did not compile:\n{}",
        String::from_utf8_lossy(&split_compile.stderr)
    );

    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .or_panic("workspace root");
    let imports = temp.path().join("import_generated.py");
    fs::write(
        &imports,
        r#"
import importlib
import sys
from pathlib import Path

output = Path(sys.argv[1])
workspace = Path(sys.argv[2])
sys.path[:0] = [
    str(output),
    str(workspace / "sdks/python"),
    str(workspace / "sdks/python/polyplug_abi"),
    str(workspace / "sdks/python/guest"),
]
for module in [
    "host.types",
    "host.callers",
    "host.contracts",
    "guest.types",
    "guest.contracts",
    "guest.host_contracts",
]:
    importlib.import_module(module)
"#,
    )
    .or_panic("write import harness");
    let import = Command::new("python3")
        .arg(imports)
        .arg(&output_root)
        .arg(workspace)
        .output()
        .or_panic("run generated Python import harness");
    assert!(
        import.status.success(),
        "generated Python did not import:\n{}",
        String::from_utf8_lossy(&import.stderr)
    );
}

#[test]
fn python_empty_language_rules_preserve_generated_bytes() {
    let temp = tempdir().or_panic("temporary api directory");
    let plain_api = temp.path().join("plain.toml");
    let empty_api = temp.path().join("empty.toml");
    let plain_source = SENTINEL_API
        .replace(
            "[langs.python]\nattributes = [\"root_first\", \"root_second\"]\n\n",
            "",
        )
        .replace("langs = { python = { attributes = [\"staticmethod\"] } }\n", "")
        .replace("langs = { python = { attributes = [\"field_marker\"] } }\n", "")
        .replace("langs = { python = { attributes = [\"variant_marker\"] } }\n", "")
        .replace(
            "langs = { python = { attributes = [\"guest_return\"] } }\n",
            "",
        )
        .replace(
            "langs = { python = { attributes = [\"guest_param_first\", \"guest_param_second\"] } }\n",
            "",
        )
        .replace(
            "langs = { python = { attributes = [\"host_return\"] } }\n",
            "",
        )
        .replace(
            "langs = { python = { attributes = [\"host_param_first\", \"host_param_second\"] } }\n",
            "",
        );
    fs::write(&plain_api, &plain_source).or_panic("write plain API");
    let empty_source = SENTINEL_API
        .replace(
            "attributes = [\"root_first\", \"root_second\"]",
            "attributes = []",
        )
        .replace("attributes = [\"staticmethod\"]", "attributes = []")
        .replace("attributes = [\"field_marker\"]", "attributes = []")
        .replace("attributes = [\"variant_marker\"]", "attributes = []")
        .replace("attributes = [\"guest_return\"]", "attributes = []")
        .replace(
            "attributes = [\"guest_param_first\", \"guest_param_second\"]",
            "attributes = []",
        )
        .replace("attributes = [\"host_return\"]", "attributes = []")
        .replace(
            "attributes = [\"host_param_first\", \"host_param_second\"]",
            "attributes = []",
        );
    fs::write(&empty_api, empty_source).or_panic("write empty-rules API");

    for side in [Side::Host, Side::Guest] {
        let plain = generate(GenerateConfig {
            api_toml: plain_api.clone(),
            lang: Lang::Python,
            side,
            layout: OutputLayout::unified(),
        })
        .or_panic("generate plain Python");
        let empty = generate(GenerateConfig {
            api_toml: empty_api.clone(),
            lang: Lang::Python,
            side,
            layout: OutputLayout::unified(),
        })
        .or_panic("generate empty-rule Python");
        assert_eq!(plain.files.len(), empty.files.len());
        for (plain_file, empty_file) in plain.files.iter().zip(&empty.files) {
            assert_eq!(plain_file.path, empty_file.path);
            assert_eq!(
                plain_file.content,
                empty_file.content,
                "empty rules changed {} output",
                plain_file.path.display()
            );
        }
    }
}

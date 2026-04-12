//! SDK generation module — integrates language generators from polyplug_codegen.
//!
//! This module provides functions to generate SDK bindings for all supported
//! languages (C++, C#, Python, Lua, JavaScript) from extracted ABI types.

#![allow(clippy::std_instead_of_core)]

use crate::mapper::map_all_abi_types;
use crate::types::AbiTypes;
use polyplug_codegen::data::Item;
use polyplug_codegen::languages::{
    CSharpGenerator, CodeGenerator, CppGenerator, GenerationContext, JsGenerator, LuaGenerator,
    PythonGenerator,
};
use std::path::{Path, PathBuf};

/// Target language for SDK generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetLang {
    /// C++ (C++17 headers).
    Cpp,
    /// C# (.NET bindings).
    CSharp,
    /// Python (ctypes bindings).
    Python,
    /// Lua (LuaJIT FFI bindings).
    Lua,
    /// JavaScript/TypeScript.
    JavaScript,
}

impl TargetLang {
    /// Return the language name for directory structure.
    pub const fn language_name(&self) -> &'static str {
        match self {
            TargetLang::Cpp => "cpp",
            TargetLang::CSharp => "csharp",
            TargetLang::Python => "python",
            TargetLang::Lua => "lua",
            TargetLang::JavaScript => "js",
        }
    }

    /// Return the output filename for the generated SDK.
    pub const fn output_filename(&self) -> &'static str {
        match self {
            TargetLang::Cpp => "abi.hpp",
            TargetLang::CSharp => "Abi.cs",
            TargetLang::Python => "abi.py",
            TargetLang::Lua => "abi.lua",
            TargetLang::JavaScript => "abi.ts",
        }
    }

    /// Return the subdirectory path for the generated SDK.
    pub const fn subdir(&self) -> &'static str {
        match self {
            TargetLang::Cpp => "polyplug",
            TargetLang::CSharp => "",
            TargetLang::Python => "",
            TargetLang::Lua => "",
            TargetLang::JavaScript => "",
        }
    }

    /// Return the file extension for the generated SDK.
    pub const fn file_extension(&self) -> &'static str {
        match self {
            TargetLang::Cpp => "hpp",
            TargetLang::CSharp => "cs",
            TargetLang::Python => "py",
            TargetLang::Lua => "lua",
            TargetLang::JavaScript => "ts",
        }
    }
}

/// Patterns in rust_type strings that indicate types which cannot be represented
/// in target languages. Simple generics (Array<T>, Option<...>) and tuples are allowed.
const UNREPRESENTABLE_PATTERNS: &[&str] = &["dyn ", "impl ", "for<", "where "];

/// Known struct sizes from Rust layout.
///
/// MAINTENANCE: Update this table when Rust struct layouts change.
/// See polyplug_abi layout tests (test_*_size) for canonical sizes.
/// Each size is verified by `static_assert`/`ctypes.sizeof` in generated SDK files,
/// so a stale table causes layout test failures, not silent corruption.
const KNOWN_SIZES: &[(&str, usize)] = &[
    ("StringView", 16),
    ("Buffer", 24),
    ("Version", 12),
    ("AbiError", 24),
    ("DependencyInfo", 24),
    ("DispatchMechanisms", 16),
    ("GuestContractInterface", 56),
    ("GuestContractInstance", 16),
    ("HostInterface", 144),
    ("HostContractInterface", 72),
    ("HostContractInstance", 8),
    ("RuntimeInterface", 96),
    ("GuestContractHandle", 4),
    ("PluginDescriptor", 48),
    ("BundleInitContext", 24),
    ("RuntimeConfig", 16),
    ("ReloadPhase", 48),
    ("NativeDispatch", 16),
    ("VmDispatch", 16),
    ("VmLoaderData", 8),
];

/// Populate `size_hint` fields on `AbiStruct` entries using the known size table.
fn populate_size_hints(abi_types: &mut AbiTypes) {
    for struct_info in &mut abi_types.structs {
        if struct_info.size_hint.is_none() {
            for (name, size) in KNOWN_SIZES {
                if struct_info.name == *name {
                    struct_info.size_hint = Some(*size);
                    break;
                }
            }
        }
    }
}

/// Validate that all field types can be represented in target languages.
///
/// Per D-09: Build fails with clear error if a type cannot be represented.
fn validate_representable_types(abi_types: &AbiTypes) -> Result<(), String> {
    for struct_info in &abi_types.structs {
        for field in &struct_info.fields {
            for pattern in UNREPRESENTABLE_PATTERNS {
                if field.rust_type.contains(pattern) {
                    return Err(format!(
                        "Cannot represent type '{}' field '{}' with type '{}' in target languages. \
                         Consider simplifying the type or adding codegen support.",
                        struct_info.name, field.name, field.rust_type
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Generate SDK for a specific language.
///
/// # Arguments
/// * `lang` - Target language.
/// * `abi_types` - Extracted ABI types.
///
/// # Returns
/// Generated SDK code as a string.
pub fn generate_language_sdk(lang: TargetLang, abi_types: &AbiTypes) -> String {
    let all_items: Vec<Item> = map_all_abi_types(&abi_types.types());

    let generator: Box<dyn CodeGenerator> = match lang {
        TargetLang::Cpp => Box::new(CppGenerator::new()),
        TargetLang::CSharp => Box::new(CSharpGenerator::new()),
        TargetLang::Python => Box::new(PythonGenerator::new()),
        TargetLang::Lua => Box::new(LuaGenerator::new()),
        TargetLang::JavaScript => Box::new(JsGenerator::new()),
    };

    let ctx: GenerationContext = GenerationContext::new();
    let mut output: String = String::new();

    output.push_str(&generator.generate_header(&ctx));

    for item in &all_items {
        let code: String = match item {
            Item::Const(c) => generator.generate_const(c, &ctx),
            Item::Struct(s) => generator.generate_struct(s, &ctx),
            Item::Enum(e) => generator.generate_enum(e, &ctx),
            Item::Union(u) => generator.generate_union(u, &ctx),
            // Function items are no longer generated from ABI extraction.
            // The Function variant remains in codegen for use by polyplugc CLI.
            Item::Function(_) => String::new(),
        };
        output.push_str(&code);
    }

    output.push_str(&generator.generate_footer(&ctx));
    output
}

/// Generate all SDKs and write to sdks/{lang}/abi/.
///
/// # Arguments
/// * `abi_types` - Extracted ABI types (will be mutated to populate size hints).
/// * `workspace_root` - Path to the workspace root directory.
/// * `tracked_files` - Source files to emit `cargo:rerun-if-changed` for.
///
/// # Returns
/// Result indicating success or failure.
pub fn generate_all_sdks(
    abi_types: &mut AbiTypes,
    workspace_root: &Path,
    tracked_files: &[PathBuf],
) -> Result<(), Box<dyn std::error::Error>> {
    // Populate size hints from known size table.
    populate_size_hints(abi_types);

    // Validate that all types can be represented in target languages (D-09).
    validate_representable_types(abi_types).map_err(|e| -> Box<dyn std::error::Error> {
        e.into()
    })?;

    // Emit cargo:rerun-if-changed for all tracked source files.
    for path in tracked_files {
        println!("cargo:rerun-if-changed={}", path.display());
    }

    let languages: [TargetLang; 5] = [
        TargetLang::Cpp,
        TargetLang::CSharp,
        TargetLang::Python,
        TargetLang::Lua,
        TargetLang::JavaScript,
    ];

    for lang in languages {
        let sdk: String = generate_language_sdk(lang, abi_types);

        let abi_dir: PathBuf = workspace_root
            .join("sdks")
            .join(lang.language_name())
            .join("abi");

        let output_path: PathBuf = if lang.subdir().is_empty() {
            abi_dir.join(lang.output_filename())
        } else {
            abi_dir.join(lang.subdir()).join(lang.output_filename())
        };

        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        std::fs::write(&output_path, sdk)?;
    }

    // Generate layout test source files per D-31.
    generate_layout_tests(abi_types, workspace_root)?;

    Ok(())
}

/// Generate layout test source files for all SDK languages per D-31.
///
/// Per D-32: Only generates test source files. Test scaffolding (project files,
/// conftest) must be created manually per SDK.
fn generate_layout_tests(
    abi_types: &AbiTypes,
    workspace_root: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    // Collect structs with known sizes.
    let sized_structs: Vec<(&str, usize)> = abi_types
        .structs
        .iter()
        .filter_map(|s| s.size_hint.map(|size| (s.name.as_str(), size)))
        .collect();

    if sized_structs.is_empty() {
        return Ok(());
    }

    // Python: test_layout.py with pytest assertions.
    let python_tests = generate_python_layout_tests(&sized_structs);
    let python_dir = workspace_root.join("sdks/python/abi");
    std::fs::create_dir_all(&python_dir)?;
    std::fs::write(python_dir.join("test_layout.py"), python_tests)?;

    // C#: LayoutTests.cs with xUnit.
    let csharp_tests = generate_csharp_layout_tests(&sized_structs);
    let csharp_dir = workspace_root.join("sdks/csharp/abi");
    std::fs::create_dir_all(&csharp_dir)?;
    std::fs::write(csharp_dir.join("LayoutTests.cs"), csharp_tests)?;

    // Lua: test_layout.lua with simple assertions.
    let lua_tests = generate_lua_layout_tests(&sized_structs);
    let lua_dir = workspace_root.join("sdks/lua/abi");
    std::fs::create_dir_all(&lua_dir)?;
    std::fs::write(lua_dir.join("test_layout.lua"), lua_tests)?;

    // JS: test_layout.ts with Deno.test.
    let js_tests = generate_js_layout_tests(&sized_structs);
    let js_dir = workspace_root.join("sdks/js/abi");
    std::fs::create_dir_all(&js_dir)?;
    std::fs::write(js_dir.join("test_layout.ts"), js_tests)?;

    // C++: test_layout.cpp with static_assert.
    let cpp_tests = generate_cpp_layout_tests(&sized_structs);
    let cpp_dir = workspace_root.join("sdks/cpp/abi");
    std::fs::create_dir_all(&cpp_dir)?;
    std::fs::write(cpp_dir.join("test_layout.cpp"), cpp_tests)?;

    Ok(())
}

/// Generate Python layout test file content.
fn generate_python_layout_tests(sized_structs: &[(&str, usize)]) -> String {
    let mut output = String::new();
    output.push_str("# Layout tests for polyplug ABI structs.\n");
    output.push_str("# AUTO-GENERATED by polyplug_abi build script — do not edit.\n\n");
    output.push_str("import ctypes\n\n");

    // Import all structs from the generated abi module.
    output.push_str("from abi import (\n");
    for (name, _) in sized_structs {
        output.push_str(&format!("    {},\n", name));
    }
    output.push_str(")\n\n\n");

    for (name, size) in sized_structs {
        let test_name = to_snake_case(name);
        output.push_str(&format!(
            "def test_{}_size():\n    assert ctypes.sizeof({}) == {}, \
             f\"{} expected {} bytes, got {{ctypes.sizeof({})}}\"\n\n\n",
            test_name, name, size, name, size, name
        ));
    }

    output
}

/// Generate C# layout test file content.
fn generate_csharp_layout_tests(sized_structs: &[(&str, usize)]) -> String {
    let mut output = String::new();
    output.push_str("// Layout tests for polyplug ABI structs.\n");
    output.push_str("// AUTO-GENERATED by polyplug_abi build script — do not edit.\n\n");
    output.push_str("using System.Runtime.InteropServices;\n");
    output.push_str("using Xunit;\n\n");
    output.push_str("namespace Polyplug.Abi.Tests\n{\n");
    output.push_str("    public class LayoutTests\n    {\n");

    for (name, size) in sized_structs {
        let test_name = format!("{}Is{}Bytes", name, size);
        output.push_str(&format!(
            "        [Fact]\n        public void {}() => \
             Assert.Equal({}, Marshal.SizeOf<{}>());\n\n",
            test_name, size, name
        ));
    }

    output.push_str("    }\n}\n");
    output
}

/// Generate Lua layout test file content.
fn generate_lua_layout_tests(sized_structs: &[(&str, usize)]) -> String {
    let mut output = String::new();
    output.push_str("-- Layout tests for polyplug ABI structs.\n");
    output.push_str("-- AUTO-GENERATED by polyplug_abi build script — do not edit.\n\n");
    output.push_str("local ffi = require(\"ffi\")\n\n");

    for (name, size) in sized_structs {
        output.push_str(&format!(
            "assert(ffi.sizeof(\"{}\") == {}, \"{} size mismatch\")\n",
            name, size, name
        ));
    }

    output.push_str("\nprint(\"All layout tests passed!\")\n");
    output
}

/// Generate JS/TS layout test file content.
fn generate_js_layout_tests(sized_structs: &[(&str, usize)]) -> String {
    let mut output = String::new();
    output.push_str("// Layout tests for polyplug ABI structs.\n");
    output.push_str("// AUTO-GENERATED by polyplug_abi build script — do not edit.\n\n");
    output.push_str("import {\n");
    for (name, _) in sized_structs {
        output.push_str(&format!("    {}_SIZE,\n", to_upper_snake_case_for_generate(name)));
    }
    output.push_str("} from \"./abi.ts\";\n\n");

    for (name, size) in sized_structs {
        let const_name = format!("{}_SIZE", to_upper_snake_case_for_generate(name));
        output.push_str(&format!(
            "Deno.test(\"{} is {} bytes\", () => {{\n    assert({} === {});\n}});\n\n",
            name, size, const_name, size
        ));
    }

    output
}

/// Generate C++ layout test file content.
fn generate_cpp_layout_tests(sized_structs: &[(&str, usize)]) -> String {
    let mut output = String::new();
    output.push_str("// Layout tests for polyplug ABI structs.\n");
    output.push_str("// AUTO-GENERATED by polyplug_abi build script — do not edit.\n\n");
    output.push_str("#include \"polyplug/abi.hpp\"\n\n");

    for (name, size) in sized_structs {
        output.push_str(&format!(
            "static_assert(sizeof({}) == {}, \"{} size mismatch\");\n",
            name, size, name
        ));
    }

    output
}

/// Convert PascalCase to snake_case.
fn to_snake_case(s: &str) -> String {
    let mut result = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                result.push('_');
            }
            result.push(c.to_ascii_lowercase());
        } else {
            result.push(c);
        }
    }
    result
}

/// Convert PascalCase to UPPER_SNAKE_CASE for JS constants.
fn to_upper_snake_case_for_generate(s: &str) -> String {
    let mut result = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                result.push('_');
            }
            result.push(c);
        } else {
            result.push(c.to_ascii_uppercase());
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::types::{AbiConst, AbiStruct};

    #[test]
    fn test_target_lang_file_extension() {
        assert_eq!(TargetLang::Cpp.file_extension(), "hpp");
        assert_eq!(TargetLang::CSharp.file_extension(), "cs");
        assert_eq!(TargetLang::Python.file_extension(), "py");
        assert_eq!(TargetLang::Lua.file_extension(), "lua");
        assert_eq!(TargetLang::JavaScript.file_extension(), "ts");
    }

    #[test]
    fn test_target_lang_language_name() {
        assert_eq!(TargetLang::Cpp.language_name(), "cpp");
        assert_eq!(TargetLang::CSharp.language_name(), "csharp");
        assert_eq!(TargetLang::Python.language_name(), "python");
        assert_eq!(TargetLang::Lua.language_name(), "lua");
        assert_eq!(TargetLang::JavaScript.language_name(), "js");
    }

    #[test]
    fn test_generate_language_sdk_cpp() {
        let mut abi_types: AbiTypes = AbiTypes::new();
        abi_types.add_const(AbiConst {
            name: String::from("TEST_CONST"),
            rust_type: String::from("u32"),
            value: String::from("42"),
            doc: Some(String::from("Test constant.")),
        });

        let sdk: String = generate_language_sdk(TargetLang::Cpp, &abi_types);

        assert!(sdk.contains("#pragma once"));
        assert!(sdk.contains("#include <cstdint>"));
        assert!(sdk.contains("TEST_CONST"));
    }

    #[test]
    fn test_generate_language_sdk_python() {
        let mut abi_types: AbiTypes = AbiTypes::new();
        abi_types.add_const(AbiConst {
            name: String::from("TEST_CONST"),
            rust_type: String::from("u32"),
            value: String::from("42"),
            doc: Some(String::from("Test constant.")),
        });

        let sdk: String = generate_language_sdk(TargetLang::Python, &abi_types);

        assert!(sdk.contains("import ctypes"));
        assert!(sdk.contains("TEST_CONST"));
    }

    /// Test that populate_size_hints fills in known struct sizes.
    #[test]
    fn test_populate_size_hints() {
        use crate::build::types::AbiField;

        let mut abi_types: AbiTypes = AbiTypes::new();
        abi_types.add_struct(AbiStruct {
            name: String::from("RuntimeConfig"),
            fields: vec![],
            doc: None,
            repr_c: true,
            size_hint: None,
        });
        abi_types.add_struct(AbiStruct {
            name: String::from("GuestContractHandle"),
            fields: vec![],
            doc: None,
            repr_c: true,
            size_hint: None,
        });
        abi_types.add_struct(AbiStruct {
            name: String::from("UnknownStruct"),
            fields: vec![],
            doc: None,
            repr_c: true,
            size_hint: None,
        });

        populate_size_hints(&mut abi_types);

        assert_eq!(
            abi_types.structs[0].size_hint,
            Some(16),
            "RuntimeConfig should be 16 bytes"
        );
        assert_eq!(
            abi_types.structs[1].size_hint,
            Some(4),
            "GuestContractHandle should be 4 bytes"
        );
        assert_eq!(
            abi_types.structs[2].size_hint,
            None,
            "Unknown struct should have no size hint"
        );
    }

    /// Test that C++ output contains static_assert for structs with size hints.
    #[test]
    fn test_cpp_output_contains_static_assert() {
        use crate::build::types::AbiField;

        let mut abi_types: AbiTypes = AbiTypes::new();
        abi_types.add_struct(AbiStruct {
            name: String::from("RuntimeConfig"),
            fields: vec![AbiField {
                name: String::from("compatibility"),
                rust_type: String::from("u32"),
                doc: None,
            }],
            doc: None,
            repr_c: true,
            size_hint: Some(16),
        });

        let sdk: String = generate_language_sdk(TargetLang::Cpp, &abi_types);
        assert!(
            sdk.contains("static_assert(sizeof(RuntimeConfig) == 16"),
            "C++ should contain static_assert for RuntimeConfig: {}",
            sdk
        );
    }

    /// Test that Python output contains ctypes.sizeof assertions for structs with size hints.
    #[test]
    fn test_python_output_contains_sizeof_assertions() {
        use crate::build::types::AbiField;

        let mut abi_types: AbiTypes = AbiTypes::new();
        abi_types.add_struct(AbiStruct {
            name: String::from("RuntimeConfig"),
            fields: vec![AbiField {
                name: String::from("compatibility"),
                rust_type: String::from("u32"),
                doc: None,
            }],
            doc: None,
            repr_c: true,
            size_hint: Some(16),
        });

        let sdk: String = generate_language_sdk(TargetLang::Python, &abi_types);
        assert!(
            sdk.contains("assert ctypes.sizeof(RuntimeConfig) == 16"),
            "Python should contain ctypes.sizeof assertion for RuntimeConfig: {}",
            sdk
        );
    }
}

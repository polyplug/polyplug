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
/// * `abi_types` - Extracted ABI types.
/// * `workspace_root` - Path to the workspace root directory.
/// * `tracked_files` - Source files to emit `cargo:rerun-if-changed` for.
///
/// # Returns
/// Result indicating success or failure.
pub fn generate_all_sdks(
    abi_types: &AbiTypes,
    workspace_root: &Path,
    tracked_files: &[PathBuf],
) -> Result<(), Box<dyn std::error::Error>> {
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

    Ok(())
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
}

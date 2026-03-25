//! Python ABI Code Generator — generates Python ctypes bindings from ABI type information.
//!
//! This module implements the `AbiGenerator` trait for Python, producing idiomatic
//! Python code with `ctypes.Structure` subclasses, `IntEnum` classes, and FNV-1a
//! hash functions.

use super::{AbiGenerator, AbiInfo, EnumInfo, StructInfo, UnionInfo};
use std::path::PathBuf;

/// Python ABI code generator.
///
/// Generates Python ctypes bindings for the polyplug ABI types, including:
/// - Constants as module-level variables
/// - Structs as `ctypes.Structure` subclasses with `_fields_`
/// - Enums as `IntEnum` classes
/// - Unions as `ctypes.Union` subclasses with `_fields_`
/// - FNV-1a hash helper functions
#[derive(Debug, Clone, Copy, Default)]
pub struct PythonGenerator;

impl PythonGenerator {
    /// Create a new Python generator.
    pub fn new() -> PythonGenerator {
        PythonGenerator
    }

    /// Convert a Rust type name to a Python ctypes type name.
    ///
    /// # Type Mappings
    /// - `*const u8`, `*mut u8` → `c_char_p` (for UTF-8 strings) or `c_void_p`
    /// - `*const ()`, `*mut ()` → `c_void_p`
    /// - `*mut c_void` → `c_void_p`
    /// - `u64` → `c_uint64`
    /// - `u32` → `c_uint32`
    /// - `u16` → `c_uint16`
    /// - `u8` → `c_uint8`
    /// - `i64` → `c_int64`
    /// - `i32` → `c_int32`
    /// - `i16` → `c_int16`
    /// - `i8` → `c_int8`
    /// - `usize` → `c_size_t`
    /// - `isize` → `c_ssize_t`
    /// - `bool` → `c_bool`
    /// - ABI struct names → same name (StringView, Buffer, etc.)
    fn rust_type_to_python(type_name: &str) -> String {
        // Handle pointer types
        if type_name.starts_with('*') {
            // Check for string-like pointers
            if type_name == "*const u8" {
                return String::from("ctypes.c_char_p");
            }
            // All other pointers use c_void_p
            return String::from("ctypes.c_void_p");
        }

        // Handle c_void
        if type_name.contains("c_void") {
            return String::from("ctypes.c_void_p");
        }

        match type_name {
            "u64" => String::from("ctypes.c_uint64"),
            "u32" => String::from("ctypes.c_uint32"),
            "u16" => String::from("ctypes.c_uint16"),
            "u8" => String::from("ctypes.c_uint8"),
            "i64" => String::from("ctypes.c_int64"),
            "i32" => String::from("ctypes.c_int32"),
            "i16" => String::from("ctypes.c_int16"),
            "i8" => String::from("ctypes.c_int8"),
            "usize" => String::from("ctypes.c_size_t"),
            "isize" => String::from("ctypes.c_ssize_t"),
            "bool" => String::from("ctypes.c_bool"),
            "()" => String::from("None"),
            _ => String::from(type_name),
        }
    }

    /// Generate a Python docstring.
    fn format_docstring(doc: &str, indent_level: usize) -> String {
        let indent: String = "    ".repeat(indent_level);
        let lines: Vec<&str> = doc.lines().collect();
        if lines.len() == 1 {
            format!("{}\"\"\"{}\"\"\"\n", indent, lines[0])
        } else {
            let mut result: String = format!("{}\"\"\"{}\n", indent, lines[0]);
            for line in &lines[1..] {
                result.push_str(&format!("{}{}\n", indent, line));
            }
            result.push_str(&format!("{}\"\"\"\n", indent));
            result
        }
    }

    /// Generate a single struct definition.
    fn generate_struct(struct_info: &StructInfo) -> String {
        let mut output: String = String::new();

        output.push_str("\n\nclass ");
        output.push_str(&struct_info.name);
        output.push_str("(ctypes.Structure):\n");

        if let Some(doc) = &struct_info.doc {
            output.push_str(&Self::format_docstring(doc, 1));
        } else {
            output.push_str("    \"\"\"ABI struct.\"\"\"\n");
        }

        // Generate _fields_ list
        output.push_str("    _fields_ = [\n");
        for field in &struct_info.fields {
            let py_type: String = Self::rust_type_to_python(&field.type_name);
            output.push_str(&format!("        (\"{}\", {}),\n", field.name, py_type));
        }
        output.push_str("    ]\n");

        output
    }

    /// Generate a single enum definition.
    fn generate_enum(enum_info: &EnumInfo) -> String {
        let mut output: String = String::new();

        output.push_str("\n\nclass ");
        output.push_str(&enum_info.name);
        output.push_str("(enum.IntEnum):\n");

        if let Some(doc) = &enum_info.doc {
            output.push_str(&Self::format_docstring(doc, 1));
        } else {
            output.push_str("    \"\"\"ABI enum.\"\"\"\n");
        }

        for (i, variant) in enum_info.variants.iter().enumerate() {
            if let Some(value) = variant.value {
                output.push_str(&format!("    {} = {}\n", variant.name, value));
            } else if i == 0 {
                output.push_str(&format!("    {} = 0\n", variant.name));
            } else {
                output.push_str(&format!("    {} = {}\n", variant.name, i));
            }
        }

        output
    }

    /// Generate a single union definition.
    fn generate_union(union_info: &UnionInfo) -> String {
        let mut output: String = String::new();

        output.push_str("\n\nclass ");
        output.push_str(&union_info.name);
        output.push_str("(ctypes.Union):\n");

        if let Some(doc) = &union_info.doc {
            output.push_str(&Self::format_docstring(doc, 1));
        } else {
            output.push_str("    \"\"\"ABI union.\"\"\"\n");
        }

        // Generate _fields_ list
        output.push_str("    _fields_ = [\n");
        for variant in &union_info.variants {
            let py_type: String = Self::rust_type_to_python(&variant.type_name);
            output.push_str(&format!("        (\"{}\", {}),\n", variant.name, py_type));
        }
        output.push_str("    ]\n");

        output
    }
}

impl AbiGenerator for PythonGenerator {
    fn generate_constants(&self, info: &AbiInfo) -> String {
        let mut output: String = String::new();

        output.push_str("# THIS FILE IS AUTO-GENERATED BY polyplug_abi\n");
        output.push_str("# DO NOT EDIT BY HAND\n");
        output.push_str("# Re-generate with: polyplug_abi generate --lang python\n\n");

        output.push_str("\"\"\"ABI constants and types for the polyplug plugin runtime.\n");
        output.push('\n');
        output.push_str(
            "This module contains the frozen ABI types that match the Rust ABI exactly.\n",
        );
        output
            .push_str("DO NOT modify field order or sizes — these must match the host runtime.\n");
        output.push_str("\"\"\"\n\n");

        output.push_str("from __future__ import annotations\n\n");
        output.push_str("import ctypes\n");
        output.push_str("import enum\n");
        output.push_str("from typing import ClassVar\n\n");

        output.push_str(
            "# ─── ABI Constants ────────────────────────────────────────────────────────────\n\n",
        );

        for constant in &info.constants {
            output.push_str(&format!("{}: int = {}\n", constant.name, constant.value));
        }

        output.push('\n');
        output
    }

    fn generate_structs(&self, info: &AbiInfo) -> String {
        let mut output: String = String::new();

        output.push_str(
            "# ─── ABI Structs ──────────────────────────────────────────────────────────────\n",
        );

        for struct_info in &info.structs {
            output.push_str(&Self::generate_struct(struct_info));
        }

        output
    }

    fn generate_enums(&self, info: &AbiInfo) -> String {
        let mut output: String = String::new();

        output.push_str(
            "\n# ─── ABI Enums ────────────────────────────────────────────────────────────────\n",
        );

        for enum_info in &info.enums {
            output.push_str(&Self::generate_enum(enum_info));
        }

        output
    }

    fn generate_unions(&self, info: &AbiInfo) -> String {
        let mut output: String = String::new();

        output.push_str(
            "\n# ─── ABI Unions ───────────────────────────────────────────────────────────────\n",
        );

        for union_info in &info.unions {
            output.push_str(&Self::generate_union(union_info));
        }

        output
    }

    fn generate_helpers(&self, _info: &AbiInfo) -> String {
        let mut output: String = String::new();

        output.push_str("\n# ─── FNV-1a Hash Helpers ──────────────────────────────────────────────────────\n\n");

        output.push_str("FNV_OFFSET: int = 0xcbf29ce484222325\n");
        output.push_str("FNV_PRIME: int = 0x00000100000001B3\n\n");

        output.push_str("def fnv1a_64(data: bytes) -> int:\n");
        output.push_str("    \"\"\"Compute FNV-1a 64-bit hash of a byte sequence.\"\"\"\n");
        output.push_str("    hash_val: int = FNV_OFFSET\n");
        output.push_str("    for byte in data:\n");
        output.push_str("        hash_val ^= byte\n");
        output.push_str("        hash_val = (hash_val * FNV_PRIME) & 0xFFFFFFFFFFFFFFFF\n");
        output.push_str("    return hash_val\n\n");

        output.push_str("def contract_id(name: str, major_version: int) -> int:\n");
        output.push_str("    \"\"\"Compute the contract ID for 'name@major_version' using FNV-1a 64-bit.\"\"\"\n");
        output.push_str("    canonical: str = f\"{name}@{major_version}\"\n");
        output.push_str("    return fnv1a_64(canonical.encode('utf-8'))\n\n");

        output.push_str("def extension_id(name: str) -> int:\n");
        output.push_str(
            "    \"\"\"Compute an extension ID from its name using FNV-1a lower 32 bits.\"\"\"\n",
        );
        output.push_str("    return fnv1a_64(name.encode('utf-8')) & 0xFFFFFFFF\n\n");

        output.push_str("def bundle_id(name: str) -> int:\n");
        output.push_str(
            "    \"\"\"Compute a bundle ID from its name using FNV-1a 64-bit hash.\"\"\"\n",
        );
        output.push_str("    return fnv1a_64(name.encode('utf-8'))\n");

        output
    }

    fn file_extension(&self) -> &'static str {
        "py"
    }

    fn output_dir(&self) -> &'static str {
        "python"
    }

    fn generate(&self, info: &AbiInfo) -> super::GeneratedFiles {
        let mut files: super::GeneratedFiles = super::GeneratedFiles::new();

        let mut content: String = String::new();
        content.push_str(&self.generate_constants(info));
        content.push_str(&self.generate_structs(info));
        content.push_str(&self.generate_enums(info));
        content.push_str(&self.generate_unions(info));
        content.push_str(&self.generate_helpers(info));

        let filename: String = format!("abi.{}", self.file_extension());
        files.push(super::GeneratedFile {
            path: PathBuf::from(filename),
            content,
        });

        files
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::{ConstantInfo, FieldInfo, GeneratedFiles, UnionVariantInfo, VariantInfo};

    #[test]
    fn python_generator_new() {
        let generator: PythonGenerator = PythonGenerator::new();
        assert_eq!(generator.file_extension(), "py");
        assert_eq!(generator.output_dir(), "python");
    }

    #[test]
    fn rust_type_to_python_primitives() {
        assert_eq!(
            PythonGenerator::rust_type_to_python("u64"),
            "ctypes.c_uint64"
        );
        assert_eq!(
            PythonGenerator::rust_type_to_python("u32"),
            "ctypes.c_uint32"
        );
        assert_eq!(
            PythonGenerator::rust_type_to_python("u16"),
            "ctypes.c_uint16"
        );
        assert_eq!(PythonGenerator::rust_type_to_python("u8"), "ctypes.c_uint8");
        assert_eq!(
            PythonGenerator::rust_type_to_python("i64"),
            "ctypes.c_int64"
        );
        assert_eq!(
            PythonGenerator::rust_type_to_python("i32"),
            "ctypes.c_int32"
        );
        assert_eq!(
            PythonGenerator::rust_type_to_python("i16"),
            "ctypes.c_int16"
        );
        assert_eq!(PythonGenerator::rust_type_to_python("i8"), "ctypes.c_int8");
        assert_eq!(
            PythonGenerator::rust_type_to_python("usize"),
            "ctypes.c_size_t"
        );
        assert_eq!(
            PythonGenerator::rust_type_to_python("isize"),
            "ctypes.c_ssize_t"
        );
        assert_eq!(
            PythonGenerator::rust_type_to_python("bool"),
            "ctypes.c_bool"
        );
    }

    #[test]
    fn rust_type_to_python_pointers() {
        assert_eq!(
            PythonGenerator::rust_type_to_python("*const u8"),
            "ctypes.c_char_p"
        );
        assert_eq!(
            PythonGenerator::rust_type_to_python("*mut u8"),
            "ctypes.c_void_p"
        );
        assert_eq!(
            PythonGenerator::rust_type_to_python("*const ()"),
            "ctypes.c_void_p"
        );
        assert_eq!(
            PythonGenerator::rust_type_to_python("*mut ()"),
            "ctypes.c_void_p"
        );
        assert_eq!(
            PythonGenerator::rust_type_to_python("*mut c_void"),
            "ctypes.c_void_p"
        );
    }

    #[test]
    fn rust_type_to_python_abi_types() {
        assert_eq!(
            PythonGenerator::rust_type_to_python("StringView"),
            "StringView"
        );
        assert_eq!(PythonGenerator::rust_type_to_python("Buffer"), "Buffer");
        assert_eq!(PythonGenerator::rust_type_to_python("AbiError"), "AbiError");
        assert_eq!(
            PythonGenerator::rust_type_to_python("PluginHandle"),
            "PluginHandle"
        );
    }

    #[test]
    fn format_docstring_single_line() {
        let result: String = PythonGenerator::format_docstring("Hello world", 1);
        assert_eq!(result, "    \"\"\"Hello world\"\"\"\n");
    }

    #[test]
    fn format_docstring_multiple_lines() {
        let result: String = PythonGenerator::format_docstring("Line 1\nLine 2", 1);
        assert_eq!(result, "    \"\"\"Line 1\n    Line 2\n    \"\"\"\n");
    }

    #[test]
    fn generate_constants_produces_valid_python() {
        let mut info: AbiInfo = AbiInfo::new();
        info.add_constant(ConstantInfo {
            name: String::from("ABI_OK"),
            value: String::from("0"),
            type_name: String::from("u32"),
        });
        info.add_constant(ConstantInfo {
            name: String::from("POLYPLUG_ABI_VERSION"),
            value: String::from("1"),
            type_name: String::from("u32"),
        });

        let generator: PythonGenerator = PythonGenerator::new();
        let output: String = generator.generate_constants(&info);

        assert!(output.contains("ABI_OK: int = 0"));
        assert!(output.contains("POLYPLUG_ABI_VERSION: int = 1"));
        assert!(output.contains("import ctypes"));
        assert!(output.contains("import enum"));
    }

    #[test]
    fn generate_struct_produces_valid_python() {
        let struct_info: StructInfo = StructInfo {
            name: String::from("StringView"),
            fields: vec![
                FieldInfo {
                    name: String::from("ptr"),
                    type_name: String::from("*const u8"),
                    doc: Some(String::from("UTF-8 bytes, NOT null-terminated.")),
                },
                FieldInfo {
                    name: String::from("len"),
                    type_name: String::from("usize"),
                    doc: Some(String::from("Byte count.")),
                },
            ],
            doc: Some(String::from("Non-owning UTF-8 string view.")),
        };

        let output: String = PythonGenerator::generate_struct(&struct_info);

        assert!(output.contains("class StringView(ctypes.Structure):"));
        assert!(output.contains("_fields_ = ["));
        assert!(output.contains("(\"ptr\", ctypes.c_char_p)"));
        assert!(output.contains("(\"len\", ctypes.c_size_t)"));
    }

    #[test]
    fn generate_enum_produces_valid_python() {
        let enum_info: EnumInfo = EnumInfo {
            name: String::from("DispatchType"),
            variants: vec![
                VariantInfo {
                    name: String::from("Native"),
                    value: Some(0),
                    doc: Some(String::from("Native dispatch.")),
                },
                VariantInfo {
                    name: String::from("VirtualMachine"),
                    value: Some(1),
                    doc: Some(String::from("VM dispatch.")),
                },
            ],
            doc: Some(String::from("Dispatch mechanism type.")),
        };

        let output: String = PythonGenerator::generate_enum(&enum_info);

        assert!(output.contains("class DispatchType(enum.IntEnum):"));
        assert!(output.contains("Native = 0"));
        assert!(output.contains("VirtualMachine = 1"));
    }

    #[test]
    fn generate_union_produces_valid_python() {
        let union_info: UnionInfo = UnionInfo {
            name: String::from("PluginDispatch"),
            variants: vec![
                UnionVariantInfo {
                    name: String::from("native"),
                    type_name: String::from("NativeDispatch"),
                    doc: None,
                },
                UnionVariantInfo {
                    name: String::from("vm"),
                    type_name: String::from("VmDispatch"),
                    doc: None,
                },
            ],
            doc: Some(String::from("Union of dispatch mechanisms.")),
        };

        let output: String = PythonGenerator::generate_union(&union_info);

        assert!(output.contains("class PluginDispatch(ctypes.Union):"));
        assert!(output.contains("_fields_ = ["));
        assert!(output.contains("(\"native\", NativeDispatch)"));
        assert!(output.contains("(\"vm\", VmDispatch)"));
    }

    #[test]
    fn generate_helpers_produces_valid_python() {
        let generator: PythonGenerator = PythonGenerator::new();
        let info: AbiInfo = AbiInfo::new();
        let output: String = generator.generate_helpers(&info);

        assert!(output.contains("FNV_OFFSET"));
        assert!(output.contains("FNV_PRIME"));
        assert!(output.contains("def fnv1a_64(data: bytes) -> int:"));
        assert!(output.contains("def contract_id(name: str, major_version: int) -> int:"));
        assert!(output.contains("def extension_id(name: str) -> int:"));
        assert!(output.contains("def bundle_id(name: str) -> int:"));
    }

    #[test]
    fn generate_produces_complete_file() {
        let mut info: AbiInfo = AbiInfo::new();
        info.add_constant(ConstantInfo {
            name: String::from("ABI_OK"),
            value: String::from("0"),
            type_name: String::from("u32"),
        });
        info.add_struct(StructInfo {
            name: String::from("StringView"),
            fields: vec![FieldInfo {
                name: String::from("ptr"),
                type_name: String::from("*const u8"),
                doc: None,
            }],
            doc: None,
        });

        let generator: PythonGenerator = PythonGenerator::new();
        let files: GeneratedFiles = generator.generate(&info);

        assert_eq!(files.files.len(), 1);
        assert_eq!(files.files[0].path, PathBuf::from("abi.py"));
        assert!(files.files[0].content.contains("ABI_OK"));
        assert!(files.files[0].content.contains("class StringView"));
    }

    /// Generate the abi.py file for the SDK.
    /// Run with: cargo test --package polyplug_abi -- generate_abi_py_file --nocapture
    #[test]
    fn generate_abi_py_file() {
        use crate::build::AbiParser;
        use std::fs;
        use std::path::Path;

        let abi_source: &str = include_str!("../lib.rs");
        let parser: AbiParser = AbiParser::new();
        let info: AbiInfo = parser
            .parse(abi_source)
            .expect("failed to parse ABI source");

        let generator: PythonGenerator = PythonGenerator::new();
        let files: GeneratedFiles = generator.generate(&info);

        let workspace_root: &Path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .expect("failed to find workspace root");
        let output_path: std::path::PathBuf = workspace_root.join("sdks/python/abi/abi.py");

        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).expect("failed to create output directory");
        }

        fs::write(&output_path, &files.files[0].content).expect("failed to write abi.py");

        println!("Generated: {}", output_path.display());
    }
}

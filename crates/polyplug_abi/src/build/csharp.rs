//! C# ABI Code Generator — generates C# bindings from ABI type information.
//!
//! This module implements the `AbiGenerator` trait for C#, producing idiomatic
//! C# code with proper `[StructLayout]` attributes, type mappings, and FNV-1a
//! hash functions.

use super::{AbiGenerator, AbiInfo, EnumInfo, StructInfo, UnionInfo};
use std::path::PathBuf;

/// C# ABI code generator.
///
/// Generates C# bindings for the polyplug ABI types, including:
/// - Constants in a `public static class AbiConstants`
/// - Structs with `[StructLayout(LayoutKind.Sequential)]`
/// - Enums as `public enum`
/// - Unions with `[StructLayout(LayoutKind.Explicit)]` and `[FieldOffset]`
/// - FNV-1a hash helper functions
#[derive(Debug, Clone, Copy, Default)]
pub struct CSharpGenerator;

impl CSharpGenerator {
    /// Create a new C# generator.
    pub fn new() -> CSharpGenerator {
        CSharpGenerator
    }

    /// Convert a Rust type name to a C# type name.
    ///
    /// # Type Mappings
    /// - `*const u8`, `*mut u8` → `IntPtr` (pointer-sized, 8 bytes on 64-bit)
    /// - `*const ()`, `*mut ()` → `IntPtr`
    /// - `*const *const ()` → `IntPtr` (pointer to pointer)
    /// - `*mut c_void` → `IntPtr`
    /// - `u64` → `ulong`
    /// - `u32` → `uint`
    /// - `u16` → `ushort`
    /// - `u8` → `byte`
    /// - `i64` → `long`
    /// - `i32` → `int`
    /// - `i16` → `short`
    /// - `i8` → `sbyte`
    /// - `usize` → `nuint` (pointer-sized unsigned)
    /// - `isize` → `nint` (pointer-sized signed)
    /// - ABI struct names → same name (StringView, Buffer, etc.)
    fn rust_type_to_csharp(type_name: &str) -> String {
        if type_name.starts_with('*') {
            return String::from("IntPtr");
        }

        if type_name.contains("c_void") {
            return String::from("IntPtr");
        }

        match type_name {
            "u64" => String::from("ulong"),
            "u32" => String::from("uint"),
            "u16" => String::from("ushort"),
            "u8" => String::from("byte"),
            "i64" => String::from("long"),
            "i32" => String::from("int"),
            "i16" => String::from("short"),
            "i8" => String::from("sbyte"),
            "usize" => String::from("nuint"),
            "isize" => String::from("nint"),
            "bool" => String::from("bool"),
            "()" => String::from("void"),
            _ => String::from(type_name),
        }
    }

    /// Generate a C# constant value with proper type suffix.
    fn format_constant_value(value: &str, type_name: &str) -> String {
        match type_name {
            "u64" => format!("{}ul", value),
            "u32" | "usize" => format!("{}u", value),
            "i64" => format!("{}L", value),
            _ => String::from(value),
        }
    }

    /// Generate XML documentation comment for C#.
    fn format_xml_doc(doc: &str) -> String {
        doc.lines()
            .map(|line: &str| {
                if line.is_empty() {
                    String::from("///")
                } else {
                    format!("/// {}", line)
                }
            })
            .collect::<Vec<String>>()
            .join("\n")
    }

    /// Generate a single struct definition.
    fn generate_struct(struct_info: &StructInfo) -> String {
        let mut output: String = String::new();

        if let Some(doc) = &struct_info.doc {
            output.push_str(&Self::format_xml_doc(doc));
            output.push('\n');
        }

        output.push_str("[StructLayout(LayoutKind.Sequential)]\n");
        output.push_str(&format!("public struct {}\n", struct_info.name));
        output.push_str("{\n");

        for field in &struct_info.fields {
            if let Some(doc) = &field.doc {
                output.push_str(&Self::format_xml_doc(doc));
                output.push('\n');
            }

            let csharp_type: String = Self::rust_type_to_csharp(&field.type_name);
            let field_name: String = super::to_pascal_case(&field.name);
            output.push_str(&format!("    public {} {};\n", csharp_type, field_name));
        }

        output.push_str("}\n\n");
        output
    }

    /// Generate a single enum definition.
    fn generate_enum(enum_info: &EnumInfo) -> String {
        let mut output: String = String::new();

        if let Some(doc) = &enum_info.doc {
            output.push_str(&Self::format_xml_doc(doc));
            output.push('\n');
        }

        output.push_str(&format!("public enum {} : uint\n", enum_info.name));
        output.push_str("{\n");

        for (i, variant) in enum_info.variants.iter().enumerate() {
            if let Some(doc) = &variant.doc {
                output.push_str(&Self::format_xml_doc(doc));
                output.push('\n');
            }

            if let Some(value) = variant.value {
                output.push_str(&format!("    {} = {},\n", variant.name, value));
            } else if i == 0 {
                output.push_str(&format!("    {} = 0,\n", variant.name));
            } else {
                output.push_str(&format!("    {},\n", variant.name));
            }
        }

        output.push_str("}\n\n");
        output
    }

    /// Generate a single union definition with explicit field offsets.
    fn generate_union(union_info: &UnionInfo) -> String {
        let mut output: String = String::new();

        if let Some(doc) = &union_info.doc {
            output.push_str(&Self::format_xml_doc(doc));
            output.push('\n');
        }

        output.push_str("[StructLayout(LayoutKind.Explicit)]\n");
        output.push_str(&format!("public struct {}\n", union_info.name));
        output.push_str("{\n");

        for variant in &union_info.variants {
            if let Some(doc) = &variant.doc {
                output.push_str(&Self::format_xml_doc(doc));
                output.push('\n');
            }

            let csharp_type: String = Self::rust_type_to_csharp(&variant.type_name);
            let variant_name: String = super::to_pascal_case(&variant.name);
            output.push_str("    [FieldOffset(0)]\n");
            output.push_str(&format!("    public {} {};\n", csharp_type, variant_name));
        }

        output.push_str("}\n\n");
        output
    }
}

impl AbiGenerator for CSharpGenerator {
    fn generate_constants(&self, info: &AbiInfo) -> String {
        let mut output: String = String::new();

        output.push_str("// THIS FILE IS AUTO-GENERATED BY polyplug_abi\n");
        output.push_str("// DO NOT EDIT BY HAND\n");
        output.push_str("// Re-generate with: polyplug_abi generate --lang csharp\n\n");
        output.push_str("using System.Runtime.InteropServices;\n\n");
        output.push_str("namespace Polyplug.Abi;\n\n");

        output.push_str("/// <summary>ABI error code constants. Must match Rust ABI constants exactly.</summary>\n");
        output.push_str("public static class AbiConstants\n");
        output.push_str("{\n");

        for constant in &info.constants {
            let value: String = Self::format_constant_value(&constant.value, &constant.type_name);
            let csharp_type: String = Self::rust_type_to_csharp(&constant.type_name);
            output.push_str(&format!(
                "    public const {} {} = {};\n",
                csharp_type, constant.name, value
            ));
        }

        output.push_str("}\n\n");
        output
    }

    fn generate_structs(&self, info: &AbiInfo) -> String {
        let mut output: String = String::new();

        output.push_str(
            "// ─── ABI Structs ─────────────────────────────────────────────────────────────\n\n",
        );

        for struct_info in &info.structs {
            output.push_str(&Self::generate_struct(struct_info));
        }

        output
    }

    fn generate_enums(&self, info: &AbiInfo) -> String {
        let mut output: String = String::new();

        output.push_str(
            "// ─── ABI Enums ────────────────────────────────────────────────────────────────\n\n",
        );

        for enum_info in &info.enums {
            output.push_str(&Self::generate_enum(enum_info));
        }

        output
    }

    fn generate_unions(&self, info: &AbiInfo) -> String {
        let mut output: String = String::new();

        output.push_str(
            "// ─── ABI Unions ───────────────────────────────────────────────────────────────\n\n",
        );

        for union_info in &info.unions {
            output.push_str(&Self::generate_union(union_info));
        }

        output
    }

    fn generate_helpers(&self, _info: &AbiInfo) -> String {
        let mut output: String = String::new();

        output.push_str(
            "// ─── FNV-1a Hash Helpers ──────────────────────────────────────────────────────\n\n",
        );

        output.push_str("/// <summary>FNV-1a hash helpers for computing contract, extension, and bundle IDs.</summary>\n");
        output.push_str("public static class ContractId\n");
        output.push_str("{\n");

        output.push_str("    private const ulong FNV_OFFSET = 0xcbf29ce484222325ul;\n");
        output.push_str("    private const ulong FNV_PRIME = 0x00000100000001B3ul;\n\n");

        output.push_str("    /// <summary>Compute FNV-1a 64-bit hash of a byte span.</summary>\n");
        output.push_str("    public static ulong Fnv1a64(ReadOnlySpan<byte> data)\n");
        output.push_str("    {\n");
        output.push_str("        ulong hash = FNV_OFFSET;\n");
        output.push_str("        foreach (byte b in data)\n");
        output.push_str("        {\n");
        output.push_str("            hash ^= b;\n");
        output.push_str("            hash *= FNV_PRIME;\n");
        output.push_str("        }\n");
        output.push_str("        return hash;\n");
        output.push_str("    }\n\n");

        output.push_str("    /// <summary>Compute the contract ID for \"name@major_version\" using FNV-1a 64-bit.</summary>\n");
        output.push_str("    public static ulong Compute(string name, uint majorVersion)\n");
        output.push_str("    {\n");
        output.push_str("        string canonical = $\"{name}@{majorVersion}\";\n");
        output.push_str(
            "        int maxLen = System.Text.Encoding.UTF8.GetMaxByteCount(canonical.Length);\n",
        );
        output.push_str("        Span<byte> buffer = stackalloc byte[maxLen];\n");
        output
            .push_str("        int len = System.Text.Encoding.UTF8.GetBytes(canonical, buffer);\n");
        output.push_str("        return Fnv1a64(buffer.Slice(0, len));\n");
        output.push_str("    }\n\n");

        output.push_str("    /// <summary>Compute an extension ID from its name using FNV-1a lower 32 bits.</summary>\n");
        output.push_str("    public static uint ExtensionId(string name)\n");
        output.push_str("    {\n");
        output.push_str(
            "        int maxLen = System.Text.Encoding.UTF8.GetMaxByteCount(name.Length);\n",
        );
        output.push_str("        Span<byte> buffer = stackalloc byte[maxLen];\n");
        output.push_str("        int len = System.Text.Encoding.UTF8.GetBytes(name, buffer);\n");
        output.push_str("        return (uint)Fnv1a64(buffer.Slice(0, len));\n");
        output.push_str("    }\n\n");

        output.push_str("    /// <summary>Compute a bundle ID from its name using FNV-1a 64-bit hash.</summary>\n");
        output.push_str("    public static ulong BundleId(string name)\n");
        output.push_str("    {\n");
        output.push_str(
            "        int maxLen = System.Text.Encoding.UTF8.GetMaxByteCount(name.Length);\n",
        );
        output.push_str("        Span<byte> buffer = stackalloc byte[maxLen];\n");
        output.push_str("        int len = System.Text.Encoding.UTF8.GetBytes(name, buffer);\n");
        output.push_str("        return Fnv1a64(buffer.Slice(0, len));\n");
        output.push_str("    }\n");

        output.push_str("}\n");
        output
    }

    fn file_extension(&self) -> &'static str {
        "cs"
    }

    fn output_dir(&self) -> &'static str {
        "csharp"
    }

    fn generate(&self, info: &AbiInfo) -> super::GeneratedFiles {
        let mut files: super::GeneratedFiles = super::GeneratedFiles::new();

        let mut content: String = String::new();
        content.push_str(&self.generate_constants(info));
        content.push_str(&self.generate_structs(info));
        content.push_str(&self.generate_enums(info));
        content.push_str(&self.generate_unions(info));
        content.push_str(&self.generate_helpers(info));

        let filename: String = format!("Abi.{}", self.file_extension());
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
    use crate::build::{ConstantInfo, FieldInfo, GeneratedFiles};

    #[test]
    fn csharp_generator_new() {
        let generator: CSharpGenerator = CSharpGenerator::new();
        assert_eq!(generator.file_extension(), "cs");
        assert_eq!(generator.output_dir(), "csharp");
    }

    #[test]
    fn rust_type_to_csharp_primitives() {
        assert_eq!(CSharpGenerator::rust_type_to_csharp("u64"), "ulong");
        assert_eq!(CSharpGenerator::rust_type_to_csharp("u32"), "uint");
        assert_eq!(CSharpGenerator::rust_type_to_csharp("u16"), "ushort");
        assert_eq!(CSharpGenerator::rust_type_to_csharp("u8"), "byte");
        assert_eq!(CSharpGenerator::rust_type_to_csharp("i64"), "long");
        assert_eq!(CSharpGenerator::rust_type_to_csharp("i32"), "int");
        assert_eq!(CSharpGenerator::rust_type_to_csharp("i16"), "short");
        assert_eq!(CSharpGenerator::rust_type_to_csharp("i8"), "sbyte");
        assert_eq!(CSharpGenerator::rust_type_to_csharp("usize"), "nuint");
        assert_eq!(CSharpGenerator::rust_type_to_csharp("isize"), "nint");
        assert_eq!(CSharpGenerator::rust_type_to_csharp("bool"), "bool");
    }

    #[test]
    fn rust_type_to_csharp_pointers() {
        assert_eq!(CSharpGenerator::rust_type_to_csharp("*const u8"), "IntPtr");
        assert_eq!(CSharpGenerator::rust_type_to_csharp("*mut u8"), "IntPtr");
        assert_eq!(CSharpGenerator::rust_type_to_csharp("*const ()"), "IntPtr");
        assert_eq!(CSharpGenerator::rust_type_to_csharp("*mut ()"), "IntPtr");
        assert_eq!(
            CSharpGenerator::rust_type_to_csharp("*const *const ()"),
            "IntPtr"
        );
        assert_eq!(
            CSharpGenerator::rust_type_to_csharp("*mut c_void"),
            "IntPtr"
        );
    }

    #[test]
    fn rust_type_to_csharp_abi_types() {
        assert_eq!(
            CSharpGenerator::rust_type_to_csharp("StringView"),
            "StringView"
        );
        assert_eq!(CSharpGenerator::rust_type_to_csharp("Buffer"), "Buffer");
        assert_eq!(CSharpGenerator::rust_type_to_csharp("AbiError"), "AbiError");
        assert_eq!(
            CSharpGenerator::rust_type_to_csharp("PluginHandle"),
            "PluginHandle"
        );
    }

    #[test]
    fn format_constant_value() {
        assert_eq!(CSharpGenerator::format_constant_value("42", "u64"), "42ul");
        assert_eq!(CSharpGenerator::format_constant_value("42", "u32"), "42u");
        assert_eq!(CSharpGenerator::format_constant_value("42", "i64"), "42L");
        assert_eq!(CSharpGenerator::format_constant_value("42", "i32"), "42");
    }

    #[test]
    fn format_xml_doc_single_line() {
        let result: String = CSharpGenerator::format_xml_doc("Hello world");
        assert_eq!(result, "/// Hello world");
    }

    #[test]
    fn format_xml_doc_multiple_lines() {
        let result: String = CSharpGenerator::format_xml_doc("Line 1\nLine 2");
        assert_eq!(result, "/// Line 1\n/// Line 2");
    }

    #[test]
    fn generate_constants_produces_valid_csharp() {
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

        let generator: CSharpGenerator = CSharpGenerator::new();
        let output: String = generator.generate_constants(&info);

        assert!(output.contains("public static class AbiConstants"));
        assert!(output.contains("public const uint ABI_OK = 0u;"));
        assert!(output.contains("public const uint POLYPLUG_ABI_VERSION = 1u;"));
    }

    #[test]
    fn generate_struct_produces_valid_csharp() {
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

        let output: String = CSharpGenerator::generate_struct(&struct_info);

        assert!(output.contains("[StructLayout(LayoutKind.Sequential)]"));
        assert!(output.contains("public struct StringView"));
        assert!(output.contains("public IntPtr Ptr;"));
        assert!(output.contains("public nuint Len;"));
    }

    #[test]
    fn generate_enum_produces_valid_csharp() {
        let enum_info: EnumInfo = EnumInfo {
            name: String::from("DispatchType"),
            variants: vec![
                super::super::VariantInfo {
                    name: String::from("Native"),
                    value: Some(0),
                    doc: Some(String::from("Native dispatch.")),
                },
                super::super::VariantInfo {
                    name: String::from("VirtualMachine"),
                    value: Some(1),
                    doc: Some(String::from("VM dispatch.")),
                },
            ],
            doc: Some(String::from("Dispatch mechanism type.")),
        };

        let output: String = CSharpGenerator::generate_enum(&enum_info);

        assert!(output.contains("public enum DispatchType : uint"));
        assert!(output.contains("Native = 0,"));
        assert!(output.contains("VirtualMachine = 1,"));
    }

    #[test]
    fn generate_union_produces_valid_csharp() {
        let union_info: UnionInfo = UnionInfo {
            name: String::from("PluginDispatch"),
            variants: vec![
                super::super::UnionVariantInfo {
                    name: String::from("native"),
                    type_name: String::from("NativeDispatch"),
                    doc: None,
                },
                super::super::UnionVariantInfo {
                    name: String::from("vm"),
                    type_name: String::from("VmDispatch"),
                    doc: None,
                },
            ],
            doc: Some(String::from("Union of dispatch mechanisms.")),
        };

        let output: String = CSharpGenerator::generate_union(&union_info);

        assert!(output.contains("[StructLayout(LayoutKind.Explicit)]"));
        assert!(output.contains("public struct PluginDispatch"));
        assert!(output.contains("[FieldOffset(0)]"));
        assert!(output.contains("public NativeDispatch Native;"));
        assert!(output.contains("public VmDispatch Vm;"));
    }

    #[test]
    fn generate_helpers_produces_valid_csharp() {
        let generator: CSharpGenerator = CSharpGenerator::new();
        let info: AbiInfo = AbiInfo::new();
        let output: String = generator.generate_helpers(&info);

        assert!(output.contains("public static class ContractId"));
        assert!(output.contains("public static ulong Fnv1a64"));
        assert!(output.contains("public static ulong Compute"));
        assert!(output.contains("public static uint ExtensionId"));
        assert!(output.contains("public static ulong BundleId"));
        assert!(output.contains("FNV_OFFSET"));
        assert!(output.contains("FNV_PRIME"));
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

        let generator: CSharpGenerator = CSharpGenerator::new();
        let files: GeneratedFiles = generator.generate(&info);

        assert_eq!(files.files.len(), 1);
        assert_eq!(files.files[0].path, PathBuf::from("Abi.cs"));
        assert!(files.files[0].content.contains("AbiConstants"));
        assert!(files.files[0].content.contains("StringView"));
    }

    /// Generate the Abi.cs file for the SDK.
    /// Run with: cargo test --package polyplug_abi -- generate_abi_cs_file --ignored --nocapture
    #[test]
    #[ignore]
    fn generate_abi_cs_file() {
        use crate::build::AbiParser;
        use std::fs;
        use std::path::Path;

        let abi_source: &str = include_str!("../lib.rs");
        let parser: AbiParser = AbiParser::new();
        let info: AbiInfo = parser
            .parse(abi_source)
            .expect("failed to parse ABI source");

        let generator: CSharpGenerator = CSharpGenerator::new();
        let files: GeneratedFiles = generator.generate(&info);

        let workspace_root: &Path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .expect("failed to find workspace root");
        let output_path: std::path::PathBuf = workspace_root.join("sdks/csharp/abi/Abi.cs");

        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).expect("failed to create output directory");
        }

        fs::write(&output_path, &files.files[0].content).expect("failed to write Abi.cs");

        println!("Generated: {}", output_path.display());
    }
}

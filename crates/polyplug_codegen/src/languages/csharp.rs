//! C# code generator — produces C# bindings from ABI items.

use crate::data::{ConstInfo, EnumInfo, FunctionInfo, StructInfo, UnionInfo};
use crate::languages::{CodeGenerator, GenerationContext};

/// C# ABI code generator.
pub struct CSharpGenerator;

impl CSharpGenerator {
    pub fn new() -> Self {
        CSharpGenerator
    }

    fn rust_type_to_csharp(rust_type: &str) -> String {
        if rust_type.starts_with('*') {
            return String::from("IntPtr");
        }

        if rust_type.contains("c_void") {
            return String::from("IntPtr");
        }

        if rust_type == "&str" {
            return String::from("string");
        }

        if rust_type.starts_with("&[u8]") || rust_type.starts_with("&[") {
            return String::from("byte[]");
        }

        if let Some(inner) = rust_type.strip_prefix('&') {
            return Self::rust_type_to_csharp(inner);
        }

        match rust_type {
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
            other => String::from(other),
        }
    }

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

    fn format_indented_xml_doc(doc: &str, indent: &str) -> String {
        doc.lines()
            .map(|line: &str| {
                if line.is_empty() {
                    format!("{}///", indent)
                } else {
                    format!("{}/// {}", indent, line)
                }
            })
            .collect::<Vec<String>>()
            .join("\n")
    }

    fn to_pascal_case(s: &str) -> String {
        s.split(['_', '.'])
            .filter(|seg| !seg.is_empty())
            .map(|seg| {
                let mut chars = seg.chars();
                match chars.next() {
                    None => String::new(),
                    Some(first) => first.to_uppercase().to_string() + chars.as_str(),
                }
            })
            .collect::<Vec<String>>()
            .join("")
    }
}

impl CodeGenerator for CSharpGenerator {
    fn generate_const(&self, _item: &ConstInfo, _ctx: &GenerationContext) -> String {
        // C# ABI bindings don't include constants at namespace level.
        // Constants are provided by the AbiConstants static class in the host/guest SDKs.
        String::new()
    }

    fn generate_struct(&self, item: &StructInfo, _ctx: &GenerationContext) -> String {
        let mut output: String = String::new();

        if let Some(doc) = &item.doc {
            output.push_str(&Self::format_xml_doc(doc));
            output.push('\n');
        }

        output.push_str("[StructLayout(LayoutKind.Sequential)]\n");
        output.push_str(&format!("public struct {}\n", item.name));
        output.push_str("{\n");

        for field in &item.fields {
            if let Some(doc) = &field.doc {
                output.push_str(&Self::format_indented_xml_doc(doc, "    "));
                output.push('\n');
            }

            let csharp_type: String = Self::rust_type_to_csharp(&field.rust_type);
            let field_name: String = Self::to_pascal_case(&field.name);
            output.push_str(&format!("    public {} {};\n", csharp_type, field_name));
        }

        output.push_str("}\n\n");
        output
    }

    fn generate_enum(&self, item: &EnumInfo, _ctx: &GenerationContext) -> String {
        let mut output: String = String::new();

        if let Some(doc) = &item.doc {
            output.push_str(&Self::format_xml_doc(doc));
            output.push('\n');
        }

        output.push_str(&format!("public enum {} : uint\n", item.name));
        output.push_str("{\n");

        for (i, variant) in item.variants.iter().enumerate() {
            if let Some(doc) = &variant.doc {
                output.push_str(&Self::format_indented_xml_doc(doc, "    "));
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

    fn generate_union(&self, item: &UnionInfo, _ctx: &GenerationContext) -> String {
        let mut output: String = String::new();

        if let Some(doc) = &item.doc {
            output.push_str(&Self::format_xml_doc(doc));
            output.push('\n');
        }

        output.push_str("[StructLayout(LayoutKind.Explicit)]\n");
        output.push_str(&format!("public struct {}\n", item.name));
        output.push_str("{\n");

        for variant in &item.variants {
            let csharp_type: String = Self::rust_type_to_csharp(&variant.type_name);
            let variant_name: String = Self::to_pascal_case(&variant.name);
            output.push_str("    [FieldOffset(0)]\n");
            output.push_str(&format!("    public {} {};\n", csharp_type, variant_name));
        }

        output.push_str("}\n\n");
        output
    }

    fn generate_function(&self, _item: &FunctionInfo, _ctx: &GenerationContext) -> String {
        // C# ABI bindings don't include functions - only structs, enums, and constants.
        // Functions are either P/Invoke declarations (host side) or exports (guest side),
        // both of which are generated separately by the host/guest code generators.
        String::new()
    }

    fn file_extension(&self) -> &'static str {
        "cs"
    }

    fn language_name(&self) -> &'static str {
        "csharp"
    }

    fn generate_header(&self, _ctx: &GenerationContext) -> String {
        "using System.Runtime.InteropServices;\n\nnamespace Polyplug.Abi {\n\n".to_string()
    }

    fn generate_footer(&self, _ctx: &GenerationContext) -> String {
        r#"
/// ABI constants for polyplug.
public static class AbiConstants
{
    public const uint ABI_OK = 0u;
    public const uint ABI_ERROR_GENERIC = 1u;
    public const uint ABI_ERROR_BUFFER_TOO_SMALL = 2u;
    public const uint ABI_ERROR_PANIC = 3u;
    public const uint ABI_ERROR_NOT_FOUND = 4u;
    public const uint ABI_ERROR_STALE_HANDLE = 5u;
    public const uint ABI_ERROR_FUNCTION_NOT_AVAILABLE = 6u;
    public const uint ABI_ERROR_DUPLICATE_PROVIDER = 7u;
    public const uint ABI_ERROR_INVALID_POINTER = 8u;
    public const uint ABI_HOST_CONTRACT_NOT_FOUND = 100u;
    public const uint ABI_HOST_CONTRACT_VERSION_MISMATCH = 101u;
    public const uint ABI_HOST_CONTRACT_CALL_FAILED = 102u;
    public const uint POLYPLUG_ABI_VERSION = 1u;
}
}
"#
        .to_string()
    }
}

impl Default for CSharpGenerator {
    fn default() -> Self {
        Self::new()
    }
}

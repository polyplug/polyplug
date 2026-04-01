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

        if rust_type.starts_with('&') {
            let inner: &str = &rust_type[1..];
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

    fn format_constant_value(value: &str, type_name: &str) -> String {
        match type_name {
            "u64" => format!("{}ul", value),
            "u32" | "usize" => format!("{}u", value),
            "i64" => format!("{}L", value),
            _ => String::from(value),
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
    fn generate_const(&self, item: &ConstInfo, _ctx: &GenerationContext) -> String {
        let value: String = Self::format_constant_value(&item.value, &item.rust_type);
        let csharp_type: String = Self::rust_type_to_csharp(&item.rust_type);
        format!("public const {} {} = {};\n", csharp_type, item.name, value)
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

    fn generate_function(&self, item: &FunctionInfo, _ctx: &GenerationContext) -> String {
        let ret_type: String = item
            .return_type
            .as_ref()
            .map(|t| Self::rust_type_to_csharp(t))
            .unwrap_or_else(|| "void".to_string());

        let params: String = item
            .params
            .iter()
            .map(|p| format!("{} {}", Self::rust_type_to_csharp(&p.rust_type), p.name))
            .collect::<Vec<_>>()
            .join(", ");

        format!("public static {} {}({});\n\n", ret_type, item.name, params)
    }

    fn file_extension(&self) -> &'static str {
        "cs"
    }

    fn language_name(&self) -> &'static str {
        "csharp"
    }

    fn generate_header(&self, _ctx: &GenerationContext) -> String {
        "using System.Runtime.InteropServices;\n\n".to_string()
    }
}

impl Default for CSharpGenerator {
    fn default() -> Self {
        Self::new()
    }
}

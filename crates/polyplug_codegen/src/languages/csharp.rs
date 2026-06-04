//! C# code generator — produces C# bindings from ABI items.
//!
//! Emits `IntPtr` for all function pointer fields (blittable, no managed
//! delegates in ABI structs), correct Array<T> representations, and
//! PascalCase naming per D-35.

use crate::data::{ConstInfo, EnumInfo, FunctionInfo, StructInfo, UnionInfo};
use crate::languages::{CodeGenerator, GenerationContext};

/// C# ABI code generator.
pub struct CSharpGenerator;

impl CSharpGenerator {
    pub fn new() -> Self {
        CSharpGenerator
    }

    /// Check if a rust_type string represents a function pointer.
    fn is_function_pointer(rust_type: &str) -> bool {
        let type_str = Self::strip_option(rust_type);
        type_str.contains("extern\"C\"fn") || type_str.contains("extern\"C\"")
    }

    /// Strip `Option<...>` wrapper if present.
    fn strip_option(rust_type: &str) -> &str {
        if let Some(inner) = rust_type.strip_prefix("Option<") {
            if let Some(stripped) = inner.strip_suffix('>') {
                return stripped;
            }
        }
        rust_type
    }

    /// Check if a rust_type is Option<...>.
    fn is_option(rust_type: &str) -> bool {
        rust_type.starts_with("Option<") && rust_type.ends_with('>')
    }

    /// Check if a rust_type represents Array<T>.
    fn is_array(rust_type: &str) -> bool {
        rust_type.starts_with("Array<")
    }

    fn rust_type_to_csharp(rust_type: &str) -> String {
        // Handle Option<...> wrapper.
        if Self::is_option(rust_type) {
            let inner = &rust_type["Option<".len()..rust_type.len() - 1];
            if Self::is_function_pointer(rust_type) {
                return Self::rust_type_to_csharp(inner);
            }
            return Self::rust_type_to_csharp(inner);
        }

        // Handle Array<T> — return placeholder; actual handling in generate_struct.
        if Self::is_array(rust_type) {
            return String::from("IntPtr");
        }

        // Function pointers as raw types resolve to IntPtr (delegate handled at struct level).
        if rust_type.contains("extern\"C\"fn") || rust_type.contains("extern\"C\"") {
            return String::from("IntPtr");
        }

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

        // Strip Rust module paths (e.g., "crate::host::HostContractInstance" -> "HostContractInstance").
        if let Some(short) = rust_type.rsplit("::").next() {
            // Only strip if it actually had a :: separator (avoid stripping single-word types).
            if rust_type.contains("::") {
                return Self::rust_type_to_csharp(short);
            }
        }

        match rust_type {
            // `#[repr(transparent)]` u64 newtypes from polyplug_utils.
            "u64" | "BundleId" | "GuestContractId" | "HostContractId" => String::from("ulong"),
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
        let mut output = String::new();

        if let Some(doc) = &item.doc {
            output.push_str(&Self::format_xml_doc(doc));
            output.push('\n');
        }

        // Use Size attribute if size hint is known.
        if let Some(size) = item.size_hint {
            output.push_str(&format!(
                "[StructLayout(LayoutKind.Sequential, Size = {})]\n",
                size
            ));
        } else {
            output.push_str("[StructLayout(LayoutKind.Sequential)]\n");
        }
        output.push_str(&format!("public struct {}\n", item.name));
        output.push_str("{\n");

        for field in &item.fields {
            if let Some(doc) = &field.doc {
                output.push_str(&Self::format_indented_xml_doc(doc, "    "));
                output.push('\n');
            }

            // Handle Array<T> — expand into 3 sub-fields per D-21.
            if Self::is_array(&field.rust_type) {
                let field_name = Self::to_pascal_case(&field.name);
                output.push_str(&format!("    public IntPtr {};\n", field_name));
                output.push_str(&format!("    public nuint {}Len;\n", field_name));
                output.push_str(&format!("    public nuint {}Align;\n", field_name));
                continue;
            }

            // Handle function pointer fields — emit IntPtr (blittable, no managed
            // delegate in the ABI struct so unions stay overlappable in .NET).
            if Self::is_function_pointer(&field.rust_type) {
                let field_name = Self::to_pascal_case(&field.name);
                output.push_str(&format!("    public IntPtr {};\n", field_name));
                continue;
            }

            let csharp_type = Self::rust_type_to_csharp(&field.rust_type);
            let field_name = Self::to_pascal_case(&field.name);
            output.push_str(&format!("    public {} {};\n", csharp_type, field_name));
        }

        output.push_str("}\n");

        // Emit size documentation comment if known (actual validation is in LayoutTests.cs).
        if let Some(size) = item.size_hint {
            output.push_str(&format!("\n/// Expected size: {} bytes\n", size));
        }

        output.push('\n');
        output
    }

    fn generate_enum(&self, item: &EnumInfo, _ctx: &GenerationContext) -> String {
        let mut output = String::new();

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
        let mut output = String::new();

        if let Some(doc) = &item.doc {
            output.push_str(&Self::format_xml_doc(doc));
            output.push('\n');
        }

        output.push_str("[StructLayout(LayoutKind.Explicit)]\n");
        output.push_str(&format!("public struct {}\n", item.name));
        output.push_str("{\n");

        for variant in &item.variants {
            let csharp_type = Self::rust_type_to_csharp(&variant.type_name);
            let variant_name = Self::to_pascal_case(&variant.name);
            output.push_str("    [FieldOffset(0)]\n");
            output.push_str(&format!("    public {} {};\n", csharp_type, variant_name));
        }

        output.push_str("}\n\n");
        output
    }

    fn generate_function(&self, _item: &FunctionInfo, _ctx: &GenerationContext) -> String {
        // C# ABI bindings don't include functions - only structs, enums, and constants.
        String::new()
    }

    fn file_extension(&self) -> &'static str {
        "cs"
    }

    fn language_name(&self) -> &'static str {
        "csharp"
    }

    fn generate_header(&self, _ctx: &GenerationContext) -> String {
        "using System.Runtime.InteropServices;\nusing System.Text;\n\nnamespace Polyplug.Abi {\n\n"
            .to_string()
    }

    fn generate_footer(&self, _ctx: &GenerationContext) -> String {
        r#"
/// ABI constants for polyplug.
public static class AbiConstants
{
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{FieldInfo, StructInfo};

    /// Test that struct fn ptr fields are emitted as blittable IntPtr, with no
    /// managed delegate definitions (so ABI unions stay overlappable in .NET).
    #[test]
    fn csharp_struct_with_fn_ptr_emits_intptr() {
        let generator = CSharpGenerator::new();
        let ctx = GenerationContext::new();
        let item = StructInfo {
            name: String::from("TestStruct"),
            fields: vec![
                FieldInfo {
                    name: String::from("callback"),
                    rust_type: String::from("unsafeextern\"C\"fn(*constu8,usize)->u32"),
                    doc: None,
                },
                FieldInfo {
                    name: String::from("value"),
                    rust_type: String::from("u32"),
                    doc: None,
                },
            ],
            doc: None,
            attributes: vec![],
            size_hint: None,
        };

        let output = generator.generate_struct(&item, &ctx);
        assert!(
            output.contains("public IntPtr Callback;"),
            "fn ptr field should be IntPtr: {}",
            output
        );
        assert!(
            !output.contains("delegate"),
            "should not emit any managed delegate: {}",
            output
        );
        assert!(
            output.contains("public uint Value;"),
            "non-pointer field should keep its mapped type: {}",
            output
        );
    }

    /// Test that Array<T> fields expand into 3 sub-fields with PascalCase.
    #[test]
    fn csharp_array_field_expands() {
        let generator = CSharpGenerator::new();
        let ctx = GenerationContext::new();
        let item = StructInfo {
            name: String::from("WithArray"),
            fields: vec![FieldInfo {
                name: String::from("data"),
                rust_type: String::from("Array<u8>"),
                doc: None,
            }],
            doc: None,
            attributes: vec![],
            size_hint: None,
        };

        let output = generator.generate_struct(&item, &ctx);
        assert!(
            output.contains("public IntPtr Data;"),
            "Array items should be IntPtr with PascalCase: {}",
            output
        );
        assert!(
            output.contains("public nuint DataLen;"),
            "Array should have Len field: {}",
            output
        );
        assert!(
            output.contains("public nuint DataAlign;"),
            "Array should have Align field: {}",
            output
        );
    }
}

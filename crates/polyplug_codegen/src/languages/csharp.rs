//! C# code generator — produces C# bindings from ABI items.
//!
//! Generates typed delegate definitions for function pointer fields,
//! correct Array<T> representations, and PascalCase naming per D-35.

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
            if inner.ends_with('>') {
                return &inner[..inner.len() - 1];
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

    /// Parse a function pointer type string and return (return_type, param_types).
    fn parse_function_pointer(type_name: &str) -> Option<(String, Vec<String>)> {
        let type_str = Self::strip_option(type_name);

        let fn_start = type_str.find("fn(")?;
        let params_start = fn_start + 3;

        let mut depth = 1i32;
        let mut params_end = params_start;
        for (i, c) in type_str[params_start..].chars().enumerate() {
            match c {
                '(' | '<' | '[' => depth += 1,
                ')' | '>' | ']' => {
                    depth -= 1;
                    if depth == 0 {
                        params_end = params_start + i;
                        break;
                    }
                }
                _ => {}
            }
        }

        let params_str = &type_str[params_start..params_end];
        let return_type = if type_str.len() > params_end + 1 {
            let after = &type_str[params_end + 1..];
            let trimmed = after.trim_start_matches('-').trim_start_matches('>').trim();
            if trimmed.is_empty() {
                "void".to_string()
            } else {
                Self::rust_type_to_csharp(trimmed)
            }
        } else {
            "void".to_string()
        };

        let mut params = Vec::new();
        let mut current = String::new();
        let mut pdepth = 0i32;
        for c in params_str.chars() {
            match c {
                '(' | '<' | '[' => {
                    pdepth += 1;
                    current.push(c);
                }
                ')' | '>' | ']' => {
                    pdepth -= 1;
                    current.push(c);
                }
                ',' if pdepth == 0 => {
                    let p = current.trim();
                    if !p.is_empty() {
                        params.push(Self::rust_type_to_csharp(p));
                    }
                    current.clear();
                }
                _ => {
                    current.push(c);
                }
            }
        }
        if !current.trim().is_empty() {
            params.push(Self::rust_type_to_csharp(current.trim()));
        }

        Some((return_type, params))
    }

    /// Generate a delegate definition for a function pointer field.
    ///
    /// Returns (delegate_definition, delegate_type_name).
    fn generate_delegate(struct_name: &str, field_name: &str, rust_type: &str) -> (String, String) {
        let (return_type, params) = Self::parse_function_pointer(rust_type)
            .unwrap_or_else(|| ("void".to_string(), Vec::new()));

        let delegate_name = format!(
            "{}{}Delegate",
            Self::to_pascal_case(struct_name),
            Self::to_pascal_case(field_name)
        );

        let param_list: String = params
            .iter()
            .enumerate()
            .map(|(i, t)| format!("{} arg{}", t, i))
            .collect::<Vec<_>>()
            .join(", ");

        let mut definition = format!(
            "[UnmanagedFunctionPointer(CallingConvention.Cdecl)]\npublic delegate {} {}({});\n\n",
            return_type, delegate_name, param_list
        );

        if Self::is_option(rust_type) {
            definition.push_str("// Nullable delegate (reference type, can be null).\n");
        }

        (definition, delegate_name)
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
        let mut output = String::new();
        let mut delegates = String::new();

        // Pre-scan fields for function pointer types — collect delegate definitions.
        for field in &item.fields {
            if Self::is_function_pointer(&field.rust_type) {
                let (delegate_def, _type_name) =
                    Self::generate_delegate(&item.name, &field.name, &field.rust_type);
                delegates.push_str(&delegate_def);
            }
        }

        if let Some(doc) = &item.doc {
            output.push_str(&Self::format_xml_doc(doc));
            output.push('\n');
        }

        // Emit delegate definitions before the struct.
        output.push_str(&delegates);

        output.push_str("[StructLayout(LayoutKind.Sequential)]\n");
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

            // Handle function pointer fields — use the delegate type.
            if Self::is_function_pointer(&field.rust_type) {
                let (_, delegate_type) =
                    Self::generate_delegate(&item.name, &field.name, &field.rust_type);
                let field_name = Self::to_pascal_case(&field.name);
                output.push_str(&format!("    public {} {};\n", delegate_type, field_name));
                continue;
            }

            let csharp_type = Self::rust_type_to_csharp(&field.rust_type);
            let field_name = Self::to_pascal_case(&field.name);
            output.push_str(&format!("    public {} {};\n", csharp_type, field_name));
        }

        output.push_str("}\n\n");
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
        "using System.Runtime.InteropServices;\n\nnamespace Polyplug.Abi {\n\n".to_string()
    }

    fn generate_footer(&self, _ctx: &GenerationContext) -> String {
        r#"
/// ABI error codes — returned by all ABI functions.
public enum AbiErrorCode : uint
{
    Ok = 0,
    Generic = 1,
    BufferTooSmall = 2,
    Panic = 3,
    NotFound = 4,
    StaleHandle = 5,
    FunctionNotAvailable = 6,
    DuplicateProvider = 7,
    InvalidPointer = 8,
    HostContractNotFound = 100,
    HostContractVersionMismatch = 101,
    HostContractCallFailed = 102,
}

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

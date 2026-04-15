//! C++ code generator — produces C++ headers from ABI items.
//!
//! Generates typed function pointer typedefs, correct Array<T> representations,
//! and snake_case naming in the `polyplug` namespace per D-35.

use crate::data::{ConstInfo, EnumInfo, FunctionInfo, StructInfo, UnionInfo};
use crate::languages::{CodeGenerator, GenerationContext};

/// C++ ABI code generator.
pub struct CppGenerator;

impl CppGenerator {
    pub fn new() -> Self {
        CppGenerator
    }

    /// Check if a rust_type represents Array<T>.
    fn is_array(rust_type: &str) -> bool {
        rust_type.starts_with("Array<")
    }

    /// Check if a rust_type is Option<...>.
    fn is_option(rust_type: &str) -> bool {
        rust_type.starts_with("Option<") && rust_type.ends_with('>')
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

    fn rust_type_to_cpp(rust_type: &str) -> String {
        // Handle Option<...> wrapper.
        if Self::is_option(rust_type) {
            let inner = &rust_type["Option<".len()..rust_type.len() - 1];
            return Self::rust_type_to_cpp(inner);
        }

        // Handle Array<T> — returns void* for items; actual handling in generate_struct.
        if Self::is_array(rust_type) {
            return String::from("void*");
        }

        if rust_type.contains("extern\"C\"fn") || rust_type.contains("extern\"C\"") {
            return Self::convert_function_pointer(rust_type);
        }

        if rust_type.starts_with("*const*const") {
            return String::from("void* const*");
        }
        if rust_type.starts_with("*mut*const") {
            return String::from("void* const*");
        }
        if rust_type.starts_with("*const*mut") {
            return String::from("void**");
        }
        if rust_type.starts_with("*mut*mut") {
            return String::from("void**");
        }

        if rust_type.starts_with('*') {
            let rest = rust_type.trim_start_matches('*').trim();
            if rest.starts_with("const") {
                let inner = rest.trim_start_matches("const").trim();
                let cpp_inner = Self::rust_type_to_cpp(inner);
                return format!("const {}*", cpp_inner);
            }
            if rest.starts_with("mut") {
                let inner = rest.trim_start_matches("mut").trim();
                let cpp_inner = Self::rust_type_to_cpp(inner);
                return format!("{}*", cpp_inner);
            }
            return String::from("void*");
        }

        if rust_type.contains("c_void") {
            return String::from("void");
        }

        // Strip Rust module paths (e.g., "crate::host::HostContractInstance" -> "HostContractInstance").
        if let Some(short) = rust_type.rsplit("::").next() {
            // Only strip if it actually had a :: separator (avoid stripping single-word types).
            if rust_type.contains("::") {
                return Self::rust_type_to_cpp(short);
            }
        }

        match rust_type {
            "u64" => String::from("uint64_t"),
            "u32" => String::from("uint32_t"),
            "u16" => String::from("uint16_t"),
            "u8" => String::from("uint8_t"),
            "i64" => String::from("int64_t"),
            "i32" => String::from("int32_t"),
            "i16" => String::from("int16_t"),
            "i8" => String::from("int8_t"),
            "usize" => String::from("size_t"),
            "isize" => String::from("ptrdiff_t"),
            "bool" => String::from("bool"),
            "()" => String::from("void"),
            "c_char" => String::from("char"),
            "T" => String::from("void"), // Generic placeholder — used as void* for opaque pointers
            other => String::from(other),
        }
    }

    fn convert_function_pointer(type_name: &str) -> String {
        let type_str = Self::strip_option(type_name);

        let fn_start = type_str.find("fn(").unwrap_or(0);
        let params_start = fn_start + 3;

        // Find the matching closing paren for the fn parameter list.
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

        let cpp_return = if type_str.len() > params_end + 1 {
            let after = &type_str[params_end + 1..];
            let trimmed = after.trim_start_matches('-').trim_start_matches('>').trim();
            if trimmed.is_empty() {
                String::from("void")
            } else {
                Self::rust_type_to_cpp(trimmed)
            }
        } else {
            String::from("void")
        };

        if params_start == 0 || params_end <= params_start {
            return format!("{}(*)()", cpp_return);
        }

        let params_str = &type_str[params_start..params_end];
        let params = Self::parse_function_params(params_str);

        if params.is_empty() {
            return format!("{}(*)()", cpp_return);
        }

        format!("{}(*)({})", cpp_return, params.join(", "))
    }

    fn parse_function_params(params_str: &str) -> Vec<String> {
        if params_str.is_empty() {
            return Vec::new();
        }

        let mut params = Vec::new();
        let mut current_param = String::new();
        let mut depth = 0i32;

        for c in params_str.chars() {
            match c {
                '(' | '<' | '[' => {
                    depth += 1;
                    current_param.push(c);
                }
                ')' | '>' | ']' => {
                    depth -= 1;
                    current_param.push(c);
                }
                ',' if depth == 0 => {
                    let param = current_param.trim().to_string();
                    if !param.is_empty() {
                        params.push(Self::convert_param(&param));
                    }
                    current_param.clear();
                }
                _ => {
                    current_param.push(c);
                }
            }
        }

        if !current_param.trim().is_empty() {
            params.push(Self::convert_param(current_param.trim()));
        }

        params
    }

    fn convert_param(param: &str) -> String {
        let parts: Vec<&str> = param.splitn(2, ':').collect();
        let type_part = if parts.len() == 2 { parts[1] } else { parts[0] };
        Self::rust_type_to_cpp(type_part.trim())
    }

    fn format_constant_value(value: &str, type_name: &str) -> String {
        match type_name {
            "u64" => format!("{}ULL", value),
            "u32" => format!("{}U", value),
            "i64" => format!("{}LL", value),
            _ => String::from(value),
        }
    }

    fn format_doc_comment(doc: &str, indent: usize) -> String {
        let indent_str = " ".repeat(indent);
        let lines: Vec<&str> = doc.lines().collect();
        if lines.len() == 1 {
            format!("{}/// {}\n", indent_str, lines[0])
        } else {
            let mut result = format!("{}/// {}\n", indent_str, lines[0]);
            for line in &lines[1..] {
                if line.is_empty() {
                    result.push_str(&format!("{}///\n", indent_str));
                } else {
                    result.push_str(&format!("{}/// {}\n", indent_str, line));
                }
            }
            result
        }
    }

    /// Generate a typedef for a function pointer type.
    ///
    /// Returns (typedef_line, type_name_to_use_in_struct).
    fn generate_fn_ptr_typedef(
        struct_name: &str,
        field_name: &str,
        rust_type: &str,
    ) -> (String, String) {
        let fn_type = Self::convert_function_pointer(rust_type);
        let typedef_name = format!("{}_{}_fn", struct_name, field_name);

        let typedef = format!("typedef {} {};\n", fn_type, typedef_name);

        let mut extra = String::new();
        if Self::is_option(rust_type) {
            extra.push_str("// Nullable function pointer.\n");
        }

        (format!("{}{}", typedef, extra), typedef_name)
    }
}

impl CodeGenerator for CppGenerator {
    fn generate_const(&self, item: &ConstInfo, _ctx: &GenerationContext) -> String {
        let value = Self::format_constant_value(&item.value, &item.rust_type);
        format!("#define {} {}\n", item.name, value)
    }

    fn generate_struct(&self, item: &StructInfo, _ctx: &GenerationContext) -> String {
        let mut output = String::new();
        let mut typedefs = String::new();

        // Pre-scan fields for function pointer types — collect typedefs.
        for field in &item.fields {
            if field.rust_type.contains("extern\"C\"fn") || field.rust_type.contains("extern\"C\"")
            {
                let (typedef, _type_name) =
                    Self::generate_fn_ptr_typedef(&item.name, &field.name, &field.rust_type);
                typedefs.push_str(&typedef);
            }
        }

        if let Some(doc) = &item.doc {
            output.push_str(&Self::format_doc_comment(doc, 0));
        }

        // Emit typedefs before the struct.
        output.push_str(&typedefs);

        output.push_str("struct ");
        output.push_str(&item.name);
        output.push_str(" {\n");

        for field in &item.fields {
            if let Some(doc) = &field.doc {
                output.push_str(&Self::format_doc_comment(doc, 4));
            }

            // Handle Array<T> — expand into 3 sub-fields per D-21.
            if Self::is_array(&field.rust_type) {
                output.push_str(&format!("    void* {};\n", field.name));
                output.push_str(&format!("    size_t {}_len;\n", field.name));
                output.push_str(&format!("    size_t {}__align;\n", field.name));
                continue;
            }

            // Handle function pointer fields — use the typedef name.
            if field.rust_type.contains("extern\"C\"fn") || field.rust_type.contains("extern\"C\"")
            {
                let (_, typedef_name) =
                    Self::generate_fn_ptr_typedef(&item.name, &field.name, &field.rust_type);
                output.push_str(&format!("    {} {};\n", typedef_name, field.name));
                continue;
            }

            let cpp_type = Self::rust_type_to_cpp(&field.rust_type);
            output.push_str(&format!("    {} {};\n", cpp_type, field.name));
        }

        output.push_str("};\n");

        // Emit static_assert for size validation if known.
        if let Some(size) = item.size_hint {
            output.push_str(&format!(
                "static_assert(sizeof({}) == {}, \"{} size mismatch\");\n\n",
                item.name, size, item.name
            ));
        } else {
            output.push('\n');
        }

        output
    }

    fn generate_enum(&self, item: &EnumInfo, _ctx: &GenerationContext) -> String {
        let mut output = String::new();

        if let Some(doc) = &item.doc {
            output.push_str(&Self::format_doc_comment(doc, 0));
        }

        let repr = Self::rust_type_to_cpp(&item.repr);
        output.push_str(&format!("enum class {} : {} {{\n", item.name, repr));
        for (i, variant) in item.variants.iter().enumerate() {
            if let Some(doc) = &variant.doc {
                output.push_str(&Self::format_doc_comment(doc, 4));
            }

            if let Some(value) = variant.value {
                output.push_str(&format!("    {} = {},\n", variant.name, value));
            } else if i == 0 {
                output.push_str(&format!("    {} = 0,\n", variant.name));
            } else {
                output.push_str(&format!("    {},\n", variant.name));
            }
        }
        output.push_str("};\n\n");
        output
    }

    fn generate_union(&self, item: &UnionInfo, _ctx: &GenerationContext) -> String {
        let mut output = String::new();

        if let Some(doc) = &item.doc {
            output.push_str(&Self::format_doc_comment(doc, 0));
        }

        output.push_str(&format!("union {} {{\n", item.name));
        for variant in &item.variants {
            let cpp_type = Self::rust_type_to_cpp(&variant.type_name);
            output.push_str(&format!("    {} {};\n", cpp_type, variant.name));
        }
        output.push_str("};\n\n");
        output
    }

    fn generate_function(&self, item: &FunctionInfo, _ctx: &GenerationContext) -> String {
        let ret_type = item
            .return_type
            .as_ref()
            .map(|t| Self::rust_type_to_cpp(t))
            .unwrap_or_else(|| "void".to_string());

        let params = item
            .params
            .iter()
            .map(|p| format!("{} {}", Self::rust_type_to_cpp(&p.rust_type), p.name))
            .collect::<Vec<_>>()
            .join(", ");

        if item.is_constexpr {
            format!(
                "constexpr {} {}({}) {{ /* implementation */ }}\n\n",
                ret_type, item.name, params
            )
        } else {
            format!("{} {}({});\n\n", ret_type, item.name, params)
        }
    }

    fn file_extension(&self) -> &'static str {
        "hpp"
    }

    fn language_name(&self) -> &'static str {
        "cpp"
    }

    fn generate_header(&self, _ctx: &GenerationContext) -> String {
        "#pragma once\n#include <cstdint>\n#include <cstddef>\n\n".to_string()
    }
}

impl Default for CppGenerator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{FieldInfo, StructInfo};

    /// Test that fn ptr typedefs produce valid C++ types.
    #[test]
    fn cpp_fn_ptr_typedef_uses_c_types() {
        let (typedef, _type_name) = CppGenerator::generate_fn_ptr_typedef(
            "TestStruct",
            "callback",
            "unsafeextern\"C\"fn(ptr:*constu8,len:usize)->u32",
        );
        assert!(
            typedef.contains("uint32_t"),
            "u32 return type should be uint32_t: {}",
            typedef
        );
        assert!(
            typedef.contains("const uint8_t*"),
            "*const u8 should produce const uint8_t*: {}",
            typedef
        );
        assert!(
            typedef.contains("size_t"),
            "usize should produce size_t: {}",
            typedef
        );
    }

    /// Test that Array<T> fields expand into 3 sub-fields.
    #[test]
    fn cpp_array_field_expands() {
        let generator = CppGenerator::new();
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
            output.contains("void* data;"),
            "Array items should be void*: {}",
            output
        );
        assert!(
            output.contains("size_t data_len;"),
            "Array should have len field: {}",
            output
        );
        assert!(
            output.contains("size_t data__align;"),
            "Array should have align field: {}",
            output
        );
    }

    /// Test that void-returning fn ptrs produce correct typedefs.
    #[test]
    fn cpp_fn_ptr_void_return_correct() {
        let (typedef, _type_name) = CppGenerator::generate_fn_ptr_typedef(
            "Test",
            "destroy",
            "unsafeextern\"C\"fn(ptr:*mutu8)->()",
        );
        assert!(
            typedef.contains("void(*)"),
            "void return should produce void(*): {}",
            typedef
        );
        assert!(
            !typedef.contains("()"),
            "return type () should not appear in typedef: {}",
            typedef
        );
    }
}

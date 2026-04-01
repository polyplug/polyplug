//! C++ code generator — produces C++ headers from ABI items.

use crate::data::{ConstInfo, EnumInfo, FunctionInfo, StructInfo, UnionInfo};
use crate::languages::{CodeGenerator, GenerationContext};

/// C++ ABI code generator.
pub struct CppGenerator;

impl CppGenerator {
    pub fn new() -> Self {
        CppGenerator
    }

    fn rust_type_to_cpp(rust_type: &str) -> String {
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
            let rest: &str = rust_type.trim_start_matches('*').trim();
            if rest.starts_with("const") {
                let inner: &str = rest.trim_start_matches("const").trim();
                let cpp_inner: String = Self::rust_type_to_cpp(inner);
                return format!("const {}*", cpp_inner);
            }
            if rest.starts_with("mut") {
                let inner: &str = rest.trim_start_matches("mut").trim();
                let cpp_inner: String = Self::rust_type_to_cpp(inner);
                return format!("{}*", cpp_inner);
            }
            return String::from("void*");
        }

        if rust_type.contains("c_void") {
            return String::from("void");
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
            other => String::from(other),
        }
    }

    fn convert_function_pointer(type_name: &str) -> String {
        let return_type: &str = if let Some(pos) = type_name.find(")->") {
            &type_name[pos + 3..]
        } else {
            "void"
        };

        let cpp_return: String = Self::rust_type_to_cpp(return_type);

        let params_start: usize = type_name.find("fn(").map(|p| p + 3).unwrap_or(0);
        let params_end: usize = type_name.find(")->").unwrap_or(type_name.len());

        if params_start == 0 || params_end <= params_start {
            return format!("{}(*)()", cpp_return);
        }

        let params_str: &str = &type_name[params_start..params_end];
        let params: Vec<String> = Self::parse_function_params(params_str);

        if params.is_empty() {
            return format!("{}(*)()", cpp_return);
        }

        format!("{}(*)({})", cpp_return, params.join(", "))
    }

    fn parse_function_params(params_str: &str) -> Vec<String> {
        if params_str.is_empty() {
            return Vec::new();
        }

        let mut params: Vec<String> = Vec::new();
        let mut current_param: String = String::new();
        let mut depth: i32 = 0;

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
                    let param: String = current_param.trim().to_string();
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
        let type_part: &str = if parts.len() == 2 { parts[1] } else { parts[0] };
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
        let indent_str: String = " ".repeat(indent);
        let lines: Vec<&str> = doc.lines().collect();
        if lines.len() == 1 {
            format!("{}/// {}\n", indent_str, lines[0])
        } else {
            let mut result: String = format!("{}/// {}\n", indent_str, lines[0]);
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

    fn generate_field_declaration(type_name: &str, field_name: &str) -> String {
        if type_name.contains("extern\"C\"fn") || type_name.contains("extern\"C\"") {
            return Self::generate_function_pointer_field(type_name, field_name);
        }

        let cpp_type: String = Self::rust_type_to_cpp(type_name);
        format!("{} {}", cpp_type, field_name)
    }

    fn generate_function_pointer_field(type_name: &str, field_name: &str) -> String {
        let return_type: &str = if let Some(pos) = type_name.find(")->") {
            &type_name[pos + 3..]
        } else {
            "void"
        };

        let cpp_return: String = Self::rust_type_to_cpp(return_type);

        let params_start: usize = type_name.find("fn(").map(|p| p + 3).unwrap_or(0);

        let params_end: usize = if let Some(pos) = type_name.find(")->") {
            pos
        } else if params_start > 0 {
            let mut depth: i32 = 1;
            let mut end: usize = params_start;
            for (i, c) in type_name[params_start..].chars().enumerate() {
                match c {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            end = params_start + i;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            end
        } else {
            0
        };

        if params_start == 0 || params_end <= params_start {
            return format!("{} (*{} )()", cpp_return, field_name);
        }

        let params_str: &str = &type_name[params_start..params_end];
        let params: Vec<String> = Self::parse_function_params(params_str);

        if params.is_empty() {
            return format!("{} (*{} )()", cpp_return, field_name);
        }

        format!("{} (*{} )({})", cpp_return, field_name, params.join(", "))
    }
}

impl CodeGenerator for CppGenerator {
    fn generate_const(&self, item: &ConstInfo, _ctx: &GenerationContext) -> String {
        let value: String = Self::format_constant_value(&item.value, &item.rust_type);
        format!("#define {} {}\n", item.name, value)
    }

    fn generate_struct(&self, item: &StructInfo, _ctx: &GenerationContext) -> String {
        let mut output: String = String::new();

        if let Some(doc) = &item.doc {
            output.push_str(&Self::format_doc_comment(doc, 0));
        }

        output.push_str("struct ");
        output.push_str(&item.name);
        output.push_str(" {\n");

        for field in &item.fields {
            if let Some(doc) = &field.doc {
                output.push_str(&Self::format_doc_comment(doc, 4));
            }

            let field_decl: String =
                Self::generate_field_declaration(&field.rust_type, &field.name);
            output.push_str(&format!("    {};\n", field_decl));
        }

        output.push_str("};\n\n");
        output
    }

    fn generate_enum(&self, item: &EnumInfo, _ctx: &GenerationContext) -> String {
        let mut output: String = String::new();

        if let Some(doc) = &item.doc {
            output.push_str(&Self::format_doc_comment(doc, 0));
        }

        let repr: String = Self::rust_type_to_cpp(&item.repr);
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
        let mut output: String = String::new();

        if let Some(doc) = &item.doc {
            output.push_str(&Self::format_doc_comment(doc, 0));
        }

        output.push_str(&format!("union {} {{\n", item.name));
        for variant in &item.variants {
            let cpp_type: String = Self::rust_type_to_cpp(&variant.type_name);
            output.push_str(&format!("    {} {};\n", cpp_type, variant.name));
        }
        output.push_str("};\n\n");
        output
    }

    fn generate_function(&self, item: &FunctionInfo, _ctx: &GenerationContext) -> String {
        let ret_type: String = item
            .return_type
            .as_ref()
            .map(|t| Self::rust_type_to_cpp(t))
            .unwrap_or_else(|| "void".to_string());

        let params: String = item
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

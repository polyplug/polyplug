//! Lua code generator — produces LuaJIT FFI bindings from ABI items.

use crate::data::{ConstInfo, EnumInfo, FunctionInfo, StructInfo, UnionInfo};
use crate::languages::{CodeGenerator, GenerationContext};

/// Lua/LuaJIT ABI code generator.
pub struct LuaGenerator;

impl LuaGenerator {
    pub fn new() -> Self {
        LuaGenerator
    }

    fn rust_type_to_lua(rust_type: &str) -> String {
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
                if inner.contains("c_void") {
                    return String::from("const void*");
                }
                let lua_inner: String = Self::rust_type_to_lua(inner);
                return format!("const {}*", lua_inner);
            }
            if rest.starts_with("mut") {
                let inner: &str = rest.trim_start_matches("mut").trim();
                if inner.contains("c_void") {
                    return String::from("void*");
                }
                let lua_inner: String = Self::rust_type_to_lua(inner);
                return format!("{}*", lua_inner);
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
            "bool" => String::from("uint8_t"),
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

        let lua_return: String = Self::rust_type_to_lua(return_type);

        let params_start: usize = type_name.find("fn(").map(|p| p + 3).unwrap_or(0);
        let params_end: usize = type_name.find(")->").unwrap_or(type_name.len());

        if params_start == 0 || params_end <= params_start {
            return format!("{}(*)()", lua_return);
        }

        let params_str: &str = &type_name[params_start..params_end];
        let params: Vec<String> = Self::parse_function_params(params_str);

        if params.is_empty() {
            return format!("{}(*)()", lua_return);
        }

        format!("{}(*)({})", lua_return, params.join(", "))
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
        Self::rust_type_to_lua(type_part.trim())
    }

    fn format_c_comment(doc: &str, indent: usize) -> String {
        let indent_str: String = " ".repeat(indent);
        doc.lines()
            .map(|line: &str| format!("{}// {}\n", indent_str, line))
            .collect::<Vec<String>>()
            .join("")
    }
}

impl CodeGenerator for LuaGenerator {
    fn generate_const(&self, item: &ConstInfo, _ctx: &GenerationContext) -> String {
        let c_type: &str = match item.rust_type.as_str() {
            "u64" => "uint64_t",
            "u32" => "uint32_t",
            "i64" => "int64_t",
            "i32" => "int32_t",
            _ => &item.rust_type,
        };
        format!(
            "M.{} = ffi.cast(\"{}\", {})\n",
            item.name, c_type, item.value
        )
    }

    fn generate_struct(&self, item: &StructInfo, _ctx: &GenerationContext) -> String {
        let mut output: String = String::new();

        if let Some(doc) = &item.doc {
            output.push_str(&Self::format_c_comment(doc, 4));
        }

        output.push_str(&format!("    typedef struct {} {{\n", item.name));

        for field in &item.fields {
            if let Some(doc) = &field.doc {
                output.push_str(&Self::format_c_comment(doc, 8));
            }

            let lua_type: String = Self::rust_type_to_lua(&field.rust_type);
            output.push_str(&format!("        {} {};\n", lua_type, field.name));
        }

        output.push_str("    } ");
        output.push_str(&item.name);
        output.push_str(";\n\n");
        output
    }

    fn generate_enum(&self, item: &EnumInfo, _ctx: &GenerationContext) -> String {
        let mut output: String = String::new();

        if let Some(doc) = &item.doc {
            output.push_str(&Self::format_c_comment(doc, 4));
        }

        output.push_str(&format!("    typedef enum {} {{\n", item.name));

        for (i, variant) in item.variants.iter().enumerate() {
            if let Some(doc) = &variant.doc {
                output.push_str(&Self::format_c_comment(doc, 8));
            }

            if let Some(value) = variant.value {
                output.push_str(&format!(
                    "        {}_{} = {},\n",
                    item.name, variant.name, value
                ));
            } else if i == 0 {
                output.push_str(&format!("        {}_{} = 0,\n", item.name, variant.name));
            } else {
                output.push_str(&format!("        {}_{},\n", item.name, variant.name));
            }
        }

        output.push_str(&format!("    }} {};\n\n", item.name));
        output
    }

    fn generate_union(&self, item: &UnionInfo, _ctx: &GenerationContext) -> String {
        let mut output: String = String::new();

        if let Some(doc) = &item.doc {
            output.push_str(&Self::format_c_comment(doc, 4));
        }

        output.push_str(&format!("    typedef union {} {{\n", item.name));

        for variant in &item.variants {
            let lua_type: String = Self::rust_type_to_lua(&variant.type_name);
            output.push_str(&format!("        {} {};\n", lua_type, variant.name));
        }

        output.push_str(&format!("    }} {};\n\n", item.name));
        output
    }

    fn generate_function(&self, item: &FunctionInfo, _ctx: &GenerationContext) -> String {
        let _ret_type: String = item
            .return_type
            .as_ref()
            .map(|t| Self::rust_type_to_lua(t))
            .unwrap_or_else(|| "void".to_string());

        let params: String = item
            .params
            .iter()
            .map(|p| format!("{} {}", Self::rust_type_to_lua(&p.rust_type), p.name))
            .collect::<Vec<_>>()
            .join(", ");

        if params.is_empty() {
            format!("local function {}() end\n\n", item.name)
        } else {
            format!("local function {}({}) end\n\n", item.name, params)
        }
    }

    fn file_extension(&self) -> &'static str {
        "lua"
    }

    fn language_name(&self) -> &'static str {
        "lua"
    }

    fn generate_header(&self, _ctx: &GenerationContext) -> String {
        "local ffi = require(\"ffi\")\nlocal M = {}\n\n".to_string()
    }

    fn generate_footer(&self, _ctx: &GenerationContext) -> String {
        "return M\n".to_string()
    }
}

impl Default for LuaGenerator {
    fn default() -> Self {
        Self::new()
    }
}

//! Lua code generator — produces LuaJIT FFI bindings from ABI items.
//!
//! Generates typed function pointer typedefs in ffi.cdef, correct Array<T>
//! representations, and snake_case naming per D-35.

use crate::data::{ConstInfo, EnumInfo, FunctionInfo, StructInfo, UnionInfo};
use crate::languages::{CodeGenerator, GenerationContext};

/// Lua/LuaJIT ABI code generator.
pub struct LuaGenerator;

impl LuaGenerator {
    pub fn new() -> Self {
        LuaGenerator
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

    fn rust_type_to_lua(rust_type: &str) -> String {
        // Handle Option<...> wrapper — unwrap for type resolution.
        if Self::is_option(rust_type) {
            let inner = &rust_type["Option<".len()..rust_type.len() - 1];
            return Self::rust_type_to_lua(inner);
        }

        // Handle Array<T> — expand into 3 fields; this returns the items type.
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
                if inner.contains("c_void") {
                    return String::from("const void*");
                }
                let lua_inner = Self::rust_type_to_lua(inner);
                return format!("const {}*", lua_inner);
            }
            if rest.starts_with("mut") {
                let inner = rest.trim_start_matches("mut").trim();
                if inner.contains("c_void") {
                    return String::from("void*");
                }
                let lua_inner = Self::rust_type_to_lua(inner);
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
        let type_str = Self::strip_option(type_name);

        let return_type = if let Some(pos) = type_str.find(")->") {
            &type_str[pos + 3..]
        } else {
            "void"
        };

        let lua_return = Self::rust_type_to_lua(return_type);

        let params_start = type_str.find("fn(").map(|p| p + 3).unwrap_or(0);
        let params_end = type_str.find(")->").unwrap_or(type_str.len());

        if params_start == 0 || params_end <= params_start {
            return format!("{}(*)()", lua_return);
        }

        let params_str = &type_str[params_start..params_end];
        let params = Self::parse_function_params(params_str);

        if params.is_empty() {
            return format!("{}(*)()", lua_return);
        }

        format!("{}(*)({})", lua_return, params.join(", "))
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
        Self::rust_type_to_lua(type_part.trim())
    }

    fn format_c_comment(doc: &str, indent: usize) -> String {
        let indent_str = " ".repeat(indent);
        doc.lines()
            .map(|line: &str| format!("{}// {}\n", indent_str, line))
            .collect::<Vec<String>>()
            .join("")
    }

    /// Generate a typedef for a function pointer type.
    ///
    /// Returns (typedef_line, type_name_to_use_in_struct).
    fn generate_fn_ptr_typedef(struct_name: &str, field_name: &str, rust_type: &str) -> (String, String) {
        let fn_type = Self::convert_function_pointer(rust_type);
        let typedef_name = format!("{}_{}_fn", struct_name, field_name);

        let typedef = format!("    typedef {} {};\n", fn_type, typedef_name);

        (typedef, typedef_name)
    }
}

impl CodeGenerator for LuaGenerator {
    fn generate_const(&self, item: &ConstInfo, _ctx: &GenerationContext) -> String {
        let c_type = match item.rust_type.as_str() {
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
        let mut output = String::new();
        let mut typedefs = String::new();

        // Pre-scan fields for function pointer types — collect typedefs.
        for field in &item.fields {
            if Self::is_function_pointer(&field.rust_type) {
                let (typedef, _type_name) =
                    Self::generate_fn_ptr_typedef(&item.name, &field.name, &field.rust_type);
                typedefs.push_str(&typedef);
            }
        }

        if let Some(doc) = &item.doc {
            output.push_str(&Self::format_c_comment(doc, 4));
        }

        output.push_str(&format!("    typedef struct {} {{\n", item.name));

        for field in &item.fields {
            if let Some(doc) = &field.doc {
                output.push_str(&Self::format_c_comment(doc, 8));
            }

            // Handle Array<T> — expand into 3 sub-fields per D-21.
            if Self::is_array(&field.rust_type) {
                output.push_str(&format!("        void* {};\n", field.name));
                output.push_str(&format!("        size_t {}_len;\n", field.name));
                output.push_str(&format!("        size_t {}__align;\n", field.name));
                continue;
            }

            // Handle function pointer fields — use the typedef name.
            if Self::is_function_pointer(&field.rust_type) {
                let (_, typedef_name) =
                    Self::generate_fn_ptr_typedef(&item.name, &field.name, &field.rust_type);
                output.push_str(&format!("        {} {};\n", typedef_name, field.name));
                continue;
            }

            let lua_type = Self::rust_type_to_lua(&field.rust_type);
            output.push_str(&format!("        {} {};\n", lua_type, field.name));
        }

        output.push_str("    } ");
        output.push_str(&item.name);
        output.push_str(";\n\n");

        // Prepend typedefs before the struct.
        let mut result = typedefs;
        result.push_str(&output);
        result
    }

    fn generate_enum(&self, item: &EnumInfo, _ctx: &GenerationContext) -> String {
        let mut output = String::new();

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
        let mut output = String::new();

        if let Some(doc) = &item.doc {
            output.push_str(&Self::format_c_comment(doc, 4));
        }

        output.push_str(&format!("    typedef union {} {{\n", item.name));

        for variant in &item.variants {
            let lua_type = Self::rust_type_to_lua(&variant.type_name);
            output.push_str(&format!("        {} {};\n", lua_type, variant.name));
        }

        output.push_str(&format!("    }} {};\n\n", item.name));
        output
    }

    fn generate_function(&self, item: &FunctionInfo, _ctx: &GenerationContext) -> String {
        let _ret_type = item
            .return_type
            .as_ref()
            .map(|t| Self::rust_type_to_lua(t))
            .unwrap_or_else(|| "void".to_string());

        let params = item
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

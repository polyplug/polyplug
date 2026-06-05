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

    /// Return the named aggregate this field references *by value*, if any.
    ///
    /// LuaJIT can reference a forward-declared type by pointer, but a by-value
    /// field requires the referenced struct/union/enum to be fully defined
    /// first. Pointer fields, arrays, and function pointers impose no ordering
    /// constraint and yield `None`; primitives also yield `None`.
    pub fn value_dependency(rust_type: &str) -> Option<String> {
        let inner: &str = Self::strip_option(rust_type);

        if Self::is_array(inner) || inner.starts_with('*') || Self::is_function_pointer(inner) {
            return None;
        }

        let lua: String = Self::rust_type_to_lua(inner);
        // A by-value dependency is a named aggregate — anything that did not map
        // to a primitive (lowercase, e.g. `uint32_t`), pointer, or `void`.
        match lua.chars().next() {
            Some(c) if c.is_ascii_uppercase() => Some(lua),
            _ => None,
        }
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
            // Nested/anonymous function pointer (e.g. a fn-ptr parameter): emit
            // the unnamed C declarator `RET (*)(PARAMS)`.
            let (return_type, params): (String, String) = Self::convert_function_pointer(rust_type);
            return format!("{} (*)({})", return_type, params);
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

        // Strip Rust module paths (e.g., "crate::host::HostContractInstance" -> "HostContractInstance").
        if let Some(short) = rust_type.rsplit("::").next() {
            // Only strip if it actually had a :: separator (avoid stripping single-word types).
            if rust_type.contains("::") {
                return Self::rust_type_to_lua(short);
            }
        }

        match rust_type {
            // `#[repr(transparent)]` u64 newtypes from polyplug_utils.
            "u64" | "BundleId" | "GuestContractId" | "HostContractId" => String::from("uint64_t"),
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
            "c_char" => String::from("int8_t"),
            "T" => String::from("void"), // Generic placeholder — used as void* for opaque pointers
            other => String::from(other),
        }
    }

    /// Parse a function pointer rust_type into its C return type and the
    /// joined C parameter list (without surrounding parentheses).
    ///
    /// The declarator name is spliced in by the caller so the resulting
    /// typedef is valid C: `typedef RET (*NAME)(PARAMS);`.
    fn convert_function_pointer(type_name: &str) -> (String, String) {
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

        let lua_return: String = if type_str.len() > params_end + 1 {
            let after = &type_str[params_end + 1..];
            let trimmed = after.trim_start_matches('-').trim_start_matches('>').trim();
            if trimmed.is_empty() {
                String::from("void")
            } else {
                Self::rust_type_to_lua(trimmed)
            }
        } else {
            String::from("void")
        };

        if params_start == 0 || params_end <= params_start {
            return (lua_return, String::new());
        }

        let params_str = &type_str[params_start..params_end];
        let params: Vec<String> = Self::parse_function_params(params_str);

        (lua_return, params.join(", "))
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
        // Split on the `name: type` separator, which is the first single `:`.
        // Rust path separators (`::`) must not be treated as the separator, so
        // skip any `:` that is part of a `::` sequence.
        let bytes: &[u8] = param.as_bytes();
        let mut type_part: &str = param;
        let mut i: usize = 0;
        while i < bytes.len() {
            if bytes[i] == b':' {
                let prev_colon: bool = i > 0 && bytes[i - 1] == b':';
                let next_colon: bool = i + 1 < bytes.len() && bytes[i + 1] == b':';
                if !prev_colon && !next_colon {
                    type_part = &param[i + 1..];
                    break;
                }
            }
            i += 1;
        }
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
    fn generate_fn_ptr_typedef(
        struct_name: &str,
        field_name: &str,
        rust_type: &str,
    ) -> (String, String) {
        let (return_type, params): (String, String) = Self::convert_function_pointer(rust_type);
        let typedef_name: String = format!("{}_{}_fn", struct_name, field_name);

        // Valid C function-pointer typedef: the declarator name lives inside
        // the `(*name)` group, e.g. `typedef AbiError (*Foo_bar_fn)(int);`.
        let typedef: String = format!(
            "    typedef {} (*{})({});\n",
            return_type, typedef_name, params
        );

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
        output.push_str(";\n");

        // Emit size hint comment if known (C-style since inside ffi.cdef).
        if let Some(size) = item.size_hint {
            output.push_str(&format!("    // Expected size: {} bytes\n", size));
        }

        output.push('\n');

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
        "local ffi = require(\"ffi\")\nlocal M = {}\n\nffi.cdef[[\n".to_string()
    }

    fn generate_footer(&self, _ctx: &GenerationContext) -> String {
        // The `ffi.cdef[[ ... ]]` block is opened by `generate_header` and
        // closed by the orchestrator before emitting Lua-statement constants,
        // so the footer only returns the module table.
        "return M\n".to_string()
    }
}

impl Default for LuaGenerator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{FieldInfo, StructInfo};

    /// Test that fn ptr typedefs produce valid C types (not Rust syntax).
    #[test]
    fn lua_fn_ptr_typedef_uses_c_types() {
        let (typedef, _type_name) = LuaGenerator::generate_fn_ptr_typedef(
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

    /// Test that void-returning fn ptrs don't produce extra ')' chars.
    #[test]
    fn lua_fn_ptr_void_return_no_extra_parens() {
        let (typedef, _type_name) = LuaGenerator::generate_fn_ptr_typedef(
            "Test",
            "destroy",
            "unsafeextern\"C\"fn(this:*constHostInterface,instance:GuestContractInstance)->()",
        );
        assert!(
            typedef.contains("void (*Test_destroy_fn)("),
            "void return type should produce a named C function-pointer typedef: {}",
            typedef
        );
    }

    /// Test that Array<T> fields expand into 3 sub-fields.
    #[test]
    fn lua_array_field_expands() {
        let generator = LuaGenerator::new();
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
}

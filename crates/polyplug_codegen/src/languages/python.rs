//! Python code generator — produces Python ctypes bindings from ABI items.
//!
//! Generates typed CFUNCTYPE typedefs for function pointer fields,
//! correct Array<T> representations, and idiomatic Python naming.

use crate::data::{ConstInfo, EnumInfo, FunctionInfo, StructInfo, UnionInfo};
use crate::languages::{CodeGenerator, GenerationContext};

/// Python ABI code generator.
pub struct PythonGenerator;

impl PythonGenerator {
    pub fn new() -> Self {
        PythonGenerator
    }

    /// Check if a rust_type string represents a function pointer.
    fn is_function_pointer(rust_type: &str) -> bool {
        // Strip Option<...> wrapper first.
        let type_str = Self::strip_option(rust_type);
        type_str.contains("extern\"C\"fn") || type_str.contains("extern\"C\"")
    }

    /// Strip `Option<...>` wrapper if present, returning inner type.
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
    ///
    /// The compact `quote!()` output looks like:
    /// `unsafeextern"C"fn(*constHostInterface,*constPluginDescriptor)->AbiError`
    fn parse_function_pointer(type_name: &str) -> Option<(String, Vec<String>)> {
        let type_str = Self::strip_option(type_name);

        let fn_start = type_str.find("fn(")?;
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

        let params_str = &type_str[params_start..params_end];
        let return_type = if type_str.len() > params_end + 1 {
            // After `)` there should be `->ReturnType` or `)-> ` or `)->`.
            let after = &type_str[params_end + 1..];
            let trimmed = after.trim_start_matches('-').trim_start_matches('>').trim();
            if trimmed.is_empty() {
                "None".to_string()
            } else {
                Self::rust_type_to_python(trimmed)
            }
        } else {
            "None".to_string()
        };

        // Parse parameters separated by commas at depth 0.
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
                        params.push(Self::rust_type_to_python(p));
                    }
                    current.clear();
                }
                _ => {
                    current.push(c);
                }
            }
        }
        if !current.trim().is_empty() {
            params.push(Self::rust_type_to_python(current.trim()));
        }

        Some((return_type, params))
    }

    /// Generate a CFUNCTYPE typedef string for a function pointer field.
    ///
    /// Returns (typedef_line, type_name_to_use_in_fields).
    fn generate_cfunctype(struct_name: &str, field_name: &str, rust_type: &str) -> (String, String) {
        let (return_type, params) = Self::parse_function_pointer(rust_type)
            .unwrap_or_else(|| ("None".to_string(), Vec::new()));

        // Build a unique Python identifier for this callback type.
        let callback_name = format!("_{}_{}_t", to_snake_case(struct_name), field_name);

        let mut typedef = format!(
            "{} = ctypes.CFUNCTYPE({}, {})\n",
            callback_name,
            return_type,
            params.join(", ")
        );

        // For Option<fn ptr>, add a comment that it's nullable.
        if Self::is_option(rust_type) {
            typedef.push_str("# Nullable function pointer (Option<fn>). Can be set to None.\n");
        }

        (typedef, callback_name)
    }

    fn rust_type_to_python(rust_type: &str) -> String {
        // Handle Option<...> wrapper.
        if Self::is_option(rust_type) {
            let inner = &rust_type["Option<".len()..rust_type.len() - 1];
            // Option<fn ptr> is still a fn ptr type in ctypes (nullable).
            if Self::is_function_pointer(rust_type) {
                // The actual type resolution happens at the struct level,
                // not here. Return the inner type for parsing.
                return Self::rust_type_to_python(inner);
            }
            // Option<primitive> maps to the primitive type (nullable via None).
            return Self::rust_type_to_python(inner);
        }

        // Handle Array<T> — generates as void* items + size_t len + size_t align.
        if Self::is_array(rust_type) {
            // This should not be called for Array types at the field level;
            // Array fields are expanded into 3 sub-fields in generate_struct.
            return String::from("ctypes.c_void_p");
        }

        // Handle function pointers — return c_void_p as fallback.
        // Actual CFUNCTYPE typedefs are handled in generate_struct.
        if rust_type.contains("extern\"C\"fn") || rust_type.contains("extern\"C\"") {
            return String::from("ctypes.c_void_p");
        }

        if rust_type.starts_with('*') {
            if rust_type == "*const u8" {
                return String::from("ctypes.c_char_p");
            }
            return String::from("ctypes.c_void_p");
        }

        if rust_type.contains("c_void") {
            return String::from("ctypes.c_void_p");
        }

        match rust_type {
            "u64" => String::from("ctypes.c_uint64"),
            "u32" => String::from("ctypes.c_uint32"),
            "u16" => String::from("ctypes.c_uint16"),
            "u8" => String::from("ctypes.c_uint8"),
            "i64" => String::from("ctypes.c_int64"),
            "i32" => String::from("ctypes.c_int32"),
            "i16" => String::from("ctypes.c_int16"),
            "i8" => String::from("ctypes.c_int8"),
            "usize" => String::from("ctypes.c_size_t"),
            "isize" => String::from("ctypes.c_ssize_t"),
            "bool" => String::from("ctypes.c_bool"),
            "()" => String::from("None"),
            other => String::from(other),
        }
    }

    fn format_docstring(doc: &str, indent_level: usize) -> String {
        let indent = "    ".repeat(indent_level);
        let lines: Vec<&str> = doc.lines().collect();
        if lines.len() == 1 {
            format!("{}\"\"\"{}\"\"\"\n", indent, lines[0])
        } else {
            let mut result = format!("{}\"\"\"{}\n", indent, lines[0]);
            for line in &lines[1..] {
                result.push_str(&format!("{}{}\n", indent, line));
            }
            result.push_str(&format!("{}\"\"\"\n", indent));
            result
        }
    }
}

/// Convert PascalCase or camelCase to snake_case.
fn to_snake_case(s: &str) -> String {
    let mut result = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                result.push('_');
            }
            result.push(c.to_ascii_lowercase());
        } else {
            result.push(c);
        }
    }
    result
}

impl CodeGenerator for PythonGenerator {
    fn generate_const(&self, item: &ConstInfo, _ctx: &GenerationContext) -> String {
        format!("{}: int = {}\n", item.name, item.value)
    }

    fn generate_struct(&self, item: &StructInfo, _ctx: &GenerationContext) -> String {
        let mut output = String::new();
        let mut typedefs = String::new();

        // Pre-scan fields for function pointer types — collect CFUNCTYPE typedefs.
        for field in &item.fields {
            if Self::is_function_pointer(&field.rust_type) {
                let (typedef, _type_name) =
                    Self::generate_cfunctype(&item.name, &field.name, &field.rust_type);
                typedefs.push_str(&typedef);
            }
        }

        output.push_str("\n\n");
        // Emit CFUNCTYPE typedefs before the class.
        output.push_str(&typedefs);

        output.push_str("class ");
        output.push_str(&item.name);
        output.push_str("(ctypes.Structure):\n");

        if let Some(doc) = &item.doc {
            output.push_str(&Self::format_docstring(doc, 1));
        } else {
            output.push_str("    \"\"\"ABI struct.\"\"\"\n");
        }

        output.push_str("    _fields_ = [\n");
        for field in &item.fields {
            // Handle Array<T> — expand into 3 sub-fields per D-21.
            if Self::is_array(&field.rust_type) {
                output.push_str(&format!(
                    "        (\"{}\", ctypes.c_void_p),\n",
                    field.name
                ));
                output.push_str(&format!(
                    "        (\"{}_len\", ctypes.c_size_t),\n",
                    field.name
                ));
                output.push_str(&format!(
                    "        (\"{}__align\", ctypes.c_size_t),\n",
                    field.name
                ));
                continue;
            }

            // Handle function pointer fields — use the CFUNCTYPE typedef name.
            if Self::is_function_pointer(&field.rust_type) {
                let (_, type_name) =
                    Self::generate_cfunctype(&item.name, &field.name, &field.rust_type);
                output.push_str(&format!("        (\"{}\", {}),\n", field.name, type_name));
                continue;
            }

            let py_type = Self::rust_type_to_python(&field.rust_type);
            output.push_str(&format!("        (\"{}\", {}),\n", field.name, py_type));
        }
        output.push_str("    ]\n");

        output
    }

    fn generate_enum(&self, item: &EnumInfo, _ctx: &GenerationContext) -> String {
        let mut output = String::new();

        output.push_str("\n\nclass ");
        output.push_str(&item.name);
        output.push_str("(enum.IntEnum):\n");

        if let Some(doc) = &item.doc {
            output.push_str(&Self::format_docstring(doc, 1));
        } else {
            output.push_str("    \"\"\"ABI enum.\"\"\"\n");
        }

        for (i, variant) in item.variants.iter().enumerate() {
            if let Some(value) = variant.value {
                output.push_str(&format!("    {} = {}\n", variant.name, value));
            } else if i == 0 {
                output.push_str(&format!("    {} = 0\n", variant.name));
            } else {
                output.push_str(&format!("    {} = {}\n", variant.name, i));
            }
        }

        output
    }

    fn generate_union(&self, item: &UnionInfo, _ctx: &GenerationContext) -> String {
        let mut output = String::new();

        output.push_str("\n\nclass ");
        output.push_str(&item.name);
        output.push_str("(ctypes.Union):\n");

        if let Some(doc) = &item.doc {
            output.push_str(&Self::format_docstring(doc, 1));
        } else {
            output.push_str("    \"\"\"ABI union.\"\"\"\n");
        }

        output.push_str("    _fields_ = [\n");
        for variant in &item.variants {
            let py_type = Self::rust_type_to_python(&variant.type_name);
            output.push_str(&format!("        (\"{}\", {}),\n", variant.name, py_type));
        }
        output.push_str("    ]\n");

        output
    }

    fn generate_function(&self, item: &FunctionInfo, _ctx: &GenerationContext) -> String {
        let ret_type = item
            .return_type
            .as_ref()
            .map(|t| Self::rust_type_to_python(t))
            .unwrap_or_else(|| "None".to_string());

        let params = item
            .params
            .iter()
            .map(|p| format!("{}: {}", p.name, Self::rust_type_to_python(&p.rust_type)))
            .collect::<Vec<_>>()
            .join(", ");

        format!(
            "def {}({}) -> {}:\n    pass\n\n",
            item.name, params, ret_type
        )
    }

    fn file_extension(&self) -> &'static str {
        "py"
    }

    fn language_name(&self) -> &'static str {
        "python"
    }

    fn generate_header(&self, _ctx: &GenerationContext) -> String {
        "from __future__ import annotations\n\nimport ctypes\nimport enum\nfrom typing import ClassVar\n\n".to_string()
    }
}

impl Default for PythonGenerator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{FieldInfo, StructInfo};

    /// Test that CFUNCTYPE typedefs use proper ctypes types (not raw Rust syntax).
    #[test]
    fn python_cfunctype_uses_ctypes_params() {
        let rust_type = "unsafeextern\"C\"fn(host:*constHostInterface,contract_id:u64)->AbiError";
        let (return_type, params) = PythonGenerator::parse_function_pointer(rust_type)
            .expect("should parse fn ptr");

        assert_eq!(return_type, "AbiError");
        // Params must be ctypes types, not raw Rust syntax.
        assert!(
            !params.iter().any(|p| p.contains("*const")),
            "params should not contain raw pointer syntax: {:?}",
            params
        );
        assert!(
            params.iter().any(|p| p == "ctypes.c_void_p"),
            "pointer param should be ctypes.c_void_p, got: {:?}",
            params
        );
        assert!(
            params.iter().any(|p| p == "ctypes.c_uint64"),
            "u64 param should be ctypes.c_uint64, got: {:?}",
            params
        );
    }

    /// Test that CFUNCTYPE handles Option<fn ptr> (nullable).
    #[test]
    fn python_cfunctype_option_nullable() {
        let rust_type = "Option<unsafeextern\"C\"fn(ReloadPhase)>";
        let (typedef, _type_name) =
            PythonGenerator::generate_cfunctype("RuntimeConfig", "on_reload", rust_type);
        assert!(
            typedef.contains("CFUNCTYPE"),
            "should contain CFUNCTYPE: {}",
            typedef
        );
        assert!(
            typedef.contains("Nullable"),
            "Option<fn ptr> should be marked nullable: {}",
            typedef
        );
    }

    /// Test that a struct with fn ptr fields generates CFUNCTYPE typedefs.
    #[test]
    fn python_struct_with_fn_ptr_generates_cfunctype() {
        let generator = PythonGenerator::new();
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
            output.contains("CFUNCTYPE"),
            "struct with fn ptr should emit CFUNCTYPE: {}",
            output
        );
        assert!(
            output.contains("ctypes.c_void_p"),
            "pointer param should be ctypes.c_void_p: {}",
            output
        );
    }

    /// Test that Array<T> fields expand into 3 sub-fields.
    #[test]
    fn python_array_field_expands() {
        let generator = PythonGenerator::new();
        let ctx = GenerationContext::new();
        let item = StructInfo {
            name: String::from("WithArray"),
            fields: vec![FieldInfo {
                name: String::from("items"),
                rust_type: String::from("Array<u8>"),
                doc: None,
            }],
            doc: None,
            attributes: vec![],
            size_hint: None,
        };

        let output = generator.generate_struct(&item, &ctx);
        assert!(
            output.contains(r#"("items", ctypes.c_void_p)"#),
            "Array items should be c_void_p: {}",
            output
        );
        assert!(
            output.contains("items_len"),
            "Array should have len field: {}",
            output
        );
        assert!(
            output.contains("items__align"),
            "Array should have align field: {}",
            output
        );
    }

    /// Test that compact fn ptr param with pointer is correctly converted.
    #[test]
    fn python_fn_ptr_with_const_ptr_param() {
        let rust_type = "unsafeextern\"C\"fn(ptr:*constu8,len:usize)->()";
        let (return_type, params) = PythonGenerator::parse_function_pointer(rust_type)
            .expect("should parse fn ptr");

        assert_eq!(return_type, "None");
        assert!(
            params.contains(&"ctypes.c_char_p".to_string())
                || params.contains(&"ctypes.c_void_p".to_string()),
            "*const u8 should map to ctypes.c_char_p or c_void_p, got: {:?}",
            params
        );
        assert!(
            params.contains(&"ctypes.c_size_t".to_string()),
            "usize should map to ctypes.c_size_t, got: {:?}",
            params
        );
    }
}

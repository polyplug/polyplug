//! Generation context with language-specific type mappings.

use std::collections::HashMap;

/// Target language for code generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Cpp,
    CSharp,
    Python,
    Lua,
    JavaScript,
}

/// Context for code generation with type mappings and formatting settings.
#[derive(Debug, Clone)]
pub struct GenerationContext {
    /// Target language.
    pub language: Language,
    /// Type mapping from Rust types to target language types.
    pub type_map: HashMap<String, String>,
    /// Current indentation level.
    pub indent: usize,
    /// Indentation string (e.g., "    " for 4-space indent).
    pub indent_str: String,
}

/// The primitive ABI type keys every language context maps, defined once so a
/// language's value list cannot drift out of sync with the others. Each factory
/// supplies its 12 target-language spellings in this exact order.
const PRIMITIVE_TYPE_KEYS: [&str; 12] = [
    "u64", "u32", "u16", "u8", "i64", "i32", "i16", "i8", "usize", "isize", "bool", "()",
];

/// Pair `PRIMITIVE_TYPE_KEYS` with `values` (same order) into a type map.
fn primitive_type_map(values: [&str; 12]) -> HashMap<String, String> {
    PRIMITIVE_TYPE_KEYS
        .into_iter()
        .zip(values)
        .map(|(key, value): (&str, &str)| (String::from(key), String::from(value)))
        .collect()
}

impl GenerationContext {
    /// Create a new context with the given language and type mappings.
    pub fn new(language: Language, type_map: HashMap<String, String>) -> GenerationContext {
        GenerationContext {
            language,
            type_map,
            indent: 0,
            indent_str: String::from("    "),
        }
    }

    /// Map a Rust type name to the target language type.
    pub fn map_type(&self, rust_type: &str) -> String {
        // Handle function pointer types
        if rust_type.contains("extern\"C\"fn") || rust_type.contains("extern\"C\"") {
            return self.convert_function_pointer(rust_type);
        }

        // Handle double pointers
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

        // Handle pointer types
        if rust_type.starts_with('*') {
            let rest: &str = rust_type.trim_start_matches('*').trim();
            if rest.starts_with("const") {
                let inner: &str = rest.trim_start_matches("const").trim();
                let mapped_inner: String = self.map_type(inner);
                return format!("const {}*", mapped_inner);
            }
            if rest.starts_with("mut") {
                let inner: &str = rest.trim_start_matches("mut").trim();
                let mapped_inner: String = self.map_type(inner);
                return format!("{}*", mapped_inner);
            }
            return String::from("void*");
        }

        // Handle c_void
        if rust_type.contains("c_void") {
            return String::from("void");
        }

        // Check type map
        if let Some(mapped) = self.type_map.get(rust_type) {
            return mapped.clone();
        }

        // Default: pass through the type name (for ABI struct names)
        String::from(rust_type)
    }

    /// Convert a Rust function pointer type to target language syntax.
    fn convert_function_pointer(&self, type_name: &str) -> String {
        let return_type: &str = if let Some(pos) = type_name.find(")->") {
            &type_name[pos + 3..]
        } else {
            "void"
        };

        let mapped_return: String = self.map_type(return_type);

        let params_start: usize = type_name.find("fn(").map(|p| p + 3).unwrap_or(0);
        let params_end: usize = type_name.find(")->").unwrap_or(type_name.len());

        if params_start == 0 || params_end <= params_start {
            return format!("{}(*)()", mapped_return);
        }

        let params_str: &str = &type_name[params_start..params_end];
        let params: Vec<String> = self.parse_function_params(params_str);

        if params.is_empty() {
            return format!("{}(*)()", mapped_return);
        }

        format!("{}(*)({})", mapped_return, params.join(", "))
    }

    /// Parse function parameters from a Rust function signature.
    fn parse_function_params(&self, params_str: &str) -> Vec<String> {
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
                        params.push(self.convert_param(&param));
                    }
                    current_param.clear();
                }
                _ => {
                    current_param.push(c);
                }
            }
        }

        if !current_param.trim().is_empty() {
            params.push(self.convert_param(current_param.trim()));
        }

        params
    }

    /// Convert a single parameter to target language syntax.
    fn convert_param(&self, param: &str) -> String {
        let parts: Vec<&str> = param.splitn(2, ':').collect();
        let type_part: &str = if parts.len() == 2 { parts[1] } else { parts[0] };
        self.map_type(type_part.trim())
    }

    /// Get the current indentation string.
    pub fn current_indent(&self) -> String {
        self.indent_str.repeat(self.indent)
    }

    /// Increase indentation level.
    pub fn increase_indent(&mut self) {
        self.indent += 1;
    }

    /// Decrease indentation level.
    pub fn decrease_indent(&mut self) {
        if self.indent > 0 {
            self.indent -= 1;
        }
    }

    /// Create a C++ generation context with standard type mappings.
    pub fn cpp() -> GenerationContext {
        let type_map: HashMap<String, String> = primitive_type_map([
            "uint64_t",
            "uint32_t",
            "uint16_t",
            "uint8_t",
            "int64_t",
            "int32_t",
            "int16_t",
            "int8_t",
            "size_t",
            "ptrdiff_t",
            "bool",
            "void",
        ]);
        GenerationContext::new(Language::Cpp, type_map)
    }

    /// Create a C# generation context with standard type mappings.
    pub fn csharp() -> GenerationContext {
        let type_map: HashMap<String, String> = primitive_type_map([
            "ulong", "uint", "ushort", "byte", "long", "int", "short", "sbyte", "nuint", "nint",
            "bool", "void",
        ]);
        GenerationContext::new(Language::CSharp, type_map)
    }

    /// Create a Python generation context with ctypes type mappings.
    pub fn python() -> GenerationContext {
        let type_map: HashMap<String, String> = primitive_type_map([
            "ctypes.c_uint64",
            "ctypes.c_uint32",
            "ctypes.c_uint16",
            "ctypes.c_uint8",
            "ctypes.c_int64",
            "ctypes.c_int32",
            "ctypes.c_int16",
            "ctypes.c_int8",
            "ctypes.c_size_t",
            "ctypes.c_ssize_t",
            "ctypes.c_bool",
            "None",
        ]);
        GenerationContext::new(Language::Python, type_map)
    }

    /// Create a Lua generation context with standard type mappings.
    pub fn lua() -> GenerationContext {
        let type_map: HashMap<String, String> = primitive_type_map([
            "uint64_t",
            "uint32_t",
            "uint16_t",
            "uint8_t",
            "int64_t",
            "int32_t",
            "int16_t",
            "int8_t",
            "size_t",
            "ptrdiff_t",
            "bool",
            "void",
        ]);
        GenerationContext::new(Language::Lua, type_map)
    }

    /// Create a JavaScript generation context with standard type mappings.
    pub fn javascript() -> GenerationContext {
        let type_map: HashMap<String, String> = primitive_type_map([
            "bigint", "number", "number", "number", "bigint", "number", "number", "number",
            "bigint", "bigint", "boolean", "void",
        ]);
        GenerationContext::new(Language::JavaScript, type_map)
    }
}

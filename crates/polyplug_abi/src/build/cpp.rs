//! C++ ABI Code Generator — generates C++ headers from ABI type information.
//!
//! This module implements the `AbiGenerator` trait for C++, producing idiomatic
//! C++ code with proper `struct` definitions, `enum class` types, `union` types,
//! and FNV-1a hash functions.

use super::{AbiGenerator, AbiInfo, EnumInfo, StructInfo, UnionInfo};
use std::path::PathBuf;

/// C++ ABI code generator.
///
/// Generates C++ headers for the polyplug ABI types, including:
/// - Constants as `constexpr` values in a namespace
/// - Structs with C-compatible layout
/// - Enums as `enum class`
/// - Unions with proper C layout
/// - FNV-1a hash helper functions
#[derive(Debug, Clone, Copy, Default)]
pub struct CppGenerator;

impl CppGenerator {
    /// Create a new C++ generator.
    pub fn new() -> CppGenerator {
        CppGenerator
    }

    /// Convert a Rust type name to a C++ type name.
    ///
    /// # Type Mappings
    /// - `*const u8`, `*mut u8` → `const uint8_t*`, `uint8_t*`
    /// - `*const ()`, `*mut ()` → `const void*`, `void*`
    /// - `*mut c_void` → `void*`
    /// - `u64` → `uint64_t`
    /// - `u32` → `uint32_t`
    /// - `u16` → `uint16_t`
    /// - `u8` → `uint8_t`
    /// - `i64` → `int64_t`
    /// - `i32` → `int32_t`
    /// - `i16` → `int16_t`
    /// - `i8` → `int8_t`
    /// - `usize` → `size_t`
    /// - `isize` → `ptrdiff_t`
    /// - `bool` → `bool`
    /// - ABI struct names → same name (StringView, Buffer, etc.)
    fn rust_type_to_cpp(type_name: &str) -> String {
        // Handle function pointer types (extern "C" fn)
        if type_name.contains("extern\"C\"fn") || type_name.contains("extern\"C\"") {
            return Self::convert_function_pointer(type_name);
        }

        // Handle double pointer: *const*const() → void* const*
        if type_name.starts_with("*const*const") {
            return String::from("void* const*");
        }
        if type_name.starts_with("*mut*const") {
            return String::from("void* const*");
        }
        if type_name.starts_with("*const*mut") {
            return String::from("void**");
        }
        if type_name.starts_with("*mut*mut") {
            return String::from("void**");
        }

        // Handle pointer types
        if type_name.starts_with('*') {
            let rest: &str = type_name.trim_start_matches('*').trim();
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

        // Handle c_void
        if type_name.contains("c_void") {
            return String::from("void");
        }

        match type_name {
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
            _ => String::from(type_name),
        }
    }

    /// Convert a Rust function pointer type to C++ function pointer syntax.
    fn convert_function_pointer(type_name: &str) -> String {
        // Parse the function pointer type
        // Format: unsafeextern"C"fn(param1:type,param2:type)->return_type
        // or: extern"C"fn(...)->return_type

        // Extract return type
        let return_type: &str = if let Some(pos) = type_name.find(")->") {
            &type_name[pos + 3..]
        } else {
            "void"
        };

        let cpp_return: String = Self::rust_type_to_cpp(return_type);

        // Extract parameters
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

    /// Parse function parameters from a Rust function signature.
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

    /// Convert a single parameter to C++ syntax.
    fn convert_param(param: &str) -> String {
        // Format: name:type or just type
        let parts: Vec<&str> = param.splitn(2, ':').collect();
        let type_part: &str = if parts.len() == 2 { parts[1] } else { parts[0] };
        Self::rust_type_to_cpp(type_part.trim())
    }

    /// Generate a C++ constant value with proper type suffix.
    fn format_constant_value(value: &str, type_name: &str) -> String {
        match type_name {
            "u64" => format!("{}ULL", value),
            "u32" => format!("{}U", value),
            "i64" => format!("{}LL", value),
            _ => String::from(value),
        }
    }

    /// Generate a Doxygen-style documentation comment.
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

    /// Generate a single struct definition.
    fn generate_struct(struct_info: &StructInfo) -> String {
        let mut output: String = String::new();

        if let Some(doc) = &struct_info.doc {
            output.push_str(&Self::format_doc_comment(doc, 0));
        }

        output.push_str("struct ");
        output.push_str(&struct_info.name);
        output.push_str(" {\n");

        for field in &struct_info.fields {
            if let Some(doc) = &field.doc {
                output.push_str(&Self::format_doc_comment(doc, 4));
            }

            let field_decl: String =
                Self::generate_field_declaration(&field.type_name, &field.name);
            output.push_str(&format!("    {};\n", field_decl));
        }

        output.push_str("};\n\n");
        output
    }

    /// Generate a field declaration, handling function pointer syntax.
    fn generate_field_declaration(type_name: &str, field_name: &str) -> String {
        // Check if this is a function pointer type
        if type_name.contains("extern\"C\"fn") || type_name.contains("extern\"C\"") {
            return Self::generate_function_pointer_field(type_name, field_name);
        }

        // Regular type
        let cpp_type: String = Self::rust_type_to_cpp(type_name);
        format!("{} {}", cpp_type, field_name)
    }

    /// Generate a function pointer field declaration.
    fn generate_function_pointer_field(type_name: &str, field_name: &str) -> String {
        // Extract return type
        let return_type: &str = if let Some(pos) = type_name.find(")->") {
            &type_name[pos + 3..]
        } else {
            "void"
        };

        let cpp_return: String = Self::rust_type_to_cpp(return_type);

        // Extract parameters
        let params_start: usize = type_name.find("fn(").map(|p| p + 3).unwrap_or(0);

        // Find the end of parameters - either at )-> or at the matching )
        let params_end: usize = if let Some(pos) = type_name.find(")->") {
            pos
        } else if params_start > 0 {
            // Find matching closing paren
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

    /// Generate a single enum definition.
    fn generate_enum(enum_info: &EnumInfo) -> String {
        let mut output: String = String::new();

        if let Some(doc) = &enum_info.doc {
            output.push_str(&Self::format_doc_comment(doc, 0));
        }

        output.push_str("enum class ");
        output.push_str(&enum_info.name);
        output.push_str(" : uint32_t {\n");

        for (i, variant) in enum_info.variants.iter().enumerate() {
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

    /// Generate a single union definition.
    fn generate_union(union_info: &UnionInfo) -> String {
        let mut output: String = String::new();

        if let Some(doc) = &union_info.doc {
            output.push_str(&Self::format_doc_comment(doc, 0));
        }

        output.push_str("union ");
        output.push_str(&union_info.name);
        output.push_str(" {\n");

        for variant in &union_info.variants {
            if let Some(doc) = &variant.doc {
                output.push_str(&Self::format_doc_comment(doc, 4));
            }

            let cpp_type: String = Self::rust_type_to_cpp(&variant.type_name);
            output.push_str(&format!("    {} {};\n", cpp_type, variant.name));
        }

        output.push_str("};\n\n");
        output
    }

    /// Generate structs that must come before unions.
    fn generate_structs_before_unions(info: &AbiInfo) -> String {
        let mut output: String = String::new();

        output.push_str("extern \"C\" {\n\n");

        let struct_names: &[&str] = &[
            "StringView",
            "Buffer",
            "PluginHandle",
            "HostContext",
            "AbiError",
            "NativeDispatch",
            "VmDispatch",
        ];

        let mut struct_map: std::collections::HashMap<&str, &StructInfo> =
            std::collections::HashMap::new();
        for struct_info in &info.structs {
            struct_map.insert(&struct_info.name, struct_info);
        }

        for name in struct_names {
            if let Some(struct_info) = struct_map.get(name) {
                output.push_str(&Self::generate_struct(struct_info));
            }
        }

        output.push_str("} // extern \"C\"\n\n");
        output
    }

    /// Generate structs that come after unions.
    fn generate_structs_after_unions(info: &AbiInfo) -> String {
        let mut output: String = String::new();

        output.push_str("extern \"C\" {\n\n");

        let struct_names: &[&str] = &[
            "PluginDescriptor",
            "PluginInterface",
            "HostVTable",
            "PluginContext",
            "ExtensionEntry",
            "RuntimeConfig",
        ];

        let mut struct_map: std::collections::HashMap<&str, &StructInfo> =
            std::collections::HashMap::new();
        for struct_info in &info.structs {
            struct_map.insert(&struct_info.name, struct_info);
        }

        for name in struct_names {
            if let Some(struct_info) = struct_map.get(name) {
                output.push_str(&Self::generate_struct(struct_info));
            }
        }

        output.push_str("} // extern \"C\"\n\n");
        output
    }
}

impl AbiGenerator for CppGenerator {
    fn generate_constants(&self, info: &AbiInfo) -> String {
        let mut output: String = String::new();

        output.push_str("// THIS FILE IS AUTO-GENERATED BY polyplug_abi\n");
        output.push_str("// DO NOT EDIT BY HAND\n");
        output.push_str("// Re-generate with: polyplug_abi generate --lang cpp\n\n");

        output.push_str("#pragma once\n\n");

        output.push_str("#include <cstddef>\n");
        output.push_str("#include <cstdint>\n\n");

        output.push_str(
            "// ─── ABI Constants ────────────────────────────────────────────────────────────\n\n",
        );

        for constant in &info.constants {
            let value: String = Self::format_constant_value(&constant.value, &constant.type_name);
            output.push_str(&format!("#define {} {}\n", constant.name, value));
        }

        output.push('\n');
        output
    }

    fn generate_structs(&self, info: &AbiInfo) -> String {
        let mut output: String = String::new();

        output.push_str(
            "// ─── ABI Structs ──────────────────────────────────────────────────────────────\n\n",
        );

        output.push_str(&Self::generate_structs_before_unions(info));
        output.push_str(&self.generate_unions(info));
        output.push_str(&Self::generate_structs_after_unions(info));

        output
    }

    fn generate_enums(&self, info: &AbiInfo) -> String {
        let mut output: String = String::new();

        output.push_str(
            "// ─── ABI Enums ────────────────────────────────────────────────────────────────\n\n",
        );

        for enum_info in &info.enums {
            output.push_str(&Self::generate_enum(enum_info));
        }

        output
    }

    fn generate_unions(&self, info: &AbiInfo) -> String {
        let mut output: String = String::new();

        output.push_str(
            "// ─── ABI Unions ───────────────────────────────────────────────────────────────\n\n",
        );

        output.push_str("extern \"C\" {\n\n");

        for union_info in &info.unions {
            output.push_str(&Self::generate_union(union_info));
        }

        output.push_str("} // extern \"C\"\n\n");
        output
    }

    fn generate_helpers(&self, _info: &AbiInfo) -> String {
        let mut output: String = String::new();

        output.push_str(
            "// ─── FNV-1a Hash Helpers ──────────────────────────────────────────────────────\n\n",
        );

        output.push_str("namespace polyplug {\n\n");

        output.push_str("namespace detail {\n\n");

        output.push_str("/// FNV-1a 64-bit hash of a byte sequence.\n");
        output
            .push_str("constexpr uint64_t fnv1a_64(const uint8_t* data, size_t len) noexcept {\n");
        output.push_str("    constexpr uint64_t FNV_OFFSET = 0xcbf29ce484222325ULL;\n");
        output.push_str("    constexpr uint64_t FNV_PRIME = 0x00000100000001B3ULL;\n");
        output.push_str("    uint64_t hash = FNV_OFFSET;\n");
        output.push_str("    for (size_t i = 0; i < len; ++i) {\n");
        output.push_str("        hash ^= static_cast<uint64_t>(data[i]);\n");
        output.push_str("        hash *= FNV_PRIME;\n");
        output.push_str("    }\n");
        output.push_str("    return hash;\n");
        output.push_str("}\n\n");

        output.push_str("} // namespace detail\n\n");

        output.push_str(
            "/// Compute the contract ID for \"name@major_version\" using FNV-1a 64-bit.\n",
        );
        output.push_str(
            "constexpr uint64_t contract_id(const char* name, uint32_t major_version) noexcept {\n",
        );
        output.push_str("    // Hash name\n");
        output.push_str("    constexpr uint64_t FNV_OFFSET = 0xcbf29ce484222325ULL;\n");
        output.push_str("    constexpr uint64_t FNV_PRIME = 0x00000100000001B3ULL;\n");
        output.push_str("    uint64_t hash = FNV_OFFSET;\n");
        output.push_str("    while (*name) {\n");
        output.push_str("        hash ^= static_cast<uint64_t>(static_cast<uint8_t>(*name));\n");
        output.push_str("        hash *= FNV_PRIME;\n");
        output.push_str("        ++name;\n");
        output.push_str("    }\n");
        output.push_str("    // Hash '@'\n");
        output.push_str("    hash ^= static_cast<uint64_t>('@');\n");
        output.push_str("    hash *= FNV_PRIME;\n");
        output.push_str("    // Hash major_version as decimal\n");
        output.push_str("    if (major_version == 0) {\n");
        output.push_str("        hash ^= static_cast<uint64_t>('0');\n");
        output.push_str("        hash *= FNV_PRIME;\n");
        output.push_str("    } else {\n");
        output.push_str("        uint32_t v = major_version;\n");
        output.push_str("        char buf[12] = {};\n");
        output.push_str("        int i = 11;\n");
        output.push_str("        while (v > 0) {\n");
        output.push_str("            buf[--i] = '0' + (v % 10);\n");
        output.push_str("            v /= 10;\n");
        output.push_str("        }\n");
        output.push_str("        const char* p = buf + i;\n");
        output.push_str("        while (*p) {\n");
        output.push_str("            hash ^= static_cast<uint64_t>(static_cast<uint8_t>(*p));\n");
        output.push_str("            hash *= FNV_PRIME;\n");
        output.push_str("            ++p;\n");
        output.push_str("        }\n");
        output.push_str("    }\n");
        output.push_str("    return hash;\n");
        output.push_str("}\n\n");

        output.push_str("/// Compute an extension ID from its name using FNV-1a lower 32 bits.\n");
        output.push_str("constexpr uint32_t extension_id(const char* name) noexcept {\n");
        output.push_str("    constexpr uint64_t FNV_OFFSET = 0xcbf29ce484222325ULL;\n");
        output.push_str("    constexpr uint64_t FNV_PRIME = 0x00000100000001B3ULL;\n");
        output.push_str("    uint64_t hash = FNV_OFFSET;\n");
        output.push_str("    while (*name) {\n");
        output.push_str("        hash ^= static_cast<uint64_t>(static_cast<uint8_t>(*name));\n");
        output.push_str("        hash *= FNV_PRIME;\n");
        output.push_str("        ++name;\n");
        output.push_str("    }\n");
        output.push_str("    return static_cast<uint32_t>(hash);\n");
        output.push_str("}\n\n");

        output.push_str("/// Compute a bundle ID from its name using FNV-1a 64-bit hash.\n");
        output.push_str("constexpr uint64_t bundle_id(const char* name) noexcept {\n");
        output.push_str("    constexpr uint64_t FNV_OFFSET = 0xcbf29ce484222325ULL;\n");
        output.push_str("    constexpr uint64_t FNV_PRIME = 0x00000100000001B3ULL;\n");
        output.push_str("    uint64_t hash = FNV_OFFSET;\n");
        output.push_str("    while (*name) {\n");
        output.push_str("        hash ^= static_cast<uint64_t>(static_cast<uint8_t>(*name));\n");
        output.push_str("        hash *= FNV_PRIME;\n");
        output.push_str("        ++name;\n");
        output.push_str("    }\n");
        output.push_str("    return hash;\n");
        output.push_str("}\n\n");

        output.push_str("} // namespace polyplug\n");
        output
    }

    fn file_extension(&self) -> &'static str {
        "hpp"
    }

    fn output_dir(&self) -> &'static str {
        "cpp"
    }

    fn generate(&self, info: &AbiInfo) -> super::GeneratedFiles {
        let mut files: super::GeneratedFiles = super::GeneratedFiles::new();

        let mut content: String = String::new();
        content.push_str(&self.generate_constants(info));
        content.push_str(&self.generate_enums(info));
        content.push_str(&self.generate_structs(info));
        content.push_str(&self.generate_helpers(info));

        let filename: String = format!("abi.{}", self.file_extension());
        files.push(super::GeneratedFile {
            path: PathBuf::from(filename),
            content,
        });

        files
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::{ConstantInfo, FieldInfo, GeneratedFiles, UnionVariantInfo, VariantInfo};

    #[test]
    fn cpp_generator_new() {
        let generator: CppGenerator = CppGenerator::new();
        assert_eq!(generator.file_extension(), "hpp");
        assert_eq!(generator.output_dir(), "cpp");
    }

    #[test]
    fn rust_type_to_cpp_primitives() {
        assert_eq!(CppGenerator::rust_type_to_cpp("u64"), "uint64_t");
        assert_eq!(CppGenerator::rust_type_to_cpp("u32"), "uint32_t");
        assert_eq!(CppGenerator::rust_type_to_cpp("u16"), "uint16_t");
        assert_eq!(CppGenerator::rust_type_to_cpp("u8"), "uint8_t");
        assert_eq!(CppGenerator::rust_type_to_cpp("i64"), "int64_t");
        assert_eq!(CppGenerator::rust_type_to_cpp("i32"), "int32_t");
        assert_eq!(CppGenerator::rust_type_to_cpp("i16"), "int16_t");
        assert_eq!(CppGenerator::rust_type_to_cpp("i8"), "int8_t");
        assert_eq!(CppGenerator::rust_type_to_cpp("usize"), "size_t");
        assert_eq!(CppGenerator::rust_type_to_cpp("isize"), "ptrdiff_t");
        assert_eq!(CppGenerator::rust_type_to_cpp("bool"), "bool");
    }

    #[test]
    fn rust_type_to_cpp_pointers() {
        assert_eq!(
            CppGenerator::rust_type_to_cpp("*const u8"),
            "const uint8_t*"
        );
        assert_eq!(CppGenerator::rust_type_to_cpp("*mut u8"), "uint8_t*");
        assert_eq!(CppGenerator::rust_type_to_cpp("*const ()"), "const void*");
        assert_eq!(CppGenerator::rust_type_to_cpp("*mut ()"), "void*");
        assert_eq!(CppGenerator::rust_type_to_cpp("*mut c_void"), "void*");
    }

    #[test]
    fn rust_type_to_cpp_abi_types() {
        assert_eq!(CppGenerator::rust_type_to_cpp("StringView"), "StringView");
        assert_eq!(CppGenerator::rust_type_to_cpp("Buffer"), "Buffer");
        assert_eq!(CppGenerator::rust_type_to_cpp("AbiError"), "AbiError");
        assert_eq!(
            CppGenerator::rust_type_to_cpp("PluginHandle"),
            "PluginHandle"
        );
    }

    #[test]
    fn format_constant_value() {
        assert_eq!(CppGenerator::format_constant_value("42", "u64"), "42ULL");
        assert_eq!(CppGenerator::format_constant_value("42", "u32"), "42U");
        assert_eq!(CppGenerator::format_constant_value("42", "i64"), "42LL");
        assert_eq!(CppGenerator::format_constant_value("42", "i32"), "42");
    }

    #[test]
    fn format_doc_comment_single_line() {
        let result: String = CppGenerator::format_doc_comment("Hello world", 0);
        assert_eq!(result, "/// Hello world\n");
    }

    #[test]
    fn format_doc_comment_multiple_lines() {
        let result: String = CppGenerator::format_doc_comment("Line 1\nLine 2", 0);
        assert_eq!(result, "/// Line 1\n/// Line 2\n");
    }

    #[test]
    fn generate_constants_produces_valid_cpp() {
        let mut info: AbiInfo = AbiInfo::new();
        info.add_constant(ConstantInfo {
            name: String::from("ABI_OK"),
            value: String::from("0"),
            type_name: String::from("u32"),
        });
        info.add_constant(ConstantInfo {
            name: String::from("POLYPLUG_ABI_VERSION"),
            value: String::from("1"),
            type_name: String::from("u32"),
        });

        let generator: CppGenerator = CppGenerator::new();
        let output: String = generator.generate_constants(&info);

        assert!(output.contains("#define ABI_OK 0U"));
        assert!(output.contains("#define POLYPLUG_ABI_VERSION 1U"));
        assert!(output.contains("#include <cstdint>"));
    }

    #[test]
    fn generate_struct_produces_valid_cpp() {
        let struct_info: StructInfo = StructInfo {
            name: String::from("StringView"),
            fields: vec![
                FieldInfo {
                    name: String::from("ptr"),
                    type_name: String::from("*const u8"),
                    doc: Some(String::from("UTF-8 bytes, NOT null-terminated.")),
                },
                FieldInfo {
                    name: String::from("len"),
                    type_name: String::from("usize"),
                    doc: Some(String::from("Byte count.")),
                },
            ],
            doc: Some(String::from("Non-owning UTF-8 string view.")),
        };

        let output: String = CppGenerator::generate_struct(&struct_info);

        assert!(output.contains("struct StringView"));
        assert!(output.contains("const uint8_t* ptr;"));
        assert!(output.contains("size_t len;"));
    }

    #[test]
    fn generate_enum_produces_valid_cpp() {
        let enum_info: EnumInfo = EnumInfo {
            name: String::from("DispatchType"),
            variants: vec![
                VariantInfo {
                    name: String::from("Native"),
                    value: Some(0),
                    doc: Some(String::from("Native dispatch.")),
                },
                VariantInfo {
                    name: String::from("VirtualMachine"),
                    value: Some(1),
                    doc: Some(String::from("VM dispatch.")),
                },
            ],
            doc: Some(String::from("Dispatch mechanism type.")),
        };

        let output: String = CppGenerator::generate_enum(&enum_info);

        assert!(output.contains("enum class DispatchType : uint32_t"));
        assert!(output.contains("Native = 0,"));
        assert!(output.contains("VirtualMachine = 1,"));
    }

    #[test]
    fn generate_union_produces_valid_cpp() {
        let union_info: UnionInfo = UnionInfo {
            name: String::from("PluginDispatch"),
            variants: vec![
                UnionVariantInfo {
                    name: String::from("native"),
                    type_name: String::from("NativeDispatch"),
                    doc: None,
                },
                UnionVariantInfo {
                    name: String::from("vm"),
                    type_name: String::from("VmDispatch"),
                    doc: None,
                },
            ],
            doc: Some(String::from("Union of dispatch mechanisms.")),
        };

        let output: String = CppGenerator::generate_union(&union_info);

        assert!(output.contains("union PluginDispatch"));
        assert!(output.contains("NativeDispatch native;"));
        assert!(output.contains("VmDispatch vm;"));
    }

    #[test]
    fn generate_helpers_produces_valid_cpp() {
        let generator: CppGenerator = CppGenerator::new();
        let info: AbiInfo = AbiInfo::new();
        let output: String = generator.generate_helpers(&info);

        assert!(output.contains("namespace polyplug"));
        assert!(output.contains("constexpr uint64_t contract_id"));
        assert!(output.contains("constexpr uint32_t extension_id"));
        assert!(output.contains("constexpr uint64_t bundle_id"));
        assert!(output.contains("FNV_OFFSET"));
        assert!(output.contains("FNV_PRIME"));
    }

    #[test]
    fn generate_produces_complete_file() {
        let mut info: AbiInfo = AbiInfo::new();
        info.add_constant(ConstantInfo {
            name: String::from("ABI_OK"),
            value: String::from("0"),
            type_name: String::from("u32"),
        });
        info.add_struct(StructInfo {
            name: String::from("StringView"),
            fields: vec![FieldInfo {
                name: String::from("ptr"),
                type_name: String::from("*const u8"),
                doc: None,
            }],
            doc: None,
        });

        let generator: CppGenerator = CppGenerator::new();
        let files: GeneratedFiles = generator.generate(&info);

        assert_eq!(files.files.len(), 1);
        assert_eq!(files.files[0].path, PathBuf::from("abi.hpp"));
        assert!(files.files[0].content.contains("#define ABI_OK"));
        assert!(files.files[0].content.contains("struct StringView"));
    }

    /// Generate the abi.hpp file for the SDK.
    /// Run with: cargo test --package polyplug_abi -- generate_abi_hpp_file --nocapture
    #[test]
    fn generate_abi_hpp_file() {
        use crate::build::AbiParser;
        use std::fs;
        use std::path::Path;

        let abi_source: &str = include_str!("../lib.rs");
        let parser: AbiParser = AbiParser::new();
        let info: AbiInfo = parser
            .parse(abi_source)
            .expect("failed to parse ABI source");

        let generator: CppGenerator = CppGenerator::new();
        let files: GeneratedFiles = generator.generate(&info);

        let workspace_root: &Path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .expect("failed to find workspace root");
        let output_path: std::path::PathBuf = workspace_root.join("sdks/cpp/abi/polyplug/abi.hpp");

        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).expect("failed to create output directory");
        }

        fs::write(&output_path, &files.files[0].content).expect("failed to write abi.hpp");

        println!("Generated: {}", output_path.display());
    }
}

//! Lua ABI Code Generator — generates LuaJIT FFI bindings from ABI type information.
//!
//! This module implements the `AbiGenerator` trait for Lua/LuaJIT, producing
//! `ffi.cdef` declarations for structs, enums, unions, and helper functions.

use super::{AbiGenerator, AbiInfo, EnumInfo, StructInfo, UnionInfo};
use std::path::PathBuf;

/// Lua/LuaJIT ABI code generator.
///
/// Generates LuaJIT FFI bindings for the polyplug ABI types, including:
/// - Constants as module-level variables
/// - Structs as `ffi.cdef` declarations
/// - Enums as C-style enums in cdef
/// - Unions as `ffi.cdef` union declarations
/// - FNV-1a hash helper functions
#[derive(Debug, Clone, Copy, Default)]
pub struct LuaGenerator;

impl LuaGenerator {
    /// Create a new Lua generator.
    pub fn new() -> LuaGenerator {
        LuaGenerator
    }

    /// Convert a Rust type name to a LuaJIT FFI C type name.
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
    /// - `bool` → `bool` (or `int` in some contexts)
    /// - ABI struct names → same name (StringView, Buffer, etc.)
    fn rust_type_to_lua(type_name: &str) -> String {
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
                // Handle c_void specially for pointers
                if inner.contains("c_void") {
                    return String::from("const void*");
                }
                let lua_inner: String = Self::rust_type_to_lua(inner);
                return format!("const {}*", lua_inner);
            }
            if rest.starts_with("mut") {
                let inner: &str = rest.trim_start_matches("mut").trim();
                // Handle c_void specially for pointers
                if inner.contains("c_void") {
                    return String::from("void*");
                }
                let lua_inner: String = Self::rust_type_to_lua(inner);
                return format!("{}*", lua_inner);
            }
            return String::from("void*");
        }

        // Handle c_void (non-pointer context, should not happen in practice)
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
            "bool" => String::from("uint8_t"), // C bool in FFI
            "()" => String::from("void"),
            _ => String::from(type_name),
        }
    }

    /// Convert a Rust function pointer type to C function pointer syntax.
    fn convert_function_pointer(type_name: &str) -> String {
        // Extract return type
        let return_type: &str = if let Some(pos) = type_name.find(")->") {
            &type_name[pos + 3..]
        } else {
            "void"
        };

        let lua_return: String = Self::rust_type_to_lua(return_type);

        // Extract parameters
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

    /// Convert a single parameter to C syntax.
    fn convert_param(param: &str) -> String {
        // Format: name:type or just type
        let parts: Vec<&str> = param.splitn(2, ':').collect();
        let type_part: &str = if parts.len() == 2 { parts[1] } else { parts[0] };
        Self::rust_type_to_lua(type_part.trim())
    }

    /// Generate a C-style comment for use inside ffi.cdef.
    fn format_c_comment(doc: &str, indent: usize) -> String {
        let indent_str: String = " ".repeat(indent);
        doc.lines()
            .map(|line: &str| format!("{}// {}\n", indent_str, line))
            .collect::<Vec<String>>()
            .join("")
    }

    /// Generate a single struct definition for ffi.cdef.
    fn generate_struct(struct_info: &StructInfo) -> String {
        let mut output: String = String::new();

        if let Some(doc) = &struct_info.doc {
            output.push_str(&Self::format_c_comment(doc, 4));
        }

        output.push_str(&format!("    typedef struct {} {{\n", struct_info.name));

        for field in &struct_info.fields {
            if let Some(doc) = &field.doc {
                output.push_str(&Self::format_c_comment(doc, 8));
            }

            let field_decl: String =
                Self::generate_field_declaration(&field.type_name, &field.name);
            output.push_str(&format!("        {};\n", field_decl));
        }

        output.push_str("    } ");
        output.push_str(&struct_info.name);
        output.push_str(";\n\n");
        output
    }

    /// Generate a field declaration, handling function pointer syntax.
    fn generate_field_declaration(type_name: &str, field_name: &str) -> String {
        // Check if this is a function pointer type
        if type_name.contains("extern\"C\"fn") || type_name.contains("extern\"C\"") {
            return Self::generate_function_pointer_field(type_name, field_name);
        }

        // Regular type
        let lua_type: String = Self::rust_type_to_lua(type_name);
        format!("{} {}", lua_type, field_name)
    }

    /// Generate a function pointer field declaration.
    fn generate_function_pointer_field(type_name: &str, field_name: &str) -> String {
        // Extract return type
        let return_type: &str = if let Some(pos) = type_name.find(")->") {
            &type_name[pos + 3..]
        } else {
            "void"
        };

        let lua_return: String = Self::rust_type_to_lua(return_type);

        // Extract parameters
        let params_start: usize = type_name.find("fn(").map(|p| p + 3).unwrap_or(0);

        // Find the end of parameters
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
            return format!("{} (*{} )()", lua_return, field_name);
        }

        let params_str: &str = &type_name[params_start..params_end];
        let params: Vec<String> = Self::parse_function_params(params_str);

        if params.is_empty() {
            return format!("{} (*{} )()", lua_return, field_name);
        }

        format!("{} (*{} )({})", lua_return, field_name, params.join(", "))
    }

    /// Generate a single enum definition for ffi.cdef.
    fn generate_enum(enum_info: &EnumInfo) -> String {
        let mut output: String = String::new();

        if let Some(doc) = &enum_info.doc {
            output.push_str(&Self::format_c_comment(doc, 4));
        }

        output.push_str(&format!("    typedef enum {} {{\n", enum_info.name));

        for (i, variant) in enum_info.variants.iter().enumerate() {
            if let Some(doc) = &variant.doc {
                output.push_str(&Self::format_c_comment(doc, 8));
            }

            if let Some(value) = variant.value {
                output.push_str(&format!(
                    "        {}_{} = {},\n",
                    enum_info.name, variant.name, value
                ));
            } else if i == 0 {
                output.push_str(&format!(
                    "        {}_{} = 0,\n",
                    enum_info.name, variant.name
                ));
            } else {
                output.push_str(&format!("        {}_{},\n", enum_info.name, variant.name));
            }
        }

        output.push_str(&format!("    }} {};\n\n", enum_info.name));
        output
    }

    /// Generate a single union definition for ffi.cdef.
    fn generate_union(union_info: &UnionInfo) -> String {
        let mut output: String = String::new();

        if let Some(doc) = &union_info.doc {
            output.push_str(&Self::format_c_comment(doc, 4));
        }

        output.push_str(&format!("    typedef union {} {{\n", union_info.name));

        for variant in &union_info.variants {
            if let Some(doc) = &variant.doc {
                output.push_str(&Self::format_c_comment(doc, 8));
            }

            let lua_type: String = Self::rust_type_to_lua(&variant.type_name);
            output.push_str(&format!("        {} {};\n", lua_type, variant.name));
        }

        output.push_str(&format!("    }} {};\n\n", union_info.name));
        output
    }
}

impl AbiGenerator for LuaGenerator {
    fn generate_constants(&self, info: &AbiInfo) -> String {
        let mut output: String = String::new();

        output.push_str("-- THIS FILE IS AUTO-GENERATED BY polyplug_abi\n");
        output.push_str("-- DO NOT EDIT BY HAND\n");
        output.push_str("-- Re-generate with: polyplug_abi generate --lang lua\n\n");

        output.push_str("--- ABI constants and types for the polyplug plugin runtime.\n");
        output.push_str(
            "-- This module contains the frozen ABI types that match the Rust ABI exactly.\n",
        );
        output.push_str(
            "-- DO NOT modify field order or sizes — these must match the host runtime.\n\n",
        );

        output.push_str("local ffi = require(\"ffi\")\n");
        output.push_str("local M = {}\n\n");

        output.push_str(
            "-- ─── ABI Constants ────────────────────────────────────────────────────────────\n\n",
        );

        for constant in &info.constants {
            let value: &str = &constant.value;
            let c_type: &str = match constant.type_name.as_str() {
                "u64" => "uint64_t",
                "u32" => "uint32_t",
                "i64" => "int64_t",
                "i32" => "int32_t",
                _ => &constant.type_name,
            };
            output.push_str(&format!(
                "M.{} = ffi.cast(\"{}\", {})\n",
                constant.name, c_type, value
            ));
        }

        output.push('\n');
        output
    }

    fn generate_structs(&self, info: &AbiInfo) -> String {
        let mut output: String = String::new();

        output.push_str(
            "-- ─── ABI Structs ──────────────────────────────────────────────────────────────\n\n",
        );

        output.push_str("ffi.cdef[[\n");

        let struct_names_before: &[&str] = &[
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

        for name in struct_names_before {
            if let Some(struct_info) = struct_map.get(name) {
                output.push_str(&Self::generate_struct(struct_info));
            }
        }

        output.push_str("]]\n\n");

        output.push_str(&self.generate_unions(info));

        output.push_str(
            "-- ─── ABI Structs (after unions) ──────────────────────────────────────────────\n\n",
        );

        output.push_str("ffi.cdef[[\n");

        let struct_names_after: &[&str] = &[
            "PluginDescriptor",
            "PluginInterface",
            "HostVTable",
            "PluginContext",
            "ExtensionEntry",
            "RuntimeConfig",
        ];

        for name in struct_names_after {
            if let Some(struct_info) = struct_map.get(name) {
                output.push_str(&Self::generate_struct(struct_info));
            }
        }

        output.push_str("]]\n\n");
        output
    }

    fn generate_enums(&self, info: &AbiInfo) -> String {
        let mut output: String = String::new();

        output.push_str(
            "-- ─── ABI Enums ────────────────────────────────────────────────────────────────\n\n",
        );

        output.push_str("ffi.cdef[[\n");

        for enum_info in &info.enums {
            output.push_str(&Self::generate_enum(enum_info));
        }

        output.push_str("]]\n\n");
        output
    }

    fn generate_unions(&self, info: &AbiInfo) -> String {
        let mut output: String = String::new();

        output.push_str(
            "-- ─── ABI Unions ───────────────────────────────────────────────────────────────\n\n",
        );

        output.push_str("ffi.cdef[[\n");

        for union_info in &info.unions {
            output.push_str(&Self::generate_union(union_info));
        }

        output.push_str("]]\n\n");
        output
    }

    fn generate_helpers(&self, _info: &AbiInfo) -> String {
        let mut output: String = String::new();

        output.push_str(
            "-- ─── FNV-1a Hash Helpers ──────────────────────────────────────────────────────\n\n",
        );

        output.push_str("local bit = require(\"bit\")\n");
        output.push_str("local FNV_OFFSET = 0xcbf29ce484222325ULL\n");
        output.push_str("local FNV_PRIME = 0x00000100000001B3ULL\n\n");

        output.push_str("--- Compute FNV-1a 64-bit hash of a string.\n");
        output.push_str("-- @param str string  The input string.\n");
        output.push_str("-- @return number     The 64-bit hash value.\n");
        output.push_str("local function fnv1a_64(str)\n");
        output.push_str("    local h = FNV_OFFSET\n");
        output.push_str("    for i = 1, #str do\n");
        output.push_str("        local b = str:byte(i)\n");
        output.push_str("        h = bit.bxor(h, b)\n");
        output.push_str("        h = h * FNV_PRIME\n");
        output.push_str("    end\n");
        output.push_str("    return h\n");
        output.push_str("end\n\n");

        output.push_str(
            "--- Compute the contract ID for \"name@major_version\" using FNV-1a 64-bit.\n",
        );
        output.push_str("-- @param name string         The contract name.\n");
        output.push_str("-- @param major_version number The major version.\n");
        output.push_str("-- @return number             The contract ID.\n");
        output.push_str("function M.contract_id(name, major_version)\n");
        output.push_str("    local s = name .. '@' .. tostring(major_version)\n");
        output.push_str("    return fnv1a_64(s)\n");
        output.push_str("end\n\n");

        output.push_str("--- Compute an extension ID from its name using FNV-1a lower 32 bits.\n");
        output.push_str("-- @param name string  The extension name.\n");
        output.push_str("-- @return number      The extension ID (uint32).\n");
        output.push_str("function M.extension_id(name)\n");
        output.push_str("    local h = fnv1a_64(name)\n");
        output.push_str("    return ffi.cast(\"uint32_t\", h)\n");
        output.push_str("end\n\n");

        output.push_str("--- Compute a bundle ID from its name using FNV-1a 64-bit hash.\n");
        output.push_str("-- @param name string  The bundle name.\n");
        output.push_str("-- @return number      The bundle ID.\n");
        output.push_str("function M.bundle_id(name)\n");
        output.push_str("    return fnv1a_64(name)\n");
        output.push_str("end\n\n");

        output.push_str("return M\n");
        output
    }

    fn file_extension(&self) -> &'static str {
        "lua"
    }

    fn output_dir(&self) -> &'static str {
        "lua"
    }

    fn generate(&self, info: &AbiInfo) -> super::GeneratedFiles {
        let mut files: super::GeneratedFiles = super::GeneratedFiles::new();

        let mut content: String = String::new();
        content.push_str(&self.generate_constants(info));
        content.push_str(&self.generate_enums(info));
        content.push_str(&self.generate_structs(info));
        content.push_str(&self.generate_helpers(info));

        let filename: String = format!("polyplug_abi.{}", self.file_extension());
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
    fn lua_generator_new() {
        let generator: LuaGenerator = LuaGenerator::new();
        assert_eq!(generator.file_extension(), "lua");
        assert_eq!(generator.output_dir(), "lua");
    }

    #[test]
    fn rust_type_to_lua_primitives() {
        assert_eq!(LuaGenerator::rust_type_to_lua("u64"), "uint64_t");
        assert_eq!(LuaGenerator::rust_type_to_lua("u32"), "uint32_t");
        assert_eq!(LuaGenerator::rust_type_to_lua("u16"), "uint16_t");
        assert_eq!(LuaGenerator::rust_type_to_lua("u8"), "uint8_t");
        assert_eq!(LuaGenerator::rust_type_to_lua("i64"), "int64_t");
        assert_eq!(LuaGenerator::rust_type_to_lua("i32"), "int32_t");
        assert_eq!(LuaGenerator::rust_type_to_lua("i16"), "int16_t");
        assert_eq!(LuaGenerator::rust_type_to_lua("i8"), "int8_t");
        assert_eq!(LuaGenerator::rust_type_to_lua("usize"), "size_t");
        assert_eq!(LuaGenerator::rust_type_to_lua("isize"), "ptrdiff_t");
    }

    #[test]
    fn rust_type_to_lua_pointers() {
        assert_eq!(
            LuaGenerator::rust_type_to_lua("*const u8"),
            "const uint8_t*"
        );
        assert_eq!(LuaGenerator::rust_type_to_lua("*mut u8"), "uint8_t*");
        assert_eq!(LuaGenerator::rust_type_to_lua("*const ()"), "const void*");
        assert_eq!(LuaGenerator::rust_type_to_lua("*mut ()"), "void*");
        assert_eq!(LuaGenerator::rust_type_to_lua("*mut c_void"), "void*");
    }

    #[test]
    fn rust_type_to_lua_abi_types() {
        assert_eq!(LuaGenerator::rust_type_to_lua("StringView"), "StringView");
        assert_eq!(LuaGenerator::rust_type_to_lua("Buffer"), "Buffer");
        assert_eq!(LuaGenerator::rust_type_to_lua("AbiError"), "AbiError");
        assert_eq!(
            LuaGenerator::rust_type_to_lua("PluginHandle"),
            "PluginHandle"
        );
    }

    #[test]
    fn format_c_comment_single_line() {
        let result: String = LuaGenerator::format_c_comment("Hello world", 0);
        assert_eq!(result, "// Hello world\n");
    }

    #[test]
    fn format_c_comment_multiple_lines() {
        let result: String = LuaGenerator::format_c_comment("Line 1\nLine 2", 0);
        assert_eq!(result, "// Line 1\n// Line 2\n");
    }

    #[test]
    fn generate_constants_produces_valid_lua() {
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

        let generator: LuaGenerator = LuaGenerator::new();
        let output: String = generator.generate_constants(&info);

        assert!(output.contains("local ffi = require(\"ffi\")"));
        assert!(output.contains("M.ABI_OK = ffi.cast(\"uint32_t\", 0)"));
        assert!(output.contains("M.POLYPLUG_ABI_VERSION = ffi.cast(\"uint32_t\", 1)"));
    }

    #[test]
    fn generate_struct_produces_valid_lua() {
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

        let output: String = LuaGenerator::generate_struct(&struct_info);

        assert!(output.contains("typedef struct StringView"));
        assert!(output.contains("const uint8_t* ptr;"));
        assert!(output.contains("size_t len;"));
    }

    #[test]
    fn generate_enum_produces_valid_lua() {
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

        let output: String = LuaGenerator::generate_enum(&enum_info);

        assert!(output.contains("typedef enum DispatchType"));
        assert!(output.contains("DispatchType_Native = 0"));
        assert!(output.contains("DispatchType_VirtualMachine = 1"));
    }

    #[test]
    fn generate_union_produces_valid_lua() {
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

        let output: String = LuaGenerator::generate_union(&union_info);

        assert!(output.contains("typedef union PluginDispatch"));
        assert!(output.contains("NativeDispatch native;"));
        assert!(output.contains("VmDispatch vm;"));
    }

    #[test]
    fn generate_helpers_produces_valid_lua() {
        let generator: LuaGenerator = LuaGenerator::new();
        let info: AbiInfo = AbiInfo::new();
        let output: String = generator.generate_helpers(&info);

        assert!(output.contains("FNV_OFFSET"));
        assert!(output.contains("FNV_PRIME"));
        assert!(output.contains("function M.contract_id"));
        assert!(output.contains("function M.extension_id"));
        assert!(output.contains("function M.bundle_id"));
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

        let generator: LuaGenerator = LuaGenerator::new();
        let files: GeneratedFiles = generator.generate(&info);

        assert_eq!(files.files.len(), 1);
        assert_eq!(files.files[0].path, PathBuf::from("polyplug_abi.lua"));
        assert!(files.files[0].content.contains("M.ABI_OK"));
        assert!(files.files[0].content.contains("typedef struct StringView"));
    }

    /// Generate the polyplug_abi.lua file for the SDK.
    /// Run with: cargo test --package polyplug_abi -- generate_abi_lua_file --nocapture
    #[test]
    fn generate_abi_lua_file() {
        use crate::build::AbiParser;
        use std::fs;
        use std::path::Path;

        let abi_source: &str = include_str!("../lib.rs");
        let parser: AbiParser = AbiParser::new();
        let info: AbiInfo = parser
            .parse(abi_source)
            .expect("failed to parse ABI source");

        let generator: LuaGenerator = LuaGenerator::new();
        let files: GeneratedFiles = generator.generate(&info);

        let workspace_root: &Path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .expect("failed to find workspace root");
        let output_path: std::path::PathBuf = workspace_root.join("sdks/lua/abi/polyplug_abi.lua");

        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).expect("failed to create output directory");
        }

        fs::write(&output_path, &files.files[0].content).expect("failed to write polyplug_abi.lua");

        println!("Generated: {}", output_path.display());
    }
}

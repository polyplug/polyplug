//! JavaScript/TypeScript ABI Code Generator — generates TypeScript types from ABI type information.
//!
//! This module implements the `AbiGenerator` trait for JavaScript/TypeScript, producing
//! TypeScript type definitions and helper functions for the polyplug ABI.
//!
//! # Key Design Decisions
//!
//! - **BigInt for 64-bit values**: JavaScript `number` is a 64-bit float and loses precision
//!   for integers above 2^53. We use `bigint` for `u64`, `i64`, and pointer types.
//! - **TypeScript types**: Generate `.ts` files with proper type annotations.
//! - **No PluginVTable**: Only `PluginInterface` is generated (per plan requirements).
//! - **String helpers**: Native TypeScript implementations for zero overhead.

use super::{AbiGenerator, AbiInfo, EnumInfo, StructInfo, UnionInfo};
use std::path::PathBuf;

/// JavaScript/TypeScript ABI code generator.
///
/// Generates TypeScript bindings for the polyplug ABI types, including:
/// - Constants as `export const` with type annotations
/// - Structs as TypeScript interfaces
/// - Enums as TypeScript string literal unions
/// - Unions as TypeScript discriminated unions
/// - FNV-1a hash helper functions
/// - String helper functions (stripPrefix, startsWith, split)
#[derive(Debug, Clone, Copy, Default)]
pub struct JsGenerator;

impl JsGenerator {
    /// Create a new JavaScript generator.
    pub fn new() -> JsGenerator {
        JsGenerator
    }

    /// Convert a Rust type name to a TypeScript type name.
    ///
    /// # Type Mappings
    /// - `u64`, `i64` → `bigint` (BigInt for 64-bit precision)
    /// - `u32`, `i32`, `u16`, `i16`, `u8`, `i8` → `number`
    /// - `usize`, `isize` → `number` (platform-dependent, but fits in number)
    /// - `bool` → `boolean`
    /// - `*const T`, `*mut T` → `bigint` (pointer as BigInt)
    /// - `*const ()`, `*mut ()` → `bigint` (void pointer)
    /// - `*mut c_void` → `bigint`
    /// - ABI struct names → same name (StringView, Buffer, etc.)
    fn rust_type_to_ts(type_name: &str) -> String {
        // Handle function pointer types (extern "C" fn)
        if type_name.contains("extern\"C\"fn") || type_name.contains("extern\"C\"") {
            return Self::convert_function_pointer(type_name);
        }

        // Handle double pointer: *const*const() → bigint
        if type_name.starts_with("*const*const")
            || type_name.starts_with("*mut*const")
            || type_name.starts_with("*const*mut")
            || type_name.starts_with("*mut*mut")
        {
            return String::from("bigint");
        }

        // Handle pointer types
        if type_name.starts_with('*') {
            // All pointers become bigint in TypeScript
            return String::from("bigint");
        }

        // Handle c_void (non-pointer context, should not happen in practice)
        if type_name.contains("c_void") {
            return String::from("void");
        }

        match type_name {
            "u64" | "i64" => String::from("bigint"),
            "u32" | "i32" | "u16" | "i16" | "u8" | "i8" => String::from("number"),
            "usize" | "isize" => String::from("number"),
            "bool" => String::from("boolean"),
            "()" => String::from("void"),
            _ => String::from(type_name),
        }
    }

    /// Convert a Rust function pointer type to TypeScript function type.
    fn convert_function_pointer(type_name: &str) -> String {
        // Extract return type
        let return_type: &str = if let Some(pos) = type_name.find(")->") {
            &type_name[pos + 3..]
        } else {
            "void"
        };

        let ts_return: String = Self::rust_type_to_ts(return_type);

        // Extract parameters
        let params_start: usize = type_name.find("fn(").map(|p| p + 3).unwrap_or(0);
        let params_end: usize = type_name.find(")->").unwrap_or(type_name.len());

        if params_start == 0 || params_end <= params_start {
            return format!("() => {}", ts_return);
        }

        let params_str: &str = &type_name[params_start..params_end];
        let params: Vec<String> = Self::parse_function_params(params_str);

        if params.is_empty() {
            return format!("() => {}", ts_return);
        }

        format!("({}) => {}", params.join(", "), ts_return)
    }

    /// Parse function parameters from a Rust function signature.
    fn parse_function_params(params_str: &str) -> Vec<String> {
        if params_str.is_empty() {
            return Vec::new();
        }

        let mut params: Vec<String> = Vec::new();
        let mut current_param: String = String::new();
        let mut depth: i32 = 0;
        let mut param_index: usize = 0;

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
                        params.push(Self::convert_param(&param, param_index));
                        param_index += 1;
                    }
                    current_param.clear();
                }
                _ => {
                    current_param.push(c);
                }
            }
        }

        if !current_param.trim().is_empty() {
            params.push(Self::convert_param(current_param.trim(), param_index));
        }

        params
    }

    /// Convert a single parameter to TypeScript syntax.
    fn convert_param(param: &str, index: usize) -> String {
        // Format: name:type or just type
        let parts: Vec<&str> = param.splitn(2, ':').collect();
        let type_part: &str = if parts.len() == 2 { parts[1] } else { parts[0] };
        let ts_type: String = Self::rust_type_to_ts(type_part.trim());
        let name: &str = if parts.len() == 2 {
            parts[0].trim()
        } else {
            // Generate a parameter name based on index
            match index {
                0 => "arg0",
                1 => "arg1",
                2 => "arg2",
                3 => "arg3",
                4 => "arg4",
                5 => "arg5",
                _ => {
                    return format!("arg{}: {}", index, ts_type);
                }
            }
        };
        format!("{}: {}", name, ts_type)
    }

    /// Generate a JSDoc comment.
    fn format_jsdoc(doc: &str, indent: usize) -> String {
        let indent_str: String = " ".repeat(indent);
        let lines: Vec<&str> = doc.lines().collect();
        if lines.len() == 1 {
            format!("{}/** {} */\n", indent_str, lines[0])
        } else {
            let mut output: String = format!("{}/**\n", indent_str);
            for line in lines {
                output.push_str(&format!("{} * {}\n", indent_str, line));
            }
            output.push_str(&format!("{} */\n", indent_str));
            output
        }
    }

    /// Generate a single struct definition as a TypeScript interface.
    fn generate_struct(struct_info: &StructInfo) -> String {
        let mut output: String = String::new();

        if let Some(doc) = &struct_info.doc {
            output.push_str(&Self::format_jsdoc(doc, 0));
        }

        output.push_str(&format!("export interface {} {{\n", struct_info.name));

        for field in &struct_info.fields {
            if let Some(doc) = &field.doc {
                output.push_str(&Self::format_jsdoc(doc, 4));
            }

            let ts_type: String = Self::rust_type_to_ts(&field.type_name);
            output.push_str(&format!("    {}: {};\n", field.name, ts_type));
        }

        output.push_str("}\n\n");
        output
    }

    /// Generate a single enum definition as a TypeScript const enum.
    fn generate_enum(enum_info: &EnumInfo) -> String {
        let mut output: String = String::new();

        if let Some(doc) = &enum_info.doc {
            output.push_str(&Self::format_jsdoc(doc, 0));
        }

        output.push_str(&format!("export const enum {} {{\n", enum_info.name));

        for (i, variant) in enum_info.variants.iter().enumerate() {
            if let Some(doc) = &variant.doc {
                output.push_str(&Self::format_jsdoc(doc, 4));
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

    /// Generate a single union definition as a TypeScript interface with discriminant.
    fn generate_union(union_info: &UnionInfo) -> String {
        let mut output: String = String::new();

        if let Some(doc) = &union_info.doc {
            output.push_str(&Self::format_jsdoc(doc, 0));
        }

        // Generate as a union type with nested interfaces
        output.push_str(&format!("export type {} =\n", union_info.name));

        for variant in union_info.variants.iter() {
            let ts_type: String = Self::rust_type_to_ts(&variant.type_name);
            output.push_str(&format!("    | {{ {}: {} }}\n", variant.name, ts_type));
        }

        output.push_str(";\n\n");
        output
    }
}

impl AbiGenerator for JsGenerator {
    fn generate_constants(&self, info: &AbiInfo) -> String {
        let mut output: String = String::new();

        output.push_str("// THIS FILE IS AUTO-GENERATED BY polyplug_abi\n");
        output.push_str("// DO NOT EDIT BY HAND\n");
        output.push_str("// Re-generate with: polyplug_abi generate --lang js\n\n");

        output.push_str("/**\n");
        output.push_str(" * ABI constants and types for the polyplug plugin runtime.\n");
        output.push_str(
            " * This module contains the frozen ABI types that match the Rust ABI exactly.\n",
        );
        output.push_str(
            " * DO NOT modify field order or sizes — these must match the host runtime.\n",
        );
        output.push_str(" *\n");
        output.push_str(" * @module polyplug_abi\n");
        output.push_str(" */\n\n");

        output.push_str(
            "// ─── ABI Constants ────────────────────────────────────────────────────────────\n\n",
        );

        for constant in &info.constants {
            let ts_type: String = Self::rust_type_to_ts(&constant.type_name);
            let value: &str = &constant.value;

            // Format the value appropriately for the type
            let formatted_value: String = if ts_type == "bigint" {
                format!("{}n", value)
            } else {
                String::from(value)
            };

            output.push_str(&format!("/** ABI constant: {} */\n", constant.name));
            output.push_str(&format!(
                "export const {}: {} = {};\n\n",
                constant.name, ts_type, formatted_value
            ));
        }

        output
    }

    fn generate_structs(&self, info: &AbiInfo) -> String {
        let mut output: String = String::new();

        output.push_str(
            "// ─── ABI Structs ──────────────────────────────────────────────────────────────\n\n",
        );

        // Define structs in dependency order
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

        output.push_str(&self.generate_unions(info));

        output.push_str(
            "// ─── ABI Structs (after unions) ──────────────────────────────────────────────\n\n",
        );

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

        for union_info in &info.unions {
            output.push_str(&Self::generate_union(union_info));
        }

        output
    }

    fn generate_helpers(&self, _info: &AbiInfo) -> String {
        let mut output: String = String::new();

        output.push_str(
            "// ─── FNV-1a Hash Helpers ──────────────────────────────────────────────────────\n\n",
        );

        output.push_str("/** FNV-1a offset basis for 64-bit hash */\n");
        output.push_str("const FNV_OFFSET: bigint = 0xcbf29ce484222325n;\n");
        output.push_str("/** FNV-1a prime for 64-bit hash */\n");
        output.push_str("const FNV_PRIME: bigint = 0x00000100000001B3n;\n");
        output.push_str("/** 64-bit mask */\n");
        output.push_str("const MASK_64: bigint = 0xFFFFFFFFFFFFFFFFn;\n\n");

        output.push_str("/**\n");
        output.push_str(" * Compute FNV-1a 64-bit hash of a string.\n");
        output.push_str(" * @param str - The input string.\n");
        output.push_str(" * @returns The 64-bit hash value as bigint.\n");
        output.push_str(" */\n");
        output.push_str("export function fnv1a_64(str: string): bigint {\n");
        output.push_str("    let h: bigint = FNV_OFFSET;\n");
        output.push_str("    const encoder = new TextEncoder();\n");
        output.push_str("    const bytes = encoder.encode(str);\n");
        output.push_str("    for (const b of bytes) {\n");
        output.push_str("        h = (h ^ BigInt(b)) * FNV_PRIME;\n");
        output.push_str("        h = h & MASK_64;\n");
        output.push_str("    }\n");
        output.push_str("    return h;\n");
        output.push_str("}\n\n");

        output.push_str("/**\n");
        output.push_str(
            " * Compute the contract ID for \"name@major_version\" using FNV-1a 64-bit.\n",
        );
        output.push_str(" * @param name - The contract name.\n");
        output.push_str(" * @param majorVersion - The major version.\n");
        output.push_str(" * @returns The contract ID as bigint.\n");
        output.push_str(" */\n");
        output
            .push_str("export function contractId(name: string, majorVersion: number): bigint {\n");
        output.push_str("    return fnv1a_64(`${name}@${majorVersion}`);\n");
        output.push_str("}\n\n");

        output.push_str("/**\n");
        output.push_str(" * Compute an extension ID from its name using FNV-1a lower 32 bits.\n");
        output.push_str(" * @param name - The extension name.\n");
        output.push_str(" * @returns The extension ID as number (uint32).\n");
        output.push_str(" */\n");
        output.push_str("export function extensionId(name: string): number {\n");
        output.push_str("    const h: bigint = fnv1a_64(name);\n");
        output.push_str("    return Number(h & 0xFFFFFFFFn);\n");
        output.push_str("}\n\n");

        output.push_str("/**\n");
        output.push_str(" * Compute a bundle ID from its name using FNV-1a 64-bit hash.\n");
        output.push_str(" * @param name - The bundle name.\n");
        output.push_str(" * @returns The bundle ID as bigint.\n");
        output.push_str(" */\n");
        output.push_str("export function bundleId(name: string): bigint {\n");
        output.push_str("    return fnv1a_64(name);\n");
        output.push_str("}\n\n");

        // String helpers
        output.push_str("// ─── String Helpers ────────────────────────────────────────────────────────────\n\n");

        output.push_str("/**\n");
        output.push_str(" * Convert a StringView to a JavaScript string.\n");
        output.push_str(" * @param sv - The StringView to convert.\n");
        output.push_str(" * @returns The JavaScript string, or empty string if null/empty.\n");
        output.push_str(" */\n");
        output.push_str(
            "export function stringViewToString(sv: StringView | null | undefined): string {\n",
        );
        output.push_str("    if (!sv || sv.ptr === 0n || sv.len === 0) return '';\n");
        output.push_str("    // Note: Actual implementation requires FFI access to read memory.\n");
        output.push_str("    // This is a placeholder - the host/guest libraries provide actual implementation.\n");
        output.push_str("    return '';\n");
        output.push_str("}\n\n");

        output.push_str("/**\n");
        output.push_str(" * Strip a prefix from a string.\n");
        output.push_str(" * @param sv - The input StringView or string.\n");
        output.push_str(" * @param prefix - The prefix to strip.\n");
        output.push_str(
            " * @returns The string without prefix, or original if prefix not present.\n",
        );
        output.push_str(" */\n");
        output.push_str(
            "export function stripPrefix(sv: StringView | string, prefix: string): string {\n",
        );
        output.push_str(
            "    const s: string = typeof sv === 'string' ? sv : stringViewToString(sv);\n",
        );
        output.push_str("    if (s.startsWith(prefix)) {\n");
        output.push_str("        return s.slice(prefix.length);\n");
        output.push_str("    }\n");
        output.push_str("    return s;\n");
        output.push_str("}\n\n");

        output.push_str("/**\n");
        output.push_str(" * Check if a string starts with a prefix.\n");
        output.push_str(" * @param sv - The input StringView or string.\n");
        output.push_str(" * @param prefix - The prefix to check.\n");
        output.push_str(" * @returns True if the string starts with the prefix.\n");
        output.push_str(" */\n");
        output.push_str(
            "export function startsWith(sv: StringView | string, prefix: string): boolean {\n",
        );
        output.push_str(
            "    const s: string = typeof sv === 'string' ? sv : stringViewToString(sv);\n",
        );
        output.push_str("    return s.startsWith(prefix);\n");
        output.push_str("}\n\n");

        output.push_str("/**\n");
        output.push_str(" * Split a string by a delimiter.\n");
        output.push_str(" * @param sv - The input StringView or string.\n");
        output.push_str(" * @param delimiter - The delimiter to split by.\n");
        output.push_str(" * @returns An array of strings.\n");
        output.push_str(" */\n");
        output.push_str(
            "export function split(sv: StringView | string, delimiter: string): string[] {\n",
        );
        output.push_str(
            "    const s: string = typeof sv === 'string' ? sv : stringViewToString(sv);\n",
        );
        output.push_str("    return s.split(delimiter);\n");
        output.push_str("}\n");

        output
    }

    fn file_extension(&self) -> &'static str {
        "ts"
    }

    fn output_dir(&self) -> &'static str {
        "js"
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
    fn js_generator_new() {
        let generator: JsGenerator = JsGenerator::new();
        assert_eq!(generator.file_extension(), "ts");
        assert_eq!(generator.output_dir(), "js");
    }

    #[test]
    fn rust_type_to_ts_primitives() {
        assert_eq!(JsGenerator::rust_type_to_ts("u64"), "bigint");
        assert_eq!(JsGenerator::rust_type_to_ts("i64"), "bigint");
        assert_eq!(JsGenerator::rust_type_to_ts("u32"), "number");
        assert_eq!(JsGenerator::rust_type_to_ts("i32"), "number");
        assert_eq!(JsGenerator::rust_type_to_ts("u16"), "number");
        assert_eq!(JsGenerator::rust_type_to_ts("i16"), "number");
        assert_eq!(JsGenerator::rust_type_to_ts("u8"), "number");
        assert_eq!(JsGenerator::rust_type_to_ts("i8"), "number");
        assert_eq!(JsGenerator::rust_type_to_ts("usize"), "number");
        assert_eq!(JsGenerator::rust_type_to_ts("isize"), "number");
        assert_eq!(JsGenerator::rust_type_to_ts("bool"), "boolean");
    }

    #[test]
    fn rust_type_to_ts_pointers() {
        assert_eq!(JsGenerator::rust_type_to_ts("*const u8"), "bigint");
        assert_eq!(JsGenerator::rust_type_to_ts("*mut u8"), "bigint");
        assert_eq!(JsGenerator::rust_type_to_ts("*const ()"), "bigint");
        assert_eq!(JsGenerator::rust_type_to_ts("*mut ()"), "bigint");
        assert_eq!(JsGenerator::rust_type_to_ts("*mut c_void"), "bigint");
    }

    #[test]
    fn rust_type_to_ts_abi_types() {
        assert_eq!(JsGenerator::rust_type_to_ts("StringView"), "StringView");
        assert_eq!(JsGenerator::rust_type_to_ts("Buffer"), "Buffer");
        assert_eq!(JsGenerator::rust_type_to_ts("AbiError"), "AbiError");
        assert_eq!(JsGenerator::rust_type_to_ts("PluginHandle"), "PluginHandle");
    }

    #[test]
    fn format_jsdoc_single_line() {
        let result: String = JsGenerator::format_jsdoc("Hello world", 0);
        assert_eq!(result, "/** Hello world */\n");
    }

    #[test]
    fn format_jsdoc_multiple_lines() {
        let result: String = JsGenerator::format_jsdoc("Line 1\nLine 2", 0);
        assert_eq!(result, "/**\n * Line 1\n * Line 2\n */\n");
    }

    #[test]
    fn generate_constants_produces_valid_ts() {
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

        let generator: JsGenerator = JsGenerator::new();
        let output: String = generator.generate_constants(&info);

        assert!(output.contains("export const ABI_OK: number = 0;"));
        assert!(output.contains("export const POLYPLUG_ABI_VERSION: number = 1;"));
    }

    #[test]
    fn generate_constants_bigint() {
        let mut info: AbiInfo = AbiInfo::new();
        info.add_constant(ConstantInfo {
            name: String::from("TEST_U64"),
            value: String::from("12345678901234567890"),
            type_name: String::from("u64"),
        });

        let generator: JsGenerator = JsGenerator::new();
        let output: String = generator.generate_constants(&info);

        assert!(output.contains("export const TEST_U64: bigint = 12345678901234567890n;"));
    }

    #[test]
    fn generate_struct_produces_valid_ts() {
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

        let output: String = JsGenerator::generate_struct(&struct_info);

        assert!(output.contains("export interface StringView"));
        assert!(output.contains("ptr: bigint;"));
        assert!(output.contains("len: number;"));
    }

    #[test]
    fn generate_enum_produces_valid_ts() {
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

        let output: String = JsGenerator::generate_enum(&enum_info);

        assert!(output.contains("export const enum DispatchType"));
        assert!(output.contains("Native = 0"));
        assert!(output.contains("VirtualMachine = 1"));
    }

    #[test]
    fn generate_union_produces_valid_ts() {
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

        let output: String = JsGenerator::generate_union(&union_info);

        assert!(output.contains("export type PluginDispatch"));
        assert!(output.contains("native: NativeDispatch"));
        assert!(output.contains("vm: VmDispatch"));
    }

    #[test]
    fn generate_helpers_produces_valid_ts() {
        let generator: JsGenerator = JsGenerator::new();
        let info: AbiInfo = AbiInfo::new();
        let output: String = generator.generate_helpers(&info);

        assert!(output.contains("const FNV_OFFSET: bigint"));
        assert!(output.contains("const FNV_PRIME: bigint"));
        assert!(output.contains("export function fnv1a_64"));
        assert!(output.contains("export function contractId"));
        assert!(output.contains("export function extensionId"));
        assert!(output.contains("export function bundleId"));
        assert!(output.contains("export function stripPrefix"));
        assert!(output.contains("export function startsWith"));
        assert!(output.contains("export function split"));
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

        let generator: JsGenerator = JsGenerator::new();
        let files: GeneratedFiles = generator.generate(&info);

        assert_eq!(files.files.len(), 1);
        assert_eq!(files.files[0].path, PathBuf::from("polyplug_abi.ts"));
        assert!(files.files[0].content.contains("export const ABI_OK"));
        assert!(
            files.files[0]
                .content
                .contains("export interface StringView")
        );
    }

    /// Generate the polyplug_abi.ts file for the SDK.
    /// Run with: cargo test --package polyplug_abi -- generate_abi_ts_file --ignored --nocapture
    #[test]
    #[ignore]
    fn generate_abi_ts_file() {
        use crate::build::AbiParser;
        use std::fs;
        use std::path::Path;

        let abi_source: &str = include_str!("../lib.rs");
        let parser: AbiParser = AbiParser::new();
        let info: AbiInfo = parser
            .parse(abi_source)
            .expect("failed to parse ABI source");

        let generator: JsGenerator = JsGenerator::new();
        let files: GeneratedFiles = generator.generate(&info);

        let workspace_root: &Path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .expect("failed to find workspace root");
        let output_path: std::path::PathBuf = workspace_root.join("sdks/js/abi/polyplug_abi.ts");

        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).expect("failed to create output directory");
        }

        fs::write(&output_path, &files.files[0].content).expect("failed to write polyplug_abi.ts");

        println!("Generated: {}", output_path.display());
    }
}

//! JavaScript/TypeScript code generator — produces TypeScript types from ABI items.
//!
//! Per D-33: emits BOTH TypeScript interfaces AND binary offset constants
//! for DataView/UnsafePointerView access. Targets Deno host per D-34.

use crate::data::{ConstInfo, EnumInfo, FunctionInfo, StructInfo, UnionInfo};
use crate::error::PolyplugcError;
use crate::languages::{CodeGenerator, GenerationContext};

/// JavaScript/TypeScript ABI code generator.
pub struct JsGenerator;

impl JsGenerator {
    pub fn new() -> Self {
        JsGenerator
    }

    /// Check if a rust_type represents Array<T>.
    fn is_array(rust_type: &str) -> bool {
        rust_type.starts_with("Array<")
    }

    /// Parse a fixed-size array type in compacted `quote!()` form, e.g. `[u8;32]`.
    ///
    /// Returns `(element_size_bytes, count)` when the input matches `[T;N]` where T
    /// is a known primitive and N is a positive integer; returns `None` otherwise.
    fn parse_fixed_array(rust_type: &str) -> Option<(usize, usize)> {
        let inner: &str = rust_type.strip_prefix('[')?.strip_suffix(']')?;
        let semi: usize = inner.find(';')?;
        let elem: &str = inner[..semi].trim();
        let count_str: &str = inner[semi + 1..].trim();
        let count: usize = count_str.parse().ok().filter(|&n: &usize| n > 0)?;
        let elem_size: usize = match elem {
            "u8" | "i8" => 1,
            "u16" | "i16" => 2,
            "u32" | "i32" => 4,
            "u64" | "i64" => 8,
            _ => return None,
        };
        Some((elem_size, count))
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

    fn rust_type_to_ts(rust_type: &str) -> String {
        // Handle Option<...> wrapper.
        if Self::is_option(rust_type) {
            let inner: &str = &rust_type["Option<".len()..rust_type.len() - 1];
            let ts_inner: String = Self::rust_type_to_ts(inner);
            // In TypeScript, function pointer Option is just the type itself (can be null).
            return ts_inner;
        }

        // Handle Array<T> — return a typed object.
        if Self::is_array(rust_type) {
            return String::from("{ items: number; len: number; align: number }");
        }

        // Handle fixed-size primitive arrays, e.g. `[u8;32]` from `quote!()`.
        // TS interfaces are structural; the raw bytes live in the FFI buffer,
        // so a plain `number[]` field type conveys the right intent to callers.
        if Self::parse_fixed_array(rust_type).is_some() {
            return String::from("number[]");
        }

        if rust_type.contains("extern\"C\"fn") || rust_type.contains("extern\"C\"") {
            return String::from("number");
        }

        if rust_type.starts_with("*const*const")
            || rust_type.starts_with("*mut*const")
            || rust_type.starts_with("*const*mut")
            || rust_type.starts_with("*mut*mut")
        {
            return String::from("bigint");
        }

        if rust_type.starts_with('*') {
            return String::from("bigint");
        }

        if rust_type.contains("c_void") {
            return String::from("void");
        }

        match rust_type {
            // `#[repr(transparent)]` u64 newtypes from polyplug_utils.
            "u64" | "i64" | "BundleId" | "GuestContractId" | "HostContractId" => {
                String::from("bigint")
            }
            "u32" | "i32" | "u16" | "i16" | "u8" | "i8" => String::from("number"),
            "usize" | "isize" => String::from("number"),
            "bool" => String::from("boolean"),
            "()" => String::from("void"),
            other => String::from(other),
        }
    }

    /// Look up the (size, alignment) of a named ABI type.
    ///
    /// These must match the Rust `#[repr(C)]` layouts exactly.
    fn named_type_layout(type_str: &str) -> Option<(usize, usize)> {
        match type_str {
            // Structs — sizes from Rust offset_of / std::mem::size_of
            "StringView" => Some((16, 8)),       // { ptr(8), len(8) }
            "Version" => Some((12, 4)),          // { major(4), minor(4), patch(4) }
            "Buffer" => Some((24, 8)),           // { ptr(8), len(8), align(8) }
            "AbiError" => Some((24, 8)),         // { code(4), _pad(4), message(16) }
            "Array" => Some((24, 8)),            // { items(8), len(8), align(8) }
            "PluginDescriptor" => Some((48, 8)), // { name(16), contract_name(16), version(12), _pad(4) }
            // Enums are resolved from `GenerationContext::enum_reprs` in
            // `type_size` / `type_align`, keyed by their actual Rust `repr`.
            _ => None,
        }
    }

    /// Map a Rust enum `repr` string to its `(size, alignment)` in bytes.
    fn repr_layout(repr: &str) -> Option<(usize, usize)> {
        match repr {
            "u8" | "i8" => Some((1, 1)),
            "u16" | "i16" => Some((2, 2)),
            "u32" | "i32" => Some((4, 4)),
            "u64" | "i64" => Some((8, 8)),
            _ => None,
        }
    }

    /// Compute the byte size of a known Rust type for offset calculation.
    fn type_size(rust_type: &str, ctx: &GenerationContext) -> usize {
        let type_str: &str = Self::strip_option(rust_type);
        if type_str.contains("extern\"C\"fn") || type_str.contains("extern\"C\"") {
            return 8; // fn pointer = 8 bytes on 64-bit
        }
        if Self::is_array(rust_type) {
            return 24; // Array<T>: ptr(8) + len(8) + align(8) = 24 bytes
        }
        // Fixed-size primitive array: total bytes = elem_size * count.
        if let Some((elem_size, count)) = Self::parse_fixed_array(type_str) {
            return elem_size * count;
        }
        if type_str.starts_with('*') {
            return 8; // raw pointer
        }
        if let Some((size, _)) = Self::named_type_layout(type_str) {
            return size;
        }
        if let Some((size, _)) = ctx
            .enum_reprs
            .get(type_str)
            .and_then(|repr: &String| Self::repr_layout(repr))
        {
            return size;
        }
        match type_str {
            "u64" | "i64" | "usize" | "isize" => 8,
            "u32" | "i32" => 4,
            "u16" | "i16" => 2,
            "u8" | "i8" | "bool" => 1,
            _ => 8, // Assume 8 bytes for unknown types (pointer-sized)
        }
    }

    /// Compute the alignment of a known Rust type.
    fn type_align(rust_type: &str, ctx: &GenerationContext) -> usize {
        let type_str: &str = Self::strip_option(rust_type);
        if type_str.contains("extern\"C\"fn") || type_str.contains("extern\"C\"") {
            return 8;
        }
        if Self::is_array(rust_type) {
            return 8;
        }
        // Fixed-size primitive array: alignment equals the element alignment.
        if let Some((elem_size, _count)) = Self::parse_fixed_array(type_str) {
            return elem_size;
        }
        if type_str.starts_with('*') {
            return 8;
        }
        if let Some((_, align)) = Self::named_type_layout(type_str) {
            return align;
        }
        if let Some((_, align)) = ctx
            .enum_reprs
            .get(type_str)
            .and_then(|repr: &String| Self::repr_layout(repr))
        {
            return align;
        }
        match type_str {
            "u64" | "i64" | "usize" | "isize" => 8,
            "u32" | "i32" => 4,
            "u16" | "i16" => 2,
            "u8" | "i8" | "bool" => 1,
            _ => 8,
        }
    }

    /// Align `offset` up to the given alignment boundary.
    fn align_up(offset: usize, align: usize) -> usize {
        (offset + align - 1) & !(align - 1)
    }

    fn format_jsdoc(doc: &str, indent: usize) -> String {
        let indent_str: String = " ".repeat(indent);
        let lines: Vec<&str> = doc.lines().collect();
        if lines.len() == 1 {
            format!("{}/** {} */\n", indent_str, lines[0])
        } else {
            let mut output: String = format!("{}/**\n", indent_str);
            for line in lines {
                output.push_str(&format!(
                    "{} *{}\n",
                    indent_str,
                    if line.is_empty() {
                        String::new()
                    } else {
                        format!(" {line}")
                    }
                ));
            }
            output.push_str(&format!("{} */\n", indent_str));
            output
        }
    }
}

impl CodeGenerator for JsGenerator {
    fn generate_const(
        &self,
        item: &ConstInfo,
        _ctx: &GenerationContext,
    ) -> Result<String, PolyplugcError> {
        // Stays hand-emitted WIRING. This is a bare typed module constant
        // (`export const NAME: number = VALUE;`) — the exact FORM the offset/size
        // constants in `generate_struct` also use. langprint's JS backend models
        // plain-JavaScript runtime declarations (an untyped `class`, an
        // `Object.freeze` const-object enum, a `function`), not TypeScript
        // type-level declarations, so it has no typed-const FORM to delegate to;
        // the `bigint` → `{value}n` suffix mapping is polyplug LOGIC emitted
        // directly (the Python ABI mirror likewise keeps its module constant local).
        let ts_type: String = Self::rust_type_to_ts(&item.rust_type);
        let formatted_value: String = if ts_type == "bigint" {
            format!("{}n", item.value)
        } else {
            item.value.clone()
        };

        Ok(format!(
            "export const {}: {} = {};\n\n",
            item.name, ts_type, formatted_value
        ))
    }

    fn generate_struct(
        &self,
        item: &StructInfo,
        ctx: &GenerationContext,
    ) -> Result<String, PolyplugcError> {
        // Stays hand-emitted WIRING. The mirror's struct FORM is a TypeScript
        // `export interface Name { field: type; }` followed by a block of
        // `export const NAME_FIELD_OFFSET: number = N;` layout constants.
        // langprint's JS backend renders PLAIN JavaScript — an untyped
        // `class Name { field = value; }`, never a typed `interface` — so there is
        // no interface FORM to delegate to. Everything here is polyplug LOGIC
        // regardless: rust→TS type mapping, Array<T> field expansion, fn-pointer →
        // `number`, and the field offset/size arithmetic that dominates the output.
        // (Contrast the C#/Python mirrors, whose langprint backends genuinely model
        // the target's struct declaration; the JS backend models no TS type form.)
        let mut output = String::new();

        if let Some(doc) = &item.doc {
            output.push_str(&Self::format_jsdoc(doc, 0));
        }

        // TypeScript interface.
        output.push_str(&format!("export interface {} {{\n", item.name));

        for field in &item.fields {
            if let Some(doc) = &field.doc {
                output.push_str(&Self::format_jsdoc(doc, 4));
            }

            // Handle Array<T> — expand into 3 sub-fields.
            if Self::is_array(&field.rust_type) {
                output.push_str(&format!("    {}: number;\n", field.name));
                output.push_str(&format!("    {}_len: number;\n", field.name));
                output.push_str(&format!("    {}__align: number;\n", field.name));
                continue;
            }

            let ts_type: String = Self::rust_type_to_ts(&field.rust_type);
            output.push_str(&format!("    {}: {};\n", field.name, ts_type));
        }

        output.push_str("}\n\n");

        // Binary offset constants per D-33.
        let struct_align: usize = item
            .fields
            .iter()
            .map(|f| Self::type_align(&f.rust_type, ctx))
            .max()
            .unwrap_or(1);

        let mut offset: usize = 0usize;
        let mut offset_constants: String = String::new();

        for field in &item.fields {
            let field_align: usize = Self::type_align(&field.rust_type, ctx);
            offset = Self::align_up(offset, field_align);

            let const_name: String = format!(
                "{}_{}_OFFSET",
                to_upper_snake_case(&item.name),
                to_upper_snake_case(&field.name)
            );
            offset_constants.push_str(&format!(
                "export const {}: number = {};\n",
                const_name, offset
            ));

            if Self::is_array(&field.rust_type) {
                // Array<T> expands to 3 consecutive fields.
                offset += 8; // items pointer
                offset_constants.push_str(&format!(
                    "export const {}_LEN_OFFSET: number = {};\n",
                    to_upper_snake_case(&format!("{}_{}", item.name, field.name)),
                    offset
                ));
                offset += 8; // len
                offset_constants.push_str(&format!(
                    "export const {}_ALIGN_OFFSET: number = {};\n",
                    to_upper_snake_case(&format!("{}_{}", item.name, field.name)),
                    offset
                ));
                offset += 8; // align
            } else {
                offset += Self::type_size(&field.rust_type, ctx);
            }
        }

        // Total struct size constant — prefer size_hint from Rust if available.
        let total_size: usize = item
            .size_hint
            .unwrap_or_else(|| Self::align_up(offset, struct_align));
        offset_constants.push_str(&format!(
            "export const {}_SIZE: number = {};\n\n",
            to_upper_snake_case(&item.name),
            total_size
        ));

        output.push_str(&offset_constants);
        Ok(output)
    }

    fn generate_enum(
        &self,
        item: &EnumInfo,
        _ctx: &GenerationContext,
    ) -> Result<String, PolyplugcError> {
        // Stays hand-emitted WIRING. The mirror's enum FORM is a TypeScript
        // `export const enum Name { Member = value, }` block with per-variant JSDoc.
        // langprint's JS `render_enum` produces the runtime const-object idiom
        // instead (`export const Name = Object.freeze({ … } as const);` plus a
        // companion `type`) and carries no per-member doc — a different artifact
        // from the mirror's `const enum`, so it cannot be delegated. The
        // explicit-value LOGIC (valueless first variant pinned to 0, later valueless
        // variants take their ordinal index) is emitted directly.
        let mut output = String::new();

        if let Some(doc) = &item.doc {
            output.push_str(&Self::format_jsdoc(doc, 0));
        }

        output.push_str(&format!("export const enum {} {{\n", item.name));

        for (i, variant) in item.variants.iter().enumerate() {
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
        Ok(output)
    }

    fn generate_union(
        &self,
        item: &UnionInfo,
        _ctx: &GenerationContext,
    ) -> Result<String, PolyplugcError> {
        // Stays hand-emitted WIRING. The mirror's union FORM is a TypeScript
        // discriminated union (`export type Name = | { variant: type } … ;`).
        // langprint's JS backend has no `type`/union FORM at all, and this ABI has
        // exactly ONE union — adding a whole JsUnion type + renderer (plus JSDoc)
        // to langprint to replace a single ~6-line string join would be a
        // negative-value cross-crate abstraction (the variant type-string mapping
        // is polyplug LOGIC regardless). Emitted directly.
        let mut output = String::new();

        if let Some(doc) = &item.doc {
            output.push_str(&Self::format_jsdoc(doc, 0));
        }

        output.push_str(&format!("export type {} =\n", item.name));

        for variant in item.variants.iter() {
            let ts_type: String = Self::rust_type_to_ts(&variant.type_name);
            output.push_str(&format!("    | {{ {}: {} }}\n", variant.name, ts_type));
        }

        output.push_str(";\n\n");
        Ok(output)
    }

    fn generate_function(
        &self,
        item: &FunctionInfo,
        _ctx: &GenerationContext,
    ) -> Result<String, PolyplugcError> {
        let ret_type: String = item
            .return_type
            .as_ref()
            .map(|t| Self::rust_type_to_ts(t))
            .unwrap_or_else(|| "void".to_string());

        let params: String = item
            .params
            .iter()
            .map(|p| format!("{}: {}", p.name, Self::rust_type_to_ts(&p.rust_type)))
            .collect::<Vec<_>>()
            .join(", ");

        Ok(format!(
            "export function {}({}): {} {{}}\n\n",
            item.name, params, ret_type
        ))
    }

    fn file_extension(&self) -> &'static str {
        "ts"
    }

    fn language_name(&self) -> &'static str {
        "js"
    }

    fn generate_header(&self, _ctx: &GenerationContext) -> Result<String, PolyplugcError> {
        // No header WIRING: the abi.ts mirror opens straight into declarations
        // (the file banner is prepended by the build script). langprint models no
        // whole-module JS/TS scaffold to delegate to here.
        Ok(String::new())
    }
}

/// Convert a string to UPPER_SNAKE_CASE for JS constant names.
fn to_upper_snake_case(s: &str) -> String {
    let mut result: String = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                result.push('_');
            }
            result.push(c);
        } else {
            result.push(c.to_ascii_uppercase());
        }
    }
    result
}

impl Default for JsGenerator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)] // test code: a failed generate surfaces via expect
    use std::collections::HashMap;

    use super::*;
    use crate::data::{FieldInfo, StructInfo};

    /// Test that structs emit both TypeScript interface and offset constants.
    #[test]
    fn js_struct_emits_interface_and_offsets() {
        let generator: JsGenerator = JsGenerator::new();
        let ctx: GenerationContext = GenerationContext::new();
        let item = StructInfo {
            name: String::from("TestStruct"),
            fields: vec![
                FieldInfo {
                    name: String::from("value"),
                    rust_type: String::from("u32"),
                    doc: None,
                },
                FieldInfo {
                    name: String::from("ptr"),
                    rust_type: String::from("*constu8"),
                    doc: None,
                },
            ],
            doc: None,
            attributes: vec![],
            size_hint: None,
        };

        let output: String = generator.generate_struct(&item, &ctx).expect("generate");
        assert!(
            output.contains("export interface TestStruct"),
            "should emit TypeScript interface: {}",
            output
        );
        assert!(
            output.contains("TEST_STRUCT_VALUE_OFFSET"),
            "should emit value offset constant: {}",
            output
        );
        assert!(
            output.contains("TEST_STRUCT_PTR_OFFSET"),
            "should emit ptr offset constant: {}",
            output
        );
        assert!(
            output.contains("TEST_STRUCT_SIZE"),
            "should emit size constant: {}",
            output
        );
    }

    /// Test that fn ptr fields emit as number in interface.
    #[test]
    fn js_fn_ptr_field_emits_as_number() {
        let generator: JsGenerator = JsGenerator::new();
        let ctx: GenerationContext = GenerationContext::new();
        let item = StructInfo {
            name: String::from("WithFnPtr"),
            fields: vec![FieldInfo {
                name: String::from("callback"),
                rust_type: String::from("unsafeextern\"C\"fn(*constu8)->u32"),
                doc: None,
            }],
            doc: None,
            attributes: vec![],
            size_hint: None,
        };

        let output: String = generator.generate_struct(&item, &ctx).expect("generate");
        // In the interface, fn ptr fields should be typed as number.
        assert!(
            output.contains("callback: number;"),
            "fn ptr field should be typed as number in interface: {}",
            output
        );
        assert!(
            output.contains("WITH_FN_PTR_CALLBACK_OFFSET"),
            "should emit offset constant for fn ptr: {}",
            output
        );
    }

    /// Test that Array<T> fields expand in interface and offsets.
    #[test]
    fn js_array_field_expands() {
        let generator: JsGenerator = JsGenerator::new();
        let ctx: GenerationContext = GenerationContext::new();
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

        let output: String = generator.generate_struct(&item, &ctx).expect("generate");
        assert!(
            output.contains("items: number;"),
            "Array items should be number in interface: {}",
            output
        );
        assert!(
            output.contains("items_len: number;"),
            "Array should have len field: {}",
            output
        );
        assert!(
            output.contains("WITH_ARRAY_ITEMS_OFFSET"),
            "should emit items offset: {}",
            output
        );
    }

    /// Test that a fixed-size byte array field `[u8;32]` emits `number[]` in the
    /// interface and that offset/size constants account for the 32-byte footprint.
    #[test]
    fn js_fixed_byte_array_field_emits_number_array() {
        let generator: JsGenerator = JsGenerator::new();
        let ctx: GenerationContext = GenerationContext::new();
        let item = StructInfo {
            name: String::from("Ed25519PublicKey"),
            fields: vec![FieldInfo {
                name: String::from("bytes"),
                rust_type: String::from("[u8;32]"),
                doc: None,
            }],
            doc: None,
            attributes: vec![],
            size_hint: Some(32),
        };

        let output: String = generator.generate_struct(&item, &ctx).expect("generate");
        assert!(
            output.contains("bytes: number[];"),
            "fixed byte array should emit number[] interface field: {}",
            output
        );
        assert!(
            !output.contains("[u8;32]"),
            "raw Rust array syntax must not appear in output: {}",
            output
        );
        assert!(
            output.contains("ED25519_PUBLIC_KEY_BYTES_OFFSET: number = 0;"),
            "should emit bytes offset constant at 0: {}",
            output
        );
        assert!(
            output.contains("ED25519_PUBLIC_KEY_SIZE: number = 32;"),
            "should emit size constant of 32: {}",
            output
        );
    }

    /// Regression: a `#[repr(u32)]` enum field must occupy 4 bytes, resolved
    /// from `enum_reprs`, not the 8-byte unknown-type fallback. A u32 field
    /// followed by such an enum therefore places the enum at offset 4 and keeps
    /// the struct 8 bytes. Guards the drift where an enum absent from the JS
    /// emitter's knowledge silently received the pointer-sized fallback layout
    /// (offset 8 / size 16), corrupting every downstream DataView offset.
    #[test]
    fn js_enum_field_uses_repr_layout_for_offsets() {
        let generator: JsGenerator = JsGenerator::new();
        let mut enum_reprs: HashMap<String, String> = HashMap::new();
        enum_reprs.insert(String::from("MyPolicy"), String::from("u32"));
        let ctx: GenerationContext = GenerationContext::new().with_enum_reprs(enum_reprs);

        let item = StructInfo {
            name: String::from("PolicyHolder"),
            fields: vec![
                FieldInfo {
                    name: String::from("level"),
                    rust_type: String::from("u32"),
                    doc: None,
                },
                FieldInfo {
                    name: String::from("policy"),
                    rust_type: String::from("MyPolicy"),
                    doc: None,
                },
            ],
            doc: None,
            attributes: vec![],
            size_hint: None,
        };

        let output: String = generator.generate_struct(&item, &ctx).expect("generate");
        assert!(
            output.contains("export const POLICY_HOLDER_POLICY_OFFSET: number = 4;"),
            "repr(u32) enum field must sit at offset 4, got: {output}"
        );
        assert!(
            output.contains("export const POLICY_HOLDER_SIZE: number = 8;"),
            "u32 + repr(u32) enum struct must be 8 bytes, got: {output}"
        );
    }
}

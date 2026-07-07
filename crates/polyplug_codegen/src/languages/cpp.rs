//! C++ code generator — produces C++ headers from ABI items.
//!
//! Generates typed function pointer typedefs, correct Array<T> representations,
//! and snake_case naming in the `polyplug` namespace per D-35.

use std::io;

use langprint::backends::cpp_backend::{
    CppBackend, CppDefinition, CppEnum, CppEnumVariant, CppField, CppStruct, CppStructKind,
    CppStructRenderOptions, CppVisibility, DocsStyle,
};
use langprint::renderers::{DefinitionRenderer, EnumRenderer, StructRenderer};

use crate::data::{
    ConstInfo, EnumInfo, EnumVariant, FunctionInfo, StructInfo, UnionInfo, UnionVariant,
};
use crate::error::PolyplugcError;
use crate::languages::{CodeGenerator, GenerationContext};

/// C++ ABI code generator.
pub struct CppGenerator;

impl CppGenerator {
    pub fn new() -> Self {
        CppGenerator
    }

    /// Check if a rust_type represents Array<T>.
    fn is_array(rust_type: &str) -> bool {
        rust_type.starts_with("Array<")
    }

    /// Parse a fixed-size array type in compacted `quote!()` form, e.g. `[u8;32]`.
    ///
    /// Returns `(c_element_type, count)` when the input matches `[T;N]` where T is
    /// a known primitive and N is a positive integer; returns `None` otherwise.
    fn parse_fixed_array(rust_type: &str) -> Option<(&'static str, usize)> {
        let inner: &str = rust_type.strip_prefix('[')?.strip_suffix(']')?;
        let semi: usize = inner.find(';')?;
        let elem: &str = inner[..semi].trim();
        let count_str: &str = inner[semi + 1..].trim();
        let count: usize = count_str.parse().ok().filter(|&n: &usize| n > 0)?;
        let c_elem: &'static str = match elem {
            "u8" => "uint8_t",
            "u16" => "uint16_t",
            "u32" => "uint32_t",
            "u64" => "uint64_t",
            "i8" => "int8_t",
            "i16" => "int16_t",
            "i32" => "int32_t",
            "i64" => "int64_t",
            _ => return None,
        };
        Some((c_elem, count))
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

    fn rust_type_to_cpp(rust_type: &str) -> String {
        // Handle Option<...> wrapper.
        if Self::is_option(rust_type) {
            let inner: &str = &rust_type["Option<".len()..rust_type.len() - 1];
            return Self::rust_type_to_cpp(inner);
        }

        // Handle Array<T> — maps to the C++ Array struct. Struct field expansion
        // happens in generate_struct (which checks is_array before calling this);
        // this path is only reached for function pointer return types.
        if Self::is_array(rust_type) {
            return String::from("Array");
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

        // Strip Rust module paths (e.g., "crate::host::HostContractInstance" -> "HostContractInstance").
        if let Some(short) = rust_type.rsplit("::").next() {
            // Only strip if it actually had a :: separator (avoid stripping single-word types).
            if rust_type.contains("::") {
                return Self::rust_type_to_cpp(short);
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
            "bool" => String::from("bool"),
            "()" => String::from("void"),
            "c_char" => String::from("char"),
            "T" => String::from("void"), // Generic placeholder — used as void* for opaque pointers
            other => String::from(other),
        }
    }

    fn convert_function_pointer(type_name: &str) -> String {
        let type_str: &str = Self::strip_option(type_name);

        let fn_start: usize = type_str.find("fn(").unwrap_or(0);
        let params_start: usize = fn_start + 3;

        // Find the matching closing paren for the fn parameter list.
        let mut depth = 1i32;
        let mut params_end: usize = params_start;
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

        let cpp_return: String = if type_str.len() > params_end + 1 {
            let after: &str = &type_str[params_end + 1..];
            let trimmed: &str = after.trim_start_matches('-').trim_start_matches('>').trim();
            if trimmed.is_empty() {
                String::from("void")
            } else {
                Self::rust_type_to_cpp(trimmed)
            }
        } else {
            String::from("void")
        };

        if params_start == 0 || params_end <= params_start {
            return format!("{}(*)()", cpp_return);
        }

        let params_str: &str = &type_str[params_start..params_end];
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

    /// Split a doc string into per-line content for langprint's doc renderer.
    ///
    /// Mirrors `format_doc_comment`'s use of `str::lines()`: each line becomes a
    /// `Vec` entry with no trailing newline, and a blank source line becomes an
    /// empty entry (which langprint renders as a bare `///`).
    fn doc_lines(doc: &str) -> Vec<String> {
        doc.lines().map(String::from).collect::<Vec<String>>()
    }

    /// Build a langprint `CppField` with the ABI-mirror defaults: default
    /// (public) visibility, no bit-field / over-alignment / initializer. The
    /// caller sets `array_size` after the fact for fixed-size array fields.
    fn cpp_field(field_type: &str, name: &str, docs: Option<Vec<String>>) -> CppField {
        CppField {
            name: String::from(name),
            field_type: String::from(field_type),
            visibility: CppVisibility::Default,
            array_size: None,
            bit_field_size: None,
            alignment: None,
            is_static: false,
            is_const: false,
            is_inline: false,
            initialization_value: None,
            inline_comment: None,
            docs,
        }
    }

    /// Generate a typedef for a function pointer type.
    ///
    /// Returns (typedef_line, type_name_to_use_in_struct).
    fn generate_fn_ptr_typedef(
        struct_name: &str,
        field_name: &str,
        rust_type: &str,
    ) -> (String, String) {
        let fn_type: String = Self::convert_function_pointer(rust_type);
        let typedef_name: String = format!("{}_{}_fn", struct_name, field_name);

        let typedef: String = format!("using {} = {};\n", typedef_name, fn_type);

        let mut extra: String = String::new();
        if Self::is_option(rust_type) {
            extra.push_str("// Nullable function pointer.\n");
        }

        (format!("{}{}", typedef, extra), typedef_name)
    }

    /// Resolve a field's by-value dependency, if any.
    ///
    /// Returns the C++ type name that must be a complete type before the
    /// enclosing struct/union can be defined. Pointer fields, arrays, function
    /// pointers, and primitive types impose no ordering constraint and yield
    /// `None`.
    pub fn value_dependency(rust_type: &str) -> Option<String> {
        let inner: &str = Self::strip_option(rust_type);

        if Self::is_array(inner)
            || inner.starts_with('*')
            || inner.contains("extern\"C\"fn")
            || inner.contains("extern\"C\"")
        {
            return None;
        }

        let cpp: String = Self::rust_type_to_cpp(inner);
        // A by-value dependency is a named aggregate type — i.e. anything that
        // did not map to a primitive, pointer, or void.
        let is_named: bool = cpp.chars().next().is_some_and(|c| c.is_ascii_uppercase());
        if is_named { Some(cpp) } else { None }
    }

    /// Format a C++ forward declaration for a struct, enum, or union by name.
    ///
    /// Enums require their fixed underlying type so they can be used by value
    /// after only a forward declaration.
    pub fn forward_declaration(name: &str, kind: ForwardKind) -> String {
        match kind {
            ForwardKind::Struct => format!("struct {};\n", name),
            ForwardKind::Union => format!("union {};\n", name),
            ForwardKind::Enum(repr) => {
                format!("enum class {} : {};\n", name, Self::rust_type_to_cpp(&repr))
            }
        }
    }
}

/// Kind of aggregate being forward-declared in generated C++ output.
pub enum ForwardKind {
    /// A `struct`.
    Struct,
    /// A `union`.
    Union,
    /// An `enum class` with the given Rust `repr` for its fixed underlying type.
    Enum(String),
}

impl CodeGenerator for CppGenerator {
    fn generate_const(
        &self,
        item: &ConstInfo,
        _ctx: &GenerationContext,
    ) -> Result<String, PolyplugcError> {
        // langprint renders the `#define {name} {value}` FORM; polyplug_codegen
        // keeps the value-suffix LOGIC. The mirror carries no doc on its define,
        // so docs are left off to match byte-for-byte. langprint ends the
        // directive without a newline; the mirror separates items with one.
        let value: String = Self::format_constant_value(&item.value, &item.rust_type);
        let define: CppDefinition = CppDefinition {
            name: item.name.clone(),
            value: Some(value),
            docs: None,
        };
        let backend: CppBackend = CppBackend::default();
        let mut indent_level: i32 = 0;
        let mut rendered: String = backend
            .render_definition::<&str>(&define, None, None, None, &mut indent_level)
            .map_err(|source: io::Error| PolyplugcError::WriteFailed {
                path: String::from("sdks/cpp/abi/polyplug/abi.hpp"),
                source,
            })?;
        rendered.push('\n');
        Ok(rendered)
    }

    fn generate_struct(
        &self,
        item: &StructInfo,
        _ctx: &GenerationContext,
    ) -> Result<String, PolyplugcError> {
        // langprint renders the `struct Name { … };` FORM; polyplug_codegen keeps
        // the field type-string LOGIC and two pieces of surrounding WIRING that
        // langprint cannot express: the struct-level doc and the fn-pointer
        // typedefs must sit BEFORE the `struct` keyword, but langprint's struct
        // renderer writes docs immediately before it — so the doc and the
        // typedefs are hand-emitted here, and the struct is rendered with no doc.
        let mut output: String = String::new();

        if let Some(doc) = &item.doc {
            output.push_str(&Self::format_doc_comment(doc, 0));
        }

        // Function-pointer typedefs emitted before the struct (WIRING).
        for field in &item.fields {
            if field.rust_type.contains("extern\"C\"fn") || field.rust_type.contains("extern\"C\"")
            {
                let (typedef, _type_name): (String, String) =
                    Self::generate_fn_ptr_typedef(&item.name, &field.name, &field.rust_type);
                output.push_str(&typedef);
            }
        }

        // Map each ABI field to one or more langprint fields (the LOGIC).
        let mut fields: Vec<CppField> = Vec::new();
        for field in &item.fields {
            let docs: Option<Vec<String>> = field.doc.as_deref().map(Self::doc_lines);

            // Array<T> — expand into 3 sub-fields per D-21; the field doc rides
            // the first sub-field only.
            if Self::is_array(&field.rust_type) {
                fields.push(Self::cpp_field("void*", &field.name, docs));
                fields.push(Self::cpp_field(
                    "size_t",
                    &format!("{}_len", field.name),
                    None,
                ));
                fields.push(Self::cpp_field(
                    "size_t",
                    &format!("{}__align", field.name),
                    None,
                ));
                continue;
            }

            // Function pointer field — use the typedef name emitted above.
            if field.rust_type.contains("extern\"C\"fn") || field.rust_type.contains("extern\"C\"")
            {
                let (_, typedef_name): (String, String) =
                    Self::generate_fn_ptr_typedef(&item.name, &field.name, &field.rust_type);
                fields.push(Self::cpp_field(&typedef_name, &field.name, docs));
                continue;
            }

            // Fixed-size primitive array, e.g. `[u8;32]` — the C array dimension
            // follows the identifier: `uint8_t bytes[32];`.
            if let Some((c_elem, count)) = Self::parse_fixed_array(&field.rust_type) {
                let mut fixed_field: CppField = Self::cpp_field(c_elem, &field.name, docs);
                fixed_field.array_size = Some(count.to_string());
                fields.push(fixed_field);
                continue;
            }

            let cpp_type: String = Self::rust_type_to_cpp(&field.rust_type);
            fields.push(Self::cpp_field(&cpp_type, &field.name, docs));
        }

        let cpp_struct: CppStruct = CppStruct {
            struct_kind: CppStructKind::Struct,
            is_final: false,
            alignment: None,
            is_packed: false,
            name: item.name.clone(),
            template_params: Vec::new(),
            bases: Vec::new(),
            fields,
            methods: Vec::new(),
            docs: None,
        };
        let backend: CppBackend = CppBackend {
            docs_style: DocsStyle::TripleSlash,
            ..CppBackend::default()
        };
        let options: CppStructRenderOptions = CppStructRenderOptions {
            render_default_visibility: false,
            ..CppStructRenderOptions::default()
        };
        let mut indent_level: i32 = 0;
        let rendered: String = backend
            .render_struct::<&str>(&cpp_struct, None, None, Some(&options), &mut indent_level)
            .map_err(|source: io::Error| PolyplugcError::WriteFailed {
                path: String::from("sdks/cpp/abi/polyplug/abi.hpp"),
                source,
            })?;
        output.push_str(&rendered);

        // Emit static_assert for size validation if known (WIRING).
        if let Some(size) = item.size_hint {
            output.push_str(&format!(
                "static_assert(sizeof({}) == {}, \"{} size mismatch\");\n\n",
                item.name, size, item.name
            ));
        } else {
            output.push('\n');
        }

        Ok(output)
    }

    fn generate_enum(
        &self,
        item: &EnumInfo,
        _ctx: &GenerationContext,
    ) -> Result<String, PolyplugcError> {
        // langprint renders the `enum class Name : repr { … };` FORM (TripleSlash
        // docs, `Name : repr` spacing); polyplug_codegen keeps the repr mapping
        // and the mirror's explicit-value LOGIC: a valueless first variant is
        // pinned to `= 0`, later valueless variants stay bare.
        let repr: String = Self::rust_type_to_cpp(&item.repr);
        let variants: Vec<CppEnumVariant> = item
            .variants
            .iter()
            .enumerate()
            .map(|(i, variant): (usize, &EnumVariant)| {
                let value: Option<String> = match variant.value {
                    Some(value) => Some(value.to_string()),
                    None if i == 0 => Some(String::from("0")),
                    None => None,
                };
                CppEnumVariant {
                    name: variant.name.clone(),
                    value,
                    docs: variant.doc.as_deref().map(Self::doc_lines),
                }
            })
            .collect::<Vec<CppEnumVariant>>();

        let cpp_enum: CppEnum = CppEnum {
            name: item.name.clone(),
            variants,
            is_enum_class: true,
            underlying_type: Some(repr),
            docs: item.doc.as_deref().map(Self::doc_lines),
        };
        let backend: CppBackend = CppBackend {
            docs_style: DocsStyle::TripleSlash,
            space_before_enum_base: true,
            ..CppBackend::default()
        };
        let mut indent_level: i32 = 0;
        let mut rendered: String = backend
            .render_enum(
                &cpp_enum,
                None::<&str>,
                None::<&str>,
                None,
                &mut indent_level,
            )
            .map_err(|source: io::Error| PolyplugcError::WriteFailed {
                path: String::from("sdks/cpp/abi/polyplug/abi.hpp"),
                source,
            })?;
        // The mirror separates enums with a trailing blank line; langprint ends
        // the declaration with a single newline.
        rendered.push('\n');
        Ok(rendered)
    }

    fn generate_union(
        &self,
        item: &UnionInfo,
        _ctx: &GenerationContext,
    ) -> Result<String, PolyplugcError> {
        // langprint renders the `union Name { … };` FORM (kind = Union); union
        // variants carry no docs, and the union-level doc sits directly before
        // the `union` keyword (no typedefs between), so it is passed through to
        // langprint. The mirror separates unions with a trailing blank line.
        let fields: Vec<CppField> = item
            .variants
            .iter()
            .map(|variant: &UnionVariant| {
                let cpp_type: String = Self::rust_type_to_cpp(&variant.type_name);
                Self::cpp_field(&cpp_type, &variant.name, None)
            })
            .collect::<Vec<CppField>>();

        let cpp_union: CppStruct = CppStruct {
            struct_kind: CppStructKind::Union,
            is_final: false,
            alignment: None,
            is_packed: false,
            name: item.name.clone(),
            template_params: Vec::new(),
            bases: Vec::new(),
            fields,
            methods: Vec::new(),
            docs: item.doc.as_deref().map(Self::doc_lines),
        };
        let backend: CppBackend = CppBackend {
            docs_style: DocsStyle::TripleSlash,
            ..CppBackend::default()
        };
        let options: CppStructRenderOptions = CppStructRenderOptions {
            render_default_visibility: false,
            ..CppStructRenderOptions::default()
        };
        let mut indent_level: i32 = 0;
        let mut rendered: String = backend
            .render_struct::<&str>(&cpp_union, None, None, Some(&options), &mut indent_level)
            .map_err(|source: io::Error| PolyplugcError::WriteFailed {
                path: String::from("sdks/cpp/abi/polyplug/abi.hpp"),
                source,
            })?;
        rendered.push('\n');
        Ok(rendered)
    }

    fn generate_function(
        &self,
        item: &FunctionInfo,
        _ctx: &GenerationContext,
    ) -> Result<String, PolyplugcError> {
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
            Ok(format!(
                "constexpr {} {}({}) {{ /* implementation */ }}\n\n",
                ret_type, item.name, params
            ))
        } else {
            Ok(format!("{} {}({});\n\n", ret_type, item.name, params))
        }
    }

    fn file_extension(&self) -> &'static str {
        "hpp"
    }

    fn language_name(&self) -> &'static str {
        "cpp"
    }

    fn generate_header(&self, _ctx: &GenerationContext) -> Result<String, PolyplugcError> {
        let mut header: String = String::from("#pragma once\n");
        header.push_str("#include <cstdint>\n");
        header.push_str("#include <cstddef>\n");
        header.push_str("#include <cstring>\n");
        header.push_str("#include <stdexcept>\n");
        header.push_str("#include <string>\n");
        header.push_str("#include <string_view>\n");
        header.push_str("#include <vector>\n\n");
        // abi.hpp is pure ABI: structs, enums, and borrowing helpers only — no
        // link-time dependency on the host. Cross-boundary allocation lives in the
        // guest SDK (polyplug::alloc_string), which routes through the stored
        // HostApi function pointers.
        Ok(header)
    }
}

impl Default for CppGenerator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{FieldInfo, StructInfo};

    /// Test that fn ptr typedefs produce valid C++ types.
    #[test]
    fn cpp_fn_ptr_typedef_uses_c_types() {
        let (typedef, _type_name) = CppGenerator::generate_fn_ptr_typedef(
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

    /// Test that Array<T> fields expand into 3 sub-fields.
    #[test]
    fn cpp_array_field_expands() {
        let generator: CppGenerator = CppGenerator::new();
        let ctx: GenerationContext = GenerationContext::new();
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

        let output: String = generator.generate_struct(&item, &ctx).expect("generate");
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

    /// Test that void-returning fn ptrs produce correct typedefs.
    #[test]
    fn cpp_fn_ptr_void_return_correct() {
        let (typedef, _type_name) = CppGenerator::generate_fn_ptr_typedef(
            "Test",
            "destroy",
            "unsafeextern\"C\"fn(ptr:*mutu8)->()",
        );
        assert!(
            typedef.contains("void(*)"),
            "void return should produce void(*): {}",
            typedef
        );
        assert!(
            !typedef.contains("()"),
            "return type () should not appear in typedef: {}",
            typedef
        );
    }

    /// Function-pointer aliases must use the `using Name = Type;` form so the
    /// pointer name is bound — a bare `typedef Ret(*)(...) Name;` is invalid C++.
    #[test]
    fn cpp_fn_ptr_typedef_uses_using_alias() {
        let (typedef, type_name) = CppGenerator::generate_fn_ptr_typedef(
            "Test",
            "callback",
            "unsafeextern\"C\"fn(ptr:*constu8)->u32",
        );
        assert!(
            typedef.starts_with(&format!("using {} = ", type_name)),
            "fn ptr should be a using-alias: {}",
            typedef
        );
        assert!(
            !typedef.contains("typedef"),
            "fn ptr should not use the invalid typedef form: {}",
            typedef
        );
    }

    /// Test that a fixed-size byte array field `[u8;32]` emits a C array field
    /// with the dimension after the identifier — `uint8_t bytes[32];`.
    #[test]
    fn cpp_fixed_byte_array_field_emits_c_array() {
        let generator: CppGenerator = CppGenerator::new();
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
            output.contains("uint8_t bytes[32];"),
            "fixed byte array should emit C array field: {}",
            output
        );
        assert!(
            !output.contains("[u8;32]"),
            "raw Rust array syntax must not appear in output: {}",
            output
        );
    }

    /// By-value aggregate fields impose an ordering dependency; pointers,
    /// arrays, function pointers, and primitives do not.
    #[test]
    fn cpp_value_dependency_detects_aggregates() {
        assert_eq!(
            CppGenerator::value_dependency("Version"),
            Some(String::from("Version"))
        );
        assert_eq!(
            CppGenerator::value_dependency("Option<DispatchMechanisms>"),
            Some(String::from("DispatchMechanisms"))
        );
        assert_eq!(CppGenerator::value_dependency("u32"), None);
        assert_eq!(CppGenerator::value_dependency("*const HostApi"), None);
        assert_eq!(CppGenerator::value_dependency("Array<u8>"), None);
        assert_eq!(
            CppGenerator::value_dependency("unsafeextern\"C\"fn(ptr:*constu8)->u32"),
            None
        );
    }

    /// Forward declarations must carry the kind and, for enums, a fixed
    /// underlying type so the enum can be used by value after the declaration.
    #[test]
    fn cpp_forward_declaration_formats() {
        assert_eq!(
            CppGenerator::forward_declaration("Buffer", ForwardKind::Struct),
            "struct Buffer;\n"
        );
        assert_eq!(
            CppGenerator::forward_declaration("DispatchMechanisms", ForwardKind::Union),
            "union DispatchMechanisms;\n"
        );
        assert_eq!(
            CppGenerator::forward_declaration(
                "DispatchType",
                ForwardKind::Enum(String::from("u32"))
            ),
            "enum class DispatchType : uint32_t;\n"
        );
    }
}

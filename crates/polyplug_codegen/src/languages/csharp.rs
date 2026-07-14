//! C# code generator — produces C# bindings from ABI items.
//!
//! Emits `IntPtr` for all function pointer fields (blittable, no managed
//! delegates in ABI structs), correct `Array<T>` representations, and
//! PascalCase naming per D-35.

use std::io;

use langprint::backends::csharp_backend::{
    CSharpBackend, CSharpEnum, CSharpEnumMember, CSharpField, CSharpType, CSharpTypeKind,
    CSharpVisibility,
};
use langprint::renderers::{EnumRenderer, StructRenderer};

use crate::data::{
    ConstInfo, EnumInfo, EnumVariant, FunctionInfo, StructInfo, UnionInfo, UnionVariant,
};
use crate::error::PolyplugcError;
use crate::languages::{CodeGenerator, GenerationContext};

/// C# ABI code generator.
pub struct CSharpGenerator;

impl CSharpGenerator {
    pub fn new() -> Self {
        CSharpGenerator
    }

    /// Check if a rust_type string represents a function pointer.
    fn is_function_pointer(rust_type: &str) -> bool {
        let type_str: &str = Self::strip_option(rust_type);
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

    /// Parse a fixed-size array type in compacted `quote!()` form, e.g. `[u8;32]`.
    ///
    /// Returns `(csharp_element_type, count)` when the input matches `[T;N]` where T
    /// is a known primitive and N is a positive integer; returns `None` otherwise.
    fn parse_fixed_array(rust_type: &str) -> Option<(&'static str, usize)> {
        let inner: &str = rust_type.strip_prefix('[')?.strip_suffix(']')?;
        let semi: usize = inner.find(';')?;
        let elem: &str = inner[..semi].trim();
        let count_str: &str = inner[semi + 1..].trim();
        let count: usize = count_str.parse().ok().filter(|&n: &usize| n > 0)?;
        let cs_elem: &'static str = match elem {
            "u8" => "byte",
            "u16" => "ushort",
            "u32" => "uint",
            "u64" => "ulong",
            "i8" => "sbyte",
            "i16" => "short",
            "i32" => "int",
            "i64" => "long",
            _ => return None,
        };
        Some((cs_elem, count))
    }

    fn rust_type_to_csharp(rust_type: &str) -> String {
        // Handle Option<...> wrapper.
        if Self::is_option(rust_type) {
            let inner: &str = &rust_type["Option<".len()..rust_type.len() - 1];
            if Self::is_function_pointer(rust_type) {
                return Self::rust_type_to_csharp(inner);
            }
            return Self::rust_type_to_csharp(inner);
        }

        // Handle Array<T> — return placeholder; actual handling in generate_struct.
        if Self::is_array(rust_type) {
            return String::from("IntPtr");
        }

        // Function pointers as raw types resolve to IntPtr (delegate handled at struct level).
        if rust_type.contains("extern\"C\"fn") || rust_type.contains("extern\"C\"") {
            return String::from("IntPtr");
        }

        if rust_type.starts_with('*') {
            return String::from("IntPtr");
        }

        if rust_type.contains("c_void") {
            return String::from("IntPtr");
        }

        if rust_type == "&str" {
            return String::from("string");
        }

        if rust_type.starts_with("&[u8]") || rust_type.starts_with("&[") {
            return String::from("byte[]");
        }

        if let Some(inner) = rust_type.strip_prefix('&') {
            return Self::rust_type_to_csharp(inner);
        }

        // Strip Rust module paths (e.g., "crate::host::HostContractInstance" -> "HostContractInstance").
        if let Some(short) = rust_type.rsplit("::").next() {
            // Only strip if it actually had a :: separator (avoid stripping single-word types).
            if rust_type.contains("::") {
                return Self::rust_type_to_csharp(short);
            }
        }

        match rust_type {
            // `#[repr(transparent)]` u64 newtypes from polyplug_utils.
            "u64" | "BundleId" | "GuestContractId" | "HostContractId" => String::from("ulong"),
            "u32" => String::from("uint"),
            "u16" => String::from("ushort"),
            "u8" => String::from("byte"),
            "i64" => String::from("long"),
            "i32" => String::from("int"),
            "i16" => String::from("short"),
            "i8" => String::from("sbyte"),
            "usize" => String::from("nuint"),
            "isize" => String::from("nint"),
            "bool" => String::from("bool"),
            "()" => String::from("void"),
            other => String::from(other),
        }
    }

    /// Split a doc string into per-line content for langprint's doc renderer.
    ///
    /// Each line becomes a `Vec` entry with no trailing newline; a blank source
    /// line becomes an empty entry, which langprint renders as a bare `///` (no
    /// trailing space). The mirror's Rust doc lines carry a leading space, so
    /// langprint's `/// ` prefix produces the golden `///  content` (two spaces).
    fn doc_lines(doc: &str) -> Vec<String> {
        doc.lines().map(String::from).collect::<Vec<String>>()
    }

    /// Build a langprint `CSharpField` with the ABI-mirror defaults: public
    /// visibility, no static/const/readonly/initializer. `attributes` carries the
    /// union `FieldOffset(0)` marker; `docs` carries the field's doc lines.
    fn cs_field(
        field_type: &str,
        name: &str,
        docs: Option<Vec<String>>,
        attributes: Vec<String>,
    ) -> CSharpField {
        CSharpField {
            name: String::from(name),
            field_type: String::from(field_type),
            visibility: CSharpVisibility::Public,
            is_static: false,
            is_const: false,
            is_readonly: false,
            initializer: None,
            attributes,
            docs,
        }
    }

    /// Map a langprint render `io::Error` to a `PolyplugcError` for `Abi.cs`.
    fn write_err(source: io::Error) -> PolyplugcError {
        PolyplugcError::WriteFailed {
            path: String::from("sdks/csharp/abi/Abi.cs"),
            source,
        }
    }

    fn to_pascal_case(s: &str) -> String {
        s.split(['_', '.'])
            .filter(|seg| !seg.is_empty())
            .map(|seg| {
                let mut chars = seg.chars();
                match chars.next() {
                    None => String::new(),
                    Some(first) => first.to_uppercase().to_string() + chars.as_str(),
                }
            })
            .collect::<Vec<String>>()
            .join("")
    }
}

impl CodeGenerator for CSharpGenerator {
    fn generate_const(
        &self,
        _item: &ConstInfo,
        _ctx: &GenerationContext,
    ) -> Result<String, PolyplugcError> {
        // C# ABI bindings don't include constants at namespace level.
        // Constants are provided by the AbiConstants static class in the host/guest SDKs.
        Ok(String::new())
    }

    fn generate_struct(
        &self,
        item: &StructInfo,
        _ctx: &GenerationContext,
    ) -> Result<String, PolyplugcError> {
        // langprint renders the `[attr] public [unsafe] struct Name { … }` FORM
        // (docs, StructLayout attribute, fields, Allman braces); polyplugc keeps
        // the field type-string LOGIC (Array<T> expansion, fn-pointer → IntPtr,
        // fixed-buffer mapping) and the trailing `Expected size` WIRING comment.
        let mut output: String = String::new();

        // Pre-scan: does any field use a fixed-size primitive array?
        // If so the struct must be declared `unsafe` to allow `fixed` buffers.
        let has_fixed_array: bool = item
            .fields
            .iter()
            .any(|f| Self::parse_fixed_array(&f.rust_type).is_some());

        // Map each ABI field to one or more langprint fields (the LOGIC).
        let mut fields: Vec<CSharpField> = Vec::new();
        for field in &item.fields {
            let docs: Option<Vec<String>> = field.doc.as_deref().map(Self::doc_lines);

            // Array<T> — expand into 3 sub-fields per D-21; the field doc rides
            // the first sub-field only.
            if Self::is_array(&field.rust_type) {
                let field_name: String = Self::to_pascal_case(&field.name);
                fields.push(Self::cs_field("IntPtr", &field_name, docs, Vec::new()));
                fields.push(Self::cs_field(
                    "nuint",
                    &format!("{}Len", field_name),
                    None,
                    Vec::new(),
                ));
                fields.push(Self::cs_field(
                    "nuint",
                    &format!("{}Align", field_name),
                    None,
                    Vec::new(),
                ));
                continue;
            }

            // Function pointer field — emit IntPtr (blittable, no managed
            // delegate in the ABI struct so unions stay overlappable in .NET).
            if Self::is_function_pointer(&field.rust_type) {
                let field_name: String = Self::to_pascal_case(&field.name);
                fields.push(Self::cs_field("IntPtr", &field_name, docs, Vec::new()));
                continue;
            }

            // Fixed-size primitive array, e.g. `[u8;32]`. langprint has no
            // fixed-buffer / array-dimension model, so the `unsafe fixed` modifiers
            // ride the type string and the `[N]` dimension rides the name: langprint
            // then renders `public unsafe fixed {elem} {Name}[{count}];` verbatim.
            if let Some((cs_elem, count)) = Self::parse_fixed_array(&field.rust_type) {
                let field_name: String = Self::to_pascal_case(&field.name);
                fields.push(Self::cs_field(
                    &format!("unsafe fixed {}", cs_elem),
                    &format!("{}[{}]", field_name, count),
                    docs,
                    Vec::new(),
                ));
                continue;
            }

            let csharp_type: String = Self::rust_type_to_csharp(&field.rust_type);
            let field_name: String = Self::to_pascal_case(&field.name);
            fields.push(Self::cs_field(&csharp_type, &field_name, docs, Vec::new()));
        }

        let attribute: String = match item.size_hint {
            Some(size) => format!("StructLayout(LayoutKind.Sequential, Size = {})", size),
            None => String::from("StructLayout(LayoutKind.Sequential)"),
        };
        let cs_struct: CSharpType = CSharpType {
            kind: CSharpTypeKind::Struct,
            name: item.name.clone(),
            visibility: CSharpVisibility::Public,
            is_abstract: false,
            is_sealed: false,
            is_static: false,
            is_unsafe: has_fixed_array,
            is_partial: false,
            generic_args: Vec::new(),
            base_class: None,
            interfaces: Vec::new(),
            fields,
            properties: Vec::new(),
            methods: Vec::new(),
            attributes: vec![attribute],
            docs: item.doc.as_deref().map(Self::doc_lines),
        };
        let backend: CSharpBackend = CSharpBackend::default();
        let mut indent_level: i32 = 0;
        let rendered: String = backend
            .render_struct(
                &cs_struct,
                None::<&str>,
                None::<&str>,
                None,
                &mut indent_level,
            )
            .map_err(Self::write_err)?;
        output.push_str(&rendered);

        // Emit size documentation comment if known (actual validation is in LayoutTests.cs).
        if let Some(size) = item.size_hint {
            output.push_str(&format!("\n/// Expected size: {} bytes\n", size));
        }

        output.push('\n');
        Ok(output)
    }

    fn generate_enum(
        &self,
        item: &EnumInfo,
        _ctx: &GenerationContext,
    ) -> Result<String, PolyplugcError> {
        // langprint renders the `public enum Name : uint { … }` FORM (docs,
        // Allman braces, member docs); polyplugc keeps the mirror's explicit-value
        // LOGIC: a valueless first variant is pinned to `= 0`, later valueless
        // variants stay bare. The mirror separates enums with a trailing blank line;
        // langprint ends the declaration with a single newline.
        let members: Vec<CSharpEnumMember> = item
            .variants
            .iter()
            .enumerate()
            .map(|(i, variant): (usize, &EnumVariant)| {
                let value: Option<String> = match variant.value {
                    Some(value) => Some(value.to_string()),
                    None if i == 0 => Some(String::from("0")),
                    None => None,
                };
                CSharpEnumMember {
                    name: variant.name.clone(),
                    value,
                    docs: variant.doc.as_deref().map(Self::doc_lines),
                }
            })
            .collect::<Vec<CSharpEnumMember>>();

        let cs_enum: CSharpEnum = CSharpEnum {
            name: item.name.clone(),
            visibility: CSharpVisibility::Public,
            underlying_type: Some(String::from("uint")),
            members,
            is_flags: false,
            attributes: Vec::new(),
            docs: item.doc.as_deref().map(Self::doc_lines),
        };
        let backend: CSharpBackend = CSharpBackend::default();
        let mut indent_level: i32 = 0;
        let mut rendered: String = backend
            .render_enum(
                &cs_enum,
                None::<&str>,
                None::<&str>,
                None,
                &mut indent_level,
            )
            .map_err(Self::write_err)?;
        rendered.push('\n');
        Ok(rendered)
    }

    fn generate_union(
        &self,
        item: &UnionInfo,
        _ctx: &GenerationContext,
    ) -> Result<String, PolyplugcError> {
        // A C# union is a `[StructLayout(LayoutKind.Explicit)]` struct whose every
        // field carries `[FieldOffset(0)]`. langprint renders the struct FORM plus
        // the per-field attribute; polyplugc keeps the variant type-string LOGIC.
        // The mirror separates unions with a trailing blank line.
        let fields: Vec<CSharpField> = item
            .variants
            .iter()
            .map(|variant: &UnionVariant| {
                let csharp_type: String = Self::rust_type_to_csharp(&variant.type_name);
                let variant_name: String = Self::to_pascal_case(&variant.name);
                Self::cs_field(
                    &csharp_type,
                    &variant_name,
                    None,
                    vec![String::from("FieldOffset(0)")],
                )
            })
            .collect::<Vec<CSharpField>>();

        let cs_union: CSharpType = CSharpType {
            kind: CSharpTypeKind::Struct,
            name: item.name.clone(),
            visibility: CSharpVisibility::Public,
            is_abstract: false,
            is_sealed: false,
            is_static: false,
            is_unsafe: false,
            is_partial: false,
            generic_args: Vec::new(),
            base_class: None,
            interfaces: Vec::new(),
            fields,
            properties: Vec::new(),
            methods: Vec::new(),
            attributes: vec![String::from("StructLayout(LayoutKind.Explicit)")],
            docs: item.doc.as_deref().map(Self::doc_lines),
        };
        let backend: CSharpBackend = CSharpBackend::default();
        let mut indent_level: i32 = 0;
        let mut rendered: String = backend
            .render_struct(
                &cs_union,
                None::<&str>,
                None::<&str>,
                None,
                &mut indent_level,
            )
            .map_err(Self::write_err)?;
        rendered.push('\n');
        Ok(rendered)
    }

    fn generate_function(
        &self,
        _item: &FunctionInfo,
        _ctx: &GenerationContext,
    ) -> Result<String, PolyplugcError> {
        // C# ABI bindings don't include functions - only structs, enums, and constants.
        Ok(String::new())
    }

    fn file_extension(&self) -> &'static str {
        "cs"
    }

    fn language_name(&self) -> &'static str {
        "csharp"
    }

    fn generate_header(&self, _ctx: &GenerationContext) -> Result<String, PolyplugcError> {
        Ok(
            "using System.Runtime.InteropServices;\nusing System.Text;\n\nnamespace Polyplug.Abi {\n\n"
                .to_string(),
        )
    }

    fn generate_footer(&self, _ctx: &GenerationContext) -> Result<String, PolyplugcError> {
        Ok(r#"
/// ABI constants for polyplug.
public static class AbiConstants
{
    public const uint POLYPLUG_ABI_VERSION = 1u;
}
}
"#
        .to_string())
    }
}

impl Default for CSharpGenerator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)] // test code: a failed generate surfaces via expect
    use super::*;
    use crate::data::{FieldInfo, StructInfo};

    /// Test that struct fn ptr fields are emitted as blittable IntPtr, with no
    /// managed delegate definitions (so ABI unions stay overlappable in .NET).
    #[test]
    fn csharp_struct_with_fn_ptr_emits_intptr() {
        let generator: CSharpGenerator = CSharpGenerator::new();
        let ctx: GenerationContext = GenerationContext::new();
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

        let output: String = generator.generate_struct(&item, &ctx).expect("generate");
        assert!(
            output.contains("public IntPtr Callback;"),
            "fn ptr field should be IntPtr: {}",
            output
        );
        assert!(
            !output.contains("delegate"),
            "should not emit any managed delegate: {}",
            output
        );
        assert!(
            output.contains("public uint Value;"),
            "non-pointer field should keep its mapped type: {}",
            output
        );
    }

    /// Test that a fixed-size byte array field `[u8;32]` emits an inline fixed
    /// buffer and that the struct is declared `unsafe` to allow it.
    #[test]
    fn csharp_fixed_byte_array_field_emits_fixed_buffer() {
        let generator: CSharpGenerator = CSharpGenerator::new();
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
            output.contains("public unsafe struct Ed25519PublicKey"),
            "struct with fixed buffer must be declared unsafe: {}",
            output
        );
        assert!(
            output.contains("public unsafe fixed byte Bytes[32];"),
            "fixed byte array should emit unsafe fixed buffer field: {}",
            output
        );
        assert!(
            !output.contains("[u8;32]"),
            "raw Rust array syntax must not appear in output: {}",
            output
        );
    }

    /// Test that Array<T> fields expand into 3 sub-fields with PascalCase.
    #[test]
    fn csharp_array_field_expands() {
        let generator: CSharpGenerator = CSharpGenerator::new();
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
            output.contains("public IntPtr Data;"),
            "Array items should be IntPtr with PascalCase: {}",
            output
        );
        assert!(
            output.contains("public nuint DataLen;"),
            "Array should have Len field: {}",
            output
        );
        assert!(
            output.contains("public nuint DataAlign;"),
            "Array should have Align field: {}",
            output
        );
    }
}

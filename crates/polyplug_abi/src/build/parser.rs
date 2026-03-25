//! ABI Parser — extracts type information from polyplug_abi source code.
//!
//! This module uses `syn` to parse Rust source code and extract ABI type
//! information (constants, structs, enums, unions, functions) into the
//! `AbiInfo` struct for use by code generators.

use super::{
    AbiInfo, ConstantInfo, EnumInfo, FieldInfo, FunctionInfo, StructInfo, UnionInfo,
    UnionVariantInfo, VariantInfo,
};
use std::collections::HashSet;
use syn::{
    parse_file, Attribute, Expr, ExprLit, Fields, File, Item, ItemConst, ItemEnum, ItemFn,
    ItemStruct, ItemUnion, Lit, Visibility,
};
use thiserror::Error;

/// ABI types that should be extracted by the parser.
const ABI_TYPES: &[&str] = &[
    "StringView",
    "Buffer",
    "AbiError",
    "PluginHandle",
    "HostContext",
    "DispatchType",
    "NativeDispatch",
    "VmDispatch",
    "PluginDispatch",
    "PluginInterface",
    "HostVTable",
    "PluginDescriptor",
    "PluginContext",
    "ExtensionEntry",
    "RuntimeConfig",
];

/// ABI constants that should be extracted by the parser.
const ABI_CONSTANTS: &[&str] = &[
    "POLYPLUG_ABI_VERSION",
    "ABI_OK",
    "ABI_ERROR_GENERIC",
    "ABI_BUFFER_TOO_SMALL",
    "ABI_ERROR_PANIC",
    "ABI_ERROR_NOT_FOUND",
    "ABI_ERROR_STALE_HANDLE",
    "ABI_FUNCTION_NOT_AVAIL",
    "ABI_ERROR_DUPLICATE_PROVIDER",
    "ABI_ERROR_INVALID_POINTER",
];

/// ABI functions that should be extracted by the parser.
const ABI_FUNCTIONS: &[&str] = &[
    "fnv1a_64",
    "fnv1a_32",
    "contract_id",
    "extension_id",
    "bundle_id",
];

/// Parser error type.
#[derive(Debug, Error)]
pub enum ParseError {
    /// Failed to parse the source code.
    #[error("failed to parse source: {0}")]
    SynError(#[from] syn::Error),
}

/// ABI parser — extracts type information from Rust source code.
#[derive(Debug, Default)]
pub struct AbiParser {
    /// Set of ABI type names to extract.
    abi_types: HashSet<&'static str>,
    /// Set of ABI constant names to extract.
    abi_constants: HashSet<&'static str>,
    /// Set of ABI function names to extract.
    abi_functions: HashSet<&'static str>,
}

impl AbiParser {
    /// Create a new ABI parser.
    pub fn new() -> AbiParser {
        AbiParser {
            abi_types: ABI_TYPES.iter().copied().collect(),
            abi_constants: ABI_CONSTANTS.iter().copied().collect(),
            abi_functions: ABI_FUNCTIONS.iter().copied().collect(),
        }
    }

    /// Parse Rust source code and extract ABI type information.
    ///
    /// # Arguments
    /// * `source` - The Rust source code to parse.
    ///
    /// # Returns
    /// An `AbiInfo` struct containing the extracted type information.
    pub fn parse(&self, source: &str) -> Result<AbiInfo, ParseError> {
        let file: File = parse_file(source)?;
        let mut info: AbiInfo = AbiInfo::new();

        for item in &file.items {
            match item {
                Item::Const(item_const) => {
                    if let Some(constant) = self.parse_constant(item_const) {
                        info.add_constant(constant);
                    }
                }
                Item::Struct(item_struct) => {
                    if let Some(struct_info) = self.parse_struct(item_struct) {
                        info.add_struct(struct_info);
                    }
                }
                Item::Enum(item_enum) => {
                    if let Some(enum_info) = self.parse_enum(item_enum) {
                        info.add_enum(enum_info);
                    }
                }
                Item::Union(item_union) => {
                    if let Some(union_info) = self.parse_union(item_union) {
                        info.add_union(union_info);
                    }
                }
                Item::Fn(item_fn) => {
                    if let Some(function) = self.parse_function(item_fn) {
                        info.add_function(function);
                    }
                }
                _ => {}
            }
        }

        Ok(info)
    }

    /// Parse a constant item and return `ConstantInfo` if it's an ABI constant.
    fn parse_constant(&self, item: &ItemConst) -> Option<ConstantInfo> {
        let name: String = item.ident.to_string();

        if !self.abi_constants.contains(name.as_str()) {
            return None;
        }

        let type_name: String = type_to_string(&item.ty);
        let value: String = expr_to_string(&item.expr);

        Some(ConstantInfo {
            name,
            value,
            type_name,
        })
    }

    /// Parse a struct item and return `StructInfo` if it's an ABI struct.
    fn parse_struct(&self, item: &ItemStruct) -> Option<StructInfo> {
        let name: String = item.ident.to_string();

        if !self.abi_types.contains(name.as_str()) {
            return None;
        }

        if !is_public(&item.vis) {
            return None;
        }

        let doc: Option<String> = extract_doc(&item.attrs);
        let fields: Vec<FieldInfo> = self.parse_fields(&item.fields);

        Some(StructInfo { name, fields, doc })
    }

    /// Parse struct fields and return a list of `FieldInfo`.
    fn parse_fields(&self, fields: &Fields) -> Vec<FieldInfo> {
        match fields {
            Fields::Named(named) => named
                .named
                .iter()
                .filter_map(|field| {
                    if !is_public(&field.vis) {
                        return None;
                    }

                    let name: String = field.ident.as_ref()?.to_string();
                    let type_name: String = type_to_string(&field.ty);
                    let doc: Option<String> = extract_doc(&field.attrs);

                    Some(FieldInfo {
                        name,
                        type_name,
                        doc,
                    })
                })
                .collect(),
            Fields::Unnamed(unnamed) => unnamed
                .unnamed
                .iter()
                .enumerate()
                .filter_map(|(index, field)| {
                    if !is_public(&field.vis) {
                        return None;
                    }

                    let name: String = format!("field_{}", index);
                    let type_name: String = type_to_string(&field.ty);
                    let doc: Option<String> = extract_doc(&field.attrs);

                    Some(FieldInfo {
                        name,
                        type_name,
                        doc,
                    })
                })
                .collect(),
            Fields::Unit => Vec::new(),
        }
    }

    /// Parse an enum item and return `EnumInfo` if it's an ABI enum.
    fn parse_enum(&self, item: &ItemEnum) -> Option<EnumInfo> {
        let name: String = item.ident.to_string();

        if !self.abi_types.contains(name.as_str()) {
            return None;
        }

        if !is_public(&item.vis) {
            return None;
        }

        let doc: Option<String> = extract_doc(&item.attrs);
        let variants: Vec<VariantInfo> = item
            .variants
            .iter()
            .map(|variant| {
                let name: String = variant.ident.to_string();
                let value: Option<i64> = variant
                    .discriminant
                    .as_ref()
                    .and_then(|(_, expr)| expr_to_int(expr));
                let doc: Option<String> = extract_doc(&variant.attrs);

                VariantInfo { name, value, doc }
            })
            .collect();

        Some(EnumInfo {
            name,
            variants,
            doc,
        })
    }

    /// Parse a union item and return `UnionInfo` if it's an ABI union.
    fn parse_union(&self, item: &ItemUnion) -> Option<UnionInfo> {
        let name: String = item.ident.to_string();

        if !self.abi_types.contains(name.as_str()) {
            return None;
        }

        if !is_public(&item.vis) {
            return None;
        }

        let doc: Option<String> = extract_doc(&item.attrs);
        let variants: Vec<UnionVariantInfo> = item
            .fields
            .named
            .iter()
            .filter_map(|field| {
                let name: String = field.ident.as_ref()?.to_string();
                let type_name: String = type_to_string(&field.ty);
                let doc: Option<String> = extract_doc(&field.attrs);

                Some(UnionVariantInfo {
                    name,
                    type_name,
                    doc,
                })
            })
            .collect();

        Some(UnionInfo {
            name,
            variants,
            doc,
        })
    }

    /// Parse a function item and return `FunctionInfo` if it's an ABI function.
    fn parse_function(&self, item: &ItemFn) -> Option<FunctionInfo> {
        let name: String = item.sig.ident.to_string();

        if !self.abi_functions.contains(name.as_str()) {
            return None;
        }

        if !is_visible(&item.vis) {
            return None;
        }

        let doc: Option<String> = extract_doc(&item.attrs);
        let return_type: String = match &item.sig.output {
            syn::ReturnType::Default => String::from("()"),
            syn::ReturnType::Type(_, ty) => type_to_string(ty),
        };

        Some(FunctionInfo {
            name,
            return_type,
            doc,
        })
    }
}

/// Check if a visibility is public or pub(crate).
fn is_visible(vis: &Visibility) -> bool {
    match vis {
        Visibility::Public(_) => true,
        Visibility::Restricted(restricted) => {
            restricted.path.segments.len() == 1 && restricted.path.segments[0].ident == "crate"
        }
        _ => false,
    }
}

/// Check if a visibility is public.
fn is_public(vis: &Visibility) -> bool {
    matches!(vis, Visibility::Public(_))
}

/// Extract documentation from attributes.
fn extract_doc(attrs: &[Attribute]) -> Option<String> {
    let doc_lines: Vec<String> = attrs
        .iter()
        .filter_map(|attr| {
            if !attr.path().is_ident("doc") {
                return None;
            }

            let meta = &attr.meta;
            match meta {
                syn::Meta::NameValue(name_value) => {
                    if let Expr::Lit(ExprLit {
                        lit: Lit::Str(lit_str),
                        ..
                    }) = &name_value.value
                    {
                        Some(lit_str.value())
                    } else {
                        None
                    }
                }
                _ => None,
            }
        })
        .collect();

    if doc_lines.is_empty() {
        None
    } else {
        Some(doc_lines.join("\n"))
    }
}

/// Convert a type to a string representation.
fn type_to_string(ty: &syn::Type) -> String {
    quote::quote!(#ty).to_string().replace(' ', "")
}

/// Convert an expression to a string representation.
fn expr_to_string(expr: &Expr) -> String {
    match expr {
        Expr::Lit(lit) => match &lit.lit {
            Lit::Int(int_lit) => int_lit.base10_digits().to_string(),
            Lit::Float(float_lit) => float_lit.base10_digits().to_string(),
            Lit::Str(str_lit) => format!("\"{}\"", str_lit.value()),
            Lit::Bool(bool_lit) => bool_lit.value().to_string(),
            _ => quote::quote!(#expr).to_string().replace(' ', ""),
        },
        _ => quote::quote!(#expr).to_string().replace(' ', ""),
    }
}

/// Convert an expression to an integer value if possible.
fn expr_to_int(expr: &Expr) -> Option<i64> {
    match expr {
        Expr::Lit(lit) => match &lit.lit {
            Lit::Int(int_lit) => int_lit.base10_parse().ok(),
            _ => None,
        },
        Expr::Unary(unary) => {
            let inner: i64 = expr_to_int(&unary.expr)?;
            match unary.op {
                syn::UnOp::Neg(_) => Some(-inner),
                _ => None,
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ABI_SOURCE: &str = include_str!("../lib.rs");

    #[test]
    fn parse_abi_source() {
        let parser: AbiParser = AbiParser::new();
        let info: AbiInfo = parser
            .parse(ABI_SOURCE)
            .expect("failed to parse ABI source");

        assert!(!info.constants.is_empty(), "should have constants");
        assert!(!info.structs.is_empty(), "should have structs");
        assert!(!info.enums.is_empty(), "should have enums");
        assert!(!info.unions.is_empty(), "should have unions");
        assert!(!info.functions.is_empty(), "should have functions");
    }

    #[test]
    fn parse_constants() {
        let parser: AbiParser = AbiParser::new();
        let info: AbiInfo = parser
            .parse(ABI_SOURCE)
            .expect("failed to parse ABI source");

        let constant_names: Vec<&str> = info.constants.iter().map(|c| c.name.as_str()).collect();

        assert!(constant_names.contains(&"POLYPLUG_ABI_VERSION"));
        assert!(constant_names.contains(&"ABI_OK"));
        assert!(constant_names.contains(&"ABI_ERROR_GENERIC"));
        assert!(constant_names.contains(&"ABI_BUFFER_TOO_SMALL"));
        assert!(constant_names.contains(&"ABI_ERROR_PANIC"));
        assert!(constant_names.contains(&"ABI_ERROR_NOT_FOUND"));
        assert!(constant_names.contains(&"ABI_ERROR_STALE_HANDLE"));
        assert!(constant_names.contains(&"ABI_FUNCTION_NOT_AVAIL"));
    }

    #[test]
    fn parse_structs() {
        let parser: AbiParser = AbiParser::new();
        let info: AbiInfo = parser
            .parse(ABI_SOURCE)
            .expect("failed to parse ABI source");

        let struct_names: Vec<&str> = info.structs.iter().map(|s| s.name.as_str()).collect();

        assert!(struct_names.contains(&"StringView"));
        assert!(struct_names.contains(&"Buffer"));
        assert!(struct_names.contains(&"AbiError"));
        assert!(struct_names.contains(&"PluginHandle"));
        assert!(struct_names.contains(&"HostContext"));
        assert!(struct_names.contains(&"NativeDispatch"));
        assert!(struct_names.contains(&"VmDispatch"));
        assert!(struct_names.contains(&"PluginInterface"));
        assert!(struct_names.contains(&"HostVTable"));
        assert!(struct_names.contains(&"PluginDescriptor"));
        assert!(struct_names.contains(&"PluginContext"));
        assert!(struct_names.contains(&"ExtensionEntry"));
        assert!(struct_names.contains(&"RuntimeConfig"));
    }

    #[test]
    fn parse_enums() {
        let parser: AbiParser = AbiParser::new();
        let info: AbiInfo = parser
            .parse(ABI_SOURCE)
            .expect("failed to parse ABI source");

        let enum_names: Vec<&str> = info.enums.iter().map(|e| e.name.as_str()).collect();

        assert!(enum_names.contains(&"DispatchType"));
    }

    #[test]
    fn parse_unions() {
        let parser: AbiParser = AbiParser::new();
        let info: AbiInfo = parser
            .parse(ABI_SOURCE)
            .expect("failed to parse ABI source");

        let union_names: Vec<&str> = info.unions.iter().map(|u| u.name.as_str()).collect();

        assert!(union_names.contains(&"PluginDispatch"));
    }

    #[test]
    fn parse_functions() {
        let parser: AbiParser = AbiParser::new();
        let info: AbiInfo = parser
            .parse(ABI_SOURCE)
            .expect("failed to parse ABI source");

        let function_names: Vec<&str> = info.functions.iter().map(|f| f.name.as_str()).collect();

        assert!(function_names.contains(&"fnv1a_64"));
        assert!(function_names.contains(&"fnv1a_32"));
        assert!(function_names.contains(&"contract_id"));
        assert!(function_names.contains(&"extension_id"));
        assert!(function_names.contains(&"bundle_id"));
    }

    #[test]
    fn string_view_has_fields() {
        let parser: AbiParser = AbiParser::new();
        let info: AbiInfo = parser
            .parse(ABI_SOURCE)
            .expect("failed to parse ABI source");

        let string_view: &StructInfo = info
            .structs
            .iter()
            .find(|s| s.name == "StringView")
            .expect("StringView should exist");

        assert_eq!(string_view.fields.len(), 2);

        let ptr_field: &FieldInfo = string_view
            .fields
            .iter()
            .find(|f| f.name == "ptr")
            .expect("ptr field should exist");
        assert!(ptr_field.type_name.contains("*constu8"));

        let len_field: &FieldInfo = string_view
            .fields
            .iter()
            .find(|f| f.name == "len")
            .expect("len field should exist");
        assert!(len_field.type_name.contains("usize"));
    }

    #[test]
    fn dispatch_type_has_variants() {
        let parser: AbiParser = AbiParser::new();
        let info: AbiInfo = parser
            .parse(ABI_SOURCE)
            .expect("failed to parse ABI source");

        let dispatch_type: &EnumInfo = info
            .enums
            .iter()
            .find(|e| e.name == "DispatchType")
            .expect("DispatchType should exist");

        assert_eq!(dispatch_type.variants.len(), 2);

        let native: &VariantInfo = dispatch_type
            .variants
            .iter()
            .find(|v| v.name == "Native")
            .expect("Native variant should exist");
        assert_eq!(native.value, Some(0));

        let vm: &VariantInfo = dispatch_type
            .variants
            .iter()
            .find(|v| v.name == "VirtualMachine")
            .expect("VirtualMachine variant should exist");
        assert_eq!(vm.value, Some(1));
    }

    #[test]
    fn plugin_dispatch_has_variants() {
        let parser: AbiParser = AbiParser::new();
        let info: AbiInfo = parser
            .parse(ABI_SOURCE)
            .expect("failed to parse ABI source");

        let plugin_dispatch: &UnionInfo = info
            .unions
            .iter()
            .find(|u| u.name == "PluginDispatch")
            .expect("PluginDispatch should exist");

        assert_eq!(plugin_dispatch.variants.len(), 2);

        let native: &UnionVariantInfo = plugin_dispatch
            .variants
            .iter()
            .find(|v| v.name == "native")
            .expect("native variant should exist");
        assert!(native.type_name.contains("NativeDispatch"));

        let vm: &UnionVariantInfo = plugin_dispatch
            .variants
            .iter()
            .find(|v| v.name == "vm")
            .expect("vm variant should exist");
        assert!(vm.type_name.contains("VmDispatch"));
    }

    #[test]
    fn constants_have_correct_values() {
        let parser: AbiParser = AbiParser::new();
        let info: AbiInfo = parser
            .parse(ABI_SOURCE)
            .expect("failed to parse ABI source");

        let abi_version: &ConstantInfo = info
            .constants
            .iter()
            .find(|c| c.name == "POLYPLUG_ABI_VERSION")
            .expect("POLYPLUG_ABI_VERSION should exist");
        assert_eq!(abi_version.value, "1");
        assert_eq!(abi_version.type_name, "u32");

        let abi_ok: &ConstantInfo = info
            .constants
            .iter()
            .find(|c| c.name == "ABI_OK")
            .expect("ABI_OK should exist");
        assert_eq!(abi_ok.value, "0");

        let abi_error_generic: &ConstantInfo = info
            .constants
            .iter()
            .find(|c| c.name == "ABI_ERROR_GENERIC")
            .expect("ABI_ERROR_GENERIC should exist");
        assert_eq!(abi_error_generic.value, "1");
    }

    #[test]
    fn structs_have_docs() {
        let parser: AbiParser = AbiParser::new();
        let info: AbiInfo = parser
            .parse(ABI_SOURCE)
            .expect("failed to parse ABI source");

        let string_view: &StructInfo = info
            .structs
            .iter()
            .find(|s| s.name == "StringView")
            .expect("StringView should exist");

        assert!(string_view.doc.is_some());
        assert!(string_view
            .doc
            .as_ref()
            .unwrap()
            .contains("Non-owning UTF-8 string view"));
    }

    #[test]
    fn functions_have_return_types() {
        let parser: AbiParser = AbiParser::new();
        let info: AbiInfo = parser
            .parse(ABI_SOURCE)
            .expect("failed to parse ABI source");

        let contract_id: &FunctionInfo = info
            .functions
            .iter()
            .find(|f| f.name == "contract_id")
            .expect("contract_id should exist");

        assert!(contract_id.return_type.contains("u64"));
    }
}

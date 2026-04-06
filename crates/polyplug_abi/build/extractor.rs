//! Type extractor — extracts ABI types from syn AST.
//!
//! This module parses Rust source code using `syn` and extracts all
//! `#[repr(C)]` types (structs, enums, unions) plus ABI constants
//! and functions into `AbiTypes` for code generation.

use syn::{
    Attribute, Expr, ExprLit, Fields, File, Item, ItemConst, ItemEnum, ItemFn, ItemStruct,
    ItemUnion, Lit, Meta, Visibility, parse_file,
};

use crate::types::{
    AbiConst, AbiEnum, AbiField, AbiFunction, AbiStruct, AbiTypes, AbiUnion, AbiUnionVariant,
    AbiVariant,
};

/// ABI types that should be extracted by the extractor.
const ABI_TYPES: &[&str] = &[
    "StringView",
    "Buffer",
    "AbiError",
    "PluginHandle",
    "HostContext",
    "RuntimeContext",     // Opaque handle wrapping HostContext
    "VmLoaderData",       // Opaque handle for VM loader state
    "GuestContractInstance", // Opaque handle for guest contract instances
    "HostContractInstance",  // Opaque handle for host contract instances
    "DispatchType",
    "NativeDispatch",
    "VmDispatch",
    "PluginDispatch",
    "PluginInterface",
    "RuntimeAbi",
    "PluginDescriptor",
    "PluginContext",
    "ExtensionEntry",
    "RuntimeConfig",
    "RuntimeLanguage",
    "HostContractVTableHeader",
    "NativeHostContractDispatch",
    "VmHostContractDispatch",
    "HostContractDispatch",
    "HostContractVTable",
    "AbiErrorCode",
];

/// ABI constants that should be extracted by the extractor.
const ABI_CONSTANTS: &[&str] = &["POLYPLUG_ABI_VERSION"];

/// ABI functions that should be extracted by the extractor.
const ABI_FUNCTIONS: &[&str] = &[
    "fnv1a_64",
    "fnv1a_32",
    "contract_id",
    "extension_id",
    "bundle_id",
    "host_contract_id",
    "plugin_contract_id",
    "string_view_from_static",
    "string_view_null",
    "string_view_as_str",
    "string_view_to_string_owned",
    "buffer_as_slice",
    "buffer_as_mut_slice",
    "abi_error_ok",
    "abi_error_panic_caught",
    "abi_error_is_ok",
    "plugin_handle_null",
    "plugin_handle_is_null",
];

/// Extract all ABI types from Rust source code.
///
/// # Arguments
/// * `source` - The Rust source code to parse.
///
/// # Returns
/// An `AbiTypes` struct containing all extracted types, or a syn error.
pub fn extract_types(source: &str) -> Result<AbiTypes, syn::Error> {
    let file: File = parse_file(source)?;
    let mut types: AbiTypes = AbiTypes::new();

    for item in &file.items {
        match item {
            Item::Const(item_const) => {
                if let Some(const_info) = extract_const(item_const) {
                    types.add_const(const_info);
                }
            }
            Item::Struct(item_struct) => {
                if let Some(struct_info) = extract_struct(item_struct) {
                    types.add_struct(struct_info);
                }
            }
            Item::Enum(item_enum) => {
                if let Some(enum_info) = extract_enum(item_enum) {
                    types.add_enum(enum_info);
                }
            }
            Item::Union(item_union) => {
                if let Some(union_info) = extract_union(item_union) {
                    types.add_union(union_info);
                }
            }
            Item::Fn(item_fn) => {
                if let Some(function_info) = extract_function(item_fn) {
                    types.add_function(function_info);
                }
            }
            _ => {}
        }
    }

    Ok(types)
}

/// Extract a constant if it's an ABI constant.
fn extract_const(item: &ItemConst) -> Option<AbiConst> {
    let name: String = item.ident.to_string();

    if !ABI_CONSTANTS.contains(&name.as_str()) {
        return None;
    }

    let rust_type: String = type_to_string(&item.ty);
    let value: String = expr_to_string(&item.expr);
    let doc: Option<String> = extract_doc(&item.attrs);

    Some(AbiConst {
        name,
        rust_type,
        value,
        doc,
    })
}

/// Extract a struct if it's an ABI struct with #[repr(C)].
fn extract_struct(item: &ItemStruct) -> Option<AbiStruct> {
    let name: String = item.ident.to_string();

    if !ABI_TYPES.contains(&name.as_str()) {
        return None;
    }

    if !is_public(&item.vis) {
        return None;
    }

    let repr_c: bool = has_repr_c(&item.attrs);
    let doc: Option<String> = extract_doc(&item.attrs);
    let fields: Vec<AbiField> = extract_fields(&item.fields);

    Some(AbiStruct {
        name,
        fields,
        doc,
        repr_c,
    })
}

/// Extract fields from a struct.
fn extract_fields(fields: &Fields) -> Vec<AbiField> {
    match fields {
        Fields::Named(named) => named
            .named
            .iter()
            .filter_map(|field| {
                if !is_public(&field.vis) {
                    return None;
                }

                let name: String = field.ident.as_ref()?.to_string();
                let rust_type: String = type_to_string(&field.ty);
                let doc: Option<String> = extract_doc(&field.attrs);

                Some(AbiField {
                    name,
                    rust_type,
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
                let rust_type: String = type_to_string(&field.ty);
                let doc: Option<String> = extract_doc(&field.attrs);

                Some(AbiField {
                    name,
                    rust_type,
                    doc,
                })
            })
            .collect(),
        Fields::Unit => Vec::new(),
    }
}

/// Extract an enum if it's an ABI enum with #[repr(C)] or #[repr(uX)].
fn extract_enum(item: &ItemEnum) -> Option<AbiEnum> {
    let name: String = item.ident.to_string();

    if !ABI_TYPES.contains(&name.as_str()) {
        return None;
    }

    if !is_public(&item.vis) {
        return None;
    }

    let repr: String = extract_enum_repr(&item.attrs);
    let doc: Option<String> = extract_doc(&item.attrs);
    let variants: Vec<AbiVariant> = item
        .variants
        .iter()
        .map(|variant| {
            let name: String = variant.ident.to_string();
            let value: Option<u64> = variant
                .discriminant
                .as_ref()
                .and_then(|(_, expr)| expr_to_u64(expr));
            let doc: Option<String> = extract_doc(&variant.attrs);

            AbiVariant { name, value, doc }
        })
        .collect();

    Some(AbiEnum {
        name,
        repr,
        variants,
        doc,
    })
}

/// Extract a union if it's an ABI union with #[repr(C)].
fn extract_union(item: &ItemUnion) -> Option<AbiUnion> {
    let name: String = item.ident.to_string();

    if !ABI_TYPES.contains(&name.as_str()) {
        return None;
    }

    if !is_public(&item.vis) {
        return None;
    }

    let doc: Option<String> = extract_doc(&item.attrs);
    let variants: Vec<AbiUnionVariant> = item
        .fields
        .named
        .iter()
        .filter_map(|field| {
            let name: String = field.ident.as_ref()?.to_string();
            let rust_type: String = type_to_string(&field.ty);
            let doc: Option<String> = extract_doc(&field.attrs);

            Some(AbiUnionVariant {
                name,
                rust_type,
                doc,
            })
        })
        .collect();

    Some(AbiUnion {
        name,
        variants,
        doc,
    })
}

/// Extract a function if it's an ABI function.
fn extract_function(item: &ItemFn) -> Option<AbiFunction> {
    let name: String = item.sig.ident.to_string();

    if !ABI_FUNCTIONS.contains(&name.as_str()) {
        return None;
    }

    if !is_visible(&item.vis) {
        return None;
    }

    let doc: Option<String> = extract_doc(&item.attrs);
    let return_type: Option<String> = match &item.sig.output {
        syn::ReturnType::Default => None,
        syn::ReturnType::Type(_, ty) => Some(type_to_string(ty)),
    };

    let params: Vec<AbiField> = item
        .sig
        .inputs
        .iter()
        .filter_map(|arg| match arg {
            syn::FnArg::Typed(pat_type) => {
                let name: String = match pat_type.pat.as_ref() {
                    syn::Pat::Ident(pat_ident) => pat_ident.ident.to_string(),
                    _ => return None,
                };
                let rust_type: String = type_to_string(&pat_type.ty);
                let doc: Option<String> = None;

                Some(AbiField {
                    name,
                    rust_type,
                    doc,
                })
            }
            syn::FnArg::Receiver(_) => None,
        })
        .collect();

    Some(AbiFunction {
        name,
        params,
        return_type,
        doc,
    })
}

/// Check if a visibility is public.
fn is_public(vis: &Visibility) -> bool {
    matches!(vis, Visibility::Public(_))
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

/// Check if attributes contain #[repr(C)].
fn has_repr_c(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if !attr.path().is_ident("repr") {
            return false;
        }

        let meta: &Meta = &attr.meta;
        match meta {
            Meta::List(list) => list.tokens.to_string().split(',').any(|s| s.trim() == "C"),
            _ => false,
        }
    })
}

/// Extract enum repr type from attributes (e.g., "u32", "u8").
fn extract_enum_repr(attrs: &[Attribute]) -> String {
    attrs
        .iter()
        .find_map(|attr| {
            if !attr.path().is_ident("repr") {
                return None;
            }

            let meta: &Meta = &attr.meta;
            match meta {
                Meta::List(list) => {
                    let tokens: String = list.tokens.to_string();
                    let parts: Vec<&str> = tokens.split(',').map(|s| s.trim()).collect();
                    for part in parts {
                        if part.starts_with('u') || part.starts_with('i') {
                            return Some(part.to_string());
                        }
                    }
                    None
                }
                _ => None,
            }
        })
        .unwrap_or_else(|| "u32".to_string())
}

/// Extract documentation from attributes.
fn extract_doc(attrs: &[Attribute]) -> Option<String> {
    let doc_lines: Vec<String> = attrs
        .iter()
        .filter_map(|attr| {
            if !attr.path().is_ident("doc") {
                return None;
            }

            let meta: &Meta = &attr.meta;
            match meta {
                Meta::NameValue(name_value) => {
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

/// Convert an expression to a u64 value if possible.
fn expr_to_u64(expr: &Expr) -> Option<u64> {
    match expr {
        Expr::Lit(lit) => match &lit.lit {
            Lit::Int(int_lit) => int_lit.base10_parse().ok(),
            _ => None,
        },
        Expr::Unary(unary) => {
            let inner: u64 = expr_to_u64(&unary.expr)?;
            match unary.op {
                syn::UnOp::Neg(_) => Some(inner), // Neg handled by parse
                _ => None,
            }
        }
        _ => None,
    }
}

//! Type extractor — recursively walks the module tree to auto-discover ABI types.
//!
//! This module parses Rust source code using `syn` and extracts all
//! `#[repr(C)]` types (structs, enums, unions) plus `POLYPLUG_` constants
//! into `AbiTypes` for code generation.
//!
//! # Auto-discovery
//!
//! No whitelists are used. Types are discovered by:
//! - **Structs**: Any `pub` struct with `#[repr(C)]`
//! - **Enums**: Any `pub` enum with `#[repr(u*)]`
//! - **Unions**: Any `pub` union with `#[repr(C)]`
//! - **Constants**: Any `pub const` whose name starts with `POLYPLUG_`
//!
//! # Module tree walking
//!
//! Starting from `lib.rs`, the extractor recursively resolves all `mod X;`
//! declarations (both `pub mod` and private `mod`) to their source files
//! (`{dir}/X.rs` or `{dir}/X/mod.rs`) and extracts types from each file.
//! Types within files are only extracted if they are `pub` and have the
//! appropriate `#[repr(...)]` attributes.

use std::fs;
use std::path::{Path, PathBuf};

use syn::{
    Attribute, Expr, ExprLit, Fields, File, Item, ItemConst, ItemEnum, ItemStruct, ItemUnion, Lit,
    Meta, Visibility, parse_file,
};

use crate::types::{
    AbiConst, AbiEnum, AbiField, AbiStruct, AbiTypes, AbiUnion, AbiUnionVariant, AbiVariant,
};

/// Extract all ABI types by recursively walking the module tree starting from `src_dir/lib.rs`.
///
/// Returns both the extracted types and all source file paths discovered
/// (for `cargo:rerun-if-changed` tracking).
///
/// # Arguments
/// * `src_dir` - The `src/` directory containing `lib.rs`.
///
/// # Returns
/// A tuple of `(AbiTypes, Vec<PathBuf>)` containing extracted types and tracked files.
pub fn extract_from_dir(src_dir: &Path) -> Result<(AbiTypes, Vec<PathBuf>), String> {
    let lib_rs: PathBuf = src_dir.join("lib.rs");
    if !lib_rs.exists() {
        return Err(format!("lib.rs not found at {}", lib_rs.display()));
    }

    let mut abi_types: AbiTypes = AbiTypes::new();
    let mut tracked_files: Vec<PathBuf> = Vec::new();

    // Track lib.rs itself
    tracked_files.push(lib_rs.clone());

    // Walk the module tree starting from lib.rs
    walk_module_tree(src_dir, &lib_rs, &mut abi_types, &mut tracked_files)?;

    Ok((abi_types, tracked_files))
}

/// Recursively walk the module tree, extracting types from each file.
///
/// For the given `module_file`, parses the source and:
/// 1. Extracts all ABI types (structs, enums, unions, consts) from the file.
/// 2. Finds `pub mod X;` declarations and resolves them to source files.
/// 3. Recursively walks each resolved sub-module.
///
/// # Arguments
/// * `dir` - The directory containing the current module file.
/// * `module_file` - Path to the `.rs` file to parse.
/// * `abi_types` - Mutable accumulator for discovered types.
/// * `tracked_files` - Mutable accumulator for all discovered source paths.
fn walk_module_tree(
    dir: &Path,
    module_file: &Path,
    abi_types: &mut AbiTypes,
    tracked_files: &mut Vec<PathBuf>,
) -> Result<(), String> {
    let source: String = fs::read_to_string(module_file).map_err(|e| {
        format!(
            "Failed to read module file {}: {}",
            module_file.display(),
            e
        )
    })?;

    let file: File = parse_file(&source)
        .map_err(|e| format!("Failed to parse {}: {}", module_file.display(), e))?;

    // Extract types from the current file
    extract_types_from_file(&file, abi_types);

    // Find and resolve mod declarations (both pub and private)
    // Private mod declarations may still contain pub #[repr(C)] types
    // that are re-exported by the parent module.
    for item in &file.items {
        if let Item::Mod(item_mod) = item {
            // Only process file-based modules (not inline `mod name { ... }`)
            if item_mod.content.is_some() {
                continue;
            }

            let mod_name: String = item_mod.ident.to_string();

            // Try dir/{name}.rs first, then dir/{name}/mod.rs
            let file_path: PathBuf = dir.join(format!("{}.rs", mod_name));
            let sub_dir: PathBuf = dir.join(&mod_name);
            let mod_rs: PathBuf = sub_dir.join("mod.rs");

            let target: &Path = if file_path.exists() {
                &file_path
            } else if mod_rs.exists() {
                &mod_rs
            } else {
                // Module file not found — may be conditionally compiled.
                // Skip silently rather than fail.
                continue;
            };

            tracked_files.push(target.to_path_buf());

            // Recursively walk the sub-module
            walk_module_tree(&sub_dir, target, abi_types, tracked_files)?;
        }
    }

    Ok(())
}

/// Extract types from a parsed file into the accumulator.
fn extract_types_from_file(file: &File, types: &mut AbiTypes) {
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
            _ => {}
        }
    }
}

/// Extract a constant if its name starts with `POLYPLUG_`.
///
/// Auto-discovers ABI constants by naming convention — no whitelist required.
fn extract_const(item: &ItemConst) -> Option<AbiConst> {
    let name: String = item.ident.to_string();

    if !name.starts_with("POLYPLUG_") {
        return None;
    }

    if !is_public(&item.vis) {
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

/// Extract a struct if it is `pub` and has `#[repr(C)]`.
///
/// Auto-discovers ABI structs by attribute — no whitelist required.
fn extract_struct(item: &ItemStruct) -> Option<AbiStruct> {
    if !is_public(&item.vis) {
        return None;
    }

    if !has_repr_c(&item.attrs) {
        return None;
    }

    let name: String = item.ident.to_string();
    let doc: Option<String> = extract_doc(&item.attrs);
    let fields: Vec<AbiField> = extract_fields(&item.fields);

    Some(AbiStruct {
        name,
        fields,
        doc,
        repr_c: true,
        size_hint: None,
    })
}

/// Extract fields from a struct.
pub(crate) fn extract_fields(fields: &Fields) -> Vec<AbiField> {
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

/// Extract an enum if it is `pub` and has `#[repr(u*)]` or `#[repr(C)]`.
///
/// Auto-discovers ABI enums by attribute — no whitelist required.
fn extract_enum(item: &ItemEnum) -> Option<AbiEnum> {
    if !is_public(&item.vis) {
        return None;
    }

    let repr: String = extract_enum_repr(&item.attrs);

    // Must have an integer repr (u8, u16, u32, u64, i8, i16, i32, i64) or C repr
    let has_int_repr: bool = repr.starts_with('u') || repr.starts_with('i');
    let has_c_repr: bool = has_repr_c(&item.attrs);
    if !has_int_repr && !has_c_repr {
        return None;
    }

    let name: String = item.ident.to_string();
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

/// Extract a union if it is `pub` and has `#[repr(C)]`.
///
/// Auto-discovers ABI unions by attribute — no whitelist required.
fn extract_union(item: &ItemUnion) -> Option<AbiUnion> {
    if !is_public(&item.vis) {
        return None;
    }

    if !has_repr_c(&item.attrs) {
        return None;
    }

    let name: String = item.ident.to_string();
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

/// Check if a visibility is public.
pub(crate) fn is_public(vis: &Visibility) -> bool {
    matches!(vis, Visibility::Public(_))
}

/// Check if attributes contain #[repr(C)].
pub(crate) fn has_repr_c(attrs: &[Attribute]) -> bool {
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
pub(crate) fn extract_doc(attrs: &[Attribute]) -> Option<String> {
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

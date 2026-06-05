//! Build script for polyplug_abi — generates SDK bindings from ABI definitions.
//!
//! This build script:
//! 1. Recursively walks the module tree starting from `src/lib.rs`
//! 2. Auto-discovers all `#[repr(C)]` structs/enums/unions and `POLYPLUG_` constants
//! 3. Scans loader crate config files for additional ABI types
//! 4. Validates that all types can be represented in target languages
//! 5. Calls polyplug_codegen for each target language
//! 6. Writes generated SDK files to `sdks/{lang}/abi/`
//! 7. Emits `cargo:rerun-if-changed` for all tracked source files

mod extractor;
mod generate;
mod mapper;
mod types;

use std::env;
use std::fs;
use std::path::PathBuf;

use crate::extractor::extract_from_dir;
use crate::generate::generate_all_sdks;

/// Loader crates whose config structs should be discovered.
const LOADER_CRATES: &[&str] = &[
    "polyplug_native",
    "polyplug_python",
    "polyplug_lua",
    "polyplug_js",
    "polyplug_dotnet",
];

fn main() -> Result<(), Box<dyn core::error::Error>> {
    let manifest_dir: PathBuf = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let workspace_root: PathBuf = manifest_dir
        .parent()
        .ok_or("polyplug_abi should be in crates/ directory")?
        .parent()
        .ok_or("crates/ should be in workspace root")?
        .to_path_buf();

    // ─── Step 1: Extract ABI types from polyplug_abi module tree ─────────────
    let src_dir: PathBuf = manifest_dir.join("src");
    let (mut abi_types, mut tracked_files) = extract_from_dir(&src_dir)?;

    // ─── Step 2: Scan loader crates for config structs ───────────────────────
    for loader_name in LOADER_CRATES {
        let loader_src_dir: PathBuf = workspace_root.join("crates").join(loader_name).join("src");

        // Try config.rs first, fall back to lib.rs
        let config_path: PathBuf = loader_src_dir.join("config.rs");
        let lib_path: PathBuf = loader_src_dir.join("lib.rs");

        let target: Option<PathBuf> = if config_path.exists() {
            Some(config_path)
        } else if lib_path.exists() {
            Some(lib_path)
        } else {
            None
        };

        if let Some(target_path) = target {
            // Extract types from the loader config file
            let source: String =
                fs::read_to_string(&target_path).map_err(|e: std::io::Error| {
                    format!("Failed to read {}: {}", target_path.display(), e)
                })?;

            let file: syn::File = syn::parse_file(&source).map_err(|e: syn::Error| {
                format!("Failed to parse {}: {}", target_path.display(), e)
            })?;

            let mut loader_types: types::AbiTypes = types::AbiTypes::new();

            // Extract structs with #[repr(C)] from the loader config
            for item in &file.items {
                if let syn::Item::Struct(item_struct) = item {
                    // Use extractor's auto-discovery logic:
                    // pub struct with #[repr(C)]
                    if is_public(&item_struct.vis) && has_repr_c(&item_struct.attrs) {
                        let name: String = item_struct.ident.to_string();
                        let fields: Vec<types::AbiField> =
                            extract_fields_from_syn(&item_struct.fields);
                        let doc: Option<String> = extract_doc_from_attrs(&item_struct.attrs);

                        loader_types.add_struct(types::AbiStruct {
                            name,
                            fields,
                            doc,
                            repr_c: true,
                            size_hint: None,
                        });
                    }
                }
            }

            abi_types.merge(loader_types);
            tracked_files.push(target_path);
        }
    }

    // ─── Step 3: Generate SDKs ───────────────────────────────────────────────
    generate_all_sdks(&mut abi_types, &workspace_root, &tracked_files)?;
    Ok(())
}

/// Check if a visibility is public.
fn is_public(vis: &syn::Visibility) -> bool {
    matches!(vis, syn::Visibility::Public(_))
}

/// Check if attributes contain #[repr(C)].
fn has_repr_c(attrs: &[syn::Attribute]) -> bool {
    use syn::Meta;
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

/// Extract fields from a syn Fields.
fn extract_fields_from_syn(fields: &syn::Fields) -> Vec<types::AbiField> {
    use syn::Fields;

    match fields {
        Fields::Named(named) => named
            .named
            .iter()
            .filter_map(|field| {
                if !is_public(&field.vis) {
                    return None;
                }
                let name: String = field.ident.as_ref()?.to_string();
                let rust_type: String = quote::quote!(#field.ty).to_string().replace(' ', "");
                let doc: Option<String> = extract_doc_from_attrs(&field.attrs);
                Some(types::AbiField {
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
                let rust_type: String = quote::quote!(#field.ty).to_string().replace(' ', "");
                let doc: Option<String> = extract_doc_from_attrs(&field.attrs);
                Some(types::AbiField {
                    name,
                    rust_type,
                    doc,
                })
            })
            .collect(),
        Fields::Unit => Vec::new(),
    }
}

/// Extract documentation from attributes.
fn extract_doc_from_attrs(attrs: &[syn::Attribute]) -> Option<String> {
    use syn::{Expr, ExprLit, Lit, Meta};

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

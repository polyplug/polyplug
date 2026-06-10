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

use crate::extractor::{extract_doc, extract_fields, extract_from_dir, has_repr_c, is_public};
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
                        let fields: Vec<types::AbiField> = extract_fields(&item_struct.fields);
                        let doc: Option<String> = extract_doc(&item_struct.attrs);

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

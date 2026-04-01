//! Build script for polyplug_abi — generates SDK bindings from ABI definitions.
//!
//! This build script:
//! 1. Reads `src/lib.rs` source code
//! 2. Parses with syn to extract ABI types (structs, enums, unions, constants, functions)
//! 3. Calls polyplug_codegen for each target language
//! 4. Writes generated SDK files to `sdks/{lang}/abi/`

mod generate;
mod extractor;
mod mapper;
mod types;

use std::env;
use std::fs;
use std::path::PathBuf;

use crate::extractor::extract_types;
use crate::generate::generate_all_sdks;

fn main() {
    println!("cargo:rerun-if-changed=src/lib.rs");

    let manifest_dir: PathBuf = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let workspace_root: PathBuf = manifest_dir
        .parent()
        .expect("polyplug_abi should be in crates/ directory")
        .parent()
        .expect("crates/ should be in workspace root")
        .to_path_buf();

    let lib_rs: PathBuf = manifest_dir.join("src/lib.rs");
    let content: String = fs::read_to_string(&lib_rs).expect("Failed to read src/lib.rs");

    let abi_types: types::AbiTypes =
        extract_types(&content).expect("Failed to extract ABI types");

    generate_all_sdks(&abi_types, &workspace_root)
        .expect("Failed to generate SDKs");
}

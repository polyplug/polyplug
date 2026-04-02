//! Build script for polyplug_abi — generates SDK bindings from ABI definitions.
//!
//! This build script:
//! 1. Reads `src/lib.rs` source code
//! 2. Parses with syn to extract ABI types (structs, enums, unions, constants, functions)
//! 3. Calls polyplug_codegen for each target language
//! 4. Writes generated SDK files to `sdks/{lang}/abi/`

#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

mod extractor;
mod generate;
mod mapper;
mod types;

use std::env;
use std::fs;
use std::path::PathBuf;

use crate::extractor::extract_types;
use crate::generate::generate_all_sdks;

fn main() {
    println!("cargo:rerun-if-changed=src/lib.rs");

    /*
    TODO:
    The idea is to inline helper methods inside the language classes or the logical way for the language
    for example C# StripPrefix would be inside StringView class. and to do it in correct way we will use
    ast-grep to to add/update/delete method signature/decleration only and method body will remain, so we
    can write helper methods body/code directly insdie the SDK files and it will remain same even after
    regenerate the code!

    Note:
    - To avoid delete methods mistake and prevent method body to get deleted by mistake, for deleted method
      will make ast-grep add `DELETED_` prefix for method name, and again will use `sdk_validator` to fail
      if found any method has `DELETED_` prefix!
    */

    // TODO: Update build script to work with the new modular rust files in abi crate

    let manifest_dir: PathBuf = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let workspace_root: PathBuf = manifest_dir
        .parent()
        .expect("polyplug_abi should be in crates/ directory")
        .parent()
        .expect("crates/ should be in workspace root")
        .to_path_buf();

    let lib_rs: PathBuf = manifest_dir.join("src/lib.rs");
    let content: String = fs::read_to_string(&lib_rs).expect("Failed to read src/lib.rs");

    let abi_types: types::AbiTypes = extract_types(&content).expect("Failed to extract ABI types");
    generate_all_sdks(&abi_types, &workspace_root).expect("Failed to generate SDKs");
}

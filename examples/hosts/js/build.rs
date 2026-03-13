// examples/hosts/js/build.rs
// Build script for polyplug_full cdylib.
fn main() {
    // Use a version script to control symbol visibility.
    // This ensures only our symbols are exported, while polyplug's rlib
    // symbols that happen to share names are hidden (not exported).
    // Combined with --allow-multiple-definition to suppress link errors.
    let manifest_dir: String = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    println!(
        "cargo:rustc-link-arg=-Wl,--version-script={manifest_dir}/polyplug_full.map"
    );
    println!("cargo:rustc-link-arg=-Wl,--allow-multiple-definition");
}

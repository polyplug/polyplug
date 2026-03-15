//! examples/hosts/rust/src/main.rs
//! Rust host example using polyplugc-generated bindings.
//!
//! This host demonstrates the real-world polyplug pattern:
//!   1. Generate host bindings: polyplugc --api api.toml --lang rust --out generated/
//!   2. Import generated callers: use generated::host::host_callers::*;
//!   3. Use type-safe contract wrappers instead of manual vtable dispatch
//!
//! Zero hand-written contract IDs, zero manual unsafe dispatch.

use std::path::Path;
use std::path::PathBuf;

use polyplug::abi::StringView;
use polyplug::loader::manifest::ManifestData;
use polyplug::loader::scanner;
use polyplug::runtime::Runtime;
use polyplug_dotnet::DotnetConfig;
use polyplug_dotnet::DotnetLoader;
use polyplug_js::JsConfig;
use polyplug_js::JsLoader;
use polyplug_js_deno::JsDenoConfig;
use polyplug_js_deno::JsDenoLoader;
use polyplug_lua::LuaConfig;
use polyplug_lua::LuaLoader;
use polyplug_native::NativeConfig;
use polyplug_native::NativeLoader;
use polyplug_python::PythonConfig;
use polyplug_python::PythonLoader;

// Import generated host bindings
mod generated {
    pub mod host {
        pub mod host_callers;
        pub mod types;
    }
}

use generated::host::host_callers::*;
use generated::host::types::*;

/// Convert a StringView to &str safely
fn string_view_to_str(sv: &StringView) -> &str {
    if sv.ptr.is_null() || sv.len == 0 {
        return "";
    }
    // SAFETY: sv.ptr is valid UTF-8 for sv.len bytes — guaranteed by guest ABI contract
    unsafe {
        core::str::from_utf8(core::slice::from_raw_parts(sv.ptr, sv.len))
            .unwrap_or("(invalid utf-8)")
    }
}

/// Resolve plugin path from env or default
fn resolve_plugin_path() -> PathBuf {
    if let Ok(path) = std::env::var("POLYPLUG_PLUGIN_PATH") {
        return PathBuf::from(path);
    }

    let candidates: [PathBuf; 2] = [
        PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    ];

    for seed in &candidates {
        let mut dir: PathBuf = seed.clone();
        for _ in 0..8_u32 {
            let plugins_path: PathBuf = dir.join("examples").join("plugins");
            if plugins_path.is_dir() {
                return plugins_path;
            }
            match dir.parent() {
                Some(p) => dir = p.to_path_buf(),
                None => break,
            }
        }
    }

    PathBuf::from("examples/plugins")
}

fn main() {
    match run() {
        Ok(()) => {}
        Err(message) => {
            eprintln!("error: {}", message);
            std::process::exit(1_i32);
        }
    }
}

fn run() -> Result<(), String> {
    let plugin_path: PathBuf = resolve_plugin_path();
    eprintln!("plugin directory: {}", plugin_path.display());

    // Build runtime with all loaders
    // Leak to get &'static reference required by generated callers
    let runtime: &'static Runtime = Box::leak(Box::new(
        Runtime::builder()
            .loader(NativeLoader::new(NativeConfig {}))
            .loader(DotnetLoader::new(DotnetConfig::default()))
            .loader(PythonLoader::new(PythonConfig::default()))
            .loader(LuaLoader::new(LuaConfig::default()))
            .loader(JsLoader::new(JsConfig {}))
            .loader(JsDenoLoader::new(JsDenoConfig::default()))
            .build()
            .map_err(|e| format!("runtime build failed: {}", e))?,
    ));

    // Scan for plugins
    let bundles: Vec<(PathBuf, ManifestData)> = scanner::scan_dir(&plugin_path);

    if bundles.is_empty() {
        return Err(format!(
            "no plugins found in {}. Run examples/build_all.sh first.",
            plugin_path.display()
        ));
    }

    eprintln!("discovered {} bundles", bundles.len());

    // Load all discovered bundles
    for (bundle_path, manifest) in &bundles {
        runtime
            .load_bundle(Path::new(bundle_path))
            .map_err(|e| format!("failed to load {}: {}", manifest.bundle_name, e))?;
        eprintln!("  loaded: {}", manifest.bundle_name);
    }

    // Process each bundle using generated contract callers
    println!("\n=== polyplug rust host example ===");

    for (_bundle_path, manifest) in &bundles {
        let bid: u64 = polyplug::abi::bundle_id(&manifest.bundle_name);
        let label: String = format!("[{}]", manifest.bundle_name);

        // Determine which contract this bundle implements
        if manifest
            .provides
            .iter()
            .any(|c| c.starts_with("pipeline.Decoder"))
        {
            let handle = runtime
                .find_by_bundle(bid, PIPELINE_DECODER_CONTRACT_ID, 0_u32)
                .map_err(|e| format!("find_by_bundle({}): {}", manifest.bundle_name, e))?;

            if !handle.is_null() {
                let decoder = PipelineDecoderContract::new(handle, runtime);
                let input: StringView = StringView::from_static(b"name,value,42");
                match decoder.decode(input) {
                    Ok(result) => {
                        let result_str: &str = string_view_to_str(&result);
                        println!(
                            "{:<30} decode(\"name,value,42\") = \"{}\"",
                            label, result_str
                        );
                    }
                    Err(e) => println!("{:<30} decode failed: code={}", label, e.code),
                }
            }
        } else if manifest
            .provides
            .iter()
            .any(|c| c.starts_with("data.Transformer"))
        {
            let handle = runtime
                .find_by_bundle(bid, DATA_TRANSFORMER_CONTRACT_ID, 0_u32)
                .map_err(|e| format!("find_by_bundle({}): {}", manifest.bundle_name, e))?;

            if !handle.is_null() {
                let transformer = DataTransformerContract::new(handle, runtime);
                let input: StringView = StringView::from_static(b"DECODED:name|value|42");
                match transformer.transform(input) {
                    Ok(result) => {
                        let result_str: &str = string_view_to_str(&result);
                        println!(
                            "{:<30} transform(\"DECODED:name|value|42\") = \"{}\"",
                            label, result_str
                        );
                    }
                    Err(e) => println!("{:<30} transform failed: code={}", label, e.code),
                }
            }
        } else if manifest
            .provides
            .iter()
            .any(|c| c.starts_with("pipeline.Encoder"))
        {
            let handle = runtime
                .find_by_bundle(bid, PIPELINE_ENCODER_CONTRACT_ID, 0_u32)
                .map_err(|e| format!("find_by_bundle({}): {}", manifest.bundle_name, e))?;

            if !handle.is_null() {
                let encoder = PipelineEncoderContract::new(handle, runtime);
                let input: StringView =
                    StringView::from_static(b"TRANSFORMED:NAME|value (transformed)|43");
                match encoder.encode(input) {
                    Ok(result) => {
                        let result_str: &str = string_view_to_str(&result);
                        println!(
                            "{:<30} encode(\"TRANSFORMED:NAME|value (transformed)|43\") = \"{}\"",
                            label, result_str
                        );
                    }
                    Err(e) => println!("{:<30} encode failed: code={}", label, e.code),
                }
            }
        } else if manifest
            .provides
            .iter()
            .any(|c| c.starts_with("data.Reporter"))
        {
            let handle = runtime
                .find_by_bundle(bid, DATA_REPORTER_CONTRACT_ID, 0_u32)
                .map_err(|e| format!("find_by_bundle({}): {}", manifest.bundle_name, e))?;

            if !handle.is_null() {
                let reporter = DataReporterContract::new(handle, runtime);
                let input: StringView =
                    StringView::from_static(b"TRANSFORMED:NAME|value (transformed)|43");
                match reporter.report(input) {
                    Ok(result) => {
                        let result_str: &str = string_view_to_str(&result);
                        println!(
                            "{:<30} report(\"TRANSFORMED:NAME|value (transformed)|43\") = \"{}\"",
                            label, result_str
                        );
                    }
                    Err(e) => println!("{:<30} report failed: code={}", label, e.code),
                }
            }
        } else if manifest
            .provides
            .iter()
            .any(|c| c.starts_with("pipeline.Validator"))
        {
            let handle = runtime
                .find_by_bundle(bid, PIPELINE_VALIDATOR_CONTRACT_ID, 0_u32)
                .map_err(|e| format!("find_by_bundle({}): {}", manifest.bundle_name, e))?;

            if !handle.is_null() {
                let validator = PipelineValidatorContract::new(handle, runtime);
                let input: StringView = StringView::from_static(b"DECODED:name|value|42");
                match validator.validate(input) {
                    Ok(result) => {
                        let result_str: &str = string_view_to_str(&result);
                        println!(
                            "{:<30} validate(\"DECODED:name|value|42\") = \"{}\"",
                            label, result_str
                        );
                    }
                    Err(e) => println!("{:<30} validate failed: code={}", label, e.code),
                }
            }
        }
    }

    println!("\nrust pipeline complete");
    Ok(())
}

use polyplug::ReloadPhase;
use polyplug::loader::manifest::ManifestData;
use polyplug::loader::scanner;
use polyplug::runtime::Runtime;
use polyplug_abi::PluginHandle;
use polyplug_abi::StringView;
use polyplug_native::{NativeConfig, NativeLoader};
use std::env;
use std::path::PathBuf;

mod generated;

use generated::host::host_callers::*;
use generated::host::types::*;

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let plugin_path = env::var("POLYPLUG_PLUGIN_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("examples/plugins"));

    eprintln!("loading plugins from: {}", plugin_path.display());

    let runtime = Box::leak(Box::new(
        Runtime::builder()
            .loader(NativeLoader::new(NativeConfig {}))
            .on_reload(|phase: ReloadPhase| match phase {
                ReloadPhase::Preparing {
                    bundle_name,
                    retry_count,
                    ..
                } => {
                    eprintln!(
                        "[HOT-RELOAD] Preparing: {} (retry {})",
                        bundle_name, retry_count
                    );
                }
                ReloadPhase::Reloaded { bundle_name, .. } => {
                    eprintln!("[HOT-RELOAD] Reloaded: {}", bundle_name);
                }
                ReloadPhase::Failed {
                    bundle_name,
                    reason,
                    ..
                } => {
                    eprintln!("[HOT-RELOAD] Failed: {} - {}", bundle_name, reason);
                }
            })
            .build()
            .map_err(|e| e.to_string())?,
    ));

    let bundles: Vec<(PathBuf, ManifestData)> = scanner::scan_dir(&plugin_path);
    if bundles.is_empty() {
        return Err("no plugins found".into());
    }

    eprintln!("discovered {} bundles", bundles.len());

    for (path, manifest) in &bundles {
        runtime
            .load_bundle(path)
            .map_err(|e| format!("load failed: {e}"))?;
        eprintln!("  loaded: {}", manifest.bundle_name);
    }

    println!("\n=== Pipeline Host (Rust) ===\n");

    let input: &str = "name,value,42";
    println!("Input: \"{input}\"\n");

    for (_, manifest) in &bundles {
        for contract in &manifest.provides {
            let contract_id = match contract.as_str() {
                "pipeline.Decoder@1.0" => PIPELINE_DECODER_CONTRACT_ID,
                "data.Transformer@1.0" => DATA_TRANSFORMER_CONTRACT_ID,
                "pipeline.Encoder@1.0" => PIPELINE_ENCODER_CONTRACT_ID,
                "data.Reporter@1.0" => DATA_REPORTER_CONTRACT_ID,
                "pipeline.Validator@1.0" => PIPELINE_VALIDATOR_CONTRACT_ID,
                _ => continue,
            };

            let handle: PluginHandle = runtime
                .find_by_contract(contract_id, 0)
                .map_err(|e| format!("find failed: {e}"))?;
            if handle.is_null() {
                continue;
            }

            match contract.as_str() {
                "pipeline.Decoder@1.0" => {
                    let decoder = PipelineDecoderContract::new(handle, runtime);
                    let result_sv = decoder
                        .decode(StringView {
                            ptr: input.as_ptr(),
                            len: input.len(),
                        })
                        .map_err(|e| format!("call failed: {}", e.code))?;
                    let result = unsafe {
                        std::str::from_utf8(std::slice::from_raw_parts(
                            result_sv.ptr,
                            result_sv.len,
                        ))
                    }
                    .map_err(|e| e.to_string())?;
                    println!(
                        "[{}] decode(\"{}\") = \"{}\"",
                        manifest.bundle_name, input, result
                    );
                }
                "data.Transformer@1.0" => {
                    let decoded = format!("DECODED:{}", input.replace(',', "|"));
                    let transformer = DataTransformerContract::new(handle, runtime);
                    let result_sv = transformer
                        .transform(StringView {
                            ptr: decoded.as_ptr(),
                            len: decoded.len(),
                        })
                        .map_err(|e| format!("call failed: {}", e.code))?;
                    let result = unsafe {
                        std::str::from_utf8(std::slice::from_raw_parts(
                            result_sv.ptr,
                            result_sv.len,
                        ))
                    }
                    .map_err(|e| e.to_string())?;
                    println!(
                        "[{}] transform(\"{}\") = \"{}\"",
                        manifest.bundle_name, decoded, result
                    );
                }
                "pipeline.Encoder@1.0" => {
                    let transformed = "TRANSFORMED:NAME|value (transformed)|43";
                    let encoder = PipelineEncoderContract::new(handle, runtime);
                    let result_sv = encoder
                        .encode(StringView {
                            ptr: transformed.as_ptr(),
                            len: transformed.len(),
                        })
                        .map_err(|e| format!("call failed: {}", e.code))?;
                    let result = unsafe {
                        std::str::from_utf8(std::slice::from_raw_parts(
                            result_sv.ptr,
                            result_sv.len,
                        ))
                    }
                    .map_err(|e| e.to_string())?;
                    println!(
                        "[{}] encode(\"{}\") = \"{}\"",
                        manifest.bundle_name, transformed, result
                    );
                }
                "data.Reporter@1.0" => {
                    let transformed = "TRANSFORMED:NAME|value (transformed)|43";
                    let reporter = DataReporterContract::new(handle, runtime);
                    let result_sv = reporter
                        .report(StringView {
                            ptr: transformed.as_ptr(),
                            len: transformed.len(),
                        })
                        .map_err(|e| format!("call failed: {}", e.code))?;
                    let result = unsafe {
                        std::str::from_utf8(std::slice::from_raw_parts(
                            result_sv.ptr,
                            result_sv.len,
                        ))
                    }
                    .map_err(|e| e.to_string())?;
                    println!(
                        "[{}] report(\"{}\") = \"{}\"",
                        manifest.bundle_name, transformed, result
                    );
                }
                "pipeline.Validator@1.0" => {
                    let decoded = format!("DECODED:{}", input.replace(',', "|"));
                    let validator = PipelineValidatorContract::new(handle, runtime);
                    let result_sv = validator
                        .validate(StringView {
                            ptr: decoded.as_ptr(),
                            len: decoded.len(),
                        })
                        .map_err(|e| format!("call failed: {}", e.code))?;
                    let result = unsafe {
                        std::str::from_utf8(std::slice::from_raw_parts(
                            result_sv.ptr,
                            result_sv.len,
                        ))
                    }
                    .map_err(|e| e.to_string())?;
                    println!(
                        "[{}] validate(\"{}\") = \"{}\"",
                        manifest.bundle_name, decoded, result
                    );
                }
                _ => {}
            }
        }
    }

    println!("\ndone.");
    Ok(())
}

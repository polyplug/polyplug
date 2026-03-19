use polyplug::loader::manifest::ManifestData;
use polyplug::loader::scanner;
use polyplug::runtime::Runtime;
use polyplug::runtime::RuntimeConfig;
use polyplug::ReloadPhase;
use polyplug_abi::PluginHandle;
use polyplug_abi::StringView;
use polyplug_native::{NativeConfig, NativeLoader};
use std::any::Any;
use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::sync::Mutex;
use std::time::Duration;

mod generated;

use generated::host::host_callers::*;
use generated::host::types::*;

/// Instance tracking for hot-reload: bundle_id -> list of contract instances.
/// Instances are cleared in Preparing phase and re-created in Reloaded phase.
static INSTANCES: LazyLock<Mutex<HashMap<u64, Vec<Box<dyn Any + Send>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

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

    let config: RuntimeConfig = RuntimeConfig {
        hot_reload_max_retries: 5,
        hot_reload_retry_interval: Duration::from_millis(200),
        hot_reload_abort_on_max_retries: false,
    };

    let runtime = Box::leak(Box::new(
        Runtime::builder()
            .loader(NativeLoader::new(NativeConfig {}))
            .config(config)
            .on_reload(|phase: ReloadPhase| match phase {
                ReloadPhase::Preparing {
                    bundle_id,
                    bundle_name,
                    retry_count,
                } => {
                    eprintln!(
                        "[HOT-RELOAD] Preparing: {} (bundle_id=0x{:016X}, retry {})",
                        bundle_name, bundle_id, retry_count
                    );
                    let mut instances = INSTANCES.lock().unwrap();
                    if instances.remove(&bundle_id).is_some() {
                        eprintln!("[HOT-RELOAD] Cleared instances for bundle {}", bundle_name);
                    }
                }
                ReloadPhase::Reloaded {
                    bundle_id,
                    bundle_name,
                } => {
                    eprintln!(
                        "[HOT-RELOAD] Reloaded: {} (bundle_id=0x{:016X})",
                        bundle_name, bundle_id
                    );
                }
                ReloadPhase::Failed {
                    bundle_id,
                    bundle_name,
                    reason,
                } => {
                    eprintln!(
                        "[HOT-RELOAD] Failed: {} (bundle_id=0x{:016X}) - {}",
                        bundle_name, bundle_id, reason
                    );
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
            let contract_id: u64 = match contract.as_str() {
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
                    let decoder: PipelineDecoderContract =
                        match PipelineDecoderContract::new(handle, runtime) {
                            Some(d) => d,
                            None => {
                                eprintln!(
                                    "[{}] failed to create decoder instance",
                                    manifest.bundle_name
                                );
                                continue;
                            }
                        };

                    if !decoder.is_valid() {
                        eprintln!("[{}] decoder instance is invalid", manifest.bundle_name);
                        continue;
                    }

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
                    let transformer: DataTransformerContract =
                        match DataTransformerContract::new(handle, runtime) {
                            Some(t) => t,
                            None => {
                                eprintln!(
                                    "[{}] failed to create transformer instance",
                                    manifest.bundle_name
                                );
                                continue;
                            }
                        };

                    if !transformer.is_valid() {
                        eprintln!("[{}] transformer instance is invalid", manifest.bundle_name);
                        continue;
                    }

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
                    let encoder: PipelineEncoderContract =
                        match PipelineEncoderContract::new(handle, runtime) {
                            Some(e) => e,
                            None => {
                                eprintln!(
                                    "[{}] failed to create encoder instance",
                                    manifest.bundle_name
                                );
                                continue;
                            }
                        };

                    if !encoder.is_valid() {
                        eprintln!("[{}] encoder instance is invalid", manifest.bundle_name);
                        continue;
                    }

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
                    let reporter: DataReporterContract =
                        match DataReporterContract::new(handle, runtime) {
                            Some(r) => r,
                            None => {
                                eprintln!(
                                    "[{}] failed to create reporter instance",
                                    manifest.bundle_name
                                );
                                continue;
                            }
                        };

                    if !reporter.is_valid() {
                        eprintln!("[{}] reporter instance is invalid", manifest.bundle_name);
                        continue;
                    }

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
                    let validator: PipelineValidatorContract =
                        match PipelineValidatorContract::new(handle, runtime) {
                            Some(v) => v,
                            None => {
                                eprintln!(
                                    "[{}] failed to create validator instance",
                                    manifest.bundle_name
                                );
                                continue;
                            }
                        };

                    if !validator.is_valid() {
                        eprintln!("[{}] validator instance is invalid", manifest.bundle_name);
                        continue;
                    }

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

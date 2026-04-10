use polyplug::ReloadPhase;
use polyplug::loader::scanner;
use polyplug::runtime::Runtime;
use polyplug::RuntimeConfig;
use polyplug_abi::GuestContractHandle;
use polyplug_abi::StringView;
use polyplug_abi::HostContractInterface;
use polyplug_js::{JsConfig, JsLoader};
use polyplug_lua::{LuaConfig, LuaLoader};
use polyplug_native::{NativeConfig, NativeLoader};
use polyplug_python::{PythonConfig, PythonLoader};
use std::env;
use std::path::PathBuf;
use std::time::Duration;

mod generated;

use generated::host::host_callers::*;
use generated::host::host_contracts::{HOSTLOGGER_CONTRACT_ID, HostLogger};
use generated::host::types::*;
use generated::host::interface_factories::create_host_logger_interface;

struct ConsoleLogger;

impl HostLogger for ConsoleLogger {
    fn log(&self, message: &str) {
        println!("[plugin] {}", message);
    }

    fn log_with_level(&self, level: &LogLevel, message: &str) {
        let level_str: &str = match level {
            LogLevel::Debug => "DEBUG",
            LogLevel::Info => "INFO",
            LogLevel::Warn => "WARN",
            LogLevel::Error => "ERROR",
        };
        println!("[plugin][{}] {}", level_str, message);
    }
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let plugin_path: PathBuf = env::var("POLYPLUG_PLUGIN_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("examples/plugins"));

    eprintln!("loading plugins from: {}", plugin_path.display());

    let config = RuntimeConfig {
        hot_reload_enabled: true,
        hot_reload_max_retries: 5,
        hot_reload_retry_interval_ms: 200,
        hot_reload_abort_on_max_retries: false,
        compatibility: polyplug_abi::Compatibility::Strict,
    };

    let runtime: &'static Runtime = Box::leak(Box::new(
        Runtime::builder()
            .loader(NativeLoader::new(NativeConfig {}))
            .loader(JsLoader::new(JsConfig {}))
            .loader(LuaLoader::new(LuaConfig::default()))
            .loader(PythonLoader::new(PythonConfig::default()))
            .config(config)
            .on_reload(|phase: ReloadPhase| match phase {
                ReloadPhase::Preparing {
                    bundle_id,
                    bundle_name,
                    retry_count,
                } => {
                    eprintln!(
                        "[HOT-RELOAD] Preparing: {} (id=0x{:016X}, retry {})",
                        bundle_name, bundle_id, retry_count
                    );
                }
                ReloadPhase::Reloaded {
                    bundle_id,
                    bundle_name,
                } => {
                    eprintln!(
                        "[HOT-RELOAD] Reloaded: {} (id=0x{:016X})",
                        bundle_name, bundle_id
                    );
                }
                ReloadPhase::Failed {
                    bundle_id,
                    bundle_name,
                    reason,
                } => {
                    eprintln!(
                        "[HOT-RELOAD] Failed: {} (id=0x{:016X}) - {}",
                        bundle_name, bundle_id, reason
                    );
                }
            })
            .build()
            .map_err(|e| e.to_string())?,
    ));

    let vtable: &'static HostContractInterface =
        create_host_logger_interface(Box::new(ConsoleLogger));
    runtime
        .register_host_contract(HOSTLOGGER_CONTRACT_ID, vtable)
        .map_err(|e| format!("failed to register host.logger contract: {e}"))?;

    let bundles: Vec<(PathBuf, _)> = scanner::scan_dirs(&[plugin_path.clone()]);
    if bundles.is_empty() {
        return Err("no plugins found".into());
    }

    eprintln!("discovered {} bundles", bundles.len());

    for (path, _manifest) in &bundles {
        runtime
            .load_bundle(path)
            .map_err(|e| format!("load failed: {e}"))?;
    }

    println!("\n=== Pipeline Host (Rust) ===\n");

    let input: &str = "name,value,42";
    println!("Input: \"{input}\"\n");

    if let Some(decoder) =
        find_contract::<PipelineDecoderContract>(runtime, PIPELINE_DECODER_CONTRACT_ID)
    {
        let result_sv: StringView = decoder
            .decode(StringView {
                ptr: input.as_ptr(),
                len: input.len(),
            })
            .map_err(|e| format!("decode failed: {}", e.code))?;
        let result: &str = unsafe {
            std::str::from_utf8(std::slice::from_raw_parts(result_sv.ptr, result_sv.len))
        }
        .map_err(|e| e.to_string())?;
        println!("[decoder] decode(\"{}\") = \"{}\"", input, result);
    }

    let decoded: String = format!("DECODED:{}", input.replace(',', "|"));
    if let Some(transformer) =
        find_contract::<DataTransformerContract>(runtime, DATA_TRANSFORMER_CONTRACT_ID)
    {
        let result_sv: StringView = transformer
            .transform(StringView {
                ptr: decoded.as_ptr(),
                len: decoded.len(),
            })
            .map_err(|e| format!("transform failed: {}", e.code))?;
        let result: &str = unsafe {
            std::str::from_utf8(std::slice::from_raw_parts(result_sv.ptr, result_sv.len))
        }
        .map_err(|e| e.to_string())?;
        println!("[transformer] transform(\"{}\") = \"{}\"", decoded, result);
    }

    let transformed: &str = "TRANSFORMED:NAME|value (transformed)|43";
    if let Some(encoder) =
        find_contract::<PipelineEncoderContract>(runtime, PIPELINE_ENCODER_CONTRACT_ID)
    {
        let result_sv: StringView = encoder
            .encode(StringView {
                ptr: transformed.as_ptr(),
                len: transformed.len(),
            })
            .map_err(|e| format!("encode failed: {}", e.code))?;
        let result: &str = unsafe {
            std::str::from_utf8(std::slice::from_raw_parts(result_sv.ptr, result_sv.len))
        }
        .map_err(|e| e.to_string())?;
        println!("[encoder] encode(\"{}\") = \"{}\"", transformed, result);
    }

    if let Some(reporter) =
        find_contract::<DataReporterContract>(runtime, DATA_REPORTER_CONTRACT_ID)
    {
        let result_sv: StringView = reporter
            .report(StringView {
                ptr: transformed.as_ptr(),
                len: transformed.len(),
            })
            .map_err(|e| format!("report failed: {}", e.code))?;
        let result: &str = unsafe {
            std::str::from_utf8(std::slice::from_raw_parts(result_sv.ptr, result_sv.len))
        }
        .map_err(|e| e.to_string())?;
        println!("[reporter] report(\"{}\") = \"{}\"", transformed, result);
    }

    if let Some(validator) =
        find_contract::<PipelineValidatorContract>(runtime, PIPELINE_VALIDATOR_CONTRACT_ID)
    {
        let result_sv: StringView = validator
            .validate(StringView {
                ptr: decoded.as_ptr(),
                len: decoded.len(),
            })
            .map_err(|e| format!("validate failed: {}", e.code))?;
        let result: &str = unsafe {
            std::str::from_utf8(std::slice::from_raw_parts(result_sv.ptr, result_sv.len))
        }
        .map_err(|e| e.to_string())?;
        println!("[validator] validate(\"{}\") = \"{}\"", decoded, result);
    }

    println!("\ndone.");
    Ok(())
}

/// Helper: Find a plugin implementing a contract and create a caller instance.
/// Uses generated contract ID constants - no manifest parsing needed.
fn find_contract<T>(runtime: &'static Runtime, contract_id: u64) -> Option<T>
where
    T: ContractCaller,
{
    let handle: GuestContractHandle = runtime.find_guest_contract(contract_id, 0).ok()?;
    if handle.is_null() {
        return None;
    }
    T::from_handle(handle, runtime)
}

/// Trait for contract callers - allows generic find_contract helper.
trait ContractCaller: Sized {
    fn from_handle(handle: GuestContractHandle, runtime: &'static Runtime) -> Option<Self>;
}

impl ContractCaller for PipelineDecoderContract {
    fn from_handle(handle: GuestContractHandle, runtime: &'static Runtime) -> Option<Self> {
        Self::new(handle, runtime.as_context_ptr())
    }
}

impl ContractCaller for DataTransformerContract {
    fn from_handle(handle: GuestContractHandle, runtime: &'static Runtime) -> Option<Self> {
        Self::new(handle, runtime.as_context_ptr())
    }
}

impl ContractCaller for PipelineEncoderContract {
    fn from_handle(handle: GuestContractHandle, runtime: &'static Runtime) -> Option<Self> {
        Self::new(handle, runtime.as_context_ptr())
    }
}

impl ContractCaller for DataReporterContract {
    fn from_handle(handle: GuestContractHandle, runtime: &'static Runtime) -> Option<Self> {
        Self::new(handle, runtime.as_context_ptr())
    }
}

impl ContractCaller for PipelineValidatorContract {
    fn from_handle(handle: GuestContractHandle, runtime: &'static Runtime) -> Option<Self> {
        Self::new(handle, runtime.as_context_ptr())
    }
}

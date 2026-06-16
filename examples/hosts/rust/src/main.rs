use polyplug::loader::scanner;
use polyplug::runtime::Runtime;
use polyplug_abi::GuestContractHandle;
use polyplug_abi::HostContractInterface;
use polyplug_abi::StringView;
use polyplug_abi::runtime::ReloadPhase;
use polyplug_abi::runtime::ReloadPhaseType;
use polyplug_abi::runtime::RuntimeConfig;
use polyplug_dotnet::{DotnetConfig, DotnetLoader};
use polyplug_js::{JsConfig, JsLoader};
use polyplug_lua::{LuaConfig, LuaLoader};
use polyplug_native::{NativeConfig, NativeLoader};
use polyplug_python::{PythonConfig, PythonLoader};
use std::env;
use std::path::PathBuf;

#[path = "../generated/mod.rs"]
mod generated;

use generated::host::host_callers::*;
use generated::host::host_contracts::{HOSTLOGGER_CONTRACT_ID, HostLogger};
use generated::host::interface_factories::create_host_logger_interface;
use generated::host::types::*;

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
        compatibility: polyplug_abi::Compatibility::Strict,
        hot_reload_enabled: true,
        on_reload: None,
        on_reload_user_data: core::ptr::null_mut(),
        ..Default::default()
    };

    let runtime: &'static Runtime = Box::leak(Box::new(
        Runtime::builder()
            .loader(NativeLoader::new(NativeConfig {}))
            .loader(JsLoader::new(JsConfig {}))
            .loader(LuaLoader::new(LuaConfig::default()))
            .loader(PythonLoader::new(PythonConfig::default()))
            .loader(DotnetLoader::new(DotnetConfig::default()))
            .config(config)
            .on_reload(|_user_data: *mut core::ffi::c_void, phase: ReloadPhase| {
                match phase.phase_type {
                    ReloadPhaseType::Preparing => {
                        eprintln!("[HOT-RELOAD] Preparing: (id=0x{:016X})", phase.bundle_id);
                    }
                    ReloadPhaseType::Reloaded => {
                        eprintln!("[HOT-RELOAD] Reloaded: (id=0x{:016X})", phase.bundle_id);
                    }
                    ReloadPhaseType::Failed => {
                        eprintln!("[HOT-RELOAD] Failed: (id=0x{:016X})", phase.bundle_id);
                    }
                    ReloadPhaseType::Unloading => {
                        eprintln!("[HOT-RELOAD] Unloading: (id=0x{:016X})", phase.bundle_id);
                    }
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

    let scan: scanner::ScanResult = scanner::scan_dirs(core::slice::from_ref(&plugin_path));
    for diagnostic in &scan.diagnostics {
        eprintln!("warning: {diagnostic}");
    }
    let bundles: Vec<(PathBuf, _)> = scan.found;
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

    if let Some(mut decoder) =
        find_contract::<PipelineDecoderContract>(runtime, PIPELINE_DECODER_CONTRACT_ID)
    {
        let result_sv: StringView = decoder
            .decode(StringView {
                ptr: input.as_ptr(),
                len: input.len(),
            })
            .map_err(|e| format!("decode failed: {}", e.code))?;
        // SAFETY: result_sv was returned by the guest method on success; per the
        // ABI its ptr is non-null and valid for `result_sv.len` bytes of
        // host-allocated UTF-8 for the duration of this borrow.
        let result: &str = unsafe {
            core::str::from_utf8(core::slice::from_raw_parts(result_sv.ptr, result_sv.len))
        }
        .map_err(|e| e.to_string())?;
        println!("[decoder] decode(\"{}\") = \"{}\"", input, result);
    }

    let decoded: String = format!("DECODED:{}", input.replace(',', "|"));
    if let Some(mut transformer) =
        find_contract::<DataTransformerContract>(runtime, DATA_TRANSFORMER_CONTRACT_ID)
    {
        let result_sv: StringView = transformer
            .transform(StringView {
                ptr: decoded.as_ptr(),
                len: decoded.len(),
            })
            .map_err(|e| format!("transform failed: {}", e.code))?;
        // SAFETY: result_sv was returned by the guest method on success; per the
        // ABI its ptr is non-null and valid for `result_sv.len` bytes of
        // host-allocated UTF-8 for the duration of this borrow.
        let result: &str = unsafe {
            core::str::from_utf8(core::slice::from_raw_parts(result_sv.ptr, result_sv.len))
        }
        .map_err(|e| e.to_string())?;
        println!("[transformer] transform(\"{}\") = \"{}\"", decoded, result);
    }

    let transformed: &str = "TRANSFORMED:NAME|value (transformed)|43";
    if let Some(mut encoder) =
        find_contract::<PipelineEncoderContract>(runtime, PIPELINE_ENCODER_CONTRACT_ID)
    {
        let result_sv: StringView = encoder
            .encode(StringView {
                ptr: transformed.as_ptr(),
                len: transformed.len(),
            })
            .map_err(|e| format!("encode failed: {}", e.code))?;
        // SAFETY: result_sv was returned by the guest method on success; per the
        // ABI its ptr is non-null and valid for `result_sv.len` bytes of
        // host-allocated UTF-8 for the duration of this borrow.
        let result: &str = unsafe {
            core::str::from_utf8(core::slice::from_raw_parts(result_sv.ptr, result_sv.len))
        }
        .map_err(|e| e.to_string())?;
        println!("[encoder] encode(\"{}\") = \"{}\"", transformed, result);
    }

    if let Some(mut reporter) =
        find_contract::<DataReporterContract>(runtime, DATA_REPORTER_CONTRACT_ID)
    {
        let result_sv: StringView = reporter
            .report(StringView {
                ptr: transformed.as_ptr(),
                len: transformed.len(),
            })
            .map_err(|e| format!("report failed: {}", e.code))?;
        // SAFETY: result_sv was returned by the guest method on success; per the
        // ABI its ptr is non-null and valid for `result_sv.len` bytes of
        // host-allocated UTF-8 for the duration of this borrow.
        let result: &str = unsafe {
            core::str::from_utf8(core::slice::from_raw_parts(result_sv.ptr, result_sv.len))
        }
        .map_err(|e| e.to_string())?;
        println!("[reporter] report(\"{}\") = \"{}\"", transformed, result);
    }

    if let Some(mut validator) =
        find_contract::<PipelineValidatorContract>(runtime, PIPELINE_VALIDATOR_CONTRACT_ID)
    {
        let result_sv: StringView = validator
            .validate(StringView {
                ptr: decoded.as_ptr(),
                len: decoded.len(),
            })
            .map_err(|e| format!("validate failed: {}", e.code))?;
        // SAFETY: result_sv was returned by the guest method on success; per the
        // ABI its ptr is non-null and valid for `result_sv.len` bytes of
        // host-allocated UTF-8 for the duration of this borrow.
        let result: &str = unsafe {
            core::str::from_utf8(core::slice::from_raw_parts(result_sv.ptr, result_sv.len))
        }
        .map_err(|e| e.to_string())?;
        println!("[validator] validate(\"{}\") = \"{}\"", decoded, result);
    }

    // Round-trip micro-benchmark (opt-in via POLYPLUG_BENCH_ITERS): times the full
    // host → runtime → native guest → return path (Rust host calling the native
    // decoder plugin and reading a StringView back). Point POLYPLUG_PLUGIN_PATH at
    // native guests only so the resolved decoder is the native cdylib.
    if let Ok(iters_str) = env::var("POLYPLUG_BENCH_ITERS")
        && let Ok(iters) = iters_str.parse::<u64>()
        && let Some(mut bench_decoder) =
            find_contract::<PipelineDecoderContract>(runtime, PIPELINE_DECODER_CONTRACT_ID)
    {
        let sv: StringView = StringView {
            ptr: input.as_ptr(),
            len: input.len(),
        };
        let warmup: u64 = iters.min(10_000);
        for _ in 0..warmup {
            let _ = bench_decoder.decode(sv);
        }
        let start: std::time::Instant = std::time::Instant::now();
        for _ in 0..iters {
            let _ = bench_decoder.decode(sv);
        }
        let elapsed_ns: u128 = start.elapsed().as_nanos();
        println!(
            "ROUNDTRIP_NS={:.2} LANG=rust",
            elapsed_ns as f64 / iters as f64
        );
    }

    // Host-call micro-benchmark (opt-in via POLYPLUG_BENCH_ITERS): times the BARE
    // host → runtime call — one find_guest_contract lookup per iteration, no guest
    // dispatch. A Rust host links the crate directly (no FFI), so this bar is the
    // runtime's registry lookup itself. Every returned handle is null-checked and
    // the hit count is fed through std::hint::black_box so the loop cannot be
    // dead-code eliminated.
    if let Ok(iters_str) = env::var("POLYPLUG_BENCH_ITERS")
        && let Ok(iters) = iters_str.parse::<u64>()
        && iters > 0
    {
        let warmup: u64 = iters.min(10_000);
        let mut hits: u64 = 0;
        for _ in 0..warmup {
            if let Ok(handle) = runtime.find_guest_contract(PIPELINE_DECODER_CONTRACT_ID, 0)
                && !handle.is_null()
            {
                hits = hits.wrapping_add(1);
            }
        }
        hits = 0;
        let start: std::time::Instant = std::time::Instant::now();
        for _ in 0..iters {
            if let Ok(handle) = runtime.find_guest_contract(PIPELINE_DECODER_CONTRACT_ID, 0)
                && !handle.is_null()
            {
                hits = hits.wrapping_add(1);
            }
        }
        let elapsed_ns: u128 = start.elapsed().as_nanos();
        if core::hint::black_box(hits) == iters {
            println!(
                "HOSTCALL_NS={:.2} LANG=rust",
                elapsed_ns as f64 / iters as f64
            );
        } else {
            eprintln!("HOSTCALL bench: lookup missed ({hits}/{iters} hits) — no result printed");
        }
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

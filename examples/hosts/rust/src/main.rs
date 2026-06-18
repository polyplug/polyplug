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
use std::sync::Arc;

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

    // Hold the runtime in an `Arc` — `build()` already returns one, and its target
    // has a stable address for the Arc's lifetime, so every generated caller's cached
    // `*const HostApi` stays valid. The callers live in inner scopes below and drop
    // before this `Arc` does at the end of `run`, so the runtime outlives them.
    let runtime: Arc<Runtime> = Runtime::builder()
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
        .map_err(|e| e.to_string())?;

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
        find_contract::<PipelineDecoderContract>(&runtime, PIPELINE_DECODER_CONTRACT_ID)
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
        find_contract::<DataTransformerContract>(&runtime, DATA_TRANSFORMER_CONTRACT_ID)
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
        find_contract::<PipelineEncoderContract>(&runtime, PIPELINE_ENCODER_CONTRACT_ID)
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
        find_contract::<DataReporterContract>(&runtime, DATA_REPORTER_CONTRACT_ID)
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
        find_contract::<PipelineValidatorContract>(&runtime, PIPELINE_VALIDATOR_CONTRACT_ID)
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
            find_contract::<PipelineDecoderContract>(&runtime, PIPELINE_DECODER_CONTRACT_ID)
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

    // Baseline anchor (opt-in: POLYPLUG_BENCH_ITERS + POLYPLUG_BENCH_DECODE_SO): the
    // SAME decode work the ROUNDTRIP cell above measures, reached WITHOUT polyplug —
    // directly in-process, and via a raw `dlsym` of the guest's `polyplug_bench_decode`
    // export. Measured the exact same way (warm loop over `iters`), so the matrix's
    // native cell minus these is polyplug's overhead over hand-rolled FFI for real
    // string-returning work. Only the native (rust/cpp) hosts emit these.
    if let Ok(iters_str) = env::var("POLYPLUG_BENCH_ITERS")
        && let Ok(iters) = iters_str.parse::<u64>()
        && iters > 0
        && let Ok(so_path) = env::var("POLYPLUG_BENCH_DECODE_SO")
    {
        run_decode_baselines(&so_path, input, iters);
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

/// The decode transformation, host-side — byte-identical to the guest's
/// `decode_body` (`examples/guests/rust/decoder`). Used by the **direct** baseline
/// arm: the same work the plugin does, with NO plugin boundary at all (the floor).
#[inline]
fn decode_reference(input: &str) -> String {
    format!("DECODED:{}", input.replace(',', "|"))
}

/// Measure the two no-polyplug baselines for the decode round trip, the same way
/// the `ROUNDTRIP_NS` cell is measured (warm loop over `iters`), and print
/// `BASELINE_DIRECT_NS` / `BASELINE_FFI_NS`:
///   - **direct**  — `decode_reference` in-process: no plugin, no FFI (the floor).
///   - **raw FFI** — `dlsym` the guest cdylib's `polyplug_bench_decode` (the SAME
///     compiled body the contract runs) and call it the way a plugin author would
///     by hand: call, read the result, free it. No registry, no instance, no safety.
///
/// Both arms allocate and release the result each call, so they differ from each
/// other only in the calling mechanism.
fn run_decode_baselines(so_path: &str, input: &str, iters: u64) {
    let warmup: u64 = iters.min(10_000);

    // ── direct: the decode body, no boundary ──────────────────────────────────
    let mut acc: usize = 0;
    for _ in 0..warmup {
        acc = acc.wrapping_add(decode_reference(core::hint::black_box(input)).len());
    }
    let start: std::time::Instant = std::time::Instant::now();
    for _ in 0..iters {
        acc = acc.wrapping_add(decode_reference(core::hint::black_box(input)).len());
    }
    let direct_ns: f64 = start.elapsed().as_nanos() as f64 / iters as f64;
    core::hint::black_box(acc);
    println!("BASELINE_DIRECT_NS={direct_ns:.2} LANG=rust");

    // ── raw FFI: dlsym the guest's own polyplug_bench_decode ──────────────────
    // SAFETY: so_path is a cdylib built by the example fixtures; the two symbols
    // below are exported by the decoder guest with the signatures transmuted to.
    let library: libloading::Library = match unsafe { libloading::Library::new(so_path) } {
        Ok(lib) => lib,
        Err(e) => {
            eprintln!("baseline: cannot load {so_path}: {e}");
            return;
        }
    };
    // SAFETY: the decoder cdylib exports these exact symbols and signatures.
    let decode: libloading::Symbol<
        '_,
        unsafe extern "C" fn(*const u8, usize, *mut *mut u8, *mut usize),
    > = match unsafe { library.get(b"polyplug_bench_decode\0") } {
        Ok(symbol) => symbol,
        Err(e) => {
            eprintln!("baseline: no polyplug_bench_decode in {so_path}: {e}");
            return;
        }
    };
    // SAFETY: matching free export for the buffers `decode` returns.
    let free: libloading::Symbol<'_, unsafe extern "C" fn(*mut u8, usize)> =
        match unsafe { library.get(b"polyplug_bench_decode_free\0") } {
            Ok(symbol) => symbol,
            Err(e) => {
                eprintln!("baseline: no polyplug_bench_decode_free in {so_path}: {e}");
                return;
            }
        };

    let mut acc2: usize = 0;
    let one_call = |acc: &mut usize| {
        let mut out_ptr: *mut u8 = core::ptr::null_mut();
        let mut out_len: usize = 0;
        // SAFETY: input is a valid byte range; out_ptr/out_len are valid out-params;
        // the returned buffer is read then freed via the matching free symbol.
        unsafe {
            decode(input.as_ptr(), input.len(), &mut out_ptr, &mut out_len);
            if !out_ptr.is_null() {
                *acc = acc.wrapping_add(out_len);
                free(out_ptr, out_len);
            }
        }
    };
    for _ in 0..warmup {
        one_call(&mut acc2);
    }
    let start_ffi: std::time::Instant = std::time::Instant::now();
    for _ in 0..iters {
        one_call(&mut acc2);
    }
    let ffi_ns: f64 = start_ffi.elapsed().as_nanos() as f64 / iters as f64;
    core::hint::black_box(acc2);
    println!("BASELINE_FFI_NS={ffi_ns:.2} LANG=rust");
}

/// Helper: Find a plugin implementing a contract and create a caller instance.
/// Uses generated contract ID constants - no manifest parsing needed.
fn find_contract<T>(runtime: &Runtime, contract_id: u64) -> Option<T>
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
    fn from_handle(handle: GuestContractHandle, runtime: &Runtime) -> Option<Self>;
}

impl ContractCaller for PipelineDecoderContract {
    fn from_handle(handle: GuestContractHandle, runtime: &Runtime) -> Option<Self> {
        Self::new(handle, runtime.as_context_ptr())
    }
}

impl ContractCaller for DataTransformerContract {
    fn from_handle(handle: GuestContractHandle, runtime: &Runtime) -> Option<Self> {
        Self::new(handle, runtime.as_context_ptr())
    }
}

impl ContractCaller for PipelineEncoderContract {
    fn from_handle(handle: GuestContractHandle, runtime: &Runtime) -> Option<Self> {
        Self::new(handle, runtime.as_context_ptr())
    }
}

impl ContractCaller for DataReporterContract {
    fn from_handle(handle: GuestContractHandle, runtime: &Runtime) -> Option<Self> {
        Self::new(handle, runtime.as_context_ptr())
    }
}

impl ContractCaller for PipelineValidatorContract {
    fn from_handle(handle: GuestContractHandle, runtime: &Runtime) -> Option<Self> {
        Self::new(handle, runtime.as_context_ptr())
    }
}

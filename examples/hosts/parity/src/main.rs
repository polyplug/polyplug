use polyplug::runtime::Runtime;
use polyplug_abi::GuestContractHandle;
use polyplug_abi::HostContractInterface;
use polyplug_abi::StringView;
use polyplug_abi::runtime::RuntimeConfig;
use polyplug_dotnet::{DotnetConfig, DotnetLoader, HostfxrLocation};
use polyplug_js::{JsConfig, JsLoader};
use polyplug_lua::{LuaConfig, LuaLoader};
use polyplug_native::{NativeConfig, NativeLoader};
use polyplug_python::{PythonConfig, PythonLoader};
use std::collections::BTreeMap;
use std::env;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[path = "../generated/mod.rs"]
mod generated;

use generated::host::host_callers::*;
use generated::host::host_contracts::{HOSTLOGGER_CONTRACT_ID, HostLogger};
use generated::host::interface_factories::create_host_logger_interface;
use generated::host::types::*;

/// Languages whose generators are compared. `cpp` is, like `rust`, a native
/// cdylib loaded by the NativeLoader; `csharp` bundles live under a separate
/// parent directory (see `bundle_dir`).
///
/// `csharp` is processed FIRST on purpose: both CPython and the CLR are
/// single-init-per-process. Initializing the CLR after CPython has already
/// initialized triggers a `free(): invalid size` abort inside libcoreclr's
/// allocator on this platform. Initializing the CLR first is stable, so the
/// .NET pass leads and the Python pass follows.
const LANGUAGES: [&str; 6] = ["csharp", "rust", "cpp", "python", "lua", "js"];

/// Contracts under test, in pipeline order.
const CONTRACTS: [&str; 5] = ["decoder", "transformer", "encoder", "reporter", "validator"];

/// Console logger so the reporter contract's host callbacks have a sink. Output
/// is intentionally discarded to keep the parity matrix clean; the call path
/// itself is what we exercise.
struct ConsoleLogger;

impl HostLogger for ConsoleLogger {
    fn log(&self, _message: &str) {}

    fn log_with_level(&self, _level: &LogLevel, _message: &str) {}
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

/// Per-contract fixed input + golden output that EVERY language must reproduce
/// byte-for-byte. Sourced from examples/hosts/golden.txt.
fn golden(contract: &str) -> (&'static str, &'static str) {
    match contract {
        "decoder" => ("name,value,42", "DECODED:name|value|42"),
        "transformer" => (
            "DECODED:name|value|42",
            "TRANSFORMED:NAME|value (transformed)|43",
        ),
        "encoder" => (
            "TRANSFORMED:NAME|value (transformed)|43",
            "NAME,value (transformed),43",
        ),
        "reporter" => (
            "TRANSFORMED:NAME|value (transformed)|43",
            "Report: NAME has value 'value (transformed)' with count 43",
        ),
        "validator" => ("DECODED:name|value|42", "VALID:name|value|42"),
        other => panic!("unknown contract: {other}"),
    }
}

/// Resolve the on-disk bundle directory for a (language, contract) pair. C#
/// bundles live under `plugins-csharp/`; every other language under `plugins/`.
fn bundle_dir(workspace: &Path, language: &str, contract: &str) -> PathBuf {
    if language == "csharp" {
        workspace
            .join("examples/plugins-csharp")
            .join(format!("csharp_{contract}"))
    } else {
        workspace
            .join("examples/plugins")
            .join(format!("{language}_{contract}"))
    }
}

/// Build a fresh runtime with all five loaders and the host.logger contract
/// registered. Each language gets its own runtime so duplicate providers never
/// collide and the single-init Python/CLR interpreters stay clean.
fn build_runtime() -> Result<Arc<Runtime>, String> {
    let config = RuntimeConfig {
        compatibility: polyplug_abi::Compatibility::Strict,
        hot_reload_enabled: false,
        on_reload: None,
        on_reload_user_data: core::ptr::null_mut(),
        ..Default::default()
    };

    // `build()` already returns an `Arc<Runtime>` whose target has a stable address
    // for the Arc's lifetime, so the generated callers' cached `*const HostApi` stays
    // valid without leaking. The Arc is returned to `run_language`, which drops it
    // after its callers, so the runtime outlives them.
    let runtime: Arc<Runtime> = Runtime::builder()
        .loader(NativeLoader::new(NativeConfig {}))
        .loader(JsLoader::new(JsConfig {}))
        .loader(LuaLoader::new(LuaConfig::default()))
        .loader(PythonLoader::new(PythonConfig::default()))
        .loader(DotnetLoader::new(DotnetConfig {
            min_framework: String::from("net10.0"),
            hostfxr: HostfxrLocation::Auto,
        }))
        .config(config)
        .build()
        .map_err(|e| e.to_string())?;

    let vtable: &'static HostContractInterface =
        create_host_logger_interface(Box::new(ConsoleLogger));
    runtime
        .register_host_contract(HOSTLOGGER_CONTRACT_ID, vtable)
        .map_err(|e| format!("failed to register host.logger contract: {e}"))?;

    Ok(runtime)
}

/// Decode a returned StringView into an owned String, copying immediately so the
/// caller never retains a borrow into host-allocator memory.
fn sv_to_owned(sv: StringView) -> Result<String, String> {
    // SAFETY: the contract caller returns a StringView whose ptr/len describe a
    // valid UTF-8 byte run owned by the host allocator for the duration of this
    // call. We copy it out immediately into an owned String and never retain the
    // borrow past this function.
    let bytes: &[u8] = unsafe { core::slice::from_raw_parts(sv.ptr, sv.len) };
    let text: &str = core::str::from_utf8(bytes).map_err(|e| e.to_string())?;
    Ok(text.to_owned())
}

/// Find a contract caller by ID in the runtime.
fn find_contract<T>(runtime: &Arc<Runtime>, contract_id: u64) -> Result<T, String>
where
    T: ContractCaller,
{
    let handle: GuestContractHandle = runtime
        .find_guest_contract(contract_id, 0)
        .map_err(|e| format!("find_guest_contract failed: {e}"))?;
    if handle.is_null() {
        return Err("contract handle is null".to_string());
    }
    T::from_handle(handle, Arc::clone(runtime))
        .ok_or_else(|| "failed to build contract caller".to_string())
}

/// Invoke every contract for one language and return its (contract -> output)
/// map. Loads all five bundle dirs into the language's runtime first.
fn run_language(workspace: &Path, language: &str) -> Result<BTreeMap<String, String>, String> {
    let runtime: Arc<Runtime> = build_runtime()?;

    for contract in CONTRACTS {
        let dir: PathBuf = bundle_dir(workspace, language, contract);
        if !dir.is_dir() {
            return Err(format!(
                "[{language}] bundle directory missing: {}",
                dir.display()
            ));
        }
        runtime
            .load_bundle(&dir)
            .map_err(|e| format!("[{language}] load_bundle({}) failed: {e}", dir.display()))?;
    }

    let mut results: BTreeMap<String, String> = BTreeMap::new();

    let (decoder_in, _): (&str, &str) = golden("decoder");
    let mut decoder: PipelineDecoderContract =
        find_contract::<PipelineDecoderContract>(&runtime, PIPELINE_DECODER_CONTRACT_ID)
            .map_err(|e| format!("[{language}] decoder: {e}"))?;
    let decoder_sv: StringView = decoder
        .decode(StringView {
            ptr: decoder_in.as_ptr(),
            len: decoder_in.len(),
        })
        .map_err(|e| format!("[{language}] decode failed: {}", e.code as u32))?;
    results.insert("decoder".to_string(), sv_to_owned(decoder_sv)?);

    let (transformer_in, _): (&str, &str) = golden("transformer");
    let mut transformer: DataTransformerContract =
        find_contract::<DataTransformerContract>(&runtime, DATA_TRANSFORMER_CONTRACT_ID)
            .map_err(|e| format!("[{language}] transformer: {e}"))?;
    let transformer_sv: StringView = transformer
        .transform(StringView {
            ptr: transformer_in.as_ptr(),
            len: transformer_in.len(),
        })
        .map_err(|e| format!("[{language}] transform failed: {}", e.code as u32))?;
    results.insert("transformer".to_string(), sv_to_owned(transformer_sv)?);

    let (encoder_in, _): (&str, &str) = golden("encoder");
    let mut encoder: PipelineEncoderContract =
        find_contract::<PipelineEncoderContract>(&runtime, PIPELINE_ENCODER_CONTRACT_ID)
            .map_err(|e| format!("[{language}] encoder: {e}"))?;
    let encoder_sv: StringView = encoder
        .encode(StringView {
            ptr: encoder_in.as_ptr(),
            len: encoder_in.len(),
        })
        .map_err(|e| format!("[{language}] encode failed: {}", e.code as u32))?;
    results.insert("encoder".to_string(), sv_to_owned(encoder_sv)?);

    let (reporter_in, _): (&str, &str) = golden("reporter");
    let mut reporter: DataReporterContract =
        find_contract::<DataReporterContract>(&runtime, DATA_REPORTER_CONTRACT_ID)
            .map_err(|e| format!("[{language}] reporter: {e}"))?;
    let reporter_sv: StringView = reporter
        .report(StringView {
            ptr: reporter_in.as_ptr(),
            len: reporter_in.len(),
        })
        .map_err(|e| format!("[{language}] report failed: {}", e.code as u32))?;
    results.insert("reporter".to_string(), sv_to_owned(reporter_sv)?);

    let (validator_in, _): (&str, &str) = golden("validator");
    let mut validator: PipelineValidatorContract =
        find_contract::<PipelineValidatorContract>(&runtime, PIPELINE_VALIDATOR_CONTRACT_ID)
            .map_err(|e| format!("[{language}] validator: {e}"))?;
    let validator_sv: StringView = validator
        .validate(StringView {
            ptr: validator_in.as_ptr(),
            len: validator_in.len(),
        })
        .map_err(|e| format!("[{language}] validate failed: {}", e.code as u32))?;
    results.insert("validator".to_string(), sv_to_owned(validator_sv)?);

    Ok(results)
}

fn run() -> Result<(), String> {
    let workspace: PathBuf = env::var("POLYPLUG_WORKSPACE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));

    println!("=== polyplug Cross-Language Differential Parity Harness ===\n");
    println!("Workspace: {}", workspace.display());
    println!(
        "Comparing {} languages across {} contracts.\n",
        LANGUAGES.len(),
        CONTRACTS.len()
    );

    // [language][contract] -> returned string.
    let mut matrix: BTreeMap<&str, BTreeMap<String, String>> = BTreeMap::new();
    for language in LANGUAGES {
        eprintln!("running language: {language}");
        let results: BTreeMap<String, String> = run_language(&workspace, language)?;
        matrix.insert(language, results);
    }

    // Compare every (contract, language) cell against the golden value and
    // build a pass/fail grid plus a list of concrete mismatches.
    let mut mismatches: Vec<String> = Vec::new();
    let mut grid: Vec<(String, Vec<bool>)> = Vec::new();

    for contract in CONTRACTS {
        let (input, expected): (&str, &str) = golden(contract);
        let mut row: Vec<bool> = Vec::new();
        for language in LANGUAGES {
            let got: &String = matrix
                .get(language)
                .and_then(|m| m.get(contract))
                .ok_or_else(|| format!("missing result for {language}/{contract}"))?;
            let ok: bool = got == expected;
            row.push(ok);
            if !ok {
                mismatches.push(format!(
                    "MISMATCH [{language}/{contract}]\n    input:    {input:?}\n    expected: {expected:?}\n    got:      {got:?}"
                ));
            }
        }
        grid.push((contract.to_string(), row));
    }

    print_matrix(&grid);

    if mismatches.is_empty() {
        println!(
            "\n✓ PARITY CONFIRMED: all {} languages produced byte-identical, golden output across all {} contracts.",
            LANGUAGES.len(),
            CONTRACTS.len()
        );
        Ok(())
    } else {
        println!(
            "\n✗ PARITY FAILED: {} mismatch(es) found:\n",
            mismatches.len()
        );
        for m in &mismatches {
            println!("{m}\n");
        }
        Err(format!("{} parity mismatch(es)", mismatches.len()))
    }
}

/// Render the contract × language pass/fail grid.
fn print_matrix(grid: &[(String, Vec<bool>)]) {
    let col_width: usize = LANGUAGES.iter().map(|l| l.len()).max().unwrap_or(6).max(6);
    let row_label_width: usize = CONTRACTS.iter().map(|c| c.len()).max().unwrap_or(11);

    print!("{:<width$} ", "contract", width = row_label_width);
    for language in LANGUAGES {
        print!("| {:^width$} ", language, width = col_width);
    }
    println!();

    print!("{:-<width$}-", "", width = row_label_width);
    for _ in LANGUAGES {
        print!("+-{:-<width$}-", "", width = col_width);
    }
    println!();

    for (contract, row) in grid {
        print!("{:<width$} ", contract, width = row_label_width);
        for ok in row {
            let mark: &str = if *ok { "✓" } else { "✗" };
            print!("| {:^width$} ", mark, width = col_width);
        }
        println!();
    }
}

/// Trait for contract callers - allows generic find_contract helper.
trait ContractCaller: Sized {
    fn from_handle(handle: GuestContractHandle, runtime: Arc<Runtime>) -> Option<Self>;
}

impl ContractCaller for PipelineDecoderContract {
    fn from_handle(handle: GuestContractHandle, runtime: Arc<Runtime>) -> Option<Self> {
        Self::new(handle, runtime)
    }
}

impl ContractCaller for DataTransformerContract {
    fn from_handle(handle: GuestContractHandle, runtime: Arc<Runtime>) -> Option<Self> {
        Self::new(handle, runtime)
    }
}

impl ContractCaller for PipelineEncoderContract {
    fn from_handle(handle: GuestContractHandle, runtime: Arc<Runtime>) -> Option<Self> {
        Self::new(handle, runtime)
    }
}

impl ContractCaller for DataReporterContract {
    fn from_handle(handle: GuestContractHandle, runtime: Arc<Runtime>) -> Option<Self> {
        Self::new(handle, runtime)
    }
}

impl ContractCaller for PipelineValidatorContract {
    fn from_handle(handle: GuestContractHandle, runtime: Arc<Runtime>) -> Option<Self> {
        Self::new(handle, runtime)
    }
}

//! examples/hosts/rust/src/main.rs
//! Rust host example for the polyplug plugin runtime.
//!
//! Loads all 12 guest plugins (Rust, C++, C#, Python, Lua, JS) and runs
//! 3 pipelines demonstrating cross-language plugin composition:
//!
//! Run 1: Rust decoder → C++ transformer → Rust encoder → C# reporter → C++ validator
//! Run 2: Python decoder → Lua transformer → C# encoder → Python reporter → Lua validator
//! Run 3: Rust decoder → C++ transformer → C# encoder → JS reporter → JS validator

use std::path::Path;
use std::path::PathBuf;

use polyplug::abi::AbiError;
use polyplug::abi::Buffer;
use polyplug::abi::PluginHandle;
use polyplug::abi::PluginVTable;
use polyplug::abi::StringView;
use polyplug::abi::ABI_OK;
use polyplug::registry::PluginVTableGuard;
use polyplug::runtime::Runtime;
use polyplug_dotnet::DotnetConfig;
use polyplug_dotnet::DotnetLoader;
use polyplug_js::JsConfig;
use polyplug_js::JsLoader;
use polyplug_lua::LuaConfig;
use polyplug_lua::LuaLoader;
use polyplug_python::PythonConfig;
use polyplug_python::PythonLoader;

// ─── Contract IDs (FNV-1a 64-bit of "pipeline.<name>@1") ────────────────────
const DECODER_CONTRACT_ID: u64 = 0x133E62ABD6E7D5BE_u64;
const TRANSFORMER_CONTRACT_ID: u64 = 0x0E3044133E12EB05_u64;
const ENCODER_CONTRACT_ID: u64 = 0x12AD37F43386F752_u64;
const REPORTER_CONTRACT_ID: u64 = 0xD50E539CAE219A15_u64;
const VALIDATOR_CONTRACT_ID: u64 = 0x027ABCEBF8020D90_u64;

// ─── ABI layout — DataRecord ─────────────────────────────────────────────────
// Mirrors examples/abi_types.md canonical layout:
// name(16) + value(16) + count(4) + _pad(4) = 40 bytes.
#[repr(C)]
struct DataRecord {
    name: StringView,
    value: StringView,
    count: u32,
    _pad: u32,
}

// SAFETY: DataRecord contains only StringViews and u32 values. All are plain data.
unsafe impl Send for DataRecord {}

// ─── ABI layout — ValidationResult ──────────────────────────────────────────
// valid(1) + _pad(7) + reason(16) = 24 bytes.
#[repr(C)]
struct ValidationResult {
    valid: u8,
    _pad: [u8; 7usize],
    reason: StringView,
}

// ─── FNV-1a 64-bit hash for bundle IDs ──────────────────────────────────────
fn fnv1a_64(data: &[u8]) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325_u64;
    const FNV_PRIME: u64 = 0x00000100000001B3_u64;
    let mut hash: u64 = FNV_OFFSET;
    for &byte in data {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

fn bundle_id(name: &str) -> u64 {
    fnv1a_64(name.as_bytes())
}

// ─── Plugin entry: vtable guard + resolved vtable pointer ───────────────────
struct PluginEntry {
    name: &'static str,
    _guard: PluginVTableGuard,
    vtable: *const PluginVTable,
}

// ─── ABI type alias ──────────────────────────────────────────────────────────
type AbiFn = unsafe extern "C" fn(*const (), *mut ()) -> AbiError;

// ─── find_by_bundle helper ───────────────────────────────────────────────────
fn resolve_plugin(
    runtime: &Runtime,
    bundle_name: &str,
    contract_id: u64,
    label: &'static str,
) -> Result<PluginEntry, String> {
    let bid: u64 = bundle_id(bundle_name);
    let handle: PluginHandle = runtime
        .find_by_bundle(bid, contract_id, 0_u32)
        .map_err(|e| format!("find_by_bundle({bundle_name}): {e}"))?;

    if handle.is_null() {
        return Err(format!("plugin not found for bundle: {bundle_name}"));
    }

    let guard: PluginVTableGuard = runtime
        .registry()
        .resolve_guard(handle)
        .map_err(|e| format!("resolve_guard({bundle_name}): {e}"))?;

    let vtable: *const PluginVTable = guard.vtable();
    if vtable.is_null() {
        return Err(format!("null vtable for bundle: {bundle_name}"));
    }

    Ok(PluginEntry {
        name: label,
        _guard: guard,
        vtable,
    })
}

// ─── call a vtable function ──────────────────────────────────────────────────
///
/// # Safety
/// `entry.vtable` must be non-null and valid. `fn_id` must be < function_count.
/// `args` must point to a valid input struct. `out` must point to a valid output struct.
unsafe fn call_fn(
    entry: &PluginEntry,
    fn_id: u32,
    args: *const (),
    out: *mut (),
) -> Result<AbiError, String> {
    // SAFETY: vtable is non-null and valid for the lifetime of _guard.
    let vt: &PluginVTable = unsafe { &*entry.vtable };

    if fn_id >= vt.function_count {
        return Err(format!(
            "fn_id {fn_id} out of range (function_count={})",
            vt.function_count
        ));
    }

    if vt.functions.is_null() {
        return Err(format!(
            "null functions pointer in vtable for {}",
            entry.name
        ));
    }

    // SAFETY: functions[fn_id] is a valid function pointer per vtable contract.
    let fn_ptr_raw: *const () = unsafe { *vt.functions.add(fn_id as usize) };
    if fn_ptr_raw.is_null() {
        return Err(format!(
            "null function pointer for fn_id {fn_id} in {}",
            entry.name
        ));
    }

    // SAFETY: fn_ptr_raw conforms to the ABI function signature (const void* args, void* out).
    let func: AbiFn = unsafe { core::mem::transmute(fn_ptr_raw) };

    // SAFETY: args and out are valid pointers per the caller's contract.
    Ok(unsafe { func(args, out) })
}

// ─── format error message from AbiError ─────────────────────────────────────
fn format_abi_error(err: AbiError) -> String {
    if err.message.ptr.is_null() || err.message.len == 0 {
        return format!("unknown error (code {})", err.code);
    }
    // SAFETY: error message is valid UTF-8 for message.len bytes, allocated by guest.
    let msg: &str = unsafe {
        core::str::from_utf8(core::slice::from_raw_parts(
            err.message.ptr,
            err.message.len,
        ))
        .unwrap_or("(invalid utf-8)")
    };
    if msg.trim().is_empty() {
        return format!("unknown error (code {})", err.code);
    }
    format!("{} (code {})", msg, err.code)
}

// ─── run one pipeline ────────────────────────────────────────────────────────
///
/// # Safety
/// All plugin entries must have valid vtable pointers backed by live guards.
unsafe fn run_pipeline(
    label: &str,
    decoder: &PluginEntry,
    transformer: &PluginEntry,
    encoder: &PluginEntry,
    reporter: &PluginEntry,
    validator: &PluginEntry,
    csv_input: &str,
) -> Result<(), String> {
    println!("--- {label} ---");

    // Stage 1: decode — Buffer → DataRecord
    let input_bytes: &[u8] = csv_input.as_bytes();
    let input_buf: Buffer = Buffer {
        ptr: input_bytes.as_ptr().cast_mut(),
        len: input_bytes.len(),
        cap: input_bytes.len(),
    };

    let mut record: DataRecord = DataRecord {
        name: StringView::null(),
        value: StringView::null(),
        count: 0_u32,
        _pad: 0_u32,
    };

    // SAFETY: input_buf is a valid Buffer; record is a valid DataRecord.
    let decode_err: AbiError = unsafe {
        call_fn(
            decoder,
            0_u32,
            core::ptr::addr_of!(input_buf).cast::<()>(),
            core::ptr::addr_of_mut!(record).cast::<()>(),
        )?
    };
    if decode_err.code != ABI_OK {
        return Err(format!("decode failed: {}", format_abi_error(decode_err)));
    }

    // Stage 2: transform — DataRecord → DataRecord
    let mut transformed: DataRecord = DataRecord {
        name: StringView::null(),
        value: StringView::null(),
        count: 0_u32,
        _pad: 0_u32,
    };

    // SAFETY: record is valid DataRecord; transformed is valid DataRecord output.
    let transform_err: AbiError = unsafe {
        call_fn(
            transformer,
            0_u32,
            core::ptr::addr_of!(record).cast::<()>(),
            core::ptr::addr_of_mut!(transformed).cast::<()>(),
        )?
    };
    if transform_err.code != ABI_OK {
        return Err(format!(
            "transform failed: {}",
            format_abi_error(transform_err)
        ));
    }

    // Stage 3: encode — DataRecord → Buffer
    let mut encoded: Buffer = Buffer {
        ptr: core::ptr::null_mut(),
        len: 0_usize,
        cap: 0_usize,
    };

    // SAFETY: transformed is valid DataRecord; encoded is valid Buffer output.
    let encode_err: AbiError = unsafe {
        call_fn(
            encoder,
            0_u32,
            core::ptr::addr_of!(transformed).cast::<()>(),
            core::ptr::addr_of_mut!(encoded).cast::<()>(),
        )?
    };
    if encode_err.code != ABI_OK {
        return Err(format!("encode failed: {}", format_abi_error(encode_err)));
    }

    let encoded_str: &str = if encoded.ptr.is_null() || encoded.len == 0 {
        ""
    } else {
        // SAFETY: encoded.ptr is valid UTF-8 for encoded.len bytes, host-allocated.
        unsafe {
            core::str::from_utf8(core::slice::from_raw_parts(encoded.ptr, encoded.len))
                .unwrap_or("(invalid utf-8)")
        }
    };
    println!("Run output: {}", encoded_str.trim_end());

    // Stage 4: report — DataRecord → StringView
    let mut report_sv: StringView = StringView::null();

    // SAFETY: transformed is valid DataRecord; report_sv is valid StringView output.
    let report_err: AbiError = unsafe {
        call_fn(
            reporter,
            0_u32,
            core::ptr::addr_of!(transformed).cast::<()>(),
            core::ptr::addr_of_mut!(report_sv).cast::<()>(),
        )?
    };
    if report_err.code != ABI_OK {
        return Err(format!("report failed: {}", format_abi_error(report_err)));
    }

    if !report_sv.ptr.is_null() && report_sv.len > 0 {
        // SAFETY: report_sv.ptr is valid UTF-8 for report_sv.len bytes.
        let report_str: &str = unsafe {
            core::str::from_utf8(core::slice::from_raw_parts(report_sv.ptr, report_sv.len))
                .unwrap_or("(invalid utf-8)")
        };
        if !report_str.trim().is_empty() {
            println!("Run summary: {}", report_str);
        }
    }

    // Stage 5: validate — DataRecord → ValidationResult
    let mut validation: ValidationResult = ValidationResult {
        valid: 0_u8,
        _pad: [0_u8; 7usize],
        reason: StringView::null(),
    };

    // SAFETY: transformed is valid DataRecord; validation is valid ValidationResult output.
    let validate_err: AbiError = unsafe {
        call_fn(
            validator,
            0_u32,
            core::ptr::addr_of!(transformed).cast::<()>(),
            core::ptr::addr_of_mut!(validation).cast::<()>(),
        )?
    };
    if validate_err.code != ABI_OK {
        return Err(format!(
            "validate failed: {}",
            format_abi_error(validate_err)
        ));
    }

    let reason_str: &str = if validation.reason.ptr.is_null() || validation.reason.len == 0 {
        ""
    } else {
        // SAFETY: reason.ptr is valid UTF-8 for reason.len bytes.
        unsafe {
            core::str::from_utf8(core::slice::from_raw_parts(
                validation.reason.ptr,
                validation.reason.len,
            ))
            .unwrap_or("(invalid utf-8)")
        }
    };
    let status: &str = if validation.valid != 0_u8 {
        "ok"
    } else {
        "invalid"
    };
    println!("Validation: {status} ({reason_str})");

    Ok(())
}

// ─── find repo root ──────────────────────────────────────────────────────────
fn find_repo_root() -> PathBuf {
    let candidates: [PathBuf; 2] = [
        PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    ];

    for seed in &candidates {
        let mut dir: PathBuf = seed.clone();
        for _ in 0..8 {
            let examples_path: PathBuf = dir.join("examples").join("guests");
            if examples_path.is_dir() {
                return dir;
            }
            match dir.parent() {
                Some(p) => dir = p.to_path_buf(),
                None => break,
            }
        }
    }

    PathBuf::from(".")
}

fn main() {
    match run() {
        Ok(()) => {}
        Err(message) => {
            eprintln!("error: {message}");
            std::process::exit(1_i32);
        }
    }
}

fn run() -> Result<(), String> {
    println!("=== polyplug C# host example ===");

    let repo_root: PathBuf = find_repo_root();

    // ─── Build runtime with all language loaders ─────────────────────────────
    let runtime: Runtime = Runtime::builder()
        .loader(DotnetLoader::new(DotnetConfig::default()))
        .loader(PythonLoader::new(PythonConfig::default()))
        .loader(LuaLoader::new(LuaConfig::default()))
        .loader(JsLoader::new(JsConfig {}))
        .build()
        .map_err(|e| format!("runtime build failed: {e}"))?;

    // ─── Load all 12 guest plugins ───────────────────────────────────────────
    // C# guests first: CLR must init before native guests dlopen'd.
    let bundle_dirs: [(&str, &str); 12] = [
        ("rust", "decoder"),
        ("rust", "encoder"),
        ("cpp", "transformer"),
        ("cpp", "validator"),
        ("csharp", "encoder"),
        ("csharp", "reporter"),
        ("python", "decoder"),
        ("python", "reporter"),
        ("lua", "transformer"),
        ("lua", "validator"),
        ("js", "validator"),
        ("js", "reporter"),
    ];

    println!("Loading 12 guest plugins...");
    for (idx, (lang, name)) in bundle_dirs.iter().enumerate() {
        let path: PathBuf = repo_root
            .join("examples")
            .join("guests")
            .join(lang)
            .join(name);
        match runtime.load_bundle(Path::new(&path)) {
            Ok(()) => println!("  [OK]  {:2}/12 {lang}/{name}", idx + 1),
            Err(e) => {
                eprintln!("  [ERR] {:2}/12 {lang}/{name}: {e}", idx + 1);
                return Err(format!("failed to load {lang}/{name}: {e}"));
            }
        }
    }

    // ─── Resolve all 12 plugin vtables ───────────────────────────────────────
    let decoder_rust: PluginEntry =
        resolve_plugin(&runtime, "csv_decoder", DECODER_CONTRACT_ID, "decoder_rust")?;
    let encoder_rust: PluginEntry = resolve_plugin(
        &runtime,
        "csv_encoder_rust",
        ENCODER_CONTRACT_ID,
        "encoder_rust",
    )?;
    let transformer_cpp: PluginEntry = resolve_plugin(
        &runtime,
        "uppercase_transformer",
        TRANSFORMER_CONTRACT_ID,
        "transformer_cpp",
    )?;
    let validator_cpp: PluginEntry = resolve_plugin(
        &runtime,
        "cpp_validator",
        VALIDATOR_CONTRACT_ID,
        "validator_cpp",
    )?;
    let encoder_csharp: PluginEntry = resolve_plugin(
        &runtime,
        "csv_encoder_csharp",
        ENCODER_CONTRACT_ID,
        "encoder_csharp",
    )?;
    let reporter_csharp: PluginEntry = resolve_plugin(
        &runtime,
        "csharp_reporter",
        REPORTER_CONTRACT_ID,
        "reporter_csharp",
    )?;
    let decoder_python: PluginEntry = resolve_plugin(
        &runtime,
        "python_decoder",
        DECODER_CONTRACT_ID,
        "decoder_python",
    )?;
    let reporter_python: PluginEntry = resolve_plugin(
        &runtime,
        "summary_reporter",
        REPORTER_CONTRACT_ID,
        "reporter_python",
    )?;
    let transformer_lua: PluginEntry = resolve_plugin(
        &runtime,
        "reverse_transformer",
        TRANSFORMER_CONTRACT_ID,
        "transformer_lua",
    )?;
    let validator_lua: PluginEntry = resolve_plugin(
        &runtime,
        "lua_validator",
        VALIDATOR_CONTRACT_ID,
        "validator_lua",
    )?;
    let validator_js: PluginEntry = resolve_plugin(
        &runtime,
        "field_validator",
        VALIDATOR_CONTRACT_ID,
        "validator_js",
    )?;
    let reporter_js: PluginEntry =
        resolve_plugin(&runtime, "js_reporter", REPORTER_CONTRACT_ID, "reporter_js")?;

    // ─── Pipeline Run 1 ──────────────────────────────────────────────────────
    // SAFETY: all plugin entries have valid, live vtable pointers backed by guards.
    unsafe {
        run_pipeline(
            "Run 1: Rust decoder, C++ transformer, Rust encoder, C# reporter, C++ validator",
            &decoder_rust,
            &transformer_cpp,
            &encoder_rust,
            &reporter_csharp,
            &validator_cpp,
            "Alice,hello,3\n",
        )?;
    }

    // ─── Pipeline Run 2 ──────────────────────────────────────────────────────
    // SAFETY: all plugin entries have valid, live vtable pointers backed by guards.
    unsafe {
        run_pipeline(
            "Run 2: Python decoder, Lua transformer, C# encoder, Python reporter, Lua validator",
            &decoder_python,
            &transformer_lua,
            &encoder_csharp,
            &reporter_python,
            &validator_lua,
            "Bob,world,4\n",
        )?;
    }

    // ─── Pipeline Run 3 ──────────────────────────────────────────────────────
    // SAFETY: all plugin entries have valid, live vtable pointers backed by guards.
    unsafe {
        run_pipeline(
            "Run 3: Rust decoder, C++ transformer, C# encoder, JS reporter, JS validator",
            &decoder_rust,
            &transformer_cpp,
            &encoder_csharp,
            &reporter_js,
            &validator_js,
            "Cara,polyplug,5\n",
        )?;
    }

    println!("pipeline complete");
    Ok(())
}

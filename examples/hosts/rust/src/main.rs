use core::fmt;
use core::result::Result;
use core::str;
use polyplug::abi::bundle_id;
use polyplug::abi::AbiError;
use polyplug::abi::Buffer;
use polyplug::abi::PluginHandle;
use polyplug::abi::PluginVTable;
use polyplug::abi::StringView;
use polyplug::abi::ABI_OK;
use polyplug::error::RegistryError;
use polyplug::error::RuntimeError;
use polyplug::extensions::trace::TraceExtension;
use polyplug::runtime::Runtime;
use polyplug::version::Compatibility;
use polyplug_dotnet::DotnetConfig;
use polyplug_dotnet::DotnetLoader;
use polyplug_js::JsConfig;
use polyplug_js::JsLoader;
use polyplug_lua::LuaConfig;
use polyplug_lua::LuaLoader;
use polyplug_python::PythonConfig;
use polyplug_python::PythonLoader;
use std::path::PathBuf;

const DECODER_CONTRACT_ID: u64 = 0x133E62ABD6E7D5BE;
const TRANSFORMER_CONTRACT_ID: u64 = 0x0E3044133E12EB05;
const ENCODER_CONTRACT_ID: u64 = 0x12AD37F43386F752;
const REPORTER_CONTRACT_ID: u64 = 0xD50E539CAE219A15;
const VALIDATOR_CONTRACT_ID: u64 = 0x027ABCEBF8020D90;

#[repr(C)]
struct DataRecord {
    name: StringView,
    value: StringView,
    count: u32,
    _pad: u32,
}

#[derive(Debug)]
enum ShowcaseError {
    Runtime(RuntimeError),
    Registry(RegistryError),
    Decode { code: u32 },
    Encode { code: u32 },
    Report { code: u32 },
}

impl fmt::Display for ShowcaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ShowcaseError::Runtime(err) => write!(f, "runtime error: {err}"),
            ShowcaseError::Registry(err) => write!(f, "registry error: {err}"),
            ShowcaseError::Decode { code } => write!(f, "decode failed: code {code}"),
            ShowcaseError::Encode { code } => write!(f, "encode failed: code {code}"),
            ShowcaseError::Report { code } => write!(f, "report failed: code {code}"),
        }
    }
}

impl core::error::Error for ShowcaseError {}

impl From<RuntimeError> for ShowcaseError {
    fn from(err: RuntimeError) -> ShowcaseError {
        ShowcaseError::Runtime(err)
    }
}

impl From<RegistryError> for ShowcaseError {
    fn from(err: RegistryError) -> ShowcaseError {
        ShowcaseError::Registry(err)
    }
}

// SAFETY: vtable is valid for Runtime lifetime; args_ptr and out_ptr point to valid objects.
unsafe fn call_fn(
    vtable: *const PluginVTable,
    fn_id: usize,
    args_ptr: *const (),
    out_ptr: *mut (),
) -> AbiError {
    // SAFETY: vtable.functions is a valid pointer array with at least fn_id+1 entries.
    let fn_ptr: *const () = unsafe { *(*vtable).functions.add(fn_id) };
    // SAFETY: fn_ptr is a valid function with the ABI signature.
    let func: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: func is a valid ABI entry; args_ptr and out_ptr are valid for this call.
    unsafe { func(args_ptr, out_ptr) }
}

fn lookup_by_contract(
    runtime: &Runtime,
    contract_id: u64,
) -> Result<*const PluginVTable, ShowcaseError> {
    let handle: PluginHandle = runtime.find_by_contract(contract_id, 0)?;
    let vtable: *const PluginVTable = runtime.resolve_plugin(handle)?;
    Ok(vtable)
}

fn lookup_by_bundle(
    runtime: &Runtime,
    bundle_name: &str,
    contract_id: u64,
) -> Result<*const PluginVTable, ShowcaseError> {
    let bid: u64 = bundle_id(bundle_name);
    let handle: PluginHandle = runtime.find_by_bundle(bid, contract_id, 0)?;
    let vtable: *const PluginVTable = runtime.resolve_plugin(handle)?;
    Ok(vtable)
}

fn run_pipeline(
    decoder_vt: *const PluginVTable,
    validator_vt: *const PluginVTable,
    transformer_vt: *const PluginVTable,
    encoder_vt: *const PluginVTable,
    reporter_vt: *const PluginVTable,
    input_csv: &[u8],
) -> Result<(), ShowcaseError> {
    let mut record: DataRecord = DataRecord {
        name: StringView::null(),
        value: StringView::null(),
        count: 0_u32,
        _pad: 0_u32,
    };
    let input_buf: Buffer = Buffer {
        ptr: input_csv.as_ptr() as *mut u8,
        len: input_csv.len(),
        cap: input_csv.len(),
    };
    // SAFETY: decoder_vt is valid; input_buf and record are valid stack objects.
    let decode_err: AbiError = unsafe {
        call_fn(
            decoder_vt,
            0,
            &input_buf as *const Buffer as *const (),
            &mut record as *mut DataRecord as *mut (),
        )
    };
    if decode_err.code != ABI_OK {
        return Err(ShowcaseError::Decode {
            code: decode_err.code,
        });
    }

    let mut _void_out: u64 = 0_u64;
    // SAFETY: validator_vt is valid; record and void_out are valid stack objects.
    let _validate_err: AbiError = unsafe {
        call_fn(
            validator_vt,
            0,
            &record as *const DataRecord as *const (),
            &mut _void_out as *mut u64 as *mut (),
        )
    };

    let mut transformed: DataRecord = DataRecord {
        name: StringView::null(),
        value: StringView::null(),
        count: 0_u32,
        _pad: 0_u32,
    };
    // SAFETY: transformer_vt valid; record and transformed are valid.
    let _transform_err: AbiError = unsafe {
        call_fn(
            transformer_vt,
            0,
            &record as *const DataRecord as *const (),
            &mut transformed as *mut DataRecord as *mut (),
        )
    };

    let mut encoded_buf: Buffer = Buffer {
        ptr: core::ptr::null_mut(),
        len: 0,
        cap: 0,
    };
    // SAFETY: encoder_vt valid; transformed and encoded_buf are valid.
    let encode_err: AbiError = unsafe {
        call_fn(
            encoder_vt,
            0,
            &transformed as *const DataRecord as *const (),
            &mut encoded_buf as *mut Buffer as *mut (),
        )
    };
    if encode_err.code != ABI_OK {
        return Err(ShowcaseError::Encode {
            code: encode_err.code,
        });
    }
    // SAFETY: encoded_buf.ptr is valid and encoded_buf.len bytes are valid UTF-8 from C# encoder.
    let output_str: &str = unsafe {
        let bytes: &[u8] = core::slice::from_raw_parts(encoded_buf.ptr, encoded_buf.len);
        str::from_utf8_unchecked(bytes)
    };
    println!("Run output: {}", output_str.trim_end());

    let mut report_sv: StringView = StringView::null();
    // SAFETY: reporter_vt valid; transformed and report_sv are valid.
    let report_err: AbiError = unsafe {
        call_fn(
            reporter_vt,
            0,
            &transformed as *const DataRecord as *const (),
            &mut report_sv as *mut StringView as *mut (),
        )
    };
    if report_err.code != ABI_OK {
        return Err(ShowcaseError::Report {
            code: report_err.code,
        });
    }
    if !report_sv.ptr.is_null() && report_sv.len > 0 {
        // SAFETY: report_sv.ptr is valid UTF-8 string data from Python reporter.
        let report_bytes: &[u8] =
            unsafe { core::slice::from_raw_parts(report_sv.ptr, report_sv.len) };
        let report_str_result: Result<&str, str::Utf8Error> = str::from_utf8(report_bytes);
        let report_str: &str = match report_str_result {
            Ok(value) => value,
            Err(_) => return Ok(()),
        };
        println!("Run summary: {}", report_str);
    }

    Ok(())
}

fn run_error_scenario(decoder_vt: *const PluginVTable) {
    let bad_input: &[u8] = b"INVALID\n";
    let input_buf: Buffer = Buffer {
        ptr: bad_input.as_ptr() as *mut u8,
        len: bad_input.len(),
        cap: bad_input.len(),
    };
    let mut record: DataRecord = DataRecord {
        name: StringView::null(),
        value: StringView::null(),
        count: 0_u32,
        _pad: 0_u32,
    };
    // SAFETY: decoder_vt is valid; input_buf and record are valid stack objects.
    let decode_err: AbiError = unsafe {
        call_fn(
            decoder_vt,
            0,
            &input_buf as *const Buffer as *const (),
            &mut record as *mut DataRecord as *mut (),
        )
    };
    if decode_err.code != ABI_OK {
        // SAFETY: decode_err.message.ptr is a valid UTF-8 byte literal from the csv_decoder plugin,
        // valid for the duration of this call. len is the exact byte count.
        let msg_str: &str = if decode_err.message.ptr.is_null() || decode_err.message.len == 0 {
            "unknown error"
        } else {
            unsafe {
                let bytes: &[u8] = core::slice::from_raw_parts(decode_err.message.ptr, decode_err.message.len);
                core::str::from_utf8_unchecked(bytes)
            }
        };
        println!("Error: decode failed: {} (code {})", msg_str, decode_err.code);
    }
}


fn build_runtime() -> Result<Runtime, ShowcaseError> {
    let guests_root: PathBuf = PathBuf::from("examples/guests");
    let runtime: Runtime = Runtime::builder()
        .plugin_dir(guests_root.join("rust"))
        .plugin_dir(guests_root.join("cpp"))
        .plugin_dir(guests_root.join("csharp"))
        .plugin_dir(guests_root.join("python"))
        .plugin_dir(guests_root.join("lua"))
        .plugin_dir(guests_root.join("js"))
        .extension(Box::new(TraceExtension::new(|msg: &str| {
            println!("[trace] {msg}");
        })))
        .on_warning(|msg: &str| {
            println!("[warning] {msg}");
        })
        .compatibility(Compatibility::Relaxed)
        .loader(PythonLoader::new(PythonConfig::default()))
        .loader(LuaLoader::new(LuaConfig::default()))
        .loader(JsLoader::new(JsConfig {}))
        .loader(DotnetLoader::new(DotnetConfig::default()))
        .build()?;
    Ok(runtime)
}

fn main() -> Result<(), ShowcaseError> {
    println!("=== polyplug examples ===");
    let runtime: Runtime = build_runtime()?;
    let decoder_vt: *const PluginVTable = lookup_by_contract(&runtime, DECODER_CONTRACT_ID)?;
    let uppercase_vt: *const PluginVTable =
        lookup_by_bundle(&runtime, "uppercase_transformer", TRANSFORMER_CONTRACT_ID)?;
    let validator_vt: *const PluginVTable = lookup_by_contract(&runtime, VALIDATOR_CONTRACT_ID)?;
    let reverse_vt: *const PluginVTable =
        lookup_by_bundle(&runtime, "reverse_transformer", TRANSFORMER_CONTRACT_ID)?;
    let encoder_vt: *const PluginVTable = lookup_by_contract(&runtime, ENCODER_CONTRACT_ID)?;
    let reporter_vt: *const PluginVTable = lookup_by_contract(&runtime, REPORTER_CONTRACT_ID)?;

    println!("--- Run 1: C++ uppercase transformer ---");
    run_pipeline(
        decoder_vt,
        validator_vt,
        uppercase_vt,
        encoder_vt,
        reporter_vt,
        b"Alice,hello,3\n",
    )?;

    println!("--- Run 2: Lua reverse transformer ---");
    run_pipeline(
        decoder_vt,
        validator_vt,
        reverse_vt,
        encoder_vt,
        reporter_vt,
        b"Alice,hello,3\n",
    )?;

    println!("--- Error scenario: malformed input ---");
    run_error_scenario(decoder_vt);

    println!("=== examples complete ===");
    Ok(())
}

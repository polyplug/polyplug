use core::fmt;
use polyplug::abi::AbiError;
use polyplug::abi::PluginHandle;
use polyplug::abi::PluginVTable;
use polyplug::abi::StringView;
use polyplug::abi::ABI_OK;
use polyplug::error::RegistryError;
use polyplug::error::RuntimeError;
use polyplug::runtime::Runtime;
use std::path::PathBuf;

// Contract ID for the data.Transformer contract (see examples/api.toml).
const TRANSFORMER_CONTRACT_ID: u64 = 0x133E62ABD6E7D5BE;

#[derive(Debug)]
enum HostError {
    Runtime(RuntimeError),
    Registry(RegistryError),
    CallFailed { code: u32 },
    InvalidUtf8,
}

impl fmt::Display for HostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HostError::Runtime(err) => write!(f, "runtime error: {err}"),
            HostError::Registry(err) => write!(f, "registry error: {err}"),
            HostError::CallFailed { code } => write!(f, "plugin call failed: code {code}"),
            HostError::InvalidUtf8 => write!(f, "plugin returned invalid UTF-8"),
        }
    }
}

impl core::error::Error for HostError {}

impl From<RuntimeError> for HostError {
    fn from(err: RuntimeError) -> HostError {
        HostError::Runtime(err)
    }
}

impl From<RegistryError> for HostError {
    fn from(err: RegistryError) -> HostError {
        HostError::Registry(err)
    }
}

/// Invoke function 0 of a plugin vtable: `transform(StringView*) -> StringView`.
///
/// # Safety
/// - `vtable` must be a valid pointer returned by `runtime.resolve_plugin()`.
/// - `input` must point to valid UTF-8 bytes for the duration of the call.
/// - `output` must point to a writable, properly-aligned `StringView`.
unsafe fn call_transform(
    vtable: *const PluginVTable,
    input: *const StringView,
    output: *mut StringView,
) -> AbiError {
    // SAFETY: vtable.functions is a valid non-null array; the manifest's function_count
    // guarantees index 0 exists.
    let fn_ptr: *const () = unsafe { *(*vtable).functions.add(0) };
    // SAFETY: fn_ptr is the ABI function at index 0: (args: *const (), out: *mut ()) -> AbiError.
    let func: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: input and output are valid stack-allocated objects whose lifetimes span this call.
    unsafe { func(input as *const (), output as *mut ()) }
}

fn main() -> Result<(), HostError> {
    println!("=== polyplug rust_minimal example ===");

    let plugin_dir: PathBuf = PathBuf::from("examples/guests/rust");
    println!("Scanning: {}", plugin_dir.display());

    let runtime: Runtime = Runtime::builder()
        .plugin_dir(plugin_dir)
        .on_warning(|msg: &str| eprintln!("[polyplug] warning: {msg}"))
        .build()?;

    let handle: PluginHandle = match runtime.find_by_contract(TRANSFORMER_CONTRACT_ID, 0) {
        Ok(h) => h,
        Err(err) => {
            println!("No plugin found (0x{TRANSFORMER_CONTRACT_ID:016X}): {err}");
            println!("Build the Rust guest first:");
            println!(
                "  cargo build --release --manifest-path examples/guests/rust/decoder/Cargo.toml"
            );
            println!("=== rust_minimal example complete (no guests loaded) ===");
            return Ok(());
        }
    };
    println!("Found plugin for contract 0x{TRANSFORMER_CONTRACT_ID:016X}");

    let vtable: *const PluginVTable = runtime.resolve_plugin(handle)?;

    let input_str: &str = "hello from rust_minimal";
    let input_sv: StringView = StringView {
        ptr: input_str.as_ptr(),
        len: input_str.len(),
    };
    let mut output_sv: StringView = StringView::null();

    // SAFETY: vtable is valid (just resolved from registry); input_sv and output_sv are
    // valid stack objects whose lifetimes span the call.
    let err: AbiError = unsafe { call_transform(vtable, &input_sv, &mut output_sv) };

    if err.code != ABI_OK {
        return Err(HostError::CallFailed { code: err.code });
    }

    if output_sv.ptr.is_null() || output_sv.len == 0 {
        println!("Plugin returned empty output.");
    } else {
        // SAFETY: output_sv was written by the plugin; the ABI StringView contract guarantees
        // ptr is valid UTF-8 for len bytes and remains live until the end of this call.
        let bytes: &[u8] = unsafe { core::slice::from_raw_parts(output_sv.ptr, output_sv.len) };
        let result: &str = core::str::from_utf8(bytes).map_err(|_| HostError::InvalidUtf8)?;
        println!("Plugin result: {result}");
    }

    println!("=== rust_minimal example complete ===");
    Ok(())
}

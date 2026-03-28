use polyplug::runtime::Runtime;
use polyplug_abi::{
    AbiError, DispatchType, HostContractDispatch, HostContractVTable, HostContractVTableHeader,
    NativeHostContractDispatch, StringView, ABI_OK,
};
use polyplug_native::{NativeConfig, NativeLoader};
use std::env;
use std::path::PathBuf;

mod generated;

use generated::host::host_callers::*;
use generated::host::host_contracts::*;
use generated::host::types::*;

struct ConsoleLogger;

impl HostLogger for ConsoleLogger {
    fn log(&self, message: &str) {
        println!("[PLUGIN LOG] {}", message);
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
        .unwrap_or_else(|_| PathBuf::from("examples/host_contracts/logger/plugins"));

    eprintln!("loading plugins from: {}", plugin_path.display());

    let runtime: &'static Runtime = Box::leak(Box::new(
        Runtime::builder()
            .loader(NativeLoader::new(NativeConfig {}))
            .build()
            .map_err(|e| e.to_string())?,
    ));

    let logger_impl: Box<ConsoleLogger> = Box::new(ConsoleLogger);
    let logger_vtable: &'static HostContractVTable = create_logger_vtable(logger_impl);

    runtime
        .register_host_contract(HOSTLOGGER_CONTRACT_ID, logger_vtable)
        .map_err(|e| e.to_string())?;

    if !plugin_path.exists() {
        return Err(format!(
            "plugin path does not exist: {}",
            plugin_path.display()
        ));
    }

    for entry in std::fs::read_dir(&plugin_path).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.path().is_dir() {
            let manifest_path = entry.path().join("manifest.toml");
            if manifest_path.exists() {
                runtime
                    .load_bundle(&entry.path())
                    .map_err(|e| format!("load failed: {e}"))?;
                eprintln!("  loaded: {}", entry.path().display());
            }
        }
    }

    println!("\n=== Logger Host (Rust) ===\n");

    let input: &str = "hello world";
    println!("Input: \"{input}\"\n");

    if let Some(worker) =
        find_contract::<ExampleWorkerContract>(runtime, EXAMPLE_WORKER_CONTRACT_ID)
    {
        let result_sv: StringView = worker
            .do_work(StringView {
                ptr: input.as_ptr(),
                len: input.len(),
            })
            .map_err(|e| format!("do_work failed: {}", e.code))?;
        let result: &str = unsafe {
            std::str::from_utf8(std::slice::from_raw_parts(result_sv.ptr, result_sv.len))
        }
        .map_err(|e| e.to_string())?;
        println!("[host] do_work(\"{}\") = \"{}\"", input, result);
    }

    println!("\ndone.");
    Ok(())
}

fn find_contract<T>(runtime: &'static Runtime, contract_id: u64) -> Option<T>
where
    T: ContractCaller,
{
    let handle: polyplug_abi::PluginHandle = runtime.find_by_contract(contract_id, 0).ok()?;
    if handle.is_null() {
        return None;
    }
    T::from_handle(handle, runtime)
}

trait ContractCaller: Sized {
    fn from_handle(handle: polyplug_abi::PluginHandle, runtime: &'static Runtime) -> Option<Self>;
}

impl ContractCaller for ExampleWorkerContract {
    fn from_handle(handle: polyplug_abi::PluginHandle, runtime: &'static Runtime) -> Option<Self> {
        Self::new(handle, runtime)
    }
}

fn create_logger_vtable(logger: Box<ConsoleLogger>) -> &'static HostContractVTable {
    let logger_ptr: *mut ConsoleLogger = Box::into_raw(logger);

    // Host contract dispatch function signature:
    // fn(impl_ptr: *const (), args: *const (), out: *mut ()) -> AbiError
    // - impl_ptr: Pointer to implementation (from vtable.dispatch.native.impl_ptr)
    // - args: Function arguments (e.g., StringView)
    // - out: Output buffer for return value
    let log_fn: unsafe extern "C" fn(*const (), *const (), *mut ()) -> AbiError = log_dispatch;

    let functions: [unsafe extern "C" fn(*const (), *const (), *mut ()) -> AbiError; 1] = [log_fn];

    let header: HostContractVTableHeader = HostContractVTableHeader {
        vtable_version: 1,
        contract_id: HOSTLOGGER_CONTRACT_ID,
        contract_major: 1,
        contract_minor: 0,
        function_count: 1,
        dispatch_type: DispatchType::Native,
    };

    let dispatch: HostContractDispatch = HostContractDispatch {
        native: NativeHostContractDispatch {
            impl_ptr: logger_ptr as *const (),
            functions: functions.as_ptr() as *const *const (),
        },
    };

    let vtable: HostContractVTable = HostContractVTable { header, dispatch };

    Box::leak(Box::new(vtable))
}

unsafe extern "C" fn log_dispatch(impl_ptr: *const (), args: *const (), _out: *mut ()) -> AbiError {
    // SAFETY: impl_ptr is set by create_logger_vtable to point to a valid ConsoleLogger
    // that was leaked via Box::leak. The pointer remains valid for the lifetime of the vtable.
    let logger: &ConsoleLogger = unsafe { &*(impl_ptr as *const ConsoleLogger) };

    // SAFETY: args is a pointer to a StringView passed by the caller.
    // The caller guarantees the pointer is valid and points to properly initialized data.
    let message_sv: StringView = unsafe { *(args as *const StringView) };

    // SAFETY: StringView.ptr and StringView.len describe a valid UTF-8 string slice
    // allocated by the host allocator, guaranteed by the ABI contract.
    let message: &str = unsafe {
        std::str::from_utf8(std::slice::from_raw_parts(message_sv.ptr, message_sv.len))
            .unwrap_or("")
    };

    logger.log(message);

    AbiError {
        code: ABI_OK,
        message: StringView {
            ptr: std::ptr::null(),
            len: 0,
        },
    }
}

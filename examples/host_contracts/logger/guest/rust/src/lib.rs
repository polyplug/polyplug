use polyplug_guest::{PluginError, StringView, alloc_string, to_str};

#[path = "../generated/guest/mod.rs"]
mod generated;

use generated::contracts::ExampleWorkerPlugin;
use generated::host_contract_callers::HostLoggerCaller;
use generated::vtables::set_worker_impl;

struct Plugin;

impl ExampleWorkerPlugin for Plugin {
    fn do_work(&self, input: StringView) -> Result<StringView, PluginError> {
        let s = to_str(input);

        let logger = unsafe {
            HostLoggerCaller::from_host(polyplug_guest::ffi::get_host_vtable(), 1)
        };

        if let Some(logger) = logger {
            if logger.is_valid() {
                let _ = logger.log(format!("Processing input: {}", s));
                let _ = logger.log("Step 1: Analyzing input".to_string());
                let _ = logger.log("Step 2: Transforming data".to_string());
                let _ = logger.log("Step 3: Generating output".to_string());
            }
        }

        alloc_string(&format!("WORKED: {}", s.to_uppercase()))
    }
}

static INIT: std::sync::OnceLock<()> = std::sync::OnceLock::new();
fn init() {
    let _ = INIT.get_or_init(|| {
        let _ = set_worker_impl(Box::new(Plugin));
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn polyplug_abi_version() -> u32 {
    polyplug_guest::POLYPLUG_ABI_VERSION
}

#[unsafe(no_mangle)]
unsafe extern "C" fn polyplug_user_init() {
    init();
}
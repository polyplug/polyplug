use polyplug_guest::{PluginError, StringView, alloc_string, get_host_vtable, to_str};

#[path = "../generated/guest/mod.rs"]
mod generated;

use generated::contracts::DataReporterPlugin;
use generated::host_contract_callers::HostLoggerCaller;
use generated::types::LogLevel;
use generated::interfaces::set_reporter_impl;

struct Plugin;

impl DataReporterPlugin for Plugin {
    fn report(&self, input: StringView) -> Result<StringView, PluginError> {
        let s = to_str(input);

        // Try to get host logger and log messages
        // SAFETY: get_host_vtable() returns a valid pointer or null
        let logger: Option<HostLoggerCaller> =
            unsafe { HostLoggerCaller::from_host(get_host_vtable(), 1) };

        if let Some(ref logger) = logger {
            if logger.is_valid() {
                // Log with different levels - errors are ignored since logging is optional
                let _ = logger.log(format!("[plugin] Starting report for: {}", s));
                let _ = logger
                    .log_with_level(LogLevel::Info, "[plugin] Step 1: Parsing input".to_string());
                let _ = logger.log_with_level(
                    LogLevel::Debug,
                    format!("[plugin] Input length: {}", s.len()),
                );
            }
        }

        let data = s.strip_prefix("TRANSFORMED:").unwrap_or(s);
        let parts: Vec<&str> = data.split('|').collect();

        if let Some(ref logger) = logger {
            if logger.is_valid() {
                let _ = logger.log_with_level(
                    LogLevel::Warn,
                    "[plugin] Step 2: Processing data".to_string(),
                );
            }
        }

        if parts.len() >= 3 {
            if let Some(ref logger) = logger {
                if logger.is_valid() {
                    let _ = logger.log_with_level(
                        LogLevel::Error,
                        "[plugin] Step 3: Finalizing report".to_string(),
                    );
                }
            }

            alloc_string(&format!(
                "Report: {} has value '{}' with count {}",
                parts[0], parts[1], parts[2]
            ))
        } else {
            Err(PluginError {
                code: polyplug_guest::ABI_ERROR_GENERIC,
                message: "invalid format".into(),
            })
        }
    }
}

static INIT: std::sync::OnceLock<()> = std::sync::OnceLock::new();
fn init() {
    let _ = INIT.get_or_init(|| {
        let _ = set_reporter_impl(Box::new(Plugin));
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

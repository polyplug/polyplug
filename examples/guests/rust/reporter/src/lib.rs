use polyplug_abi::StringView;
use polyplug_guest::{GuestError, HostContext, to_str};

#[path = "../generated/guest/mod.rs"]
mod generated;

use generated::contracts::DataReporterGuestContract;
use generated::host_contract_callers::HostLoggerCaller;
use generated::types::LogLevel;

struct Plugin {
    /// Host handle for this runtime, captured at instance creation.
    host: HostContext,
}

impl DataReporterGuestContract for Plugin {
    fn report(&self, input: StringView) -> Result<StringView, GuestError> {
        // SAFETY: `input` is a valid StringView whose bytes stay live for the
        // duration of this call, per the ABI contract for dispatch arguments.
        let s: &str = unsafe { to_str(&input) };

        // Try to get host logger and log messages.
        // min_version is PACKED (major << 16 | minor): request major 1, minor 0.
        // A bare `1` would request major 0 / minor 1 and never match.
        // SAFETY: self.host carries the HostApi pointer captured at instance
        // creation, valid for the runtime's lifetime.
        let logger: Option<HostLoggerCaller> =
            unsafe { HostLoggerCaller::from_host(self.host.as_ptr(), 0x0001_0000) };

        if let Some(ref logger) = logger
            && logger.is_valid()
        {
            // Log with different levels - errors are ignored since logging is optional
            let _ = logger.log(format!("[plugin] Starting report for: {}", s));
            let _ =
                logger.log_with_level(LogLevel::Info, "[plugin] Step 1: Parsing input".to_string());
            let _ = logger.log_with_level(
                LogLevel::Debug,
                format!("[plugin] Input length: {}", s.len()),
            );
        }

        let data: &str = s.strip_prefix("TRANSFORMED:").unwrap_or(s);
        let parts: Vec<&str> = data.split('|').collect();

        if let Some(ref logger) = logger
            && logger.is_valid()
        {
            let _ = logger.log_with_level(
                LogLevel::Warn,
                "[plugin] Step 2: Processing data".to_string(),
            );
        }

        if parts.len() >= 3 {
            if let Some(ref logger) = logger
                && logger.is_valid()
            {
                let _ = logger.log_with_level(
                    LogLevel::Error,
                    "[plugin] Step 3: Finalizing report".to_string(),
                );
            }

            self.host.alloc_string(&format!(
                "Report: {} has value '{}' with count {}",
                parts[0], parts[1], parts[2]
            ))
        } else {
            Err(GuestError {
                code: polyplug_abi::AbiErrorCode::Generic,
                message: "invalid format".into(),
            })
        }
    }
}

/// Factory called by the generated `create_instance` for every host-created
/// instance. The implementation travels in `GuestContractInstance.data`.
#[unsafe(no_mangle)]
pub fn polyplug_create_reporter(host: HostContext) -> Box<dyn DataReporterGuestContract> {
    Box::new(Plugin { host })
}

#[unsafe(no_mangle)]
pub extern "C" fn polyplug_abi_version() -> u32 {
    polyplug_abi::POLYPLUG_ABI_VERSION
}

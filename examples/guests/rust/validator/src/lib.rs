use polyplug_abi::StringView;
use polyplug_guest::{GuestError, HostContext, to_str};

#[path = "../generated/guest/mod.rs"]
mod generated;

use generated::contracts::PipelineValidatorGuestContract;

struct Plugin {
    /// Host handle for this runtime, captured at instance creation.
    host: HostContext,
}

impl PipelineValidatorGuestContract for Plugin {
    fn validate(&self, input: StringView) -> Result<StringView, GuestError> {
        // SAFETY: `input` is a valid StringView whose bytes stay live for the
        // duration of this call, per the ABI contract for dispatch arguments.
        let s: &str = unsafe { to_str(&input) };
        let data: &str = s.strip_prefix("DECODED:").unwrap_or(s);
        let parts: Vec<&str> = data.split('|').collect();
        if parts.len() == 3
            && !parts[0].is_empty()
            && !parts[1].is_empty()
            && parts[2].parse::<i32>().is_ok()
        {
            self.host.alloc_string(&format!("VALID:{}", data))
        } else {
            self.host.alloc_string("INVALID:expected name|value|count")
        }
    }
}

/// Factory called by the generated `create_instance` for every host-created
/// instance. The implementation travels in `GuestContractInstance.data`.
#[unsafe(no_mangle)]
pub fn polyplug_create_validator(host: HostContext) -> Box<dyn PipelineValidatorGuestContract> {
    Box::new(Plugin { host })
}

#[unsafe(no_mangle)]
pub extern "C" fn polyplug_abi_version() -> u32 {
    polyplug_abi::POLYPLUG_ABI_VERSION
}

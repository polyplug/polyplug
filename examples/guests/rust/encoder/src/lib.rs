use polyplug_abi::StringView;
use polyplug_guest::{GuestError, HostContext, to_str};

#[path = "../generated/guest/mod.rs"]
mod generated;

use generated::contracts::PipelineEncoderGuestContract;

struct Plugin {
    /// Host handle for this runtime, captured at instance creation.
    host: HostContext,
}

impl PipelineEncoderGuestContract for Plugin {
    fn encode(&self, input: StringView) -> Result<StringView, GuestError> {
        // SAFETY: `input` is a valid StringView whose bytes stay live for the
        // duration of this call, per the ABI contract for dispatch arguments.
        let s: &str = unsafe { to_str(&input) };
        let data: &str = s.strip_prefix("TRANSFORMED:").unwrap_or(s);
        self.host.alloc_string(&data.replace('|', ","))
    }
}

/// Factory called by the generated `create_instance` for every host-created
/// instance. The implementation travels in `GuestContractInstance.data`.
#[unsafe(no_mangle)]
pub fn polyplug_create_encoder(host: HostContext) -> Box<dyn PipelineEncoderGuestContract> {
    Box::new(Plugin { host })
}

#[unsafe(no_mangle)]
pub extern "C" fn polyplug_abi_version() -> u32 {
    polyplug_abi::POLYPLUG_ABI_VERSION
}

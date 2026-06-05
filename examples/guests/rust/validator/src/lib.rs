use polyplug_guest::{GuestError, StringView, alloc_string, to_str};

#[path = "../generated/guest/mod.rs"]
mod generated;

use generated::contracts::PipelineValidatorGuestContract;
use generated::interfaces::set_validator_impl;

struct Plugin;

impl PipelineValidatorGuestContract for Plugin {
    fn validate(&self, input: StringView) -> Result<StringView, GuestError> {
        // SAFETY: `input` is a valid StringView whose bytes stay live for the
        // duration of this call, per the ABI contract for dispatch arguments.
        let s: &str = unsafe { to_str(&input) };
        let data = s.strip_prefix("DECODED:").unwrap_or(s);
        let parts: Vec<&str> = data.split('|').collect();
        if parts.len() == 3
            && !parts[0].is_empty()
            && !parts[1].is_empty()
            && parts[2].parse::<i32>().is_ok()
        {
            alloc_string(&format!("VALID:{}", data))
        } else {
            alloc_string("INVALID:expected name|value|count")
        }
    }
}

static INIT: std::sync::OnceLock<()> = std::sync::OnceLock::new();
fn init() {
    let _ = INIT.get_or_init(|| {
        let _ = set_validator_impl(Box::new(Plugin));
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

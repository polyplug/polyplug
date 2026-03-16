use polyplug_guest::{PluginError, StringView, to_str, alloc_string};

#[path = "../generated/guest/mod.rs"]
mod generated;

use generated::contracts::PipelineValidatorPlugin;
use generated::vtables::set_validator_impl;

struct Plugin;

impl PipelineValidatorPlugin for Plugin {
    fn validate(&self, input: StringView) -> Result<StringView, PluginError> {
        let s = to_str(input);
        let data = s.strip_prefix("DECODED:").unwrap_or(s);
        let parts: Vec<&str> = data.split('|').collect();
        if parts.len() == 3 && !parts[0].is_empty() && !parts[1].is_empty() && parts[2].parse::<i32>().is_ok() {
            alloc_string(&format!("VALID:{}", data))
        } else {
            alloc_string("INVALID:expected name|value|count")
        }
    }
}

static INIT: std::sync::OnceLock<()> = std::sync::OnceLock::new();
fn init() { let _ = INIT.get_or_init(|| { let _ = set_validator_impl(Box::new(Plugin)); }); }

#[unsafe(no_mangle)]
pub extern "C" fn polyplug_abi_version() -> u32 { polyplug_guest::POLYPLUG_ABI_VERSION }

#[unsafe(no_mangle)]
unsafe extern "C" fn polyplug_user_init() {
    init();
}

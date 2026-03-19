use polyplug_guest::{PluginError, StringView, alloc_string, to_str};

#[path = "../generated/guest/mod.rs"]
mod generated;

use generated::contracts::DataReporterPlugin;
use generated::vtables::set_reporter_impl;

struct Plugin;

impl DataReporterPlugin for Plugin {
    fn report(&self, input: StringView) -> Result<StringView, PluginError> {
        let s = to_str(input);
        let data = s.strip_prefix("TRANSFORMED:").unwrap_or(s);
        let parts: Vec<&str> = data.split('|').collect();
        if parts.len() >= 3 {
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

use polyplug_guest::{PluginError, StringView, alloc_string, to_str};

#[path = "../generated/guest/mod.rs"]
mod generated;

use generated::contracts::DataTransformerPlugin;
use generated::interfaces::set_transformer_impl;

struct Plugin;

impl DataTransformerPlugin for Plugin {
    fn transform(&self, input: StringView) -> Result<StringView, PluginError> {
        let s = to_str(input);
        let data = s.strip_prefix("DECODED:").unwrap_or(s);
        let parts: Vec<&str> = data.split('|').collect();
        if parts.len() >= 3 {
            let name = parts[0].to_uppercase();
            let value = format!("{} (transformed)", parts[1]);
            let count: i32 = parts[2].parse().unwrap_or(0);
            alloc_string(&format!("TRANSFORMED:{}|{}|{}", name, value, count + 1))
        } else {
            Err(PluginError {
                code: polyplug_guest::AbiErrorCode::Generic as u32,
                message: "invalid format".into(),
            })
        }
    }
}

static INIT: std::sync::OnceLock<()> = std::sync::OnceLock::new();
fn init() {
    let _ = INIT.get_or_init(|| {
        let _ = set_transformer_impl(Box::new(Plugin));
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

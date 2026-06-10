use polyplug_abi::StringView;
use polyplug_guest::{GuestError, alloc_string, to_str};

#[path = "../generated/guest/mod.rs"]
mod generated;

use generated::contracts::DataTransformerGuestContract;
use generated::interfaces::set_transformer_impl;

struct Plugin;

impl DataTransformerGuestContract for Plugin {
    fn transform(&self, input: StringView) -> Result<StringView, GuestError> {
        // SAFETY: `input` is a valid StringView whose bytes stay live for the
        // duration of this call, per the ABI contract for dispatch arguments.
        let s: &str = unsafe { to_str(&input) };
        let data = s.strip_prefix("DECODED:").unwrap_or(s);
        let parts: Vec<&str> = data.split('|').collect();
        if parts.len() >= 3 {
            let name = parts[0].to_uppercase();
            let value = format!("{} (transformed)", parts[1]);
            let count: i32 = parts[2].parse().unwrap_or(0);
            alloc_string(&format!("TRANSFORMED:{}|{}|{}", name, value, count + 1))
        } else {
            Err(GuestError {
                code: polyplug_abi::AbiErrorCode::Generic,
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
    polyplug_abi::POLYPLUG_ABI_VERSION
}

#[unsafe(no_mangle)]
unsafe extern "C" fn polyplug_user_init() {
    init();
}

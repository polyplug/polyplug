use polyplug_guest::{GuestError, StringView, alloc_string, to_str};

#[path = "../generated/guest/mod.rs"]
mod generated;

use generated::contracts::PipelineDecoderGuestContract;
use generated::interfaces::set_decoder_impl;

struct Plugin;

impl PipelineDecoderGuestContract for Plugin {
    fn decode(&self, input: StringView) -> Result<StringView, GuestError> {
        let s = to_str(input);
        let decoded = s.replace(',', "|");
        alloc_string(&format!("DECODED:{}", decoded))
    }
}

static INIT: std::sync::OnceLock<()> = std::sync::OnceLock::new();
fn init() {
    let _ = INIT.get_or_init(|| {
        let _ = set_decoder_impl(Box::new(Plugin));
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

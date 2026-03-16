#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(non_snake_case)]
#![allow(clippy::eq_op)]
#![allow(clippy::identity_op)]
#![allow(clippy::borrowed_box)]
//! rust_decoder — Rust native plugin implementing pipeline.Decoder v1.
//!
//! This plugin uses generated code from polyplugc for all ABI types and registration.
//! Contract: decode(input: StringView) -> StringView
//! Input:  "name,value,42"
//! Output: "DECODED:name|value|42"

#[path = "../generated/guest/mod.rs"]
mod guest;

use guest::contracts::PipelineDecoderPlugin;
use guest::vtables::{set_rust_decoder_impl, RUST_DECODER_VTABLE};
use polyplug_guest::{PluginError, StringView};
use std::sync::OnceLock;

const EXT_TRACE_ID: u32 = 0xC4EB9AEE_u32;

#[repr(C)]
struct TraceVTable {
    emit: unsafe extern "C" fn(msg: StringView, state: *const ()),
    state: *const (),
}

unsafe impl Send for TraceVTable {}
unsafe impl Sync for TraceVTable {}

#[derive(Clone, Copy)]
struct TracePtr(*const TraceVTable);

unsafe impl Send for TracePtr {}
unsafe impl Sync for TracePtr {}

static TRACE_CB: OnceLock<Option<TracePtr>> = OnceLock::new();

unsafe fn emit_trace(msg: &str) {
    if let Some(Some(TracePtr(vtable_ptr))) = TRACE_CB.get() {
        let sv = StringView {
            ptr: msg.as_ptr(),
            len: msg.len(),
        };
        unsafe { ((**vtable_ptr).emit)(sv, (**vtable_ptr).state) };
    }
}

struct DecoderPlugin;

impl PipelineDecoderPlugin for DecoderPlugin {
    fn decode(&self, input: StringView) -> Result<StringView, PluginError> {
        let input_bytes: &[u8] = if input.ptr.is_null() || input.len == 0 {
            b""
        } else {
            unsafe { std::slice::from_raw_parts(input.ptr, input.len) }
        };

        let input_str: &str = match std::str::from_utf8(input_bytes) {
            Ok(s) => s,
            Err(_) => {
                return Err(PluginError {
                    code: 1,
                    message: "invalid UTF-8".to_string(),
                });
            }
        };

        let pipe_separated: String = input_str.replace(',', "|");
        let result: String = format!("DECODED:{}", pipe_separated);
        let result_bytes: Vec<u8> = result.into_bytes();
        let leaked: &'static [u8] = Box::leak(result_bytes.into_boxed_slice());

        let result_sv = StringView {
            ptr: leaked.as_ptr(),
            len: leaked.len(),
        };

        unsafe { emit_trace("[rust_decoder] decode called") };

        Ok(result_sv)
    }
}

// The generated code exports polyplug_abi_version and polyplug_init.
// We just need to register our implementation before the generated init runs.
// Since we can't intercept, we register at load time using a static initializer.

static INIT: OnceLock<()> = OnceLock::new();

fn init_plugin() {
    let _ = INIT.get_or_init(|| {
        let _ = set_rust_decoder_impl(Box::new(DecoderPlugin));
    });
}

// Force initialization on load
#[used]
static FORCE_INIT: fn() = init_plugin;

// Export ABI version sentinel
#[unsafe(no_mangle)]
pub extern "C" fn polyplug_abi_version() -> u32 {
    1
}

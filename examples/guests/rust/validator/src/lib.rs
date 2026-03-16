#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(non_snake_case)]
#![allow(clippy::eq_op)]
#![allow(clippy::identity_op)]
#![allow(clippy::borrowed_box)]
//! rust_validator — Rust native plugin implementing pipeline.Validator v1.
//!
//! This plugin uses generated code from polyplugc for all ABI types and registration.
//! Contract: validate(data: StringView) -> StringView
//! Input:  "name,value,42"
//! Output: "VALID:name,value,42" (or error if invalid)

#[path = "../generated/guest/mod.rs"]
mod guest;

use guest::contracts::PipelineValidatorPlugin;
use guest::vtables::set_rust_validator_impl;
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

struct ValidatorPlugin;

impl PipelineValidatorPlugin for ValidatorPlugin {
    fn validate(&self, data: StringView) -> Result<StringView, PluginError> {
        let data_bytes: &[u8] = if data.ptr.is_null() || data.len == 0 {
            b""
        } else {
            unsafe { std::slice::from_raw_parts(data.ptr, data.len) }
        };

        let data_str: &str = match std::str::from_utf8(data_bytes) {
            Ok(s) => s,
            Err(_) => {
                return Err(PluginError {
                    code: 1,
                    message: "invalid UTF-8".to_string(),
                });
            }
        };

        // Simple validation: check for 3 comma-separated fields
        let parts: Vec<&str> = data_str.split(',').collect();
        if parts.len() != 3 {
            return Err(PluginError {
                code: 2,
                message: "invalid format: expected 3 comma-separated fields".to_string(),
            });
        }

        let result: String = format!("VALID:{}", data_str);
        let result_bytes: Vec<u8> = result.into_bytes();
        let leaked: &'static [u8] = Box::leak(result_bytes.into_boxed_slice());

        let result_sv = StringView {
            ptr: leaked.as_ptr(),
            len: leaked.len(),
        };

        unsafe { emit_trace("[rust_validator] validate called") };

        Ok(result_sv)
    }
}

static INIT: OnceLock<()> = OnceLock::new();

fn init_plugin() {
    let _ = INIT.get_or_init(|| {
        let _ = set_rust_validator_impl(Box::new(ValidatorPlugin));
    });
}

#[used]
static FORCE_INIT: fn() = init_plugin;

#[unsafe(no_mangle)]
pub extern "C" fn polyplug_abi_version() -> u32 {
    1
}

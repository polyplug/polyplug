use polyplug_abi::{HostApi, StringView};
use polyplug_guest::{GuestError, HostContext, to_str};

#[path = "../generated/guest/mod.rs"]
mod generated;

use generated::contracts::DataTransformerGuestContract;

struct Plugin {
    /// Host handle for this runtime, captured at instance creation.
    host: HostContext,
}

impl DataTransformerGuestContract for Plugin {
    fn transform(&self, input: StringView) -> Result<StringView, GuestError> {
        // SAFETY: `input` is a valid StringView whose bytes stay live for the
        // duration of this call, per the ABI contract for dispatch arguments.
        let s: &str = unsafe { to_str(&input) };
        let data: &str = s.strip_prefix("DECODED:").unwrap_or(s);
        let parts: Vec<&str> = data.split('|').collect();
        if parts.len() >= 3 {
            let name: String = parts[0].to_uppercase();
            let value: String = format!("{} (transformed)", parts[1]);
            let count: i32 = parts[2].parse().unwrap_or(0);
            self.host
                .alloc_string(&format!("TRANSFORMED:{}|{}|{}", name, value, count + 1))
        } else {
            Err(GuestError {
                code: polyplug_abi::AbiErrorCode::Generic,
                message: "invalid format".into(),
            })
        }
    }
}

/// Factory called by the generated `create_instance` for every host-created
/// instance. The implementation travels in `GuestContractInstance.data`.
#[unsafe(no_mangle)]
pub fn polyplug_create_transformer(host: HostContext) -> Box<dyn DataTransformerGuestContract> {
    Box::new(Plugin { host })
}

#[unsafe(no_mangle)]
pub extern "C" fn polyplug_abi_version() -> u32 {
    polyplug_abi::POLYPLUG_ABI_VERSION
}

// ─── Generated-peer-caller probe ─────────────────────────────────────────────

/// Drives the GENERATED peer caller (`generated/guest/peer_callers.rs`) end to
/// end: resolves the declared `pipeline.Validator` dependency through the host
/// and dispatches `validate` via host-mediated `call_guest_method`.
///
/// This is NOT part of the polyplug ABI and is NOT registered in any interface.
/// The `integration_peer_caller_generated_rust` test resolves it via `dlsym`
/// AFTER the runtime has loaded this bundle, so the generated peer-caller code
/// itself executes — not an inline replica of it.
///
/// `host` is supplied by the caller (the test passes `Runtime::host_abi()`);
/// the plugin holds no process-wide host storage.
///
/// # Safety
/// `host` must be a valid `HostApi` pointer for the owning runtime. `out` must
/// be a valid non-null pointer to a StringView slot. The runtime must have
/// loaded this bundle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_test_peer_validate(
    host: *const HostApi,
    input: StringView,
    out: *mut StringView,
) -> u32 {
    if host.is_null() || out.is_null() {
        return polyplug_abi::AbiErrorCode::InvalidPointer as u32;
    }
    // SAFETY: host is non-null (checked above) and valid per this function's contract.
    let host_ctx: HostContext = unsafe { HostContext::new(host) };
    let mut peer: generated::peer_callers::PipelineValidatorContractPeer =
        match generated::peer_callers::PipelineValidatorContractPeer::resolve(host_ctx) {
            Some(p) => p,
            None => return polyplug_abi::AbiErrorCode::NotFound as u32,
        };
    match peer.validate(input) {
        Ok(view) => {
            // `view` borrows the peer caller's arena, which dies with `peer` at
            // the end of this function — copy into host-allocated memory so the
            // out slot outlives this call.
            // SAFETY: `view` is a valid UTF-8 StringView produced by the
            // validator guest, live until the peer's next arena-backed call.
            let s: &str = unsafe { to_str(&view) };
            match host_ctx.alloc_string(s) {
                Ok(stable) => {
                    // SAFETY: out is non-null per this function's contract.
                    unsafe { *out = stable };
                    polyplug_abi::AbiErrorCode::Ok as u32
                }
                Err(e) => e.code as u32,
            }
        }
        Err(e) => e.code as u32,
    }
}

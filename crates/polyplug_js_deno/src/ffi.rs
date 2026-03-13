//! FFI exports for polyplug_js_deno — `polyplug_js_deno_loader_create` and `polyplug_js_deno_loader_free`.

use std::ffi::c_void;

use crate::JsDenoConfig;
use crate::JsDenoLoader;

/// Opaque C-compatible config for the JS Deno loader (no required fields).
#[repr(C)]
pub struct PolyplugJsDenoConfig {
    pub _reserved: u8,
}

/// Create a new `JsDenoLoader` and return an opaque pointer.
///
/// `config` may be null — V8 is embedded in-process, no config required.
/// Free with `polyplug_js_deno_loader_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_js_deno_loader_create(
    config: *const PolyplugJsDenoConfig,
) -> *mut c_void {
    let _ = config;
    let loader: JsDenoLoader = JsDenoLoader::new(JsDenoConfig::default());
    Box::into_raw(Box::new(loader)) as *mut c_void
}

/// Free a `JsDenoLoader` created by `polyplug_js_deno_loader_create`.
///
/// # Safety
/// `ptr` must be a non-freed pointer returned by `polyplug_js_deno_loader_create`.
/// Passing null is a no-op.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_js_deno_loader_free(ptr: *mut c_void) {
    if ptr.is_null() {
        return;
    }
    // SAFETY: ptr is a valid Box<JsDenoLoader> from polyplug_js_deno_loader_create.
    unsafe { drop(Box::<JsDenoLoader>::from_raw(ptr as *mut JsDenoLoader)) };
}

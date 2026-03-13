use std::ffi::c_void;

use crate::JsConfig;
use crate::JsLoader;

#[repr(C)]
pub struct PolyplugJsConfig {
    pub _reserved: u8,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_js_loader_create(config: *const PolyplugJsConfig) -> *mut c_void {
    // Config is optional for JS (no required fields)
    let _ = config; // may be null, we don't need it
    let loader: JsLoader = JsLoader::new(JsConfig {});
    Box::into_raw(Box::new(loader)) as *mut c_void
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_js_loader_free(ptr: *mut c_void) {
    if ptr.is_null() {
        return;
    }
    // SAFETY: ptr was returned by polyplug_js_loader_create, which used Box::into_raw.
    // Caller guarantees ptr is not used after this call.
    unsafe { drop(Box::<JsLoader>::from_raw(ptr as *mut JsLoader)) };
}

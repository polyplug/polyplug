use core::ffi::c_void;

use polyplug::loader::BundleLoader;

use crate::{JsConfig, JsLoader};

#[repr(C)]
pub struct PolyplugJsConfig {
    pub _reserved: u8,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_js_loader_create(config: *const PolyplugJsConfig) -> *mut c_void {
    let _ = config;
    let loader: JsLoader = JsLoader::new(JsConfig {});
    // Double-box: inner Box<dyn BundleLoader> preserves the fat pointer (data + vtable),
    // outer Box stores it on the heap so we can pass a thin *mut c_void across FFI.
    let trait_obj: Box<dyn BundleLoader> = Box::new(loader);
    Box::into_raw(Box::new(trait_obj)) as *mut c_void
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_js_loader_free(ptr: *mut c_void) {
    if ptr.is_null() {
        return;
    }
    // SAFETY: ptr was returned by polyplug_js_loader_create via
    // Box::into_raw(Box::new(trait_obj)) where trait_obj: Box<dyn BundleLoader>.
    // Caller guarantees ptr is not used after this call.
    unsafe {
        drop(Box::<Box<dyn BundleLoader>>::from_raw(
            ptr as *mut Box<dyn BundleLoader>,
        ))
    };
}

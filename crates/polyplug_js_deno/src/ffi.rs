//! FFI exports for polyplug_js_deno — `polyplug_js_deno_loader_create` and `polyplug_js_deno_loader_free`.

use core::ffi::c_void;

use polyplug::loader::BundleLoader;

use crate::{JsDenoConfig, JsDenoLoader};

/// Opaque C-compatible config for the JS Deno loader (no required fields).
#[repr(C)]
pub struct PolyplugJsDenoConfig {
    pub _reserved: u8,
}

/// Create a new `JsDenoLoader` and return an opaque pointer.
///
/// `config` may be null — V8 is embedded in-process, no config required.
/// Free with `polyplug_js_deno_loader_free`.
///
/// # Safety
/// `config` may be null. The returned pointer must be freed with `polyplug_js_deno_loader_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_js_deno_loader_create(
    config: *const PolyplugJsDenoConfig,
) -> *mut c_void {
    let _ = config;
    let loader: JsDenoLoader = JsDenoLoader::new(JsDenoConfig::default());
    // Double-box: inner Box<dyn BundleLoader> preserves the fat pointer (data + vtable),
    // outer Box stores it on the heap so we can pass a thin *mut c_void across FFI.
    let trait_obj: Box<dyn BundleLoader> = Box::new(loader);
    Box::into_raw(Box::new(trait_obj)) as *mut c_void
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
    // SAFETY: ptr was produced by polyplug_js_deno_loader_create via
    // Box::into_raw(Box::new(trait_obj)) where trait_obj: Box<dyn BundleLoader>.
    unsafe {
        drop(Box::<Box<dyn BundleLoader>>::from_raw(
            ptr as *mut Box<dyn BundleLoader>,
        ))
    };
}

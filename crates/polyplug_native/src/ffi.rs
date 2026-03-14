use core::ffi::c_void;

use polyplug::loader::BundleLoader;

use crate::{NativeConfig, NativeLoader};

#[repr(C)]
pub struct PolyplugNativeConfig {
    pub _reserved: u8,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_native_loader_create(
    config: *const PolyplugNativeConfig,
) -> *mut c_void {
    // Config is optional for native (no required fields)
    let _ = config; // may be null, we don't need it
    let loader: NativeLoader = NativeLoader::new(NativeConfig::default());
    // Double-box: inner Box<dyn BundleLoader> preserves the fat pointer (data + vtable),
    // outer Box stores it on the heap so we can pass a thin *mut c_void across FFI.
    // polyplug_runtime_register_loader reconstitutes via Box::from_raw(*mut Box<dyn BundleLoader>).
    let trait_obj: Box<dyn BundleLoader> = Box::new(loader);
    Box::into_raw(Box::new(trait_obj)) as *mut c_void
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_native_loader_free(ptr: *mut c_void) {
    if ptr.is_null() {
        return;
    }
    // SAFETY: ptr was produced by polyplug_native_loader_create via
    // Box::into_raw(Box::new(trait_obj)) where trait_obj: Box<dyn BundleLoader>.
    // The caller guarantees ptr is not used after this call.
    drop(unsafe { Box::<Box<dyn BundleLoader>>::from_raw(ptr as *mut Box<dyn BundleLoader>) });
}

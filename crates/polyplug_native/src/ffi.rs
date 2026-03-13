use crate::{NativeConfig, NativeLoader};
use std::ffi::c_void;

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
    Box::into_raw(Box::new(loader)) as *mut c_void
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_native_loader_free(ptr: *mut c_void) {
    if ptr.is_null() {
        return;
    }
    // SAFETY: ptr was produced by polyplug_native_loader_create via Box::into_raw.
    // The caller guarantees ptr is not used after this call.
    drop(unsafe { Box::<NativeLoader>::from_raw(ptr as *mut NativeLoader) });
}

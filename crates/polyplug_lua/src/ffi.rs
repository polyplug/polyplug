//! FFI exports for polyplug_lua — `polyplug_lua_loader_create` and `polyplug_lua_loader_free`.

use core::ffi::c_void;

use polyplug::loader::BundleLoader;

use crate::{LuaConfig, LuaLoader};

#[repr(C)]
pub struct PolyplugLuaConfig {
    pub _reserved: u8,
}

/// # Safety
/// `config` may be null. The returned pointer must be freed with `polyplug_lua_loader_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_lua_loader_create(
    config: *const PolyplugLuaConfig,
) -> *mut c_void {
    let _ = config;
    let loader: LuaLoader = LuaLoader::new(LuaConfig::default());
    // Double-box: inner Box<dyn BundleLoader> preserves the fat pointer (data + vtable),
    // outer Box stores it on the heap so we can pass a thin *mut c_void across FFI.
    let trait_obj: Box<dyn BundleLoader> = Box::new(loader);
    Box::into_raw(Box::new(trait_obj)) as *mut c_void
}

/// # Safety
/// `ptr` must be a non-freed pointer returned by `polyplug_lua_loader_create`, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_lua_loader_free(ptr: *mut c_void) {
    if ptr.is_null() {
        return;
    }
    // SAFETY: ptr was produced by polyplug_lua_loader_create via
    // Box::into_raw(Box::new(trait_obj)) where trait_obj: Box<dyn BundleLoader>.
    // The caller guarantees ptr is not used after this call.
    drop(unsafe { Box::<Box<dyn BundleLoader>>::from_raw(ptr as *mut Box<dyn BundleLoader>) });
}

use crate::{LuaConfig, LuaLoader};
use std::ffi::c_void;

#[repr(C)]
pub struct PolyplugLuaConfig {
    pub _reserved: u8,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_lua_loader_create(
    config: *const PolyplugLuaConfig,
) -> *mut c_void {
    // Config is optional for Lua (no required fields)
    let _ = config; // may be null, we don't need it
    let loader: LuaLoader = LuaLoader::new(LuaConfig::default());
    Box::into_raw(Box::new(loader)) as *mut c_void
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_lua_loader_free(ptr: *mut c_void) {
    if ptr.is_null() {
        return;
    }
    // SAFETY: ptr was produced by polyplug_lua_loader_create via Box::into_raw.
    // The caller guarantees ptr is not used after this call.
    drop(unsafe { Box::<LuaLoader>::from_raw(ptr as *mut LuaLoader) });
}

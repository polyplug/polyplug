use core::ffi::c_void;

use polyplug::loader::BundleLoader;

use crate::{PythonLoader, config::PythonConfig};

/// C-visible configuration passed to `polyplug_python_loader_create`.
///
/// `min_version_ptr` must point to a valid UTF-8 string of `min_version_len`
/// bytes in the form `"<major>.<minor>"` (e.g. `"3.11"`).
#[repr(C)]
pub struct PolyplugPythonConfig {
    pub min_version_ptr: *const u8,
    pub min_version_len: usize,
}

/// Create a heap-allocated `PythonLoader` and return it as an opaque pointer.
///
/// Returns `null` on any error (null config pointer, null version pointer,
/// non-UTF-8 string, unparseable version string).
///
/// # Safety
/// - `config` must be a valid, non-null pointer to a `PolyplugPythonConfig`.
/// - `config.min_version_ptr` must point to at least `config.min_version_len`
///   readable bytes of valid UTF-8 for the duration of this call.
/// - The returned pointer must be freed by calling `polyplug_python_loader_free`
///   exactly once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_python_loader_create(
    config: *const PolyplugPythonConfig,
) -> *mut c_void {
    if config.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: caller guarantees config is a valid, non-null pointer.
    let cfg: &PolyplugPythonConfig = unsafe { &*config };
    if cfg.min_version_ptr.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: caller guarantees min_version_ptr points to min_version_len
    // valid bytes for the duration of this call.
    let bytes: &[u8] =
        unsafe { std::slice::from_raw_parts(cfg.min_version_ptr, cfg.min_version_len) };
    let version_str: &str = match std::str::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let min_version: (u32, u32) = match parse_version(version_str) {
        Some(v) => v,
        None => return std::ptr::null_mut(),
    };
    let loader: PythonLoader = PythonLoader::new(PythonConfig { min_version });
    // Double-box: inner Box<dyn BundleLoader> preserves the fat pointer (data + vtable),
    // outer Box stores it on the heap so we can pass a thin *mut c_void across FFI.
    let trait_obj: Box<dyn BundleLoader> = Box::new(loader);
    Box::into_raw(Box::new(trait_obj)) as *mut c_void
}

/// Free a `PythonLoader` previously returned by `polyplug_python_loader_create`.
///
/// Passing `null` is a no-op (safe). Passing any other pointer not returned by
/// `polyplug_python_loader_create` is undefined behaviour.
///
/// # Safety
/// - `ptr` must be either null or a pointer previously returned by
///   `polyplug_python_loader_create` and not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_python_loader_free(ptr: *mut c_void) {
    if ptr.is_null() {
        return;
    }
    // SAFETY: ptr was produced by Box::into_raw(Box::new(trait_obj)) inside
    // polyplug_python_loader_create where trait_obj: Box<dyn BundleLoader>.
    // Caller guarantees it has not been freed.
    unsafe {
        drop(Box::<Box<dyn BundleLoader>>::from_raw(
            ptr as *mut Box<dyn BundleLoader>,
        ))
    };
}

fn parse_version(s: &str) -> Option<(u32, u32)> {
    let mut parts = s.splitn(2, '.');
    let major: u32 = parts.next()?.parse().ok()?;
    let minor: u32 = parts.next()?.parse().ok()?;
    Some((major, minor))
}

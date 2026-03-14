use core::ffi::c_void;

use polyplug::loader::BundleLoader;

use crate::{DotnetConfig, DotnetLoader, config::HostfxrLocation};

/// C-visible configuration passed to `polyplug_dotnet_loader_create`.
///
/// `min_framework_ptr` must point to a valid UTF-8 byte sequence of length
/// `min_framework_len`. The buffer does not need to be null-terminated.
#[repr(C)]
pub struct PolyplugDotnetConfig {
    pub min_framework_ptr: *const u8,
    pub min_framework_len: usize,
}

/// Create a heap-allocated `DotnetLoader` and return it as an opaque `*mut c_void`.
///
/// Returns `null` on any error (null pointer, non-UTF-8 framework string).
/// The caller must eventually pass the returned pointer to
/// `polyplug_dotnet_loader_free` to avoid a memory leak.
///
/// # Safety
/// - `config` must be a valid, non-null pointer to a `PolyplugDotnetConfig`.
/// - `config.min_framework_ptr` must be a valid pointer to at least
///   `config.min_framework_len` initialised bytes of UTF-8 text.
/// - Both pointers must remain valid for the duration of this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_dotnet_loader_create(
    config: *const PolyplugDotnetConfig,
) -> *mut c_void {
    if config.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: caller guarantees config is a valid, non-null pointer.
    let cfg: &PolyplugDotnetConfig = unsafe { &*config };
    if cfg.min_framework_ptr.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: caller guarantees min_framework_ptr points to min_framework_len
    // valid, initialised bytes for the duration of this call.
    let bytes: &[u8] =
        unsafe { std::slice::from_raw_parts(cfg.min_framework_ptr, cfg.min_framework_len) };
    let min_framework: String = match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => return std::ptr::null_mut(),
    };
    let loader: DotnetLoader = DotnetLoader::new(DotnetConfig {
        min_framework,
        hostfxr: HostfxrLocation::Auto,
    });
    // Double-box: inner Box<dyn BundleLoader> is a fat pointer (data + vtable),
    // outer Box stores it on the heap so we can pass a thin *mut c_void across FFI.
    // polyplug_runtime_register_loader reconstitutes via Box::from_raw(*mut Box<dyn BundleLoader>).
    let trait_obj: Box<dyn BundleLoader> = Box::new(loader);
    Box::into_raw(Box::new(trait_obj)) as *mut c_void
}

/// Free a `DotnetLoader` previously created by `polyplug_dotnet_loader_create`.
///
/// Passing `null` is a no-op.
///
/// # Safety
/// - `ptr` must be a pointer previously returned by
///   `polyplug_dotnet_loader_create`, or null.
/// - Must not be called more than once for the same pointer.
/// - Must not be used after this call returns.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_dotnet_loader_free(ptr: *mut c_void) {
    if ptr.is_null() {
        return;
    }
    // SAFETY: ptr was created by Box::into_raw(Box::new(trait_obj)) in
    // polyplug_dotnet_loader_create where trait_obj: Box<dyn BundleLoader>.
    // Caller guarantees it has not been freed yet.
    drop(unsafe { Box::<Box<dyn BundleLoader>>::from_raw(ptr as *mut Box<dyn BundleLoader>) });
}

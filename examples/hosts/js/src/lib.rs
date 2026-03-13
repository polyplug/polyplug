// examples/hosts/js/src/lib.rs
// polyplug_full — cdylib wrapper providing polyplug FFI with all language loaders.
//
// This crate provides the same `polyplug_*` FFI surface as the core `polyplug` crate
// but creates the runtime with all language loaders (Python, Lua, JS/QuickJS, .NET)
// registered. Use this shared library with the polyplug Deno host-lib when you need
// to load non-native (interpreted-language) plugins.
//
// Build: cargo build --manifest-path examples/hosts/js/Cargo.toml
// Output: examples/hosts/js/target/debug/libpolyplug_full.so

use core::cell::Ref;
use core::cell::RefCell;
use std::path::Path;

use polyplug::abi::PluginHandle;
use polyplug::registry::PluginVTableGuard;
use polyplug::runtime::Runtime;
use polyplug_dotnet::DotnetConfig;
use polyplug_dotnet::DotnetLoader;
use polyplug_js::JsConfig;
use polyplug_js::JsLoader;
use polyplug_lua::LuaConfig;
use polyplug_lua::LuaLoader;
use polyplug_python::PythonConfig;
use polyplug_python::PythonLoader;

// ─── Global Init: ensure our symbols are in the global namespace ─────────────
// When loaded via Deno.dlopen (RTLD_LOCAL), native guest plugins cannot find
// polyplug_host_alloc/free at load time. We fix this by promoting our own
// library to RTLD_GLOBAL on startup via dlopen(current_path, RTLD_LAZY|RTLD_GLOBAL|RTLD_NOLOAD).

#[cfg(unix)]
#[used]
#[unsafe(link_section = ".init_array")]
static _INIT: unsafe extern "C" fn() = {
    unsafe extern "C" fn promote_to_global() {
        // Re-open this library with RTLD_GLOBAL so native guest plugins can find
        // polyplug_host_alloc and polyplug_host_free when they are dlopen'd.
        // RTLD_NOLOAD means we don't load a new copy — just change the flags.
        // SAFETY: Calling dlopen with a null path returns the main program handle.
        // We use /proc/self/exe or dladdr to find our own path. As a simpler
        // alternative, dlopen(null, RTLD_GLOBAL) makes the main executable's
        // symbols global. But we need our OWN symbols global.
        // Simplest: open with RTLD_LAZY|RTLD_GLOBAL after we're already loaded.
        unsafe extern "C" {
            fn dlopen(filename: *const i8, flags: i32) -> *mut ();
        }
        // RTLD_LAZY=1, RTLD_GLOBAL=0x100
        const RTLD_LAZY: i32 = 1;
        const RTLD_GLOBAL: i32 = 0x100;
        const RTLD_NOLOAD: i32 = 0x4;
        // Open ourselves with RTLD_GLOBAL | RTLD_NOLOAD to promote to global.
        // We need our own path. Use dladdr to find it.
        unsafe extern "C" {
            fn dladdr(addr: *const (), info: *mut DlInfo) -> i32;
        }
        #[repr(C)]
        struct DlInfo {
            fname: *const i8,
            fbase: *mut (),
            sname: *const i8,
            saddr: *mut (),
        }
        let mut info: DlInfo = DlInfo {
            fname: core::ptr::null(),
            fbase: core::ptr::null_mut(),
            sname: core::ptr::null(),
            saddr: core::ptr::null_mut(),
        };
        // Use our own function address as anchor for dladdr.
        let our_fn_addr: *const () = promote_to_global as *const ();
        // SAFETY: Our own function address is valid; DlInfo struct layout matches
        // the C Dl_info struct. dladdr is a safe lookup function.
        let rv: i32 = unsafe { dladdr(our_fn_addr, &mut info) };
        if rv != 0 && !info.fname.is_null() {
            // SAFETY: fname is a valid C string from the dynamic linker, valid for
            // the lifetime of the process. RTLD_NOLOAD prevents a new mapping.
            unsafe { dlopen(info.fname, RTLD_LAZY | RTLD_GLOBAL | RTLD_NOLOAD); }
        }
    }
    promote_to_global
};

thread_local! {
    static LAST_ERROR: RefCell<String> = const { RefCell::new(String::new()) };
}

fn set_last_error(msg: impl Into<String>) {
    LAST_ERROR.with(|e| *e.borrow_mut() = msg.into());
}

// ─── Opaque Types ─────────────────────────────────────────────────────────────

pub struct OpaqueRuntime(Runtime);
pub struct OpaqueGuard(PluginVTableGuard);

// ─── Handle Packing ──────────────────────────────────────────────────────────

fn pack_handle(h: PluginHandle) -> u64 {
    if h.is_null() {
        u64::MAX
    } else {
        (h.generation as u64) << 32 | h.index as u64
    }
}

fn unpack_handle(packed: u64) -> PluginHandle {
    if packed == u64::MAX {
        PluginHandle::null()
    } else {
        PluginHandle {
            index: (packed & 0xFFFF_FFFF) as u32,
            generation: (packed >> 32) as u32,
        }
    }
}

// ─── FFI: Runtime ────────────────────────────────────────────────────────────

/// Create a new polyplug runtime with all language loaders registered.
///
/// # Safety
/// Returns an opaque pointer on success, null on failure.
/// Caller must free with `polyplug_runtime_free`.
#[allow(clippy::std_instead_of_core)]
#[no_mangle]
pub unsafe extern "C" fn polyplug_runtime_new() -> *mut OpaqueRuntime {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let build_result: Result<Runtime, _> = Runtime::builder()
            .loader(PythonLoader::new(PythonConfig::default()))
            .loader(LuaLoader::new(LuaConfig::default()))
            .loader(JsLoader::new(JsConfig {}))
            .loader(DotnetLoader::new(DotnetConfig::default()))
            .build();
        match build_result {
            Ok(rt) => Box::into_raw(Box::new(OpaqueRuntime(rt))),
            Err(e) => {
                set_last_error(e.to_string());
                core::ptr::null_mut()
            }
        }
    }))
    .unwrap_or_else(|_| {
        set_last_error("panic in polyplug_runtime_new");
        core::ptr::null_mut()
    })
}

/// Free a runtime created by `polyplug_runtime_new`.
///
/// # Safety
/// `rt` must be a non-null pointer previously returned by `polyplug_runtime_new`.
/// Must not be called more than once for the same pointer.
#[allow(clippy::std_instead_of_core)]
#[no_mangle]
pub unsafe extern "C" fn polyplug_runtime_free(rt: *mut OpaqueRuntime) {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if !rt.is_null() {
            // SAFETY: rt was allocated by polyplug_runtime_new via Box::new.
            // Caller guarantees single call per pointer.
            drop(unsafe { Box::from_raw(rt) });
        }
    }))
    .unwrap_or_else(|_| {
        set_last_error("panic in polyplug_runtime_free");
    });
}

// ─── FFI: Bundle Loading ──────────────────────────────────────────────────────

/// Load a plugin bundle from `path` (a directory containing `manifest.toml`).
///
/// # Safety
/// `rt` must be valid. `path` must point to `path_len` valid UTF-8 bytes.
#[allow(clippy::std_instead_of_core)]
#[no_mangle]
pub unsafe extern "C" fn polyplug_load_bundle(
    rt: *mut OpaqueRuntime,
    path: *const u8,
    path_len: usize,
) -> u32 {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if rt.is_null() {
            set_last_error("null runtime");
            return 1u32;
        }
        // SAFETY: rt is non-null valid OpaqueRuntime per ABI contract.
        let runtime: &OpaqueRuntime = unsafe { &*rt };
        if path.is_null() {
            set_last_error("null path pointer in polyplug_load_bundle");
            return 1u32;
        }
        // SAFETY: path points to path_len valid UTF-8 bytes per ABI contract.
        let bytes: &[u8] = unsafe { core::slice::from_raw_parts(path, path_len) };
        let s: &str = match core::str::from_utf8(bytes) {
            Ok(s) => s,
            Err(e) => {
                set_last_error(e.to_string());
                return 1u32;
            }
        };
        match runtime.0.load_bundle(Path::new(s)) {
            Ok(()) => 0u32,
            Err(e) => {
                set_last_error(e.to_string());
                1u32
            }
        }
    }))
    .unwrap_or_else(|_| {
        set_last_error("panic in polyplug_load_bundle");
        1u32
    })
}

/// Reload a bundle (hot-reload).
///
/// # Safety
/// `rt` must be valid. `path` must point to `path_len` valid UTF-8 bytes.
#[allow(clippy::std_instead_of_core)]
#[no_mangle]
pub unsafe extern "C" fn polyplug_reload_bundle(
    rt: *mut OpaqueRuntime,
    path: *const u8,
    path_len: usize,
) -> u32 {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if rt.is_null() {
            set_last_error("null runtime");
            return 1u32;
        }
        // SAFETY: rt is non-null valid OpaqueRuntime per ABI contract.
        let runtime: &OpaqueRuntime = unsafe { &*rt };
        if path.is_null() {
            set_last_error("null path pointer in polyplug_reload_bundle");
            return 1u32;
        }
        // SAFETY: path points to path_len valid UTF-8 bytes per ABI contract.
        let bytes: &[u8] = unsafe { core::slice::from_raw_parts(path, path_len) };
        let s: &str = match core::str::from_utf8(bytes) {
            Ok(s) => s,
            Err(e) => {
                set_last_error(e.to_string());
                return 1u32;
            }
        };
        match runtime.0.reload_bundle(Path::new(s)) {
            Ok(()) => 0u32,
            Err(e) => {
                set_last_error(e.to_string());
                1u32
            }
        }
    }))
    .unwrap_or_else(|_| {
        set_last_error("panic in polyplug_reload_bundle");
        1u32
    })
}

// ─── FFI: Plugin Lookup ───────────────────────────────────────────────────────

/// Find the first plugin handle for a given contract ID.
///
/// # Safety
/// `rt` must be valid.
#[allow(clippy::std_instead_of_core)]
#[no_mangle]
pub unsafe extern "C" fn polyplug_rt_find_by_contract(
    rt: *const OpaqueRuntime,
    contract_id: u64,
    min_version: u32,
) -> u64 {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if rt.is_null() {
            return u64::MAX;
        }
        // SAFETY: rt is non-null valid OpaqueRuntime per ABI contract.
        let runtime: &OpaqueRuntime = unsafe { &*rt };
        match runtime.0.find_by_contract(contract_id, min_version) {
            Ok(h) => pack_handle(h),
            Err(_) => u64::MAX,
        }
    }))
    .unwrap_or_else(|_| {
        set_last_error("panic in polyplug_rt_find_by_contract");
        u64::MAX
    })
}

/// Find a plugin handle by bundle ID and contract ID.
///
/// # Safety
/// `rt` must be valid.
#[allow(clippy::std_instead_of_core)]
#[no_mangle]
pub unsafe extern "C" fn polyplug_rt_find_by_bundle(
    rt: *const OpaqueRuntime,
    bundle_id: u64,
    contract_id: u64,
    min_version: u32,
) -> u64 {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if rt.is_null() {
            return u64::MAX;
        }
        // SAFETY: rt is non-null valid OpaqueRuntime per ABI contract.
        let runtime: &OpaqueRuntime = unsafe { &*rt };
        match runtime
            .0
            .find_by_bundle(bundle_id, contract_id, min_version)
        {
            Ok(h) => pack_handle(h),
            Err(_) => u64::MAX,
        }
    }))
    .unwrap_or_else(|_| {
        set_last_error("panic in polyplug_rt_find_by_bundle");
        u64::MAX
    })
}

/// Fill `out` with up to `out_cap` plugin handles for a given contract.
///
/// # Safety
/// `rt` must be valid. `out` must be valid for `out_cap` u64 elements.
#[allow(clippy::std_instead_of_core)]
#[no_mangle]
pub unsafe extern "C" fn polyplug_rt_find_all_by_contract(
    rt: *const OpaqueRuntime,
    contract_id: u64,
    min_version: u32,
    out: *mut u64,
    out_cap: usize,
) -> usize {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if rt.is_null() {
            return 0usize;
        }
        if out.is_null() && out_cap > 0 {
            set_last_error("null out with non-zero cap in polyplug_rt_find_all_by_contract");
            return 0usize;
        }
        // SAFETY: rt is non-null valid OpaqueRuntime per ABI contract.
        let runtime: &OpaqueRuntime = unsafe { &*rt };
        if out_cap == 0usize {
            return 0usize;
        }
        let mut handle_buf: [PluginHandle; 16] = [PluginHandle {
            index: 0u32,
            generation: 0u32,
        }; 16];
        let mut total_written: usize = 0usize;
        loop {
            let remaining: usize = out_cap - total_written;
            if remaining == 0usize {
                break;
            }
            let write_cap: usize = if remaining < handle_buf.len() {
                remaining
            } else {
                handle_buf.len()
            };
            let count: usize = runtime.0.find_all_by_contract(
                contract_id,
                min_version,
                &mut handle_buf[..write_cap],
            );
            for (offset, handle) in handle_buf[..count].iter().enumerate() {
                // SAFETY: out is valid for out_cap u64 elements per ABI contract.
                unsafe {
                    out.add(total_written + offset).write(pack_handle(*handle));
                }
            }
            total_written += count;
            if count < write_cap {
                break;
            }
        }
        total_written
    }))
    .unwrap_or_else(|_| {
        set_last_error("panic in polyplug_rt_find_all_by_contract");
        0usize
    })
}

// ─── FFI: Plugin Resolution ───────────────────────────────────────────────────

/// Resolve a packed plugin handle to a guard.
///
/// # Safety
/// `rt` must be valid.
#[allow(clippy::std_instead_of_core)]
#[no_mangle]
pub unsafe extern "C" fn polyplug_rt_resolve_plugin(
    rt: *const OpaqueRuntime,
    packed_handle: u64,
) -> *mut OpaqueGuard {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if rt.is_null() {
            set_last_error("null runtime");
            return core::ptr::null_mut();
        }
        const NULL_HANDLE: u64 = u64::MAX;
        if packed_handle == NULL_HANDLE {
            return core::ptr::null_mut();
        }
        let handle: PluginHandle = unpack_handle(packed_handle);
        // SAFETY: rt is non-null valid OpaqueRuntime per ABI contract.
        let runtime: &OpaqueRuntime = unsafe { &*rt };
        match runtime.0.registry().resolve_guard(handle) {
            Ok(guard) => Box::into_raw(Box::new(OpaqueGuard(guard))),
            Err(e) => {
                set_last_error(e.to_string());
                core::ptr::null_mut()
            }
        }
    }))
    .unwrap_or_else(|_| {
        set_last_error("panic in polyplug_rt_resolve_plugin");
        core::ptr::null_mut()
    })
}

/// Free a guard returned by `polyplug_rt_resolve_plugin`.
///
/// # Safety
/// `guard` must be a non-null pointer returned by `polyplug_rt_resolve_plugin`.
/// Must not be called more than once.
#[allow(clippy::std_instead_of_core)]
#[no_mangle]
pub unsafe extern "C" fn polyplug_guard_free(guard: *mut OpaqueGuard) {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if !guard.is_null() {
            // SAFETY: guard was allocated by polyplug_rt_resolve_plugin via Box::new.
            // Caller guarantees single call per pointer.
            drop(unsafe { Box::from_raw(guard) });
        }
    }))
    .unwrap_or_else(|_| {
        set_last_error("panic in polyplug_guard_free");
    });
}

/// Get the vtable pointer from a guard.
///
/// # Safety
/// `guard` must be valid.
#[allow(clippy::std_instead_of_core)]
#[no_mangle]
pub unsafe extern "C" fn polyplug_get_vtable(guard: *const OpaqueGuard) -> *const () {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if guard.is_null() {
            set_last_error("null guard");
            return core::ptr::null();
        }
        // SAFETY: guard is non-null valid OpaqueGuard per ABI contract.
        unsafe { (*guard).0.vtable() as *const () }
    }))
    .unwrap_or_else(|_| {
        set_last_error("panic in polyplug_get_vtable");
        core::ptr::null()
    })
}

// ─── FFI: Error Handling ──────────────────────────────────────────────────────

/// Read the last error message into `buf`. Returns bytes written.
///
/// # Safety
/// `buf` must be valid for `buf_len` bytes when non-null.
#[allow(clippy::std_instead_of_core)]
#[no_mangle]
pub unsafe extern "C" fn polyplug_last_error(buf: *mut u8, buf_len: usize) -> usize {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let len: usize = LAST_ERROR.with(|e| {
            let msg: Ref<'_, String> = e.borrow();
            let bytes: &[u8] = msg.as_bytes();
            let write_n: usize = bytes.len().min(buf_len);
            if !buf.is_null() && write_n > 0 {
                // SAFETY: buf is valid for buf_len bytes per ABI contract.
                unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, write_n) };
            }
            write_n
        });
        LAST_ERROR.with(|e| e.borrow_mut().clear());
        len
    }))
    .unwrap_or_else(|_| {
        set_last_error("panic in polyplug_last_error");
        0usize
    })
}

/// Get the length of the last error message.
///
/// # Safety
/// Safe to call from any thread.
#[allow(clippy::std_instead_of_core)]
#[no_mangle]
pub unsafe extern "C" fn polyplug_error_message_len() -> usize {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        LAST_ERROR.with(|e| e.borrow().len())
    }))
    .unwrap_or_else(|_| {
        set_last_error("panic in polyplug_error_message_len");
        0usize
    })
}

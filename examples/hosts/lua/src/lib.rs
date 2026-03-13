//! examples/hosts/lua/src/lib.rs
//! Companion cdylib for the Lua host example.
//!
//! Exports `polyplug_runtime_new_full()` — a variant of the standard
//! `polyplug_runtime_new()` that builds a Runtime with ALL language loaders
//! registered (native, Lua, Python, JS-QuickJS, .NET).
//!
//! This is needed because `libpolyplug.so` (crates/polyplug) only has the
//! NativeBundleLoader. Non-Rust hosts using the FFI need this full-loader
//! runtime to load Python, Lua, JS, and C# guest plugins.
//!
//! host.lua loads this cdylib instead of bare libpolyplug.so.

use polyplug::ffi::OpaqueRuntime;
use polyplug::runtime::Runtime;
use polyplug_dotnet::DotnetConfig;
use polyplug_dotnet::DotnetLoader;
use polyplug_js::JsConfig;
use polyplug_js::JsLoader;
use polyplug_lua::LuaConfig;
use polyplug_lua::LuaLoader;
use polyplug_python::PythonConfig;
use polyplug_python::PythonLoader;

/// Build a Runtime with all language loaders registered.
///
/// Returns the runtime on success, or `Err` with an error message.
fn build_full_runtime() -> Result<Runtime, String> {
    Runtime::builder()
        .loader(LuaLoader::new(LuaConfig::default()))
        .loader(PythonLoader::new(PythonConfig::default()))
        .loader(JsLoader::new(JsConfig {}))
        .loader(DotnetLoader::new(DotnetConfig::default()))
        .build()
        .map_err(|e| e.to_string())
}

/// Create a new Runtime with ALL language loaders (native, Lua, Python, JS, .NET).
///
/// This is the full-featured variant of `polyplug_runtime_new()`. The Lua host
/// calls this instead to support loading guests written in all supported languages.
///
/// # Safety
/// Returns a heap-allocated OpaqueRuntime pointer. Caller must free it with
/// `polyplug_runtime_free()`. Returns null on failure; call `polyplug_last_error()`
/// for the error message.
#[allow(clippy::std_instead_of_core)]
#[no_mangle]
pub unsafe extern "C" fn polyplug_runtime_new_full() -> *mut OpaqueRuntime {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        match build_full_runtime() {
            Ok(rt) => Box::into_raw(Box::new(OpaqueRuntime(rt))),
            Err(msg) => {
                polyplug::ffi::set_last_error_pub(&msg);
                core::ptr::null_mut()
            }
        }
    }))
    .unwrap_or_else(|_| {
        polyplug::ffi::set_last_error_pub("panic in polyplug_runtime_new_full");
        core::ptr::null_mut()
    })
}

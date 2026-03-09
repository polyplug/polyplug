//! Node sub-loader for ts-node/js-node bundles.
//!
//! Loads `.node` shared libraries in-process via `libloading`.

use std::path::Path;
use std::sync::Mutex;
use std::sync::OnceLock;

use polyplug::abi::AbiError;
use polyplug::abi::PluginRegistrar;
use polyplug::abi::ABI_OK;
use polyplug::abi::POLYPLUG_ABI_VERSION;
use polyplug::error::LoaderError;
use polyplug::error::PolyplugError;

use crate::config::NodeConfig;

static LOADED_LIBRARIES: OnceLock<Mutex<Vec<libloading::Library>>> = OnceLock::new();

fn loaded_libraries() -> &'static Mutex<Vec<libloading::Library>> {
    LOADED_LIBRARIES.get_or_init(|| Mutex::new(Vec::new()))
}

pub(crate) fn load(
    path: &Path,
    registrar: &mut PluginRegistrar,
    _config: &NodeConfig,
) -> Result<(), PolyplugError> {
    let path_str: String = path.to_string_lossy().into_owned();

    // SAFETY: The path points to a compiled .node bundle with C ABI.
    // libloading handles platform-specific loading.
    // If the file is not a valid shared library, libloading returns Err.
    let library: libloading::Library = unsafe {
        libloading::Library::new(path).map_err(|e: libloading::Error| {
            PolyplugError::Loader(LoaderError::LoadFailed {
                path: path_str.clone(),
                source: e,
            })
        })?
    };

    // SAFETY: polyplug_abi_version is a C function with signature `extern "C" fn() -> u32`.
    // libloading resolves the symbol; if absent, get() returns Err.
    // The symbol is dropped after use to release its borrow on `library`.
    let found_version: u32 = {
        // SAFETY: polyplug_abi_version is resolved from the loaded library.
        // get() returns Err if missing; the Symbol borrow is limited to this block.
        let sym: libloading::Symbol<'_, unsafe extern "C" fn() -> u32> = unsafe {
            library.get(b"polyplug_abi_version\0").map_err(|_| {
                PolyplugError::Loader(LoaderError::MissingSymbol {
                    bundle: path_str.clone(),
                    symbol: "polyplug_abi_version".to_owned(),
                })
            })?
        };
        // SAFETY: symbol was just resolved and is valid. No side effects.
        let v: u32 = unsafe { sym() };
        v
    };
    if found_version != POLYPLUG_ABI_VERSION {
        return Err(PolyplugError::Loader(LoaderError::AbiVersionMismatch {
            bundle: path_str.clone(),
            expected: POLYPLUG_ABI_VERSION,
            found: found_version,
        }));
    }

    // SAFETY: polyplug_init is guaranteed by the plugin build process to have
    // signature: extern "C" fn(*mut PluginRegistrar) -> AbiError.
    // Symbol<F> derefs to F (a fn pointer, which is Copy).
    // The pointer remains valid as long as the library is alive.
    // Library is moved into LOADED_LIBRARIES immediately after this block.
    let init_fn_ptr: unsafe extern "C" fn(*mut PluginRegistrar) -> AbiError = {
        // SAFETY: polyplug_init is resolved from the loaded library.
        // get() returns Err if missing; the Symbol borrow is limited to this block.
        let sym: libloading::Symbol<'_, unsafe extern "C" fn(*mut PluginRegistrar) -> AbiError> = unsafe {
            library.get(b"polyplug_init\0").map_err(|_| {
                PolyplugError::Loader(LoaderError::MissingSymbol {
                    bundle: path_str.clone(),
                    symbol: "polyplug_init".to_owned(),
                })
            })?
        };
        // SAFETY: Deref of Symbol<F> where F is a fn pointer type (Copy).
        // This copies the function address without cloning Library.
        *sym
    };
    // SAFETY: library is a successfully loaded shared library.
    // Moving it into LOADED_LIBRARIES transfers ownership.
    // It will never be dropped — the never-unload invariant applies.
    {
        let mut guard: std::sync::MutexGuard<'_, Vec<libloading::Library>> = loaded_libraries()
            .lock()
            .unwrap_or_else(|e: std::sync::PoisonError<_>| e.into_inner());
        guard.push(library);
    }

    // SAFETY: init_fn_ptr was resolved from the library (now in LOADED_LIBRARIES).
    // registrar is a valid mutable reference for the duration of this call.
    let init_result: AbiError = unsafe { init_fn_ptr(registrar as *mut PluginRegistrar) };

    if init_result.code != ABI_OK {
        // Library is already in LOADED_LIBRARIES — never-unload invariant.
        // SAFETY: init_result.message.ptr is either null or a static string
        // in the plugin binary (which is never unloaded).
        let error_msg: String = if init_result.message.ptr.is_null() {
            format!("init returned error code {}", init_result.code)
        } else {
            // SAFETY: ptr is non-null and points to valid UTF-8 bytes for len bytes.
            let bytes: &[u8] = unsafe {
                core::slice::from_raw_parts(init_result.message.ptr, init_result.message.len)
            };
            String::from_utf8_lossy(bytes).into_owned()
        };
        return Err(PolyplugError::Loader(LoaderError::JsInitRaisedError {
            bundle: path_str,
            message: error_msg,
        }));
    }

    Ok(())
}

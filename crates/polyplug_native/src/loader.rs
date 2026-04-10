//! Native bundle loader — loads .so/.dll/.dylib plugins.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use polyplug::error::{LoaderError, RuntimeError};
use polyplug::loader::{BundleLoader, ManifestData};
use polyplug::Runtime;
use polyplug_abi::plugin::BundleInitContext;
use polyplug_abi::types::AbiError;
use polyplug_abi::types::AbiErrorCode;
use polyplug_abi::POLYPLUG_ABI_VERSION;
use polyplug_abi::HostInterface;
use polyplug_utils::BundleId;

use crate::config::NativeConfig;

/// Native (shared library) plugin loader.
///
/// Handles .so/.dll/.dylib bundles using dlopen/LoadLibrary.
/// Owns library handles internally — NOT stored in registry.
pub struct NativeLoader {
    config: NativeConfig,
    /// Active library handles, keyed by BundleId.
    libraries: Mutex<HashMap<BundleId, libloading::Library>>,
}

impl NativeLoader {
    /// Create a new NativeLoader.
    pub fn new(config: NativeConfig) -> Self {
        Self {
            config,
            libraries: Mutex::new(HashMap::new()),
        }
    }
}

impl BundleLoader for NativeLoader {
    fn runtime_name(&self) -> &'static str {
        "native"
    }

    fn load(&self, manifest: &ManifestData, runtime: &Runtime) -> Result<(), RuntimeError> {
        if manifest.id == 0 {
            return Err(RuntimeError::Loader(LoaderError::InitFailed {
                bundle: manifest.name.clone(),
                error: "manifest.id is required but was 0 or missing".to_owned(),
            }));
        }

        let bundle_path: PathBuf = if !manifest.file.is_empty() {
            manifest.path.join(&manifest.file)
        } else {
            return Err(RuntimeError::Loader(LoaderError::ManifestMissingFile {
                bundle: manifest.name.clone(),
            }));
        };

        let path_str: String = bundle_path.to_string_lossy().into_owned();

        // ─── Step 1: dlopen the library ────────────────────────────────────────────
        // SAFETY: path points to a compiled plugin bundle; libloading validates the shared library.
        let library: libloading::Library = unsafe {
            libloading::Library::new(&bundle_path).map_err(|e| RuntimeError::Loader(LoaderError::InitFailed {
                bundle: manifest.name.clone(),
                error: format!("failed to load plugin library at {}: {}", path_str, e),
            }))?
        };

        // ─── Step 2: Check ABI version sentinel BEFORE calling init ──────────────────────
        // SAFETY: polyplug_abi_version is a C function with signature `extern "C" fn() -> u32`.
        let abi_version_symbol: libloading::Symbol<'_, unsafe extern "C" fn() -> u32> = unsafe {
            library.get(b"polyplug_abi_version\0").map_err(|_| RuntimeError::Loader(LoaderError::InitFailed {
                bundle: manifest.name.clone(),
                error: format!("missing symbol 'polyplug_abi_version' in bundle '{}'", path_str),
            }))?
        };
        let found_version: u32 = unsafe { abi_version_symbol() };
        if found_version != POLYPLUG_ABI_VERSION {
            return Err(RuntimeError::Loader(LoaderError::InitFailed {
                bundle: manifest.name.clone(),
                error: format!("ABI version mismatch in {}: expected={}, found={}", path_str, POLYPLUG_ABI_VERSION, found_version),
            }));
        }

        // ─── Step 3: Resolve init symbol ──────────────────────────────────────────────
        // SAFETY: polyplug_init is guaranteed by the plugin build process.
        // New signature: fn(host_abi: *const HostInterface, ctx: *const BundleInitContext) -> AbiError
        let init_fn_ptr: unsafe extern "C" fn(
            *const HostInterface,
            *const BundleInitContext,
        ) -> AbiError = {
            let sym: libloading::Symbol<
                '_,
                unsafe extern "C" fn(
                    *const HostInterface,
                    *const BundleInitContext,
                ) -> AbiError,
            > = unsafe {
                library
                    .get(b"polyplug_init\0")
                    .map_err(|_| RuntimeError::Loader(LoaderError::InitSymbolMissing {
                        bundle: manifest.name.clone(),
                    }))?
            };
            *sym
        };

        // ─── Step 4: Create BundleInitContext ────────────────────────────────────────────
        let bundle_dir = bundle_path.parent().unwrap_or(std::path::Path::new("."));
        let ctx: BundleInitContext = BundleInitContext {
            bundle_id: BundleId::new(&manifest.name).id(),
            bundle_path: polyplug_abi::types::StringView {
                ptr: bundle_dir.as_os_str().as_encoded_bytes().as_ptr(),
                len: bundle_dir.as_os_str().as_encoded_bytes().len(),
            },
        };

        // ─── Step 5: Set TLS bundle_id for dependency enforcement ─────────────────────
        let expected_bundle_id: BundleId = BundleId::new(&manifest.name);
        polyplug::set_init_bundle_id(expected_bundle_id.id());

        // ─── Step 6: Get HostInterface and call init ───────────────────────────────────
        let host_abi: &'static HostInterface = runtime.host_abi();
        let init_result: AbiError =
            unsafe { init_fn_ptr(host_abi as *const HostInterface, &ctx) };

        // ─── Step 7: Clear TLS bundle_id ──────────────────────────────────────────────
        polyplug::clear_init_bundle_id();

        if init_result.code != AbiErrorCode::Ok {
            let error_msg: String = if init_result.message.ptr.is_null() {
                format!("init returned error code {:?}", init_result.code)
            } else {
                // SAFETY: ptr is non-null and points to valid UTF-8 bytes
                let bytes: &[u8] = unsafe {
                    core::slice::from_raw_parts(init_result.message.ptr, init_result.message.len)
                };
                String::from_utf8_lossy(bytes).into_owned()
            };
            return Err(RuntimeError::Loader(LoaderError::InitFailed {
                bundle: manifest.name.clone(),
                error: error_msg,
            }));
        }

        // ─── Step 8: Store library handle ─────────────────────────────────────────────
        let bundle_id: BundleId = BundleId::new(&manifest.name);
        self.libraries.lock().unwrap().insert(bundle_id, library);

        Ok(())
    }

    fn reload(&self, manifest: &ManifestData, runtime: &Runtime) -> Result<(), RuntimeError> {
        if !runtime.config().hot_reload_enabled {
            return Err(RuntimeError::HotReloadDisabled);
        }

        let bundle_id: BundleId = BundleId::new(&manifest.name);

        if manifest.file.is_empty() {
            return Err(RuntimeError::Loader(LoaderError::ManifestMissingFile {
                bundle: manifest.name.clone(),
            }));
        }

        let bundle_path: PathBuf = manifest.path.join(&manifest.file);

        let path_str: String = bundle_path.to_string_lossy().into_owned();

        // ─── Step 1: Load new library (inline, same as load()) ───────────────────────────
        // SAFETY: path points to a compiled plugin bundle; libloading validates the shared library.
        let new_library: libloading::Library = unsafe {
            libloading::Library::new(&bundle_path).map_err(|e| RuntimeError::Loader(LoaderError::InitFailed {
                bundle: manifest.name.clone(),
                error: format!("failed to load plugin library at {}: {}", path_str, e),
            }))?
        };

        // ─── Step 2: Check ABI version sentinel ──────────────────────────────────────────
        // SAFETY: polyplug_abi_version is a C function with signature `extern "C" fn() -> u32`.
        let abi_version_symbol: libloading::Symbol<'_, unsafe extern "C" fn() -> u32> = unsafe {
            new_library.get(b"polyplug_abi_version\0").map_err(|_| RuntimeError::Loader(LoaderError::InitFailed {
                bundle: manifest.name.clone(),
                error: format!("missing symbol 'polyplug_abi_version' in bundle '{}'", path_str),
            }))?
        };
        let found_version: u32 = unsafe { abi_version_symbol() };
        if found_version != POLYPLUG_ABI_VERSION {
            return Err(RuntimeError::Loader(LoaderError::InitFailed {
                bundle: manifest.name.clone(),
                error: format!("ABI version mismatch in {}: expected={}, found={}", path_str, POLYPLUG_ABI_VERSION, found_version),
            }));
        }

        // ─── Step 3: Resolve init symbol ────────────────────────────────────────────────
        // SAFETY: polyplug_init is guaranteed by the plugin build process.
        let init_fn_ptr: unsafe extern "C" fn(
            *const HostInterface,
            *const BundleInitContext,
        ) -> AbiError = {
            let sym: libloading::Symbol<
                '_,
                unsafe extern "C" fn(
                    *const HostInterface,
                    *const BundleInitContext,
                ) -> AbiError,
            > = unsafe {
                new_library
                    .get(b"polyplug_init\0")
                    .map_err(|_| RuntimeError::Loader(LoaderError::InitSymbolMissing {
                        bundle: manifest.name.clone(),
                    }))?
            };
            *sym
        };

        // ─── Step 4: Create BundleInitContext ──────────────────────────────────────────────
        let bundle_dir = bundle_path.parent().unwrap_or(std::path::Path::new("."));
        let ctx: BundleInitContext = BundleInitContext {
            bundle_id: BundleId::new(&manifest.name).id(),
            bundle_path: polyplug_abi::types::StringView {
                ptr: bundle_dir.as_os_str().as_encoded_bytes().as_ptr(),
                len: bundle_dir.as_os_str().as_encoded_bytes().len(),
            },
        };

        // ─── Step 5: Set TLS bundle_id for dependency enforcement ───────────────────────
        let expected_bundle_id: BundleId = BundleId::new(&manifest.name);
        polyplug::set_init_bundle_id(expected_bundle_id.id());

        // ─── Step 6: Get HostInterface and call init ─────────────────────────────────────
        let host_abi: &'static HostInterface = runtime.host_abi();
        let init_result: AbiError =
            unsafe { init_fn_ptr(host_abi as *const HostInterface, &ctx) };

        // ─── Step 7: Clear TLS bundle_id ────────────────────────────────────────────────
        polyplug::clear_init_bundle_id();

        if init_result.code != AbiErrorCode::Ok {
            let error_msg: String = if init_result.message.ptr.is_null() {
                format!("init returned error code {:?}", init_result.code)
            } else {
                // SAFETY: ptr is non-null and points to valid UTF-8 bytes
                let bytes: &[u8] = unsafe {
                    core::slice::from_raw_parts(init_result.message.ptr, init_result.message.len)
                };
                String::from_utf8_lossy(bytes).into_owned()
            };
            return Err(RuntimeError::Loader(LoaderError::InitFailed {
                bundle: manifest.name.clone(),
                error: error_msg,
            }));
        }

        // ─── Step 8: Remove and DROP old library ─────────────────────────────────────────
        // SAFETY CONTRACT: Host must not have cached raw function pointers!
        // If they did, this will cause SIGSEGV - that's a HOST BUG.
        // The `on_reload_cb(ReloadPhase::Reloaded)` already fired, giving host a chance to clean up.
        if let Some(old_library) = self.libraries.lock().unwrap().remove(&bundle_id) {
            drop(old_library); // dlclose() - unmaps code pages
        }

        // ─── Step 9: Store new library ───────────────────────────────────────────────────
        self.libraries
            .lock()
            .unwrap()
            .insert(bundle_id, new_library);

        Ok(())
    }
}
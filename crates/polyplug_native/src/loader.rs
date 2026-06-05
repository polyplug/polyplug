//! Native bundle loader — loads .so/.dll/.dylib plugins.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use polyplug::Runtime;
use polyplug::error::{LoaderError, RuntimeError};
use polyplug::loader::{BundleLoader, ManifestData};
use polyplug_abi::HostInterface;
use polyplug_abi::POLYPLUG_ABI_VERSION;
use polyplug_abi::plugin::BundleInitContext;
use polyplug_abi::types::AbiError;
use polyplug_abi::types::AbiErrorCode;
use polyplug_utils::BundleId;

use crate::config::NativeConfig;

/// Native (shared library) plugin loader.
///
/// Handles .so/.dll/.dylib bundles using dlopen/LoadLibrary.
/// Owns library handles internally — NOT stored in registry.
pub struct NativeLoader {
    /// Active library handles, keyed by BundleId.
    libraries: Mutex<HashMap<BundleId, libloading::Library>>,
    /// Libraries superseded by a hot-reload, retained for the loader's lifetime.
    ///
    /// On reload the old library is NOT `dlclose`d: a concurrent caller may still
    /// hold a raw function pointer resolved from the old version's vtable, and
    /// unmapping its code pages would turn that pointer into a dangling one
    /// (SIGSEGV). Retaining the handle keeps the code pages mapped, honoring the
    /// documented guarantee that the old vtable stays alive until all in-flight
    /// calls complete (TRUST_MODEL.md §Hot-Reload Safety Guarantees).
    retired: Mutex<Vec<libloading::Library>>,
}

impl NativeLoader {
    /// Create a new NativeLoader.
    ///
    /// `config` is accepted for API/FFI symmetry with the other loaders; native
    /// plugins require no configuration, so `NativeConfig` (empty) is not stored.
    pub fn new(_config: NativeConfig) -> Self {
        Self {
            libraries: Mutex::new(HashMap::new()),
            retired: Mutex::new(Vec::new()),
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
            libloading::Library::new(&bundle_path).map_err(|e| {
                RuntimeError::Loader(LoaderError::InitFailed {
                    bundle: manifest.name.clone(),
                    error: format!("failed to load plugin library at {}: {}", path_str, e),
                })
            })?
        };

        // ─── Step 2: Check ABI version sentinel BEFORE calling init ──────────────────────
        // SAFETY: polyplug_abi_version is a C function with signature `extern "C" fn() -> u32`.
        let abi_version_symbol: libloading::Symbol<'_, unsafe extern "C" fn() -> u32> = unsafe {
            library.get(b"polyplug_abi_version\0").map_err(|_| {
                RuntimeError::Loader(LoaderError::InitFailed {
                    bundle: manifest.name.clone(),
                    error: format!(
                        "missing symbol 'polyplug_abi_version' in bundle '{}'",
                        path_str
                    ),
                })
            })?
        };
        // SAFETY: abi_version_symbol was obtained from library.get() which validated the
        // symbol exists. The function has signature `extern "C" fn() -> u32` and returns
        // a plain u32, so there are no memory safety concerns.
        let found_version: u32 = unsafe { abi_version_symbol() };
        if found_version != POLYPLUG_ABI_VERSION {
            return Err(RuntimeError::Loader(LoaderError::InitFailed {
                bundle: manifest.name.clone(),
                error: format!(
                    "ABI version mismatch in {}: expected={}, found={}",
                    path_str, POLYPLUG_ABI_VERSION, found_version
                ),
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
                unsafe extern "C" fn(*const HostInterface, *const BundleInitContext) -> AbiError,
                // SAFETY: polyplug_init is an exported C symbol from the plugin library,
                // validated to exist by library.get(). The signature matches the ABI contract.
            > = unsafe {
                library.get(b"polyplug_init\0").map_err(|_| {
                    RuntimeError::Loader(LoaderError::InitSymbolMissing {
                        bundle: manifest.name.clone(),
                    })
                })?
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

        // ─── Step 5: Push bundle_id for dependency enforcement ────────────────────────
        let expected_bundle_id: BundleId = BundleId::new(&manifest.name);
        runtime.push_init_bundle_id(expected_bundle_id.id());

        // ─── Step 6: Get HostInterface and call init (panic-isolated) ──────────────────
        // A panicking plugin init must not unwind across the C ABI (UB / process abort).
        // catch_unwind contains it and maps it to a proper LoaderError. The init stack
        // is popped on BOTH the success and panic paths so it never leaks an entry.
        let host_abi: &'static HostInterface = runtime.host_abi();
        let init_outcome: Result<AbiError, Box<dyn core::any::Any + Send>> =
            std::panic::catch_unwind(core::panic::AssertUnwindSafe(|| {
                // SAFETY: host_abi is a valid HostInterface reference obtained from the runtime.
                // init_fn_ptr is a valid function pointer resolved from the plugin library.
                // ctx is a stack-allocated BundleInitContext that outlives the call.
                unsafe { init_fn_ptr(host_abi as *const HostInterface, &ctx) }
            }));

        // ─── Step 7: Pop bundle_id (always, including panic path) ──────────────────────
        runtime.pop_init_bundle_id();

        let init_result: AbiError = match init_outcome {
            Ok(result) => result,
            Err(_panic) => {
                return Err(RuntimeError::Loader(LoaderError::InitFailed {
                    bundle: manifest.name.clone(),
                    error: "plugin polyplug_init panicked".to_owned(),
                }));
            }
        };

        if init_result.code != AbiErrorCode::Ok as u32 {
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
        self.libraries
            .lock()
            .unwrap_or_else(|e| {
                eprintln!("[polyplug_native] Mutex poisoned, recovering: {}", e);
                e.into_inner()
            })
            .insert(bundle_id, library);

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
            libloading::Library::new(&bundle_path).map_err(|e| {
                RuntimeError::Loader(LoaderError::InitFailed {
                    bundle: manifest.name.clone(),
                    error: format!("failed to load plugin library at {}: {}", path_str, e),
                })
            })?
        };

        // ─── Step 2: Check ABI version sentinel ──────────────────────────────────────────
        // SAFETY: polyplug_abi_version is a C function with signature `extern "C" fn() -> u32`.
        let abi_version_symbol: libloading::Symbol<'_, unsafe extern "C" fn() -> u32> = unsafe {
            new_library.get(b"polyplug_abi_version\0").map_err(|_| {
                RuntimeError::Loader(LoaderError::InitFailed {
                    bundle: manifest.name.clone(),
                    error: format!(
                        "missing symbol 'polyplug_abi_version' in bundle '{}'",
                        path_str
                    ),
                })
            })?
        };
        // SAFETY: abi_version_symbol was obtained from new_library.get() which validated
        // the symbol exists. The function has signature `extern "C" fn() -> u32`.
        let found_version: u32 = unsafe { abi_version_symbol() };
        if found_version != POLYPLUG_ABI_VERSION {
            return Err(RuntimeError::Loader(LoaderError::InitFailed {
                bundle: manifest.name.clone(),
                error: format!(
                    "ABI version mismatch in {}: expected={}, found={}",
                    path_str, POLYPLUG_ABI_VERSION, found_version
                ),
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
                unsafe extern "C" fn(*const HostInterface, *const BundleInitContext) -> AbiError,
                // SAFETY: polyplug_init is an exported C symbol from the new plugin library,
                // validated to exist by new_library.get(). The signature matches the ABI contract.
            > = unsafe {
                new_library.get(b"polyplug_init\0").map_err(|_| {
                    RuntimeError::Loader(LoaderError::InitSymbolMissing {
                        bundle: manifest.name.clone(),
                    })
                })?
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

        // ─── Step 5: Push bundle_id for dependency enforcement ──────────────────────────
        let expected_bundle_id: BundleId = BundleId::new(&manifest.name);
        runtime.push_init_bundle_id(expected_bundle_id.id());

        // ─── Step 6: Get HostInterface and call init (panic-isolated) ────────────────────
        // A panicking plugin init must not unwind across the C ABI. catch_unwind contains
        // it; the init stack is popped on both the success and panic paths.
        let host_abi: &'static HostInterface = runtime.host_abi();
        let init_outcome: Result<AbiError, Box<dyn core::any::Any + Send>> =
            std::panic::catch_unwind(core::panic::AssertUnwindSafe(|| {
                // SAFETY: host_abi is a valid HostInterface reference obtained from the runtime.
                // init_fn_ptr is a valid function pointer resolved from the new plugin library.
                // ctx is a stack-allocated BundleInitContext that outlives the call.
                unsafe { init_fn_ptr(host_abi as *const HostInterface, &ctx) }
            }));

        // ─── Step 7: Pop bundle_id (always, including panic path) ────────────────────────
        runtime.pop_init_bundle_id();

        let init_result: AbiError = match init_outcome {
            Ok(result) => result,
            Err(_panic) => {
                return Err(RuntimeError::Loader(LoaderError::InitFailed {
                    bundle: manifest.name.clone(),
                    error: "plugin polyplug_init panicked".to_owned(),
                }));
            }
        };

        if init_result.code != AbiErrorCode::Ok as u32 {
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

        // ─── Step 8: Remove and RETIRE old library ───────────────────────────────────────
        // The old library is retained (not dlclose'd): a concurrent caller may
        // still hold a raw function pointer resolved from the old vtable, and
        // unmapping its code pages would dangle that pointer (SIGSEGV). Moving the
        // handle into `retired` keeps the code pages mapped for the loader's
        // lifetime, honoring the documented hot-reload guarantee that the old
        // vtable stays alive until all in-flight calls complete.
        if let Some(old_library) = self
            .libraries
            .lock()
            .unwrap_or_else(|e| {
                eprintln!("[polyplug_native] Mutex poisoned, recovering: {}", e);
                e.into_inner()
            })
            .remove(&bundle_id)
        {
            self.retired
                .lock()
                .unwrap_or_else(|e| {
                    eprintln!("[polyplug_native] Mutex poisoned, recovering: {}", e);
                    e.into_inner()
                })
                .push(old_library);
        }

        // ─── Step 9: Store new library ───────────────────────────────────────────────────
        self.libraries
            .lock()
            .unwrap_or_else(|e| {
                eprintln!("[polyplug_native] Mutex poisoned, recovering: {}", e);
                e.into_inner()
            })
            .insert(bundle_id, new_library);

        Ok(())
    }
}

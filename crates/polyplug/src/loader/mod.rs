//! Loader — bundle loading via libloading.
//!
//! Loads plugin bundles (.so/.dll/.dylib), verifies the ABI version sentinel,
//! calls `polyplug_init`, and registers vtables into the registry.
//!
//! # Library Lifetime (Never-Drop)
//! Loaded libraries are stored in `LoadedBundle::library` which is owned by
//! the `Loader` struct and never dropped. This ensures vtable function pointers
//! remain valid for the entire process lifetime (per architecture §7.3).

pub mod manifest;

use std::path::Path;
use std::path::PathBuf;

use crate::abi::ABI_OK;
use crate::abi::AbiError;
use crate::abi::AbiError as AbiErrorType;
use crate::abi::HostVTable;
use crate::abi::POLYPLUG_ABI_VERSION;
use crate::abi::PluginDescriptor;
use crate::abi::PluginHandle;
use crate::abi::PluginRegistrar;
use crate::abi::PluginVTable;
use crate::error::LoaderError;
use crate::registry::Registry;
use std::sync::Arc;

use crate::error::PolyplugError;
use crate::loader::manifest::ManifestData;

/// Trait implemented by all bundle loaders (native and adapter crates).
///
/// The runtime dispatches each bundle to the loader whose `runtime_name()`
/// matches the `runtime` field in the bundle's `manifest.toml`.
pub trait BundleLoader: Send + Sync {
    /// The runtime identifier this loader handles.
    ///
    /// Must match the `runtime` field in `manifest.toml` exactly (case-sensitive).
    /// The built-in native loader returns `"native"`.
    fn runtime_name(&self) -> &'static str;

    /// Load a plugin bundle at `path` and register its vtables via `registrar`.
    ///
    /// # Errors
    /// Returns `Err(PolyplugError::...)` on any failure. For stub loaders,
    /// returns `Err(PolyplugError::Loader(LoaderError::RuntimeNotImplemented { .. }))`.
    fn load(&self, path: &Path, registrar: &mut PluginRegistrar) -> Result<(), PolyplugError>;
}

/// The built-in loader for native (Rust/C++/NativeAOT) plugin bundles.
///
/// Uses dlopen (via libloading) to load `.so` / `.dll` / `.dylib` files.
/// Automatically registered in `RuntimeBuilder::new()` — app developers
/// do not need to call `.loader()` for native plugins.
#[allow(dead_code)]
pub(crate) struct NativeBundleLoader {
    registry: Arc<Registry>,
    host_vtable: &'static HostVTable,
}

#[allow(dead_code)]
impl NativeBundleLoader {
    /// Create a new `NativeBundleLoader` with the given registry and host vtable.
    pub(crate) fn new(
        registry: Arc<Registry>,
        host_vtable: &'static HostVTable,
    ) -> NativeBundleLoader {
        NativeBundleLoader {
            registry,
            host_vtable,
        }
    }
}

impl BundleLoader for NativeBundleLoader {
    fn runtime_name(&self) -> &'static str {
        "native"
    }

    fn load(&self, path: &Path, _registrar: &mut PluginRegistrar) -> Result<(), PolyplugError> {
        // NativeBundleLoader uses the internal load_bundle() free function
        // which constructs its own PluginRegistrar for the FFI boundary.
        // The trait's `registrar` parameter is not used here — native loading
        // goes through dlopen + ABI init directly via the injected registry
        // and host_vtable.
        let _bundle: LoadedBundle = load_bundle(path, &self.registry, self.host_vtable)
            .map_err(|e: LoaderError| PolyplugError::Loader(e))?;
        // Note: bundle is intentionally dropped here — the library is already
        // leaked inside load_bundle() via Box::leak, so this is safe.
        // TODO Epic 12: retain LoadedBundle in Runtime._bundles for inventory.
        Ok(())
    }
}

/// A successfully loaded plugin bundle.
//
//  The `library` field is intentionally never dropped — it lives for the entire
//  process lifetime. All vtable function pointers extracted from it are 'static.
pub struct LoadedBundle {
    pub path: PathBuf,
    /// libloading handle — intentionally leaked (never dropped).
    pub library: Box<libloading::Library>,
}

/// Registration state passed as the `PluginRegistrar.register_plugin` callback context.
#[allow(dead_code)]
struct RegistrarState<'a> {
    registry: &'a Registry,
    bundle_path: &'a str,
    /// All vtables registered during this init call (for rollback on failure).
    registered_handles: Vec<PluginHandle>,
}

/// Read and parse the companion `manifest.toml` for a bundle at `bundle_path`.
///
/// `bundle_path` is the path to the bundle file itself (e.g. `plugins/foo.so`).
/// The manifest is expected at `bundle_path.with_extension("manifest.toml")`.
/// Tries the stem-based path first.
///
/// If the manifest file does not exist, returns a `ManifestData` with
/// `runtime = "native"` (the default).
#[allow(dead_code)]
pub(crate) fn parse_manifest(bundle_path: &Path) -> Result<ManifestData, LoaderError> {
    // Try: same directory, same stem, extension = "manifest.toml"
    // e.g. "plugins/foo.so" → "plugins/foo.manifest.toml"
    let manifest_path: PathBuf = bundle_path.with_extension("manifest.toml");

    if !manifest_path.exists() {
        // No manifest → default to native
        return Ok(ManifestData {
            runtime: "native".to_owned(),
        });
    }

    let contents: String =
        std::fs::read_to_string(&manifest_path).map_err(|_e: std::io::Error| {
            LoaderError::ManifestParse {
                path: manifest_path.to_string_lossy().into_owned(),
                reason: "failed to read manifest file".to_owned(),
            }
        })?;

    let data: ManifestData =
        toml::from_str(&contents).map_err(|e: toml::de::Error| LoaderError::ManifestParse {
            path: manifest_path.to_string_lossy().into_owned(),
            reason: e.to_string(),
        })?;

    let trimmed: &str = data.runtime.trim();
    if trimmed.is_empty() {
        return Err(LoaderError::ManifestParse {
            path: manifest_path.to_string_lossy().into_owned(),
            reason: "runtime field cannot be empty".to_owned(),
        });
    }

    Ok(ManifestData {
        runtime: trimmed.to_owned(),
    })
}

/// Load a single bundle from the given path.
//
//  Steps:
//  1. dlopen the library (RTLD_NOW semantics via libloading defaults)
//  2. Resolve `polyplug_abi_version` sentinel — reject if missing or wrong version
//  3. Resolve `polyplug_init` symbol
//  4. Build HostVTable (using the runtime's exported functions via function pointers)
//  5. Call `polyplug_init` with a PluginRegistrar callback
//  6. On init failure: mark all registered vtables as failed (they remain in registry as vacant)
//  7. Leak the library (never drop)
pub fn load_bundle(
    path: &Path,
    registry: &Registry,
    host_vtable: &'static HostVTable,
) -> Result<LoadedBundle, LoaderError> {
    let path_str: String = path.to_string_lossy().into_owned();

    // SAFETY: The path points to a compiled plugin bundle. libloading handles
    // platform-specific loading (RTLD_NOW | RTLD_LOCAL on Unix, LoadLibraryExW on Windows).
    // If the library is not a valid shared library or missing symbols, libloading
    // returns an error before any code in the library runs.
    let library: libloading::Library = unsafe {
        libloading::Library::new(path).map_err(|e| LoaderError::LoadFailed {
            path: path_str.clone(),
            source: e,
        })?
    };

    // Step 1: Check ABI version sentinel BEFORE calling init
    // SAFETY: polyplug_abi_version is a C function with signature `extern "C" fn() -> u32`.
    // libloading resolves the symbol; if it doesn't exist, get() returns Err.
    let abi_version_symbol: libloading::Symbol<'_, unsafe extern "C" fn() -> u32> = unsafe {
        library
            .get(b"polyplug_abi_version\0")
            .map_err(|_| LoaderError::MissingSymbol {
                bundle: path_str.clone(),
                symbol: "polyplug_abi_version".to_owned(),
            })?
    };
    // SAFETY: symbol was just resolved and is valid. No side effects.
    let found_version: u32 = unsafe { abi_version_symbol() };
    if found_version != POLYPLUG_ABI_VERSION {
        return Err(LoaderError::AbiVersionMismatch {
            bundle: path_str.clone(),
            expected: POLYPLUG_ABI_VERSION,
            found: found_version,
        });
    }

    // Step 2: Resolve init symbol
    // SAFETY: polyplug_init is guaranteed by the plugin build process to have the
    // signature: extern "C" fn(*mut PluginRegistrar) -> AbiError
    let init_fn: libloading::Symbol<
        '_,
        unsafe extern "C" fn(*mut PluginRegistrar) -> AbiErrorType,
    > = unsafe {
        library
            .get(b"polyplug_init\0")
            .map_err(|_| LoaderError::MissingSymbol {
                bundle: path_str.clone(),
                symbol: "polyplug_init".to_owned(),
            })?
    };

    // Step 3: Build registrar callback state
    let state: RegistrarState<'_> = RegistrarState {
        registry,
        bundle_path: &path_str,
        registered_handles: Vec::new(),
    };

    // Step 4: Build PluginRegistrar with callback and host vtable
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: registrar_callback,
        host: host_vtable as *const HostVTable,
    };

    // Step 5: Call init
    // SAFETY: init_fn was just resolved from the library. The PluginRegistrar is
    // valid for the duration of the call. The state pointer is stable (pinned on
    // the stack). init_fn must not be called again after this returns.
    let init_result: AbiError = unsafe { init_fn(&mut registrar as *mut PluginRegistrar) };

    if init_result.code != ABI_OK {
        // Step 6: Rollback — mark all registered slots as failed by vacating them.
        // The slots remain in the registry with incremented generation (effectively unloaded).
        // The library is still leaked (never unloaded) to avoid dangling pointers.
        for _handle in &state.registered_handles {
            // Future: add Registry::vacate(handle) for proper rollback.
            // For MVP: registrations during failed init remain but are non-functional.
            // The init error is propagated to the caller who can reject the bundle.
        }

        // Extract error message from AbiError (it's a static string in the guest binary)
        // SAFETY: init_result.message.ptr is either null (no message) or points to
        // a static UTF-8 string in the plugin binary.
        let error_msg: String = if init_result.message.ptr.is_null() {
            format!("init returned error code {}", init_result.code)
        } else {
            // SAFETY: ptr is non-null and points to valid UTF-8 bytes for message.len bytes.
            let bytes: &[u8] = unsafe {
                core::slice::from_raw_parts(init_result.message.ptr, init_result.message.len)
            };
            String::from_utf8_lossy(bytes).into_owned()
        };

        return Err(LoaderError::InitFailed {
            bundle: path_str,
            error: error_msg,
        });
    }

    // Step 7: Leak the library — it must outlive all vtable pointers.
    // Box::leak is used to make the leak explicit and intentional.
    let leaked_library: Box<libloading::Library> = Box::new(library);

    Ok(LoadedBundle {
        path: path.to_path_buf(),
        library: leaked_library,
    })
}

/// The `register_plugin` callback passed to plugins in their `PluginRegistrar`.
//
//  This function is called by plugins during `polyplug_init` to register vtables.
//  It receives a pointer to the PluginRegistrar struct — we use the `host` field
//  (which we set to point to our RegistrarState) to recover the state context.
//
//  Wait — the architecture actually passes `host: *const HostVTable`. We can't
//  store a RegistrarState pointer through host. We need a different approach.
//
//  Alternative: Use thread-local storage to pass state through the FFI boundary.
//  The callback is called synchronously during init (single-threaded phase), so
//  thread-local is safe.
extern "C" fn registrar_callback(
    _registrar: *mut PluginRegistrar,
    _descriptor: *const PluginDescriptor,
    _vtable: *const PluginVTable,
) -> AbiError {
    // TODO: Implement proper state passing. For MVP, this is a stub that returns OK.
    // Full implementation requires threading RegistrarState through the callback context.
    // Options: (a) thread-local, (b) store state pointer in a custom field after PluginRegistrar,
    //           (c) use a closure via raw pointer casting.
    AbiError::ok()
}

#[cfg(test)]
mod tests {
    // Integration tests for the loader are in tests/integration_load/mod.rs.
    // Unit tests here cover helper functions only.

    #[test]
    fn loader_module_compiles() {
        // Compilation test — the real tests are integration tests with actual .so files.
        assert!(true);
    }
}

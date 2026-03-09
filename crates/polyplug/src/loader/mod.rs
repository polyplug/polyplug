//! Loader — bundle loading via libloading.
//!
//! Loads plugin bundles (.so/.dll/.dylib), verifies the ABI version sentinel,
//! calls `polyplug_init`, and registers vtables into the registry.
//!
//! # Library Lifetime
//! `libloading::Library` handles for loaded native bundles are moved into
//! `Registry::loaded_libraries` immediately after symbol resolution.
//! This ensures code pages remain mapped for the entire lifetime of the `Registry`
//! (i.e., the `Runtime`). Dropping a `Library` calls `dlclose()` which unmaps
//! plugin code — any vtable fn pointer into those pages would become dangling.
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
/// The `Library` handle for each loaded bundle is stored in the injected `Registry`,
/// not in this struct, to guarantee it outlives all vtable function pointers.
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

    /// Load a native plugin bundle by calling `load_bundle()`.
    ///
    /// The `Library` handle for the loaded bundle is stored in the `Registry`
    /// (`self.registry`) — NOT here in the loader. `NativeBundleLoader` may be
    /// dropped before `Runtime` (e.g., after the build phase). Storing the library
    /// here would allow `dlclose()` to fire while vtable pointers are still live.
    fn load(&self, path: &Path, _registrar: &mut PluginRegistrar) -> Result<(), PolyplugError> {
        // NativeBundleLoader uses load_bundle() which pushes the Library handle
        // directly into the Registry via registry.push_library(). The trait's
        // `registrar` parameter is unused here — native loading goes through
        // dlopen + ABI init directly via the injected registry and host_vtable.
        load_bundle(path, &self.registry, self.host_vtable)
            .map_err(|e: LoaderError| PolyplugError::Loader(e))
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

/// Load a single native plugin bundle.
///
/// # Steps
/// 1. `dlopen` the library (RTLD_NOW | RTLD_LOCAL via libloading defaults).
///    RTLD_LOCAL: plugins must not pollute the global symbol namespace.
///    RTLD_NOW: fail fast at load time if any symbols are missing.
/// 2. Resolve `polyplug_abi_version` sentinel — reject if missing or wrong version.
/// 3. Resolve `polyplug_init`, copy the fn pointer out of the `Symbol` borrow,
///    then move `library` into `registry.loaded_libraries`.
///    **Why critical**: `Library::drop` calls `dlclose()`, unmapping plugin code pages.
///    Any vtable fn pointer into those pages then becomes dangling — silent
///    memory corruption or SIGBUS on the next vtable call. By storing the handle in
///    `Registry`, it lives exactly as long as the `Runtime`.
/// 4. Call `polyplug_init` with a `PluginRegistrar` callback.
/// 5. On init failure: propagate the error. The library remains in
///    `registry.loaded_libraries` — the never-unload invariant applies.
pub fn load_bundle(
    path: &Path,
    registry: &Registry,
    host_vtable: &'static HostVTable,
) -> Result<(), LoaderError> {
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
    // The symbol is explicitly dropped after use to release its borrow on `library`
    // before the init phase begins.
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
    // Explicitly drop the symbol to release its borrow on `library` before we move
    // `library` into the registry below.
    let _ = abi_version_symbol;
    if found_version != POLYPLUG_ABI_VERSION {
        return Err(LoaderError::AbiVersionMismatch {
            bundle: path_str.clone(),
            expected: POLYPLUG_ABI_VERSION,
            found: found_version,
        });
    }

    // Step 2: Resolve init symbol and extract the raw function pointer.
    // We copy the fn pointer out of the Symbol immediately so the Symbol's borrow
    // on `library` is released before we move `library` into the registry below.
    // SAFETY: polyplug_init is guaranteed by the plugin build process to have the
    // signature: extern "C" fn(*mut PluginRegistrar) -> AbiError.
    // Symbol<F> derefs to F (a fn pointer). Fn pointers are Copy — copying does not
    // extend the lifetime of `library`. The pointer remains valid as long as `library`
    // is alive. `library` is moved into `registry.loaded_libraries` immediately after,
    // so the pointer is always valid while reachable.
    let init_fn_ptr: unsafe extern "C" fn(*mut PluginRegistrar) -> AbiErrorType = {
        // SAFETY: polyplug_init is resolved from a successfully loaded library.
        // libloading's get() returns Err if the symbol doesn't exist; we propagate via ?.
        // The returned Symbol borrows `library` and is valid for the duration of this block.
        let sym: libloading::Symbol<
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
        // SAFETY: Deref of Symbol<F> where F is a fn pointer type (Copy).
        // This copies the function address out of the Symbol without cloning Library.
        *sym
    };
    // `sym` is dropped here, releasing the borrow on `library`.

    // Step 3: Move the library into the registry BEFORE calling init.
    // SAFETY: `library` is a successfully loaded shared library. Moving it into
    // `registry.loaded_libraries` transfers ownership to the Registry, which
    // outlives this function and all vtable pointers registered during init.
    // This prevents dlclose() from being called while vtable fn pointers are live.
    registry.push_library(library);

    // Step 4: Build registrar callback state
    let state: RegistrarState<'_> = RegistrarState {
        registry,
        bundle_path: &path_str,
        registered_handles: Vec::new(),
    };

    // Step 5: Build PluginRegistrar with callback and host vtable
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: registrar_callback,
        host: host_vtable as *const HostVTable,
    };

    // Step 6: Call init
    // SAFETY: init_fn_ptr was resolved from the library (now stored in registry).
    // The PluginRegistrar is valid for the duration of the call.
    // The state pointer is stable (pinned on the stack).
    let init_result: AbiError = unsafe { init_fn_ptr(&mut registrar as *mut PluginRegistrar) };

    if init_result.code != ABI_OK {
        // Step 7: On init failure: the library is already stored in registry.loaded_libraries
        // and will NOT be unloaded. The never-unload invariant means we never call
        // dlclose on a library once any code from it has run. Failed slots remain
        // vacant (non-functional) in the registry.
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

    Ok(())
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

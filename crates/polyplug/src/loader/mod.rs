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

mod manifest;
mod bundle_loader;
mod loaded_bundle;

pub use loaded_bundle::LoadedBundle;

pub use manifest::{ManifestData, ManifestDependency, RawManifestDependency};
pub use bundle_loader::BundleLoader;
use polyplug_abi::plugin::PluginContext;
use polyplug_abi::types::AbiErrorCode;
use polyplug_abi::types::StringView;
use polyplug_utils::PluginContractId;

use std::path::Path;
use std::path::PathBuf;

use polyplug_abi::types::AbiError;
use polyplug_abi::HostVTable;
use polyplug_abi::POLYPLUG_ABI_VERSION;
use std::sync::Arc;

use crate::error::LoaderError;
use crate::plugin_registry::PluginRegistry;
use crate::runtime::HostContext;
use crate::runtime::Runtime;
use crate::error::PolyplugError;

/// The built-in loader for native (Rust/C++/NativeAOT) bundles.
///
/// Uses dlopen (via libloading) to load `.so` / `.dll` / `.dylib` files.
/// Automatically registered during `RuntimeBuilder::build()` unless a user-provided
/// loader already claims the `"native"` runtime name.
/// The `Library` handle for each loaded bundle is stored in the injected `Registry`,
/// not in this struct, to guarantee it outlives all vtable function pointers.
pub(crate) struct NativeBundleLoader {
    registry: Arc<PluginRegistry>,
    host_vtable: &'static HostVTable,
}

impl NativeBundleLoader {
    /// Create a new `NativeBundleLoader` with the given registry and host vtable.
    pub(crate) fn new(
        registry: Arc<PluginRegistry>,
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

    fn load(&self, manifest: &ManifestData, runtime: &Runtime) -> Result<(), PolyplugError> {
        if manifest.id == 0 {
            return Err(PolyplugError::Loader(LoaderError::InitFailed {
                bundle: manifest.path.display().to_string(),
                error: "manifest.id is required but was 0 or missing".to_owned(),
            }));
        }
        let bundle_path: PathBuf = if !manifest.file.is_empty() {
            manifest.path.join(&manifest.file)
        } else {
            return Err(PolyplugError::Loader(LoaderError::ManifestMissingFile {
                bundle: manifest.name.clone(),
            }));
        };
        load_bundle(
            &bundle_path,
            manifest,
            &self.registry,
            self.host_vtable,
            runtime,
        )
        .map_err(|e: LoaderError| PolyplugError::Loader(e))
    }
}

/// Load a single native bundle.
///
/// # Steps
/// 1. Parse the manifest to extract bundle_name and dependencies.
/// 2. `dlopen` the library (RTLD_NOW | RTLD_LOCAL via libloading defaults).
///    RTLD_LOCAL: plugins must not pollute the global symbol namespace.
///    RTLD_NOW: fail fast at load time if any symbols are missing.
/// 3. Resolve `polyplug_abi_version` sentinel — reject if missing or wrong version.
/// 4. Resolve `polyplug_init`, copy the fn pointer out of the `Symbol` borrow,
///    then move `library` into `registry.loaded_libraries`.
///    **Why critical**: `Library::drop` calls `dlclose()`, unmapping plugin code pages.
///    Any vtable fn pointer into those pages then becomes dangling — silent
///    memory corruption or SIGBUS on the next vtable call. By storing the handle in
///    `Registry`, it lives exactly as long as the `Runtime`.
/// 5. Call `polyplug_init` with (rt_ctx, host_vtable, ctx).
/// 6. On init failure: propagate the error. The library remains in
///    `registry.loaded_libraries` — the never-unload invariant applies.
pub fn load_bundle(
    path: &Path,
    manifest: &ManifestData,
    registry: &PluginRegistry,
    host_vtable: &'static HostVTable,
    runtime: &Runtime,
) -> Result<(), LoaderError> {
    let path_str: String = path.to_string_lossy().into_owned();
    let bundle_dir: &Path = path.parent().unwrap_or_else(|| Path::new("."));

    // Resolve dependency contract_ids and declare them to the registry
    let dep_contract_ids: Vec<u64> = manifest
        .dependencies
        .iter()
        .map(|dep: &RawManifestDependency| {
            if dep.contract_id != 0 {
                dep.contract_id
            } else {
                PluginContractId::new(&dep.contract, 0)
            }
        })
        .collect::<Vec<u64>>();
    registry
        .declare_deps(manifest.id, dep_contract_ids)
        .map_err(|e: crate::error::RegistryError| LoaderError::InitFailed {
            bundle: path_str.clone(),
            error: format!("declare_deps failed: {e}"),
        })?;

    // SAFETY: The path points to a compiled bundle. libloading handles
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
    // signature: extern "C" fn(rt_ctx: *mut c_void, host: *const HostVTable, ctx: *const PluginContext) -> AbiError.
    // Symbol<F> derefs to F (a fn pointer). Fn pointers are Copy — copying does not
    // extend the lifetime of `library`. The pointer remains valid as long as `library`
    // is alive. `library` is moved into `registry.loaded_libraries` immediately after,
    // so the pointer is always valid while reachable.
    let init_fn_ptr: unsafe extern "C" fn(
        *mut core::ffi::c_void,
        *const HostVTable,
        *const polyplug_abi::PluginContext,
    ) -> AbiError = {
        // SAFETY: polyplug_init is resolved from a successfully loaded library.
        // libloading's get() returns Err if the symbol doesn't exist; we propagate via ?.
        // The returned Symbol borrows `library` and is valid for the duration of this block.
        let sym: libloading::Symbol<
            '_,
            unsafe extern "C" fn(
                *mut core::ffi::c_void,
                *const HostVTable,
                *const polyplug_abi::PluginContext,
            ) -> AbiError,
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

    // Step 4: Create PluginContext with bundle path, host ABI version, and bundle_id
    let bundle_path_sv = StringView {
        ptr: bundle_dir.as_os_str().as_encoded_bytes().as_ptr(),
        len: bundle_dir.as_os_str().as_encoded_bytes().len(),
    };
    let ctx = PluginContext {
        bundle_path: bundle_path_sv,
        host_abi_version: POLYPLUG_ABI_VERSION,
        bundle_id: manifest.id,
    };

    // Step 5: Create HostContext on the stack for dependency enforcement
    let expected_bundle_id: u64 = manifest.id;
    let host_ctx: HostContext = HostContext {
        runtime: runtime as *const Runtime as *mut Runtime,
        bundle_id: expected_bundle_id,
    };

    // Step 6: Call init with (rt_ctx, host_vtable, ctx)
    // rt_ctx is a pointer to HostContext - host functions will cast it back
    let rt_ctx: *mut core::ffi::c_void = &host_ctx as *const HostContext as *mut core::ffi::c_void;
    // SAFETY: init_fn_ptr was resolved from the library (now stored in registry).
    // The HostVTable and PluginContext are valid for the duration of the call.
    // rt_ctx is a valid HostContext pointer.
    let init_result: AbiError =
        unsafe { init_fn_ptr(rt_ctx, host_vtable as *const HostVTable, &ctx) };

    // Step 6.5: Verify bundle_id wasn't tampered with during init
    if host_ctx.bundle_id != expected_bundle_id {
        return Err(LoaderError::BundleTampered {
            bundle: path_str.clone(),
            expected: expected_bundle_id,
            found: host_ctx.bundle_id,
        });
    }

    if init_result.code != AbiErrorCode::Ok {
        // Step 7: On init failure: the library is already stored in registry.loaded_libraries
        // and will NOT be unloaded. The never-unload invariant means we never call
        // dlclose on a library once any code from it has run. Failed slots remain
        // vacant (non-functional) in the registry.

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


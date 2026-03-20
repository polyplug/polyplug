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
pub mod scanner;

use std::path::Path;
use std::path::PathBuf;

use crate::error::LoaderError;
use crate::registry::Registry;
use polyplug_abi::ABI_OK;
use polyplug_abi::AbiError;
use polyplug_abi::AbiError as AbiErrorType;
use polyplug_abi::HostVTable;
use polyplug_abi::POLYPLUG_ABI_VERSION;
use polyplug_abi::PluginDescriptor;
use polyplug_abi::PluginRegistrar;
use polyplug_abi::PluginVTable;
use std::sync::Arc;

use crate::error::PolyplugError;
use crate::loader::manifest::ManifestData;
use crate::loader::manifest::RawManifestDependency;

// ─── Thread-locals for state passing through the FFI boundary ────────────────
//
// These are set by load_bundle() immediately before calling polyplug_init and
// cleared after by BundleInitGuard::drop(). The registrar_callback reads them
// synchronously during the same call — thread-local is safe because init is
// single-threaded per bundle.
thread_local! {
    static REGISTRAR_BUNDLE_ID: core::cell::Cell<u64> = const { core::cell::Cell::new(0) };
    static REGISTRAR_REGISTRY_PTR: core::cell::Cell<*const Registry> = const { core::cell::Cell::new(core::ptr::null()) };
}

// ─── RAII guard: clears thread-locals on drop (even on panic) ────────────────
pub(crate) struct BundleInitGuard;

impl Drop for BundleInitGuard {
    fn drop(&mut self) {
        crate::runtime::INIT_BUNDLE_ID.with(|c: &core::cell::Cell<u64>| c.set(0));
        REGISTRAR_REGISTRY_PTR
            .with(|c: &core::cell::Cell<*const Registry>| c.set(core::ptr::null()));
        REGISTRAR_BUNDLE_ID.with(|c: &core::cell::Cell<u64>| c.set(0));
    }
}

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

    /// All runtime identifiers this loader handles.
    ///
    /// Defaults to a single-element vec containing `runtime_name()`.
    /// Override this method if your loader handles multiple runtime names.
    /// `JsLoader` does NOT need to override this — each `JsLoader` instance handles exactly one name.
    fn runtime_names(&self) -> Vec<String> {
        vec![self.runtime_name().to_owned()]
    }

    /// Load a bundle at `path` and register its vtables via `registrar`.
    ///
    /// # Errors
    /// Returns `Err(PolyplugError::...)` on any failure. For stub loaders,
    /// returns `Err(PolyplugError::Loader(LoaderError::JsRuntimePanic { .. }))`.
    fn load(&self, path: &Path, registrar: &mut PluginRegistrar) -> Result<(), PolyplugError>;
}

/// The built-in loader for native (Rust/C++/NativeAOT) bundles.
///
/// Uses dlopen (via libloading) to load `.so` / `.dll` / `.dylib` files.
/// Automatically registered during `RuntimeBuilder::build()` unless a user-provided
/// loader already claims the `"native"` runtime name.
/// The `Library` handle for each loaded bundle is stored in the injected `Registry`,
/// not in this struct, to guarantee it outlives all vtable function pointers.
pub(crate) struct NativeBundleLoader {
    registry: Arc<Registry>,
    host_vtable: &'static HostVTable,
}

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

    /// Load a native bundle by calling `load_bundle()`.
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

        // Derive the bundle directory from the .so file path (parent directory).
        let bundle_dir: &Path = path.parent().unwrap_or(path);
        let manifest: ManifestData =
            parse_manifest(bundle_dir).map_err(|e: LoaderError| PolyplugError::Loader(e))?;
        if manifest.id == 0 {
            return Err(PolyplugError::Loader(LoaderError::InitFailed {
                bundle: path.display().to_string(),
                error: "manifest.id is required but was 0 or missing".to_owned(),
            }));
        }
        load_bundle(path, &manifest, &self.registry, self.host_vtable)
            .map_err(|e: LoaderError| PolyplugError::Loader(e))
    }
}

/// A successfully loaded bundle.
//
//  The `library` field is intentionally never dropped — it lives for the entire
//  process lifetime. All vtable function pointers extracted from it are 'static.
pub struct LoadedBundle {
    pub path: PathBuf,
    /// libloading handle — intentionally leaked (never dropped).
    pub library: Box<libloading::Library>,
}

/// Read and parse `manifest.toml` from a bundle directory.
///
/// `bundle_dir` must be a path to a directory containing `manifest.toml`.
///
/// # Errors
/// - `BundleNotADirectory`: if `bundle_dir` is not a directory
/// - `ManifestParse`: if `manifest.toml` is not found or is malformed
/// - `ManifestMissingFile`: if the `file` field is absent or empty
pub fn parse_manifest(bundle_dir: &Path) -> Result<ManifestData, LoaderError> {
    if !bundle_dir.is_dir() {
        return Err(LoaderError::BundleNotADirectory {
            path: bundle_dir.to_path_buf(),
        });
    }

    let manifest_path: PathBuf = bundle_dir.join("manifest.toml");

    if !manifest_path.exists() {
        return Err(LoaderError::ManifestParse {
            path: manifest_path.to_string_lossy().into_owned(),
            reason: "manifest.toml not found in bundle directory".to_owned(),
        });
    }

    let contents: String =
        std::fs::read_to_string(&manifest_path).map_err(|_e: std::io::Error| {
            LoaderError::ManifestParse {
                path: manifest_path.to_string_lossy().into_owned(),
                reason: "failed to read manifest file".to_owned(),
            }
        })?;

    let mut data: ManifestData =
        ManifestData::parse_from_str(&contents).map_err(|e| LoaderError::ManifestParse {
            path: manifest_path.to_string_lossy().into_owned(),
            reason: match e {
                LoaderError::ManifestParse { reason, .. } => reason,
                _ => e.to_string(),
            },
        })?;

    let trimmed: &str = data.runtime.trim();
    if trimmed.is_empty() {
        return Err(LoaderError::ManifestParse {
            path: manifest_path.to_string_lossy().into_owned(),
            reason: "runtime field cannot be empty".to_owned(),
        });
    }
    data.runtime = trimmed.to_owned();
    data.validate_file()?;
    data.path = bundle_dir.to_path_buf();
    Ok(data)
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
/// 5. Call `polyplug_init` with a `PluginRegistrar` callback.
/// 6. On init failure: propagate the error. The library remains in
///    `registry.loaded_libraries` — the never-unload invariant applies.
pub fn load_bundle(
    path: &Path,
    manifest: &ManifestData,
    registry: &Registry,
    host_vtable: &'static HostVTable,
) -> Result<(), LoaderError> {
    let path_str: String = path.to_string_lossy().into_owned();
    let bundle_dir: &Path = path.parent().unwrap_or_else(|| Path::new("."));

    // Step 0: manifest is provided by the caller — no internal parse needed.

    // Resolve dependency contract_ids and declare them to the registry
    let dep_contract_ids: Vec<u64> = manifest
        .dependencies
        .iter()
        .map(|dep: &RawManifestDependency| {
            if dep.contract_id != 0 {
                dep.contract_id
            } else {
                polyplug_abi::contract_id(&dep.contract, 0)
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
    // signature: extern "C" fn(*mut PluginRegistrar, *const PluginContext) -> AbiError.
    // Symbol<F> derefs to F (a fn pointer). Fn pointers are Copy — copying does not
    // extend the lifetime of `library`. The pointer remains valid as long as `library`
    // is alive. `library` is moved into `registry.loaded_libraries` immediately after,
    // so the pointer is always valid while reachable.
    let init_fn_ptr: unsafe extern "C" fn(
        *mut PluginRegistrar,
        *const polyplug_abi::PluginContext,
    ) -> AbiErrorType = {
        // SAFETY: polyplug_init is resolved from a successfully loaded library.
        // libloading's get() returns Err if the symbol doesn't exist; we propagate via ?.
        // The returned Symbol borrows `library` and is valid for the duration of this block.
        let sym: libloading::Symbol<
            '_,
            unsafe extern "C" fn(
                *mut PluginRegistrar,
                *const polyplug_abi::PluginContext,
            ) -> AbiErrorType,
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

    // Step 4: Set thread-locals for registrar_callback, then install RAII guard.
    // The guard clears all three thread-locals on drop (even on panic).
    REGISTRAR_REGISTRY_PTR.with(|c: &core::cell::Cell<*const Registry>| {
        c.set(registry as *const Registry);
    });
    REGISTRAR_BUNDLE_ID.with(|c: &core::cell::Cell<u64>| c.set(manifest.id));
    crate::runtime::INIT_BUNDLE_ID.with(|c: &core::cell::Cell<u64>| c.set(manifest.id));
    // Guard clears all three thread-locals on drop (even on panic)
    let _bundle_guard: BundleInitGuard = BundleInitGuard;

    // Step 5: Build PluginRegistrar with callback and host vtable
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: registrar_callback,
        host: host_vtable as *const HostVTable,
    };

    // Step 6: Create PluginContext with bundle path and host ABI version
    let bundle_path_sv: polyplug_abi::StringView = polyplug_abi::StringView {
        ptr: bundle_dir.as_os_str().as_encoded_bytes().as_ptr(),
        len: bundle_dir.as_os_str().as_encoded_bytes().len(),
    };
    let ctx: polyplug_abi::PluginContext = polyplug_abi::PluginContext {
        bundle_path: bundle_path_sv,
        host_abi_version: POLYPLUG_ABI_VERSION,
    };

    // Step 7: Call init with registrar and context
    // SAFETY: init_fn_ptr was resolved from the library (now stored in registry).
    // The PluginRegistrar and PluginContext are valid for the duration of the call.
    // Thread-locals are set above and will be cleared by _bundle_guard on drop.
    let init_result: AbiError = unsafe {
        init_fn_ptr(
            &mut registrar as *mut PluginRegistrar,
            &ctx as *const polyplug_abi::PluginContext,
        )
    };

    if init_result.code != ABI_OK {
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

/// Set up TLS context for the registrar callback and return a PluginRegistrar + RAII guard.
///
/// Used by `RuntimeBuilder::build()` when dispatching non-native loaders.
/// The `BundleInitGuard` returned clears all three TLS variables on drop,
/// ensuring per-bundle state does not leak across loader invocations.
///
/// # Safety
/// The caller must ensure `registry` outlives the returned `PluginRegistrar`.
/// `host_vtable` must be `'static`.
pub(crate) fn make_registrar_context(
    registry: &Registry,
    bundle_id: u64,
    host_vtable: &'static HostVTable,
) -> (PluginRegistrar, BundleInitGuard) {
    REGISTRAR_REGISTRY_PTR.with(|c: &core::cell::Cell<*const Registry>| {
        c.set(registry as *const Registry);
    });
    REGISTRAR_BUNDLE_ID.with(|c: &core::cell::Cell<u64>| c.set(bundle_id));
    crate::runtime::INIT_BUNDLE_ID.with(|c: &core::cell::Cell<u64>| c.set(bundle_id));
    let registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: registrar_callback,
        host: host_vtable as *const HostVTable,
    };
    (registrar, BundleInitGuard)
}

/// The `register_plugin` callback passed to plugins in their `PluginRegistrar`.
//
//  Called by the plugin during `polyplug_init` to register vtables.
//  Uses thread-local storage (set by load_bundle before calling polyplug_init)
//  to recover the registry and bundle_id context through the FFI boundary.
//  This is safe because polyplug_init is called synchronously on a single thread,
//  and BundleInitGuard ensures the thread-locals are cleared after init returns.
pub(crate) unsafe extern "C" fn registrar_callback(
    _registrar: *mut PluginRegistrar,
    descriptor: *const PluginDescriptor,
    vtable: *const PluginVTable,
) -> AbiError {
    let registry_ptr: *const Registry =
        REGISTRAR_REGISTRY_PTR.with(|c: &core::cell::Cell<*const Registry>| c.get());
    let bundle_id: u64 = REGISTRAR_BUNDLE_ID.with(|c: &core::cell::Cell<u64>| c.get());
    if registry_ptr.is_null() {
        return AbiError {
            code: 1,
            message: polyplug_abi::StringView::null(),
        };
    }
    // SAFETY: registry_ptr was set by load_bundle() immediately before calling polyplug_init
    // on this thread. The Registry is stored in Arc<Registry> and outlives this synchronous callback.
    let registry: &Registry = unsafe { &*registry_ptr };
    // SAFETY: descriptor and vtable are provided by the plugin's polyplug_init function.
    // They point to static data in the plugin binary (which is never unloaded per the invariant).
    let desc: PluginDescriptor = unsafe { *descriptor };
    // SAFETY: desc.contract_name is a StringView from the plugin binary, valid for the call.
    // Contract name must be provided by the plugin - no fallback.
    if desc.contract_name.ptr.is_null() || desc.contract_name.len == 0 {
        return AbiError {
            code: 1,
            message: polyplug_abi::StringView::from_static(
                b"PluginDescriptor.contract_name is null or empty",
            ),
        };
    }
    // SAFETY: desc.contract_name.ptr is non-null, valid UTF-8 for len bytes. Never unloaded.
    let contract_name: String = unsafe { desc.contract_name.to_string_owned() };
    // SAFETY: vtable is a valid 'static PluginVTable from the plugin binary.
    // Registry::register is marked unsafe because it dereferences vtable_ptr internally.
    match unsafe { registry.register(desc, vtable, contract_name, bundle_id) } {
        Ok(_handle) => AbiError::ok(),
        Err(e) => {
            eprintln!("[polyplug] registration failed for bundle {bundle_id}: {e}");
            AbiError {
                code: 1,
                message: polyplug_abi::StringView::null(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use core::ptr;
    use polyplug_abi::ABI_ERROR_GENERIC;

    use crate::registry::Registry;
    use polyplug_abi::AbiError;
    use polyplug_abi::HostVTable;
    use polyplug_abi::PluginDescriptor;
    use polyplug_abi::PluginHandle;
    use polyplug_abi::PluginRegistrar;
    use polyplug_abi::PluginVTable;
    use polyplug_abi::StringView;

    const EMPTY_FNS: [*const (); 0] = [];

    static VTABLE_MALFORMED: PluginVTable = PluginVTable {
        contract_id: 0xA1B2_C3D4_E5F6_0001_u64,
        contract_version: 0_u32,
        function_count: 0_u32,
        functions: EMPTY_FNS.as_ptr(),
    };

    static VTABLE_DUPLICATE: PluginVTable = PluginVTable {
        contract_id: 0xDEAD_BEEF_0000_0001_u64,
        contract_version: 0_u32,
        function_count: 0_u32,
        functions: EMPTY_FNS.as_ptr(),
    };

    unsafe extern "C" fn stub_alloc(_size: usize, _align: usize) -> *mut u8 {
        ptr::null_mut()
    }

    unsafe extern "C" fn stub_free(_ptr: *mut u8, _size: usize, _align: usize) {}

    unsafe extern "C" fn stub_find_by_contract(
        _contract_id: u64,
        _min_version: u32,
    ) -> PluginHandle {
        PluginHandle::null()
    }

    unsafe extern "C" fn stub_find_by_bundle(
        _bundle_id: u64,
        _contract_id: u64,
        _min_version: u32,
    ) -> PluginHandle {
        PluginHandle::null()
    }

    unsafe extern "C" fn stub_find_all_by_contract(
        _contract_id: u64,
        _min_version: u32,
        _out: *mut PluginHandle,
        _out_cap: usize,
    ) -> usize {
        0_usize
    }

    unsafe extern "C" fn stub_resolve_plugin(_handle: PluginHandle) -> *const PluginVTable {
        ptr::null()
    }

    unsafe extern "C" fn stub_get_extension(_extension_id: u32) -> *const () {
        ptr::null()
    }

    static HOST_VTABLE: HostVTable = HostVTable {
        alloc: stub_alloc,
        free: stub_free,
        find_by_contract: stub_find_by_contract,
        find_by_bundle: stub_find_by_bundle,
        find_all_by_contract: stub_find_all_by_contract,
        resolve_plugin: stub_resolve_plugin,
        get_extension: stub_get_extension,
    };

    #[allow(dead_code)] // guard is held for its Drop impl to clear TLS
    struct RegistrarContext {
        registrar: PluginRegistrar,
        guard: super::BundleInitGuard,
    }

    impl RegistrarContext {
        fn registrar_mut(&mut self) -> &mut PluginRegistrar {
            &mut self.registrar
        }
    }

    fn make_registrar_context(
        registry: &Registry,
        bundle_id: u64,
        host_vtable: &'static HostVTable,
    ) -> RegistrarContext {
        let (registrar, guard) = super::make_registrar_context(registry, bundle_id, host_vtable);
        RegistrarContext { registrar, guard }
    }

    #[test]
    fn registrar_callback_null_registry_ptr_returns_error() {
        let registry: Registry = Registry::new();
        let bundle_id: u64 = 0xABCD_u64;
        let mut context: RegistrarContext =
            make_registrar_context(&registry, bundle_id, &HOST_VTABLE);
        super::REGISTRAR_REGISTRY_PTR
            .with(|c: &core::cell::Cell<*const Registry>| c.set(core::ptr::null()));

        let descriptor: *const PluginDescriptor = ptr::null();
        let vtable: *const PluginVTable = ptr::null();
        let registrar: &mut PluginRegistrar = context.registrar_mut();
        // SAFETY: register_plugin is a valid fn pointer; registrar is valid for the call.
        let result: AbiError = unsafe {
            (registrar.register_plugin)(registrar as *mut PluginRegistrar, descriptor, vtable)
        };

        assert_eq!(result.code, 1_u32);
        assert!(result.message.ptr.is_null());
        assert_eq!(result.message.len, 0_usize);
    }

    #[test]
    fn registrar_callback_accepts_null_stringviews() {
        let registry: Registry = Registry::new();
        let bundle_id: u64 = 0x1000_u64;
        let mut context: RegistrarContext =
            make_registrar_context(&registry, bundle_id, &HOST_VTABLE);

        let descriptor: PluginDescriptor = PluginDescriptor {
            name: StringView::null(),
            contract_name: StringView::null(),
            version_major: 1_u32,
            version_minor: 0_u32,
            version_patch: 0_u32,
        };
        let descriptor_ptr: *const PluginDescriptor = &descriptor as *const PluginDescriptor;
        let vtable: *const PluginVTable = &VTABLE_MALFORMED as *const PluginVTable;

        let registrar: &mut PluginRegistrar = context.registrar_mut();
        // SAFETY: register_plugin is a valid fn pointer; all args are valid for the call.
        let result: AbiError = unsafe {
            (registrar.register_plugin)(registrar as *mut PluginRegistrar, descriptor_ptr, vtable)
        };

        assert_eq!(result.code, ABI_ERROR_GENERIC);
    }

    #[test]
    fn registrar_callback_duplicate_provider_returns_error() {
        let registry: Registry = Registry::new();
        let bundle_id: u64 = 0xBEEF_u64;
        let mut context: RegistrarContext =
            make_registrar_context(&registry, bundle_id, &HOST_VTABLE);

        let descriptor: PluginDescriptor = PluginDescriptor {
            name: StringView::from_static(b"dup-provider"),
            contract_name: StringView::from_static(b"dup.contract"),
            version_major: 1_u32,
            version_minor: 0_u32,
            version_patch: 0_u32,
        };
        let descriptor_ptr: *const PluginDescriptor = &descriptor as *const PluginDescriptor;
        let vtable: *const PluginVTable = &VTABLE_DUPLICATE as *const PluginVTable;

        let registrar: &mut PluginRegistrar = context.registrar_mut();
        // SAFETY: register_plugin is a valid fn pointer; all args are valid for the call.
        let first: AbiError = unsafe {
            (registrar.register_plugin)(registrar as *mut PluginRegistrar, descriptor_ptr, vtable)
        };
        assert_eq!(first.code, 0_u32);

        // SAFETY: same as above — testing duplicate registration path.
        let second: AbiError = unsafe {
            (registrar.register_plugin)(registrar as *mut PluginRegistrar, descriptor_ptr, vtable)
        };
        assert_eq!(second.code, 1_u32);
    }
}

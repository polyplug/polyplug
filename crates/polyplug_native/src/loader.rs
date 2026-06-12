//! Native bundle loader — loads .so/.dll/.dylib plugins.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use polyplug::Runtime;
use polyplug::error::{LoaderError, RuntimeError};
use polyplug::loader::{BundleLoader, BundleSource, ManifestData};
use polyplug::logger::RecoverPoisoned;
use polyplug_abi::HostApi;
use polyplug_abi::POLYPLUG_ABI_VERSION;
use polyplug_abi::UnloadMode;
use polyplug_abi::plugin::BundleInitContext;
use polyplug_abi::types::AbiError;
use polyplug_abi::types::AbiErrorCode;
use polyplug_abi::types::LogLevel;
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
    /// calls complete (docs/TRUST_MODEL.md §Hot-Reload Safety Guarantees).
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

    /// Handle a `load()` failure that occurred AFTER `polyplug_init` was invoked.
    ///
    /// init may have already registered one or more live, resolvable interfaces
    /// before reporting failure (or panicking). Two things must happen, in order:
    ///
    /// 1. Invalidate whatever the failed init registered so the runtime retires
    ///    those interfaces (the generation bump makes any published handle stale).
    ///    `invalidate_bundle` returns `Ok((0, _))` when nothing was registered.
    /// 2. RETIRE the library instead of dropping it. A library whose init ran must
    ///    never be `dlclose`d here: its 'static registration data (descriptor /
    ///    function-pointer arrays) backs the now-retired interface, and unmapping
    ///    its code/data pages would dangle pointers the registry still holds.
    fn retire_failed_init(
        &self,
        bundle_name: &str,
        library: libloading::Library,
        runtime: &Runtime,
    ) {
        let bundle_id: BundleId = BundleId::new(bundle_name);
        // Ignore the slot count / retired Arcs: we only need the interfaces retired.
        let _ = runtime.registry().invalidate_bundle(bundle_id);
        self.retired
            .lock()
            .recover_poisoned(runtime.logger(), "loader.native")
            .push(library);
    }
}

impl BundleLoader for NativeLoader {
    fn loader_name(&self) -> &'static str {
        "native"
    }

    fn load(
        &self,
        manifest: &ManifestData,
        source: &BundleSource,
        runtime: &Runtime,
    ) -> Result<(), RuntimeError> {
        // The native loader supports on-disk bundles only: there is no clean,
        // portable in-memory dlopen on Windows/macOS, so Code/Bytes are rejected.
        match source {
            BundleSource::Path(_) => {}
            BundleSource::Code(_) | BundleSource::Bytes(_) => {
                return Err(RuntimeError::Loader(LoaderError::UnsupportedBundleSource {
                    loader: "native",
                    source_kind: source.kind(),
                    bundle: manifest.name.clone(),
                }));
            }
        }

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
        // New signature: fn(host_abi: *const HostApi, ctx: *const BundleInitContext) -> AbiError
        let init_fn_ptr: unsafe extern "C" fn(
            *const HostApi,
            *const BundleInitContext,
        ) -> AbiError = {
            let sym: libloading::Symbol<
                '_,
                unsafe extern "C" fn(*const HostApi, *const BundleInitContext) -> AbiError,
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
        // All strings crossing the ABI are UTF-8 (`StringView`). A non-UTF-8 (or WTF-8
        // on Windows) bundle path cannot be smuggled across as "UTF-8": reject it with
        // a clear error instead.
        let bundle_dir: &std::path::Path =
            bundle_path.parent().unwrap_or(std::path::Path::new("."));
        let bundle_dir_str: &str = match bundle_dir.to_str() {
            Some(s) => s,
            None => {
                return Err(RuntimeError::Loader(LoaderError::InitFailed {
                    bundle: manifest.name.clone(),
                    error: format!(
                        "bundle path is not valid UTF-8: {}",
                        bundle_dir.to_string_lossy()
                    ),
                }));
            }
        };
        let ctx: BundleInitContext = BundleInitContext {
            bundle_id: BundleId::new(&manifest.name).id(),
            bundle_path: polyplug_abi::types::StringView {
                ptr: bundle_dir_str.as_ptr(),
                len: bundle_dir_str.len(),
            },
        };

        // ─── Step 5: Push bundle_id for dependency enforcement ────────────────────────
        let expected_bundle_id: BundleId = BundleId::new(&manifest.name);
        runtime.push_init_bundle_id(expected_bundle_id.id());

        // ─── Step 6: Get HostApi and call init (panic-isolated) ──────────────────
        // A panicking plugin init must not unwind across the C ABI (UB / process abort).
        // catch_unwind contains it and maps it to a proper LoaderError. The init stack
        // is popped on BOTH the success and panic paths so it never leaks an entry.
        let host_abi: &'static HostApi = runtime.host_abi();
        let init_outcome: Result<AbiError, Box<dyn core::any::Any + Send>> =
            std::panic::catch_unwind(core::panic::AssertUnwindSafe(|| {
                // SAFETY: host_abi is a valid HostApi reference obtained from the runtime.
                // init_fn_ptr is a valid function pointer resolved from the plugin library.
                // ctx is a stack-allocated BundleInitContext that outlives the call.
                unsafe { init_fn_ptr(host_abi as *const HostApi, &ctx) }
            }));

        // ─── Step 7: Pop bundle_id (always, including panic path) ──────────────────────
        runtime.pop_init_bundle_id();

        let init_result: AbiError = match init_outcome {
            Ok(result) => result,
            Err(_panic) => {
                // init ran (and may have registered a live interface) before panicking:
                // invalidate its registrations and RETIRE the library — never dlclose a
                // library whose init ran, its statics may back a retired interface.
                self.retire_failed_init(&manifest.name, library, runtime);
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
            // init ran and may have registered a contract before reporting failure:
            // invalidate those registrations and retire (do not dlclose) the library.
            self.retire_failed_init(&manifest.name, library, runtime);
            return Err(RuntimeError::Loader(LoaderError::InitFailed {
                bundle: manifest.name.clone(),
                error: error_msg,
            }));
        }

        // ─── Step 8: Store library handle ─────────────────────────────────────────────
        // If a bundle with the same id was already loaded (e.g. its file was replaced
        // on disk → a different mapping), RETIRE the superseded handle instead of
        // dropping it: old registry slots may still resolve raw fn pointers into the
        // prior mapping, and dlclosing it would dangle them.
        let bundle_id: BundleId = BundleId::new(&manifest.name);
        let superseded: Option<libloading::Library> = self
            .libraries
            .lock()
            .recover_poisoned(runtime.logger(), "loader.native")
            .insert(bundle_id, library);
        if let Some(old_library) = superseded {
            self.retired
                .lock()
                .recover_poisoned(runtime.logger(), "loader.native")
                .push(old_library);
        }

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
            *const HostApi,
            *const BundleInitContext,
        ) -> AbiError = {
            let sym: libloading::Symbol<
                '_,
                unsafe extern "C" fn(*const HostApi, *const BundleInitContext) -> AbiError,
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
        // All strings crossing the ABI are UTF-8 (`StringView`). Reject a non-UTF-8
        // (or WTF-8 on Windows) bundle path rather than smuggling it across.
        let bundle_dir: &std::path::Path =
            bundle_path.parent().unwrap_or(std::path::Path::new("."));
        let bundle_dir_str: &str = match bundle_dir.to_str() {
            Some(s) => s,
            None => {
                return Err(RuntimeError::Loader(LoaderError::InitFailed {
                    bundle: manifest.name.clone(),
                    error: format!(
                        "bundle path is not valid UTF-8: {}",
                        bundle_dir.to_string_lossy()
                    ),
                }));
            }
        };
        let ctx: BundleInitContext = BundleInitContext {
            bundle_id: BundleId::new(&manifest.name).id(),
            bundle_path: polyplug_abi::types::StringView {
                ptr: bundle_dir_str.as_ptr(),
                len: bundle_dir_str.len(),
            },
        };

        // ─── Step 5: Push bundle_id for dependency enforcement ──────────────────────────
        let expected_bundle_id: BundleId = BundleId::new(&manifest.name);
        runtime.push_init_bundle_id(expected_bundle_id.id());

        // ─── Step 6: Get HostApi and call init (panic-isolated) ────────────────────
        // A panicking plugin init must not unwind across the C ABI. catch_unwind contains
        // it; the init stack is popped on both the success and panic paths.
        let host_abi: &'static HostApi = runtime.host_abi();
        let init_outcome: Result<AbiError, Box<dyn core::any::Any + Send>> =
            std::panic::catch_unwind(core::panic::AssertUnwindSafe(|| {
                // SAFETY: host_abi is a valid HostApi reference obtained from the runtime.
                // init_fn_ptr is a valid function pointer resolved from the new plugin library.
                // ctx is a stack-allocated BundleInitContext that outlives the call.
                unsafe { init_fn_ptr(host_abi as *const HostApi, &ctx) }
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
            .recover_poisoned(runtime.logger(), "loader.native")
            .remove(&bundle_id)
        {
            self.retired
                .lock()
                .recover_poisoned(runtime.logger(), "loader.native")
                .push(old_library);
        }

        // ─── Step 9: Store new library ───────────────────────────────────────────────────
        self.libraries
            .lock()
            .recover_poisoned(runtime.logger(), "loader.native")
            .insert(bundle_id, new_library);

        Ok(())
    }

    /// Reclaim the bundle's `libloading::Library` according to the runtime's
    /// [`UnloadMode`].
    ///
    /// # Safety model — host-attested, NOT runtime-verified
    /// Native dispatch is zero-overhead by design: once the host resolves a contract,
    /// it calls a RAW function pointer that points directly into this library's code
    /// pages. The runtime never mediates those calls and keeps NO native-call counter,
    /// so it is *structurally blind* to whether a thread is executing inside the
    /// library right now. `dlclose` (dropping the `Library`) while such a call is in
    /// flight unmaps the code pages out from under it — a use-after-free (SIGSEGV).
    ///
    /// Consequently `UnloadMode::Reclaim` for a native bundle is an explicit HOST
    /// ATTESTATION: by selecting it the host guarantees that no thread is calling — or
    /// holds a raw pointer into — this bundle at unload time. This is the same
    /// trusted-same-process contract that hot-reload's `Preparing` phase relies on, and
    /// it is the documented price of zero-overhead native dispatch — not a bug.
    ///
    /// `reclaim_safe` is a best-effort secondary net computed by the runtime from the
    /// retired interfaces' `Arc::strong_count`. It catches *Arc-holding* paths (e.g. a
    /// future instance counter) and defers reclaim when one is found. It CANNOT see raw
    /// in-flight native calls — only the host attestation above covers those. When the
    /// hint says "unsafe", this loader retires the library instead of dropping it.
    ///
    /// Under `UnloadMode::Retire` (the default) the library is always kept mapped
    /// (retire-not-drop), exactly as before this hook existed, so previously resolved
    /// raw function pointers remain valid for the loader's lifetime.
    fn unload(
        &self,
        bundle_id: BundleId,
        runtime: &Runtime,
        reclaim_safe: bool,
    ) -> Result<(), RuntimeError> {
        let mode: UnloadMode = runtime.config().unload_mode;

        // Remove the live handle; nothing to do if this bundle isn't loaded by us.
        let library: libloading::Library = match self
            .libraries
            .lock()
            .recover_poisoned(runtime.logger(), "loader.native")
            .remove(&bundle_id)
        {
            Some(lib) => lib,
            None => return Ok(()),
        };

        match mode {
            UnloadMode::Retire => {
                // Keep the library mapped (retire-not-drop) — the current default.
                // Any raw function pointer already resolved into its code pages stays
                // valid for the loader's lifetime.
                self.retired
                    .lock()
                    .recover_poisoned(runtime.logger(), "loader.native")
                    .push(library);
            }
            UnloadMode::Reclaim => {
                if reclaim_safe {
                    // dlclose: dropping the Library unmaps its code pages and releases
                    // the on-disk file lock (on Windows) so the developer can rebuild
                    // and reload the bundle.
                    //
                    // SAFETY (host-attested): this is sound ONLY because the host
                    // selected `UnloadMode::Reclaim`, attesting that no thread is
                    // calling — or holds a raw pointer into — this bundle. The runtime
                    // cannot verify that for native dispatch (it is structurally blind
                    // to in-flight raw calls); `reclaim_safe` only rules out Arc-holding
                    // paths. See the impl-level doc comment for the full safety model.
                    drop(library);
                } else {
                    // Best-effort defer: an Arc holder still references this bundle's
                    // interface. Retire instead of risking a use-after-free.
                    runtime.logger().log(LogLevel::Warn, "loader.native", || {
                        format!(
                            "reclaim of bundle {:#x} deferred: an interface still has an extra holder; retiring its library to avoid a use-after-free",
                            bundle_id.id()
                        )
                    });
                    self.retired
                        .lock()
                        .recover_poisoned(runtime.logger(), "loader.native")
                        .push(library);
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
impl NativeLoader {
    /// Number of live (currently loaded) library handles. Test-only accessor.
    pub(crate) fn live_library_count(&self) -> usize {
        self.libraries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }

    /// Number of retired (kept-mapped) library handles. Test-only accessor.
    pub(crate) fn retired_library_count(&self) -> usize {
        self.retired
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod unload_tests {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use polyplug::Runtime;
    use polyplug::loader::{BundleLoader, BundleSource, ManifestData, parse_manifest};
    use polyplug_abi::UnloadMode;
    use polyplug_abi::runtime::RuntimeConfig;
    use polyplug_utils::BundleId;

    use crate::config::NativeConfig;
    use crate::loader::NativeLoader;

    /// Locate the pre-built `test_plugin` bundle directory.
    ///
    /// The fixture is produced by `tests/fixtures/build_all.sh` and lives at
    /// `<workspace>/tests/fixtures/test_plugin_dir`. `CARGO_MANIFEST_DIR` for this
    /// crate is `<workspace>/crates/polyplug_native`, so the fixture is two levels up.
    fn test_plugin_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("tests")
            .join("fixtures")
            .join("test_plugin_dir")
    }

    /// Build a `Runtime` with the given `unload_mode`. No loader is registered: the
    /// test drives a directly-constructed `NativeLoader`, using the runtime only for
    /// `host_abi()` / `config().unload_mode`.
    fn runtime_with_mode(mode: UnloadMode) -> Arc<Runtime> {
        Runtime::builder()
            .config(RuntimeConfig {
                unload_mode: mode,
                ..Default::default()
            })
            .build()
            .expect("runtime build should succeed")
    }

    /// Load the `test_plugin` fixture through a freshly-constructed `NativeLoader`.
    /// Returns the loader and the bundle id so the test can drive `unload` directly.
    fn load_test_plugin(runtime: &Runtime) -> (NativeLoader, BundleId) {
        let dir: PathBuf = test_plugin_dir();
        let manifest: ManifestData =
            parse_manifest(&dir).expect("parse_manifest for test_plugin_dir");
        let bundle_id: BundleId = BundleId::new(&manifest.name);
        let source: BundleSource = BundleSource::Path(manifest.path.clone());
        let loader: NativeLoader = NativeLoader::new(NativeConfig::default());
        loader
            .load(&manifest, &source, runtime)
            .expect("native load of test_plugin should succeed");
        (loader, bundle_id)
    }

    /// Under `UnloadMode::Retire`, unload keeps the library mapped (retire-not-drop).
    #[test]
    #[cfg(not(miri))]
    fn retire_mode_keeps_library_mapped() {
        let runtime: Arc<Runtime> = runtime_with_mode(UnloadMode::Retire);
        let (loader, bundle_id): (NativeLoader, BundleId) = load_test_plugin(&runtime);
        assert_eq!(loader.live_library_count(), 1);
        assert_eq!(loader.retired_library_count(), 0);

        // reclaim_safe is irrelevant under Retire; pass true to prove it is ignored.
        loader
            .unload(bundle_id, &runtime, true)
            .expect("unload should succeed");

        assert_eq!(loader.live_library_count(), 0, "live handle removed");
        assert_eq!(
            loader.retired_library_count(),
            1,
            "Retire mode must keep the library mapped"
        );
    }

    /// Under `UnloadMode::Reclaim` with a quiescent (reclaim_safe) bundle, unload
    /// DROPS the library (dlclose) rather than retiring it.
    #[test]
    #[cfg(not(miri))]
    fn reclaim_mode_drops_quiescent_library() {
        let runtime: Arc<Runtime> = runtime_with_mode(UnloadMode::Reclaim);
        let (loader, bundle_id): (NativeLoader, BundleId) = load_test_plugin(&runtime);
        assert_eq!(loader.live_library_count(), 1);

        loader
            .unload(bundle_id, &runtime, true)
            .expect("unload should succeed");

        assert_eq!(loader.live_library_count(), 0, "live handle removed");
        assert_eq!(
            loader.retired_library_count(),
            0,
            "Reclaim mode with a safe bundle must dlclose (drop) the library"
        );
    }

    /// Directly exercise the loader decision under `UnloadMode::Reclaim`:
    /// `reclaim_safe = false` defers (retires) the library to avoid a use-after-free.
    #[test]
    #[cfg(not(miri))]
    fn reclaim_mode_defers_when_not_safe() {
        let runtime: Arc<Runtime> = runtime_with_mode(UnloadMode::Reclaim);
        let (loader, bundle_id): (NativeLoader, BundleId) = load_test_plugin(&runtime);

        loader
            .unload(bundle_id, &runtime, false)
            .expect("unload should succeed");

        assert_eq!(loader.live_library_count(), 0, "live handle removed");
        assert_eq!(
            loader.retired_library_count(),
            1,
            "reclaim_safe=false must retire (defer) instead of dropping"
        );
    }

    /// Locate the pre-built `register_fail_plugin` bundle directory. Its
    /// `polyplug_init` registers one contract and THEN returns a non-Ok error,
    /// exercising the loader's "init published an interface before failing" path.
    fn register_fail_plugin_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("tests")
            .join("fixtures")
            .join("register_fail_plugin")
    }

    /// A failed `load()` whose init had already registered a contract must NOT
    /// `dlclose` the library (its registered statics back the published, still-
    /// resolvable interface). The loader instead retires the library and
    /// invalidates whatever the failed init registered.
    ///
    /// Regression for the HIGH finding: previously the local `library` dropped on
    /// the error return → `dlclose` while the registry still held live interfaces
    /// whose fn pointers dangled into unmapped pages.
    #[test]
    #[cfg(not(miri))]
    fn failed_load_after_register_retires_library_and_invalidates() {
        let runtime: Arc<Runtime> = runtime_with_mode(UnloadMode::Retire);
        let dir: PathBuf = register_fail_plugin_dir();
        let manifest: ManifestData =
            parse_manifest(&dir).expect("parse_manifest for register_fail_plugin");
        let bundle_id: BundleId = BundleId::new(&manifest.name);
        let source: BundleSource = BundleSource::Path(manifest.path.clone());
        let loader: NativeLoader = NativeLoader::new(NativeConfig::default());

        let load_result: Result<(), polyplug::error::RuntimeError> =
            loader.load(&manifest, &source, &runtime);
        assert!(
            load_result.is_err(),
            "init returns a non-Ok error, so load() must fail"
        );

        // The library must be RETIRED, never dropped: its registered statics may be
        // referenced by the (now invalidated) interface still living in the registry.
        assert_eq!(
            loader.live_library_count(),
            0,
            "no live handle after a failed load"
        );
        assert_eq!(
            loader.retired_library_count(),
            1,
            "a library whose init ran must be retired, not dlclose'd"
        );

        // The failed init's registration must have been invalidated: re-invalidating
        // the same bundle now reports zero slots (idempotent — nothing left to retire).
        let (count, _retired): (
            u32,
            Vec<std::sync::Arc<polyplug_abi::GuestContractInterface>>,
        ) = runtime
            .registry()
            .invalidate_bundle(bundle_id)
            .expect("invalidate_bundle should succeed");
        assert_eq!(
            count, 0,
            "load() must have already invalidated the failed init's registration"
        );
    }

    /// A second `load()` that re-inserts the same `BundleId` must RETIRE the
    /// superseded `Library` handle (push into `retired`) rather than dropping it:
    /// old registry slots may still resolve raw fn pointers into the prior mapping,
    /// so dlclosing the old mapping would dangle them.
    ///
    /// Regression for the double-load MEDIUM finding (Step 8 `insert` returning the
    /// old handle was previously dropped).
    ///
    /// The runtime rejects a duplicate provider for the same contract, so between
    /// the two loads the bundle's registration is invalidated in the registry
    /// (simulating the contract having been unloaded while the loader still holds
    /// the old library handle). The second `load()` then succeeds at the registry
    /// level and re-inserts the same `BundleId`, exercising the supersede path.
    #[test]
    #[cfg(not(miri))]
    fn double_load_retires_superseded_library() {
        let runtime: Arc<Runtime> = runtime_with_mode(UnloadMode::Retire);
        let dir: PathBuf = test_plugin_dir();
        let manifest: ManifestData =
            parse_manifest(&dir).expect("parse_manifest for test_plugin_dir");
        let bundle_id: BundleId = BundleId::new(&manifest.name);
        let source: BundleSource = BundleSource::Path(manifest.path.clone());
        let loader: NativeLoader = NativeLoader::new(NativeConfig::default());

        loader
            .load(&manifest, &source, &runtime)
            .expect("first native load should succeed");
        assert_eq!(loader.live_library_count(), 1);
        assert_eq!(loader.retired_library_count(), 0);

        // Drop the registry-side registration (the loader still holds the library
        // handle) so the second load's register_guest_contract is not a duplicate.
        runtime
            .registry()
            .invalidate_bundle(bundle_id)
            .expect("invalidate_bundle should succeed");

        loader
            .load(&manifest, &source, &runtime)
            .expect("second native load should succeed");

        assert_eq!(
            loader.live_library_count(),
            1,
            "the new handle replaces the old live handle"
        );
        assert_eq!(
            loader.retired_library_count(),
            1,
            "the superseded library must be retired, not dropped"
        );
    }

    /// A non-UTF-8 bundle path must fail cleanly with a clear "not valid UTF-8"
    /// error instead of being smuggled across the ABI as a UTF-8 `StringView`.
    ///
    /// Regression for the MEDIUM finding: `as_encoded_bytes()` previously crossed
    /// non-UTF-8 (and WTF-8 on Windows) bytes into `BundleInitContext.bundle_path`.
    #[test]
    #[cfg(all(unix, not(miri)))]
    fn non_utf8_bundle_path_fails_cleanly() {
        use std::os::unix::ffi::OsStrExt;

        // Build a temp bundle dir whose name contains an invalid UTF-8 byte (0xFF),
        // copy the test_plugin .so into it, and point a manifest at it.
        let src_dir: PathBuf = test_plugin_dir();
        let src_manifest: ManifestData =
            parse_manifest(&src_dir).expect("parse_manifest for test_plugin_dir");
        let dll_name: String = src_manifest.file.clone();
        let src_dll: PathBuf = src_dir.join(&dll_name);

        let mut name_bytes: Vec<u8> =
            format!("polyplug_native_badutf8_{}_", std::process::id()).into_bytes();
        name_bytes.push(0xFF);
        let dir_name: std::ffi::OsString = std::ffi::OsStr::from_bytes(&name_bytes).to_owned();
        let temp_dir: PathBuf = std::env::temp_dir().join(dir_name);
        std::fs::create_dir_all(&temp_dir).expect("create non-utf8 bundle dir");

        let dest_dll: PathBuf = temp_dir.join(&dll_name);
        std::fs::copy(&src_dll, &dest_dll).expect("copy fixture dll");

        let manifest_toml: String = format!(
            "id = {}\nname = \"{}\"\nversion = \"{}\"\nloader = \"native\"\nprovides = [\"test.add\"]\n\n[file]\nlinux.x86_64 = \"{}\"\nlinux.aarch64 = \"{}\"\nmacos.x86_64 = \"{}\"\nmacos.aarch64 = \"{}\"\n\n[function_count]\n\"test.add@1\" = 1\n",
            src_manifest.id,
            src_manifest.name,
            src_manifest.version,
            dll_name,
            dll_name,
            dll_name,
            dll_name
        );
        std::fs::write(temp_dir.join("manifest.toml"), &manifest_toml)
            .expect("write temp manifest");

        let manifest: ManifestData =
            parse_manifest(&temp_dir).expect("parse_manifest for non-utf8 bundle");
        let source: BundleSource = BundleSource::Path(manifest.path.clone());
        let runtime: Arc<Runtime> = runtime_with_mode(UnloadMode::Retire);
        let loader: NativeLoader = NativeLoader::new(NativeConfig::default());

        let load_result: Result<(), polyplug::error::RuntimeError> =
            loader.load(&manifest, &source, &runtime);

        let err: polyplug::error::RuntimeError =
            load_result.expect_err("non-UTF-8 bundle path must fail cleanly");
        let msg: String = err.to_string();
        assert!(
            msg.contains("not valid UTF-8"),
            "error must mention non-UTF-8 path, got: {msg}"
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    /// On Windows, a mapped DLL holds an exclusive file lock, so `remove_file`
    /// fails with `ERROR_SHARING_VIOLATION` while the DLL is loaded. After an unload
    /// under `UnloadMode::Reclaim` (dlclose), the lock is released and removal
    /// succeeds. This proves real OS-resource reclaim.
    ///
    /// Linux/macOS unlink a mapped file happily, so the assertion proves nothing
    /// there — the test is gated to Windows only.
    ///
    /// Windows gotcha: a held `NamedTempFile` write handle itself keeps the file
    /// locked, so the manifest and DLL copy are written and their handles closed
    /// (plain `std::fs::write`) BEFORE the loader opens the DLL.
    #[test]
    #[cfg(windows)]
    fn reclaim_releases_windows_file_lock() {
        let src_dir: PathBuf = test_plugin_dir();
        let src_manifest: ManifestData =
            parse_manifest(&src_dir).expect("parse_manifest for test_plugin_dir");
        let dll_name: String = src_manifest.file.clone();
        let src_dll: PathBuf = src_dir.join(&dll_name);

        // Unique temp dir for an isolated copy of the bundle.
        let temp_dir: PathBuf = std::env::temp_dir().join(format!(
            "polyplug_native_reclaim_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&temp_dir).expect("create temp bundle dir");

        // Copy the DLL into the temp dir; the write handle is closed by std::fs::copy.
        let dest_dll: PathBuf = temp_dir.join(&dll_name);
        std::fs::copy(&src_dll, &dest_dll).expect("copy fixture dll");

        // Write a minimal manifest pointing at the copied DLL. std::fs::write closes
        // its handle before returning, so no write handle keeps the dir/file locked.
        let manifest_toml: String = format!(
            "id = {}\nname = \"{}\"\nversion = \"{}\"\nloader = \"native\"\nprovides = [\"test.add\"]\n\n[file]\nwindows.x86_64 = \"{}\"\n\n[function_count]\n\"test.add@1\" = 1\n",
            src_manifest.id, src_manifest.name, src_manifest.version, dll_name
        );
        std::fs::write(temp_dir.join("manifest.toml"), manifest_toml).expect("write temp manifest");

        let manifest: ManifestData =
            parse_manifest(&temp_dir).expect("parse_manifest for temp bundle");
        let bundle_id: BundleId = BundleId::new(&manifest.name);
        let source: BundleSource = BundleSource::Path(manifest.path.clone());

        let runtime: Arc<Runtime> = runtime_with_mode(UnloadMode::Reclaim);
        let loader: NativeLoader = NativeLoader::new(NativeConfig::default());
        loader
            .load(&manifest, &source, &runtime)
            .expect("native load of copied bundle should succeed");

        // While loaded, the DLL is mapped and locked: removal must fail.
        assert!(
            std::fs::remove_file(&dest_dll).is_err(),
            "a mapped DLL must be locked on Windows"
        );

        loader
            .unload(bundle_id, &runtime, true)
            .expect("unload should succeed");

        // After dlclose the lock is released — removal must now succeed.
        std::fs::remove_file(&dest_dll)
            .expect("DLL must be removable after Reclaim-mode unload (dlclose)");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}

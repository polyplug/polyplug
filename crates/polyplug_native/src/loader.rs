//! Native bundle loader — loads .so/.dll/.dylib plugins.

use core::slice;
use core::sync::atomic::AtomicU64;
use core::sync::atomic::Ordering;
use std::collections::HashMap;
use std::fs::{self, Metadata};
use std::io::Error as IoError;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[cfg(test)]
use std::sync::PoisonError;

use libloading::{Library, Symbol};

use polyplug::Runtime;
use polyplug::error::LoaderError;
use polyplug::loader::{BundleLoader, BundleSource, ManifestData};
use polyplug::logger::RecoverPoisoned;
use polyplug_abi::HostApi;
use polyplug_abi::POLYPLUG_ABI_VERSION;
use polyplug_abi::SupportedLanguage;
use polyplug_abi::plugin::BundleInitContext;
use polyplug_abi::types::{AbiError, AbiErrorCode, StringView};
use polyplug_utils::BundleId;

use crate::config::NativeConfig;

/// Native (shared library) plugin loader.
///
/// Handles .so/.dll/.dylib bundles using dlopen/LoadLibrary.
/// Owns library handles internally — NOT stored in registry.
pub struct NativeLoader {
    /// Active library handles, keyed by BundleId.
    libraries: Mutex<HashMap<BundleId, Library>>,
    /// Count of libraries scheduled for epoch-deferred reclamation (unload,
    /// reload-superseded, and failed-init paths).
    ///
    /// Test/diagnostic only: epoch collection timing is non-deterministic, but
    /// this counter is incremented the instant a library is handed to
    /// `crossbeam_epoch::pin().defer(...)`, so it deterministically proves the
    /// resource was scheduled for reclaim — NOT parked alive forever. Instance
    /// state (Rule 12).
    scheduled_reclaims: AtomicU64,
}

impl NativeLoader {
    /// Create a new NativeLoader.
    ///
    /// `config` is accepted for API/FFI symmetry with the other loaders; native
    /// plugins require no configuration, so `NativeConfig` (empty) is not stored.
    pub fn new(_config: NativeConfig) -> Self {
        Self {
            libraries: Mutex::new(HashMap::new()),
            scheduled_reclaims: AtomicU64::new(0),
        }
    }

    /// Schedule `library` for epoch-deferred `dlclose` and record the scheduling.
    ///
    /// SAFETY/why: the library is already unreachable by any *new* dispatch (the
    /// bundle has been removed from the registry indices / the loader's live map
    /// before this is called). Any in-flight runtime-mediated call holds a
    /// crossbeam-epoch pin, so `defer` runs the drop (the `dlclose`) only once no
    /// such reader remains; the global epoch coordinates that with the runtime's
    /// reader pins. FFI host callers into native dispatch must quiesce before
    /// unload per the documented host contract (docs/TRUST_MODEL.md). The handle
    /// is an owned `libloading::Library` (`Send + 'static`), so moving it into the
    /// deferred closure is sound.
    fn schedule_reclaim(&self, library: Library) {
        self.scheduled_reclaims.fetch_add(1, Ordering::Relaxed);
        crossbeam_epoch::pin().defer(move || drop(library));
    }

    /// Handle a `load()` failure that occurred AFTER `polyplug_init` was invoked.
    ///
    /// init may have already registered one or more live, resolvable interfaces
    /// before reporting failure (or panicking). Two things must happen, in order:
    ///
    /// 1. Invalidate whatever the failed init registered so the runtime vacates
    ///    those interfaces (the generation bump makes any published handle stale).
    ///    `invalidate_bundle` returns `Ok(0)` when nothing was registered.
    /// 2. SCHEDULE the library for epoch-deferred `dlclose` instead of dropping it
    ///    inline. A library whose init ran must never be `dlclose`d eagerly: its
    ///    'static registration data (descriptor / function-pointer arrays) backs
    ///    the now-invalidated interface, which the runtime keeps epoch-owned in its
    ///    published `ReadView` until quiescent. The global epoch keeps both the
    ///    interface and this library alive together until no reader is pinned.
    fn vacate_failed_init(&self, bundle_name: &str, library: Library, runtime: &Runtime) {
        let bundle_id: BundleId = BundleId::new(bundle_name);
        // Ignore the slot count / vacated Arcs: we only need the interfaces vacated.
        let _ = runtime.registry().invalidate_bundle(bundle_id);
        self.schedule_reclaim(library);
    }
}

impl NativeLoader {
    /// Shared load body used by both `load()` and `reload()`.
    ///
    /// Opens the library at the path declared in `manifest`, checks the ABI
    /// version sentinel, calls `polyplug_init`, and stores the handle. If a
    /// handle for the same bundle was already live (superseded on reload or a
    /// double-load), it is scheduled for epoch-deferred `dlclose` rather than
    /// dropped inline — old registry slots may still hold raw fn pointers into
    /// the prior mapping, so reclaim is deferred until no reader is pinned.
    fn load_inner(
        &self,
        manifest: &ManifestData,
        source: &BundleSource,
        runtime: &Runtime,
    ) -> Result<(), LoaderError> {
        // The native loader supports on-disk bundles only: there is no clean,
        // portable in-memory dlopen on Windows/macOS, so Code/Bytes are rejected.
        match source {
            BundleSource::Path(_) => {}
            BundleSource::Code(_) | BundleSource::Bytes(_) => {
                return Err(LoaderError::UnsupportedBundleSource {
                    loader: "native",
                    source_kind: source.kind(),
                    bundle: manifest.name.clone(),
                });
            }
        }

        if manifest.id == 0 {
            return Err(LoaderError::InitFailed {
                bundle: manifest.name.clone(),
                error: "manifest.id is required but was 0 or missing".to_owned(),
            });
        }

        let bundle_path: PathBuf = if !manifest.file.is_empty() {
            manifest.path.join(&manifest.file)
        } else {
            return Err(LoaderError::ManifestMissingFile {
                bundle: manifest.name.clone(),
            });
        };

        // Path confinement (defense-in-depth): the artifact must resolve to a real
        // file INSIDE the bundle directory. Reject a `manifest.file` that is a symlink
        // or that canonicalizes outside the bundle root (`../../evil.so`, an absolute
        // path), since such an artifact is loaded but never covered by the signing
        // digest.
        let is_symlink: bool = fs::symlink_metadata(&bundle_path)
            .map(|m: Metadata| m.file_type().is_symlink())
            .unwrap_or(false);
        if is_symlink {
            return Err(LoaderError::ArtifactPathEscape {
                bundle: manifest.name.clone(),
                file: manifest.file.clone(),
            });
        }
        let canonical_artifact: PathBuf =
            bundle_path
                .canonicalize()
                .map_err(|e: IoError| LoaderError::InitFailed {
                    bundle: manifest.name.clone(),
                    error: format!(
                        "failed to canonicalize artifact path {}: {}",
                        bundle_path.to_string_lossy(),
                        e
                    ),
                })?;
        let canonical_root: PathBuf =
            manifest
                .path
                .canonicalize()
                .map_err(|e: IoError| LoaderError::InitFailed {
                    bundle: manifest.name.clone(),
                    error: format!(
                        "failed to canonicalize bundle root {}: {}",
                        manifest.path.to_string_lossy(),
                        e
                    ),
                })?;
        if !canonical_artifact.starts_with(&canonical_root) {
            return Err(LoaderError::ArtifactPathEscape {
                bundle: manifest.name.clone(),
                file: manifest.file.clone(),
            });
        }

        let path_str: String = bundle_path.to_string_lossy().into_owned();

        // SAFETY: path points to a compiled plugin bundle; libloading validates the shared library.
        let library: Library = unsafe {
            Library::new(&bundle_path).map_err(|e| LoaderError::InitFailed {
                bundle: manifest.name.clone(),
                error: format!("failed to load plugin library at {}: {}", path_str, e),
            })?
        };

        // SAFETY: polyplug_abi_version is a C function with signature `extern "C" fn() -> u32`.
        let abi_version_symbol: Symbol<'_, unsafe extern "C" fn() -> u32> = unsafe {
            library
                .get(b"polyplug_abi_version\0")
                .map_err(|_| LoaderError::InitFailed {
                    bundle: manifest.name.clone(),
                    error: format!(
                        "missing symbol 'polyplug_abi_version' in bundle '{}'",
                        path_str
                    ),
                })?
        };
        // SAFETY: abi_version_symbol was obtained from library.get() which validated the
        // symbol exists. The function has signature `extern "C" fn() -> u32` and returns
        // a plain u32, so there are no memory safety concerns.
        let found_version: u32 = unsafe { abi_version_symbol() };
        if found_version != POLYPLUG_ABI_VERSION {
            return Err(LoaderError::InitFailed {
                bundle: manifest.name.clone(),
                error: format!(
                    "ABI version mismatch in {}: expected={}, found={}",
                    path_str, POLYPLUG_ABI_VERSION, found_version
                ),
            });
        }

        let init_fn_ptr: unsafe extern "C" fn(
            *const HostApi,
            *const BundleInitContext,
        ) -> AbiError = {
            // SAFETY: polyplug_init is an exported C symbol from the plugin library,
            // validated to exist by library.get(). The signature matches the ABI contract.
            let sym: Symbol<
                '_,
                unsafe extern "C" fn(*const HostApi, *const BundleInitContext) -> AbiError,
            > = unsafe {
                library
                    .get(b"polyplug_init\0")
                    .map_err(|_| LoaderError::InitSymbolMissing {
                        bundle: manifest.name.clone(),
                    })?
            };
            *sym
        };

        // All strings crossing the ABI are UTF-8 (`StringView`). A non-UTF-8 (or WTF-8
        // on Windows) bundle path cannot be smuggled across as "UTF-8": reject it with
        // a clear error instead.
        let bundle_dir: &Path = bundle_path.parent().unwrap_or(Path::new("."));
        let bundle_dir_str: &str = match bundle_dir.to_str() {
            Some(s) => s,
            None => {
                return Err(LoaderError::InitFailed {
                    bundle: manifest.name.clone(),
                    error: format!(
                        "bundle path is not valid UTF-8: {}",
                        bundle_dir.to_string_lossy()
                    ),
                });
            }
        };
        let ctx: BundleInitContext = BundleInitContext {
            bundle_id: BundleId::new(&manifest.name).id(),
            bundle_path: StringView {
                ptr: bundle_dir_str.as_ptr(),
                len: bundle_dir_str.len(),
            },
        };

        let expected_bundle_id: BundleId = BundleId::new(&manifest.name);
        runtime.push_init_bundle_id(expected_bundle_id.id());

        // The runtime does NOT wrap this call in catch_unwind. Each language's
        // generated glue converts its own failures into an AbiError return before
        // crossing this boundary (the responsibility contract — docs/TRUST_MODEL.md).
        // A guard here would be a false promise: it cannot catch a C/C++ exception
        // (only Rust panics), and a modern Rust plugin's own `extern "C"` boundary
        // aborts on a panic that escapes its glue (that abort fires first). So an
        // unwind or exception leaking across this ABI call is a plugin defect with a
        // defined outcome — process abort — not something the runtime absorbs. The
        // only correctness obligation here is that the init-stack push is balanced by
        // the pop below on the normal return path; an aborting plugin tears down the
        // whole process, so no cleanup can or needs to run.
        let host_abi: *const HostApi = runtime.host_abi();
        // SAFETY: host_abi is a valid HostApi pointer obtained from the runtime.
        // init_fn_ptr is a valid function pointer resolved from the plugin library.
        // ctx is a stack-allocated BundleInitContext that outlives the call.
        let init_result: AbiError = unsafe { init_fn_ptr(host_abi, &ctx) };

        runtime.pop_init_bundle_id();

        if init_result.code != AbiErrorCode::Ok as u32 {
            let error_msg: String = if init_result.message.ptr.is_null() {
                format!("init returned error code {:?}", init_result.code)
            } else {
                // SAFETY: ptr is non-null and points to valid UTF-8 bytes
                let bytes: &[u8] = unsafe {
                    slice::from_raw_parts(init_result.message.ptr, init_result.message.len)
                };
                String::from_utf8_lossy(bytes).into_owned()
            };
            // init ran and may have registered a contract before reporting failure:
            // invalidate those registrations and schedule the library for epoch-deferred
            // dlclose (do not dlclose inline).
            self.vacate_failed_init(&manifest.name, library, runtime);
            return Err(LoaderError::InitFailed {
                bundle: manifest.name.clone(),
                error: error_msg,
            });
        }

        // If a bundle with the same id was already loaded (e.g. its file was replaced
        // on disk → a different mapping), SCHEDULE the superseded handle for
        // epoch-deferred `dlclose` instead of dropping it inline: old registry slots
        // may still resolve raw fn pointers into the prior mapping, so it is reclaimed
        // only once no reader is pinned.
        let bundle_id: BundleId = BundleId::new(&manifest.name);
        let superseded: Option<Library> = self
            .libraries
            .lock()
            .recover_poisoned(runtime.logger(), "loader.native")
            .insert(bundle_id, library);
        if let Some(old_library) = superseded {
            self.schedule_reclaim(old_library);
        }

        Ok(())
    }
}

impl BundleLoader for NativeLoader {
    fn loader_name(&self) -> &'static str {
        "native"
    }

    /// The native loader serves both Rust and C++ `cdylib` bundles (which share the
    /// native ABI); it claims Rust as its reference language. This value is not used
    /// for once-per-process boot tracking (native has no shared boot state).
    fn loader_language(&self) -> SupportedLanguage {
        SupportedLanguage::Rust
    }

    fn supports_hot_reload(&self) -> bool {
        true
    }

    fn load(
        &self,
        manifest: &ManifestData,
        source: &BundleSource,
        runtime: &Runtime,
    ) -> Result<(), LoaderError> {
        self.load_inner(manifest, source, runtime)
    }

    fn reload(&self, manifest: &ManifestData, runtime: &Runtime) -> Result<(), LoaderError> {
        // reload re-reads the on-disk file (only path-backed bundles can be
        // hot-reloaded — the runtime gates hot-reload before calling this).
        self.load_inner(
            manifest,
            &BundleSource::Path(manifest.path.clone()),
            runtime,
        )
    }

    /// Reclaim the bundle's `libloading::Library` via epoch-deferred `dlclose`.
    ///
    /// # Safety model — epoch-deferred, host-attested for raw native calls
    /// Native dispatch is zero-overhead by design: once the host resolves a contract,
    /// it calls a RAW function pointer that points directly into this library's code
    /// pages. The runtime never mediates those calls and keeps NO native-call counter,
    /// so it is *structurally blind* to whether a thread is executing inside the
    /// library right now. `dlclose` (dropping the `Library`) while such a call is in
    /// flight would unmap the code pages out from under it — a use-after-free (SIGSEGV).
    ///
    /// This hook removes the live handle and schedules it for epoch-deferred `dlclose`
    /// (see [`NativeLoader::schedule_reclaim`]): the library is reclaimed only once no
    /// crossbeam-epoch reader is pinned, so any in-flight *runtime-mediated* call (which
    /// holds an epoch pin across dispatch) keeps the code pages mapped until it
    /// completes. RAW native calls the runtime cannot see are covered by the documented
    /// trusted-same-process host contract: the host MUST NOT call — or hold a raw
    /// pointer into — a bundle concurrently with unloading it (docs/TRUST_MODEL.md).
    /// This is the same contract hot-reload's `Preparing` phase relies on and the
    /// documented price of zero-overhead native dispatch — not a bug.
    ///
    /// The library is always epoch-reclaimed (never parked alive forever).
    fn unload(&self, bundle_id: BundleId, runtime: &Runtime) -> Result<(), LoaderError> {
        // Remove the live handle; nothing to do if this bundle isn't loaded by us.
        let library: Library = match self
            .libraries
            .lock()
            .recover_poisoned(runtime.logger(), "loader.native")
            .remove(&bundle_id)
        {
            Some(lib) => lib,
            None => return Ok(()),
        };

        self.schedule_reclaim(library);

        Ok(())
    }
}

#[cfg(test)]
impl NativeLoader {
    /// Number of live (currently loaded) library handles. Test-only accessor.
    pub(crate) fn live_library_count(&self) -> usize {
        self.libraries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }

    /// Number of libraries scheduled for epoch-deferred reclamation. Deterministic
    /// (incremented at scheduling time), so tests assert the resource was handed to
    /// the epoch collector without depending on its non-deterministic timing.
    pub(crate) fn scheduled_reclaim_count(&self) -> u64 {
        self.scheduled_reclaims.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod unload_tests {
    use std::env;
    use std::ffi::{OsStr, OsString};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process;
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    use polyplug::Runtime;
    use polyplug::error::LoaderError;
    use polyplug::loader::{BundleLoader, BundleSource, ManifestData, parse_manifest};
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

    /// Build a default `Runtime`. No loader is registered: the test drives a
    /// directly-constructed `NativeLoader`, using the runtime only for `host_abi()`.
    fn test_runtime() -> Arc<Runtime> {
        Runtime::builder()
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

    /// Unload removes the live handle and SCHEDULES the library for epoch-deferred
    /// reclaim. Every unload epoch-reclaims uniformly — there is no opt-out branch that
    /// parks the library alive.
    #[test]
    #[cfg(not(miri))]
    fn unload_removes_live_and_schedules_reclaim() {
        let runtime: Arc<Runtime> = test_runtime();
        let (loader, bundle_id): (NativeLoader, BundleId) = load_test_plugin(&runtime);
        assert_eq!(loader.live_library_count(), 1);
        assert_eq!(loader.scheduled_reclaim_count(), 0);

        loader
            .unload(bundle_id, &runtime)
            .expect("unload should succeed");

        assert_eq!(loader.live_library_count(), 0, "live handle removed");
        assert_eq!(
            loader.scheduled_reclaim_count(),
            1,
            "unload must schedule the library for epoch-deferred reclaim"
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
    /// `dlclose` the library inline (its registered statics back the published,
    /// still-resolvable interface). The loader instead SCHEDULES the library for
    /// epoch-deferred reclaim and invalidates whatever the failed init registered;
    /// the global epoch keeps the library alive alongside the runtime's epoch-owned
    /// invalidated interface until no reader is pinned.
    ///
    /// Regression for the HIGH finding: previously the local `library` dropped on
    /// the error return → `dlclose` while the registry still held live interfaces
    /// whose fn pointers dangled into unmapped pages.
    #[test]
    #[cfg(not(miri))]
    fn failed_load_after_register_schedules_reclaim_and_invalidates() {
        let runtime: Arc<Runtime> = test_runtime();
        let dir: PathBuf = register_fail_plugin_dir();
        let manifest: ManifestData =
            parse_manifest(&dir).expect("parse_manifest for register_fail_plugin");
        let bundle_id: BundleId = BundleId::new(&manifest.name);
        let source: BundleSource = BundleSource::Path(manifest.path.clone());
        let loader: NativeLoader = NativeLoader::new(NativeConfig::default());

        let load_result: Result<(), LoaderError> = loader.load(&manifest, &source, &runtime);
        assert!(
            load_result.is_err(),
            "init returns a non-Ok error, so load() must fail"
        );

        // The library must be SCHEDULED for epoch-deferred reclaim, never dropped
        // inline: its registered statics may be referenced by the (now invalidated)
        // interface still epoch-owned by the runtime.
        assert_eq!(
            loader.live_library_count(),
            0,
            "no live handle after a failed load"
        );
        assert_eq!(
            loader.scheduled_reclaim_count(),
            1,
            "a library whose init ran must be scheduled for reclaim, not dropped inline"
        );

        // The failed init's registration must have been invalidated: re-invalidating
        // the same bundle now reports zero slots (idempotent — nothing left to vacate).
        let count: u32 = runtime
            .registry()
            .invalidate_bundle(bundle_id)
            .expect("invalidate_bundle should succeed");
        assert_eq!(
            count, 0,
            "load() must have already invalidated the failed init's registration"
        );
    }

    /// A second `load()` that re-inserts the same `BundleId` must SCHEDULE the
    /// superseded `Library` handle for epoch-deferred reclaim rather than dropping it
    /// inline: old registry slots may still resolve raw fn pointers into the prior
    /// mapping, so it is reclaimed only once no reader is pinned.
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
    fn double_load_schedules_superseded_library_reclaim() {
        let runtime: Arc<Runtime> = test_runtime();
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
        assert_eq!(loader.scheduled_reclaim_count(), 0);

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
            loader.scheduled_reclaim_count(),
            1,
            "the superseded library must be scheduled for reclaim, not dropped inline"
        );
    }

    /// A non-UTF-8 bundle path must fail cleanly with a clear "not valid UTF-8"
    /// error instead of being smuggled across the ABI as a UTF-8 `StringView`.
    ///
    /// Regression for the MEDIUM finding: `as_encoded_bytes()` previously crossed
    /// non-UTF-8 (and WTF-8 on Windows) bytes into `BundleInitContext.bundle_path`.
    ///
    /// Linux-only: the test must materialize a directory whose name carries an
    /// invalid UTF-8 byte (0xFF). ext4/tmpfs store raw path bytes, so this works;
    /// macOS APFS enforces UTF-8 filenames and rejects the `create_dir_all` with
    /// `Illegal byte sequence`, so the input cannot be constructed there. The
    /// loader's rejection path is byte-identical across Unix at runtime — only the
    /// test's precondition is platform-bound.
    #[test]
    #[cfg(all(target_os = "linux", not(miri)))]
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
            format!("polyplug_native_badutf8_{}_", process::id()).into_bytes();
        name_bytes.push(0xFF);
        let dir_name: OsString = OsStr::from_bytes(&name_bytes).to_owned();
        let temp_dir: PathBuf = env::temp_dir().join(dir_name);
        fs::create_dir_all(&temp_dir).expect("create non-utf8 bundle dir");

        let dest_dll: PathBuf = temp_dir.join(&dll_name);
        fs::copy(&src_dll, &dest_dll).expect("copy fixture dll");

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
        fs::write(temp_dir.join("manifest.toml"), &manifest_toml).expect("write temp manifest");

        let manifest: ManifestData =
            parse_manifest(&temp_dir).expect("parse_manifest for non-utf8 bundle");
        let source: BundleSource = BundleSource::Path(manifest.path.clone());
        let runtime: Arc<Runtime> = test_runtime();
        let loader: NativeLoader = NativeLoader::new(NativeConfig::default());

        let load_result: Result<(), LoaderError> = loader.load(&manifest, &source, &runtime);

        let err: LoaderError = load_result.expect_err("non-UTF-8 bundle path must fail cleanly");
        let msg: String = err.to_string();
        assert!(
            msg.contains("not valid UTF-8"),
            "error must mention non-UTF-8 path, got: {msg}"
        );

        let _ = fs::remove_dir_all(&temp_dir);
    }

    /// A `manifest.file` that escapes the bundle directory (`../evil.so`) must be
    /// rejected by the loader's path-confinement check before any `dlopen`, even
    /// though the target shared library is a valid, loadable plugin. This covers
    /// code that is never included in the signing digest.
    #[test]
    #[cfg(all(unix, not(miri)))]
    fn artifact_path_traversal_is_rejected() {
        let src_dir: PathBuf = test_plugin_dir();
        let src_manifest: ManifestData =
            parse_manifest(&src_dir).expect("parse_manifest for test_plugin_dir");
        let dll_name: String = src_manifest.file.clone();
        let src_dll: PathBuf = src_dir.join(&dll_name);

        // A bundle dir holding a real DLL "outside.so" right next to (one level up
        // from) the bundle, with manifest.file pointing at it via `../`.
        let parent: PathBuf = env::temp_dir().join(format!(
            "polyplug_native_escape_{}_{}",
            process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let bundle_dir: PathBuf = parent.join("bundle");
        fs::create_dir_all(&bundle_dir).expect("create bundle dir");

        // Place a loadable DLL OUTSIDE the bundle directory.
        let outside_dll: PathBuf = parent.join("outside.so");
        fs::copy(&src_dll, &outside_dll).expect("copy fixture dll outside bundle");

        let manifest_toml: String = format!(
            "id = {}\nname = \"{}\"\nversion = \"{}\"\nloader = \"native\"\nprovides = [\"test.add\"]\nfile = \"../outside.so\"\n\n[function_count]\n\"test.add@1\" = 1\n",
            src_manifest.id, src_manifest.name, src_manifest.version
        );
        fs::write(bundle_dir.join("manifest.toml"), manifest_toml).expect("write manifest");

        let manifest: ManifestData =
            parse_manifest(&bundle_dir).expect("parse_manifest for escape bundle");
        let source: BundleSource = BundleSource::Path(manifest.path.clone());
        let runtime: Arc<Runtime> = test_runtime();
        let loader: NativeLoader = NativeLoader::new(NativeConfig::default());

        let result: Result<(), LoaderError> = loader.load(&manifest, &source, &runtime);
        assert!(
            matches!(result, Err(LoaderError::ArtifactPathEscape { .. })),
            "a `../` artifact path must be rejected, got: {result:?}"
        );

        let _ = fs::remove_dir_all(&parent);
    }

    /// A `manifest.file` that is a symlink (even one pointing at a valid DLL inside
    /// the bundle) must be rejected: a symlinked artifact is loaded by the loader
    /// but excluded from the signing digest, a signature bypass.
    #[test]
    #[cfg(all(unix, not(miri)))]
    fn symlinked_artifact_is_rejected() {
        use std::os::unix::fs::symlink;

        let src_dir: PathBuf = test_plugin_dir();
        let src_manifest: ManifestData =
            parse_manifest(&src_dir).expect("parse_manifest for test_plugin_dir");
        let dll_name: String = src_manifest.file.clone();
        let src_dll: PathBuf = src_dir.join(&dll_name);

        let bundle_dir: PathBuf = env::temp_dir().join(format!(
            "polyplug_native_symlink_{}_{}",
            process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        fs::create_dir_all(&bundle_dir).expect("create bundle dir");

        // Real DLL inside the bundle, plus a symlink artifact pointing at it.
        let real_dll: PathBuf = bundle_dir.join("real.so");
        fs::copy(&src_dll, &real_dll).expect("copy fixture dll into bundle");
        let link_name: &str = "artifact.so";
        symlink(&real_dll, bundle_dir.join(link_name)).expect("create symlink artifact");

        let manifest_toml: String = format!(
            "id = {}\nname = \"{}\"\nversion = \"{}\"\nloader = \"native\"\nprovides = [\"test.add\"]\nfile = \"{}\"\n\n[function_count]\n\"test.add@1\" = 1\n",
            src_manifest.id, src_manifest.name, src_manifest.version, link_name
        );
        fs::write(bundle_dir.join("manifest.toml"), manifest_toml).expect("write manifest");

        let manifest: ManifestData =
            parse_manifest(&bundle_dir).expect("parse_manifest for symlink bundle");
        let source: BundleSource = BundleSource::Path(manifest.path.clone());
        let runtime: Arc<Runtime> = test_runtime();
        let loader: NativeLoader = NativeLoader::new(NativeConfig::default());

        let result: Result<(), LoaderError> = loader.load(&manifest, &source, &runtime);
        assert!(
            matches!(result, Err(LoaderError::ArtifactPathEscape { .. })),
            "a symlinked artifact must be rejected, got: {result:?}"
        );

        let _ = fs::remove_dir_all(&bundle_dir);
    }

    /// On Windows, a mapped DLL holds an exclusive file lock, so `remove_file`
    /// fails with `ERROR_SHARING_VIOLATION` while the DLL is loaded. After unload
    /// runs the epoch-deferred `dlclose`, the lock is released and removal
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
        let temp_dir: PathBuf = env::temp_dir().join(format!(
            "polyplug_native_reclaim_{}_{}",
            process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        fs::create_dir_all(&temp_dir).expect("create temp bundle dir");

        // Copy the DLL into the temp dir; the write handle is closed by fs::copy.
        let dest_dll: PathBuf = temp_dir.join(&dll_name);
        fs::copy(&src_dll, &dest_dll).expect("copy fixture dll");

        // Write a minimal manifest pointing at the copied DLL. fs::write closes
        // its handle before returning, so no write handle keeps the dir/file locked.
        let manifest_toml: String = format!(
            "id = {}\nname = \"{}\"\nversion = \"{}\"\nloader = \"native\"\nprovides = [\"test.add\"]\n\n[file]\nwindows.x86_64 = \"{}\"\n\n[function_count]\n\"test.add@1\" = 1\n",
            src_manifest.id, src_manifest.name, src_manifest.version, dll_name
        );
        fs::write(temp_dir.join("manifest.toml"), manifest_toml).expect("write temp manifest");

        let manifest: ManifestData =
            parse_manifest(&temp_dir).expect("parse_manifest for temp bundle");
        let bundle_id: BundleId = BundleId::new(&manifest.name);
        let source: BundleSource = BundleSource::Path(manifest.path.clone());

        let runtime: Arc<Runtime> = test_runtime();
        let loader: NativeLoader = NativeLoader::new(NativeConfig::default());
        loader
            .load(&manifest, &source, &runtime)
            .expect("native load of copied bundle should succeed");

        // While loaded, the DLL is mapped and locked: removal must fail.
        assert!(
            fs::remove_file(&dest_dll).is_err(),
            "a mapped DLL must be locked on Windows"
        );

        loader
            .unload(bundle_id, &runtime)
            .expect("unload should succeed");

        // unload schedules the `dlclose` for epoch-deferred reclaim. This test is
        // single-threaded with no epoch reader pinned, so advancing the global epoch
        // by repeatedly pinning + flushing deterministically runs the deferred drop;
        // once it does, the OS file lock is released and removal succeeds. The loop is
        // bounded so a failure to reclaim surfaces as a hard test failure, not a hang.
        let mut removed: bool = false;
        for _ in 0..1024 {
            crossbeam_epoch::pin().flush();
            if fs::remove_file(&dest_dll).is_ok() {
                removed = true;
                break;
            }
        }
        assert!(
            removed,
            "DLL must be removable after the epoch-deferred dlclose runs"
        );

        let _ = fs::remove_dir_all(&temp_dir);
    }
}

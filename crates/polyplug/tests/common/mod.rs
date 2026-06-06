#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::undocumented_unsafe_blocks)]
// This module is shared across multiple integration-test binaries via the
// standard `mod common;` pattern. Each test binary uses only the subset of
// items it needs, so unused items are expected in any single binary.
#![allow(dead_code)]

//! Shared test-local native (shared library) loader.
//!
//! The `polyplug` core crate is loader-agnostic by design: it knows the
//! `BundleLoader` trait but nothing about `libloading`. Tests that load native
//! bundles must therefore register a loader. These tests CANNOT depend on
//! `polyplug_native` — that crate depends on `polyplug`, so a dev-dependency
//! would form a circular dependency in the workspace graph. The test-local
//! loader below is the correct architecture: it reproduces the essentials of
//! `polyplug_native::NativeLoader` (dlopen via libloading, ABI version sentinel
//! check, `polyplug_init` resolution, TLS bundle-id management for dependency
//! enforcement) using only `polyplug`'s public surface.
//!
//! Two entry points are provided:
//! - [`TestNativeLoader`] for builder-oriented tests via `RuntimeBuilder::loader`.
//! - [`register_native_loader`] for FFI-oriented tests via the
//!   `HostApi::register_loader` callback.

use core::ffi::c_void;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, Once};

use libloading::Library;

use polyplug::Runtime;
use polyplug::error::{LoaderError, RuntimeError};
use polyplug::loader::{BundleLoader, ManifestData};
use polyplug_abi::HostApi;
use polyplug_abi::POLYPLUG_ABI_VERSION;
use polyplug_abi::StringView;
use polyplug_abi::plugin::BundleInitContext;
use polyplug_abi::types::{AbiError, AbiErrorCode};
use polyplug_utils::BundleId;

/// Minimal native (shared library) loader used by the `polyplug` integration
/// and stress tests.
///
/// Mirrors the essentials of `polyplug_native::NativeLoader`: dlopen via
/// libloading, ABI version sentinel check, `polyplug_init` resolution, and TLS
/// bundle-id management for dependency enforcement. Library handles are kept
/// alive in a `Mutex<HashMap<BundleId, Library>>`.
pub struct TestNativeLoader {
    libraries: Mutex<HashMap<BundleId, Library>>,
    /// Libraries superseded by a hot-reload, retained for the loader's lifetime
    /// so in-flight callers holding raw function pointers stay valid (mirrors
    /// `polyplug_native::NativeLoader`).
    retired: Mutex<Vec<Library>>,
}

impl TestNativeLoader {
    pub fn new() -> Self {
        Self {
            libraries: Mutex::new(HashMap::new()),
            retired: Mutex::new(Vec::new()),
        }
    }

    fn load_library(&self, manifest: &ManifestData, runtime: &Runtime) -> Result<(), RuntimeError> {
        if manifest.file.is_empty() {
            return Err(RuntimeError::Loader(LoaderError::ManifestMissingFile {
                bundle: manifest.name.clone(),
            }));
        }

        let bundle_path: PathBuf = manifest.path.join(&manifest.file);
        let path_str: String = bundle_path.to_string_lossy().into_owned();

        // SAFETY: path points to a compiled plugin bundle; libloading validates the shared library.
        let library: Library = unsafe {
            Library::new(&bundle_path).map_err(|e| {
                RuntimeError::Loader(LoaderError::InitFailed {
                    bundle: manifest.name.clone(),
                    error: format!("failed to load plugin library at {path_str}: {e}"),
                })
            })?
        };

        // SAFETY: polyplug_abi_version is a C function `extern "C" fn() -> u32`.
        let abi_version_symbol: libloading::Symbol<'_, unsafe extern "C" fn() -> u32> = unsafe {
            library.get(b"polyplug_abi_version\0").map_err(|_| {
                RuntimeError::Loader(LoaderError::InitFailed {
                    bundle: manifest.name.clone(),
                    error: format!("missing symbol 'polyplug_abi_version' in bundle '{path_str}'"),
                })
            })?
        };
        // SAFETY: abi_version_symbol was validated by library.get(); it returns a plain u32.
        let found_version: u32 = unsafe { abi_version_symbol() };
        if found_version != POLYPLUG_ABI_VERSION {
            return Err(RuntimeError::Loader(LoaderError::InitFailed {
                bundle: manifest.name.clone(),
                error: format!(
                    "ABI version mismatch in {path_str}: expected={POLYPLUG_ABI_VERSION}, found={found_version}"
                ),
            }));
        }

        // SAFETY: polyplug_init is an exported C symbol from the plugin library, validated
        // to exist by library.get(); the signature matches the ABI contract.
        let init_fn_ptr: unsafe extern "C" fn(
            *const HostApi,
            *const BundleInitContext,
        ) -> AbiError = {
            let sym: libloading::Symbol<
                '_,
                unsafe extern "C" fn(*const HostApi, *const BundleInitContext) -> AbiError,
            > = unsafe {
                library.get(b"polyplug_init\0").map_err(|_| {
                    RuntimeError::Loader(LoaderError::InitSymbolMissing {
                        bundle: manifest.name.clone(),
                    })
                })?
            };
            *sym
        };

        let bundle_dir: &std::path::Path =
            bundle_path.parent().unwrap_or(std::path::Path::new("."));
        let ctx: BundleInitContext = BundleInitContext {
            bundle_id: BundleId::new(&manifest.name).id(),
            bundle_path: StringView {
                ptr: bundle_dir.as_os_str().as_encoded_bytes().as_ptr(),
                len: bundle_dir.as_os_str().as_encoded_bytes().len(),
            },
        };

        let expected_bundle_id: BundleId = BundleId::new(&manifest.name);
        runtime.push_init_bundle_id(expected_bundle_id.id());

        let host_abi: &'static HostApi = runtime.host_abi();
        // SAFETY: host_abi is a valid HostApi reference from the runtime; init_fn_ptr is a
        // valid function pointer; ctx is stack-allocated and outlives the call.
        let init_result: AbiError = unsafe { init_fn_ptr(host_abi as *const HostApi, &ctx) };

        runtime.pop_init_bundle_id();

        if init_result.code != AbiErrorCode::Ok as u32 {
            let error_msg: String = if init_result.message.ptr.is_null() {
                format!("init returned error code {:?}", init_result.code)
            } else {
                // SAFETY: ptr is non-null and points to valid UTF-8 bytes for message.len.
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

        // Retire (do not drop) any superseded library so in-flight callers holding
        // raw function pointers into it stay valid.
        if let Some(old_library) = self
            .libraries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(expected_bundle_id, library)
        {
            self.retired
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(old_library);
        }

        Ok(())
    }
}

impl Default for TestNativeLoader {
    fn default() -> Self {
        Self::new()
    }
}

impl BundleLoader for TestNativeLoader {
    fn runtime_name(&self) -> &'static str {
        "native"
    }

    fn load(
        &self,
        manifest: &ManifestData,
        _source: &polyplug::loader::BundleSource,
        runtime: &Runtime,
    ) -> Result<(), RuntimeError> {
        self.load_library(manifest, runtime)
    }

    fn reload(&self, manifest: &ManifestData, runtime: &Runtime) -> Result<(), RuntimeError> {
        self.load_library(manifest, runtime)
    }
}

/// Register a [`TestNativeLoader`] with the runtime behind `host`.
///
/// # Safety
/// `host` must be a valid `HostApi` pointer returned by `polyplug_runtime_create`.
pub unsafe fn register_native_loader(host: *const HostApi) {
    let name: &[u8] = b"native";
    let loader: *mut c_void = Box::into_raw(Box::new(
        Box::new(TestNativeLoader::new()) as Box<dyn BundleLoader>
    )) as *mut c_void;
    // SAFETY: host is valid; name is a valid byte slice for the duration of the call;
    // loader is a `*mut Box<dyn BundleLoader>` erased to `*mut c_void` as host_register_loader expects.
    let rc: AbiError =
        unsafe { ((*host).register_loader)(host, StringView::from_static(name), loader) };
    assert_eq!(
        rc.code,
        AbiErrorCode::Ok as u32,
        "register_loader must succeed for native loader"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// polyplugc CLI resolution
// ─────────────────────────────────────────────────────────────────────────────

/// Resolve the path to the compiled `polyplugc` binary, building it once if absent.
///
/// `CARGO_BIN_EXE_polyplugc` is only valid for tests in the same crate as the
/// `polyplugc` bin, so it cannot be used from the `polyplug` test crate. Instead
/// the binary is located in the workspace target directory matching the active
/// build profile, and built via `cargo build -p polyplugc` if it does not yet
/// exist. The build runs at most once per test process.
pub fn polyplugc_bin() -> PathBuf {
    let target_dir: PathBuf = workspace_target_dir();
    let profile: &str = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    let bin_name: &str = if cfg!(windows) {
        "polyplugc.exe"
    } else {
        "polyplugc"
    };
    let bin_path: PathBuf = target_dir.join(profile).join(bin_name);

    if !bin_path.exists() {
        build_polyplugc(profile);
    }

    bin_path
}

/// Workspace target directory: honors `CARGO_TARGET_DIR`, else `<workspace>/target`.
fn workspace_target_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("CARGO_TARGET_DIR") {
        return PathBuf::from(dir);
    }
    // CARGO_MANIFEST_DIR = crates/polyplug; workspace root is two levels up.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("parent of crates/polyplug")
        .parent()
        .expect("workspace root")
        .join("target")
}

/// Build `polyplugc` once per process for the given profile.
fn build_polyplugc(profile: &str) {
    static BUILD: Once = Once::new();
    BUILD.call_once(|| {
        let mut cmd: Command = Command::new(env!("CARGO"));
        cmd.args(["build", "-p", "polyplugc"]);
        if profile == "release" {
            cmd.arg("--release");
        }
        let status: std::process::ExitStatus = cmd
            .status()
            .expect("failed to spawn cargo build -p polyplugc");
        assert!(status.success(), "cargo build -p polyplugc failed");
    });
}

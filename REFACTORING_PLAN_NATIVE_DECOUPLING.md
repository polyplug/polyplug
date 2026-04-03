# Refactoring Plan: Remove Native Coupling from polyplug Core

**Goal:** Move all native-plugin code from `polyplug` crate to `polyplug_native` crate, making the core runtime loader-agnostic.

**Status:** NOT STARTED

**Breaking Changes:** Yes (acceptable - not published yet)

---

## Prerequisites

- Understanding of Rust traits, generics, and ownership
- Familiarity with `libloading` crate for dynamic library loading
- Knowledge of the polyplug architecture (Registry, Runtime, Loader)

---

## Background

### Current Architecture (BROKEN)
```
polyplug (core runtime)
├── libloading::Library storage in Registry  <-- WRONG!
├── NativeBundleLoader implementation         <-- WRONG!
├── load_bundle() using libloading            <-- WRONG!
├── reload.rs with native hot-reload          <-- WRONG!
└── Auto-registers native loader              <-- WRONG!

polyplug_native
└── Just delegates to polyplug::load_bundle() <-- NOT OWNING ITS CODE!
```

### Target Architecture (CORRECT)
```
polyplug (core runtime - loader-agnostic)
├── BundleLoader trait only
├── Registry stores vtable pointers only
├── Generic reload framework (ReloadPhase, wait_for_quiescence)
├── No libloading dependency
└── No auto-registration

polyplug_native
├── NativeLoader implementing BundleLoader
├── Owns libloading::Library handles
├── Owns load_bundle() implementation
├── Owns hot-reload implementation
└── NativeConfig
```

---

## Execution Order

```
Phase 1: Update BundleLoader trait (add reload method)
    ↓
Phase 2: Create generic reload framework in core
    ↓
Phase 3: Create NativeLoader in polyplug_native
    ↓
Phase 4: Remove native coupling from core
    ↓
Phase 5: Require explicit runtime in manifest
    ↓
Phase 6: Use newtype IDs (BundleId, PluginContractId)
```

---

## Phase 1: Update BundleLoader Trait

### File: `crates/polyplug/src/loader/bundle_loader.rs`

**Action:** ADD `reload()` method to trait (MANDATORY for all loaders)

**Current code (lines 1-34):**
```rust
use crate::{error::RuntimeError, loader::manifest::ManifestData, runtime::Runtime};

pub trait BundleLoader: Send + Sync {
    fn runtime_name(&self) -> &'static str;
    fn runtime_names(&self) -> Vec<String> { ... }
    fn load(&self, manifest: &ManifestData, runtime: &Runtime) -> Result<(), RuntimeError>;
}
```

**Replace with:**
```rust
use polyplug_utils::BundleId;
use crate::{error::RuntimeError, loader::manifest::ManifestData, runtime::Runtime};

/// Trait implemented by all bundle loaders (native, python, lua, js, .net).
///
/// The runtime dispatches each bundle to the loader whose `runtime_name()`
/// matches the `runtime` field in the bundle's `manifest.toml`.
pub trait BundleLoader: Send + Sync {
    /// The runtime identifier this loader handles.
    ///
    /// Must match the `runtime` field in `manifest.toml` exactly (case-sensitive).
    fn runtime_name(&self) -> &'static str;

    /// All runtime identifiers this loader handles.
    ///
    /// Defaults to a single-element vec containing `runtime_name()`.
    /// Override this method if your loader handles multiple runtime names.
    fn runtime_names(&self) -> Vec<String> {
        vec![self.runtime_name().to_owned()]
    }

    /// Load a bundle for the first time.
    ///
    /// The manifest contains all metadata needed to load the bundle:
    /// - `manifest.path` - the bundle directory
    /// - `manifest.file` - the plugin file (relative to bundle directory)
    /// - `manifest.id` - the bundle ID
    ///
    /// # Errors
    /// Returns `Err(RuntimeError::...)` on any failure.
    fn load(&self, manifest: &ManifestData, runtime: &Runtime) -> Result<(), RuntimeError>;

    /// Reload a bundle - MANDATORY for all loaders.
    ///
    /// Called when a bundle needs to be hot-reloaded (e.g., file changed).
    ///
    /// Implementation must:
    /// 1. Load/reload the bundle code (loader-specific mechanism)
    /// 2. Call init to get new vtables
    /// 3. Register new vtables with registry (vtable swap happens in registry)
    /// 4. Return Ok(()) - runtime handles callback and quiescence wait
    ///
    /// # Safety Contract
    /// After return, old resources should be cleaned up:
    /// - Native: drop old library (caller must not have cached raw pointers)
    /// - VMs: let GC handle cleanup
    ///
    /// # Errors
    /// Returns `Err(RuntimeError::...)` on any failure.
    fn reload(&self, manifest: &ManifestData, runtime: &Runtime) -> Result<(), RuntimeError>;
}
```

**Why:** Every loader must support hot-reload. The pattern is universal: load new code -> call init -> swap vtables.

---

## Phase 2: Create Generic Reload Framework

### File: `crates/polyplug/src/reload.rs`

**Action:** REWRITE - remove native-specific code, keep generic utilities

**Keep these (already exist):**
- `ReloadPhase` enum (lines 48-63)
- `ReloadEvent` struct (lines 38-44)

**Remove these (native-specific - will move to polyplug_native):**
- `VTablePtr` struct (lines 26-36)
- `reload_register_callback()` (lines 70-90)
- `reload_bundle_impl()` function (lines 93-571) - ALL OF IT
- `find_cascade_targets()` (lines 574-597)
- `impl Runtime` methods (lines 599-611)

**Replace entire file with:**

```rust
//! Reload — generic hot-reload framework for all loaders.
//!
//! Provides:
//! - `ReloadPhase` enum for notification callbacks
//! - `wait_for_quiescence()` utility for waiting until no in-flight calls
//!
//! Each loader implements its own `reload()` method using these utilities.

use core::hint::spin_loop;
use std::sync::Arc;
use std::time::{Duration, Instant};

use polyplug_utils::BundleId;

use crate::error::RuntimeError;
use crate::loader::manifest::ManifestData;
use crate::plugin_registry::VTableSlot;
use crate::runtime::Runtime;

const QUIESCENCE_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_CASCADE_DEPTH: usize = 16;

// ─── Reload Phase Notifications ──────────────────────────────────────────────

/// Phase of a hot-reload operation for notification callbacks.
#[derive(Debug, Clone)]
pub enum ReloadPhase {
    /// Bundle is being prepared for reload (before vtable swap).
    Preparing {
        bundle_id: BundleId,
        bundle_name: String,
        retry_count: u32,
    },
    /// Bundle has been successfully reloaded.
    /// Host MUST release all cached raw pointers NOW.
    Reloaded {
        bundle_id: BundleId,
        bundle_name: String,
    },
    /// Bundle reload failed.
    Failed {
        bundle_id: BundleId,
        bundle_name: String,
        reason: String,
    },
}

/// Event describing a completed reload (for logging/telemetry).
#[derive(Debug, Clone)]
pub struct ReloadEvent {
    pub bundle_name: String,
    pub bundle_path: String,
    pub old_version: String,
    pub new_version: String,
}

// ─── Quiescence Wait Utility ─────────────────────────────────────────────────

/// Wait for quiescence - no in-flight calls using vtables from this bundle.
///
/// Uses `Arc::strong_count` to detect when all `PluginGuard` handles are dropped.
/// When count == 1, only the registry holds the vtable (no active calls).
///
/// # Arguments
/// - `registry`: The plugin registry
/// - `bundle_id`: The bundle being reloaded
/// - `timeout`: Maximum time to wait
///
/// # Returns
/// - `Ok(())` if quiescence achieved
/// - `Err(QuiescenceTimeout)` if timeout exceeded
///
/// # Important
/// This only tracks `Arc<VTableSlot>` references (PluginGuard).
/// It does NOT track raw function pointers extracted by callers!
/// Callers must release those BEFORE this is called, via `on_reload_cb(Reloaded)`.
pub fn wait_for_quiescence(
    registry: &crate::registry::PluginRegistry,
    bundle_id: BundleId,
    timeout: Duration,
) -> Result<(), RuntimeError> {
    let slot_indices: Vec<u32> = registry.find_slots_by_bundle(bundle_id.id());

    let start: Instant = Instant::now();
    loop {
        let mut all_quiescent: bool = true;

        for &slot_idx in &slot_indices {
            if let Some(arc) = registry.get_vtable_arc(slot_idx) {
                // Count == 1 means only registry holds it (no in-flight calls)
                if Arc::strong_count(&arc) > 1 {
                    all_quiescent = false;
                    break;
                }
            }
        }

        if all_quiescent {
            return Ok(());
        }

        if start.elapsed() > timeout {
            return Err(RuntimeError::QuiescenceTimeout {
                bundle: format!("bundle_id={}", bundle_id.id())
            });
        }

        std::thread::sleep(Duration::from_millis(1));
        spin_loop();
    }
}

// ─── Runtime Reload Method ───────────────────────────────────────────────────

impl Runtime {
    /// Reload a bundle using its registered loader.
    ///
    /// Dispatches to the loader's `reload()` method.
    /// Fires `on_reload_cb` with phase notifications.
    ///
    /// # Arguments
    /// - `path`: Path to the bundle directory or .so/.dll/.dylib file
    ///
    /// # Errors
    /// - `NoLoaderForRuntime`: No loader registered for this runtime type
    /// - `HotReloadDisabled`: Hot-reload disabled in config
    /// - Other errors from the loader's `reload()` implementation
    pub fn reload_bundle(&self, path: &std::path::Path) -> Result<(), RuntimeError> {
        if !self.config().hot_reload_enabled {
            return Err(RuntimeError::HotReloadDisabled);
        }

        let bundle_dir: &std::path::Path = if path.is_file() {
            path.parent().unwrap_or(path)
        } else {
            path
        };

        let manifest: ManifestData = crate::loader::parse_manifest(bundle_dir)
            .map_err(|e| RuntimeError::Loader(e))?;

        // Find the loader
        let loader: &dyn crate::loader::BundleLoader = self.loaders
            .get(&manifest.runtime)
            .map(Box::as_ref)
            .ok_or_else(|| RuntimeError::Loader(crate::error::LoaderError::NoLoaderForRuntime {
                bundle: path.display().to_string(),
                runtime_name: manifest.runtime.clone(),
            }))?;

        let bundle_id: BundleId = BundleId::new(&manifest.name);

        // Fire Preparing callback
        if let Some(ref cb) = self.on_reload_cb() {
            cb(ReloadPhase::Preparing {
                bundle_id,
                bundle_name: manifest.name.clone(),
                retry_count: 0,
            });
        }

        // Call loader's reload()
        let result: Result<(), crate::error::RuntimeError> = loader.reload(&manifest, self);

        match result {
            Ok(()) => {
                // Fire Reloaded callback
                // IMPORTANT: Host must release cached raw pointers NOW!
                if let Some(ref cb) = self.on_reload_cb() {
                    cb(ReloadPhase::Reloaded {
                        bundle_id,
                        bundle_name: manifest.name.clone(),
                    });
                }
                Ok(())
            }
            Err(e) => {
                // Fire Failed callback
                if let Some(ref cb) = self.on_reload_cb() {
                    cb(ReloadPhase::Failed {
                        bundle_id,
                        bundle_name: manifest.name.clone(),
                        reason: e.to_string(),
                    });
                }
                Err(RuntimeError::from(e))
            }
        }
    }

    /// Refresh a plugin handle after reload.
    ///
    /// Returns a new handle with updated generation.
    pub fn refresh_handle(
        &self,
        contract_id: u64,
        min_version: u32,
    ) -> Result<polyplug_abi::plugin::PluginHandle, crate::error::RegistryError> {
        self.registry().find_by_contract(contract_id, min_version)
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;
    use std::time::Duration;

    #[test]
    fn reload_phase_preparing_construction() {
        let bundle_id: BundleId = BundleId::new("test-bundle");
        let phase: ReloadPhase = ReloadPhase::Preparing {
            bundle_id,
            bundle_name: "test_bundle".to_owned(),
            retry_count: 2,
        };

        match phase {
            ReloadPhase::Preparing { bundle_id: id, bundle_name, retry_count } => {
                assert_eq!(id, BundleId::new("test-bundle"));
                assert_eq!(bundle_name, "test_bundle");
                assert_eq!(retry_count, 2);
            }
            _ => panic!("expected Preparing variant"),
        }
    }

    #[test]
    fn reload_phase_reloaded_construction() {
        let bundle_id: BundleId = BundleId::new("test-bundle");
        let phase: ReloadPhase = ReloadPhase::Reloaded {
            bundle_id,
            bundle_name: "test_bundle".to_owned(),
        };

        match phase {
            ReloadPhase::Reloaded { bundle_id: id, bundle_name } => {
                assert_eq!(id, BundleId::new("test-bundle"));
                assert_eq!(bundle_name, "test_bundle");
            }
            _ => panic!("expected Reloaded variant"),
        }
    }

    #[test]
    fn reload_phase_failed_construction() {
        let bundle_id: BundleId = BundleId::new("test-bundle");
        let phase: ReloadPhase = ReloadPhase::Failed {
            bundle_id,
            bundle_name: "test_bundle".to_owned(),
            reason: "init failed".to_owned(),
        };

        match phase {
            ReloadPhase::Failed { bundle_id: id, bundle_name, reason } => {
                assert_eq!(id, BundleId::new("test-bundle"));
                assert_eq!(bundle_name, "test_bundle");
                assert_eq!(reason, "init failed");
            }
            _ => panic!("expected Failed variant"),
        }
    }

    #[test]
    fn reload_phase_clone() {
        let bundle_id: BundleId = BundleId::new("test-bundle");
        let original: ReloadPhase = ReloadPhase::Preparing {
            bundle_id,
            bundle_name: "test".to_owned(),
            retry_count: 1,
        };
        let cloned: ReloadPhase = original.clone();

        match (original, cloned) {
            (
                ReloadPhase::Preparing { bundle_id: id1, retry_count: c1, .. },
                ReloadPhase::Preparing { bundle_id: id2, retry_count: c2, .. },
            ) => {
                assert_eq!(id1, id2);
                assert_eq!(c1, c2);
            }
            _ => panic!("both should be Preparing"),
        }
    }

    #[test]
    fn reload_event_construction() {
        let event: ReloadEvent = ReloadEvent {
            bundle_name: "my_bundle".to_owned(),
            bundle_path: "/path/to/bundle".to_owned(),
            old_version: "1.0.0".to_owned(),
            new_version: "2.0.0".to_owned(),
        };

        assert_eq!(event.bundle_name, "my_bundle");
        assert_eq!(event.bundle_path, "/path/to/bundle");
        assert_eq!(event.old_version, "1.0.0");
        assert_eq!(event.new_version, "2.0.0");
    }
}
```

---

## Phase 3: Create Native Loader in polyplug_native

### Step 3.1: File `crates/polyplug_native/src/lib.rs`

**Action:** UPDATE exports

**Replace with:**
```rust
//! polyplug_native: Native (shared library) plugin loader for the polyplug runtime.

pub mod config;
pub mod error;
pub mod loader;

pub use config::NativeConfig;
pub use error::NativeLoaderError;
pub use loader::NativeLoader;
```

### Step 3.2: File `crates/polyplug_native/src/error.rs`

**Action:** CREATE NEW FILE

```rust
//! Native-specific error types.

use thiserror::Error;

/// Errors from the native loader.
#[derive(Debug, Error)]
pub enum NativeLoaderError {
    #[error("failed to load plugin bundle at `{path}`: {source}")]
    LoadFailed {
        path: String,
        #[source]
        source: libloading::Error,
    },

    #[error("ABI version mismatch in `{bundle}`: expected={expected}, found={found}")]
    AbiVersionMismatch {
        bundle: String,
        expected: u32,
        found: u32,
    },

    #[error("missing symbol `{symbol}` in bundle `{bundle}`")]
    MissingSymbol {
        bundle: String,
        symbol: String,
    },

    #[error("init failed for bundle `{bundle}`: {error}")]
    InitFailed {
        bundle: String,
        error: String,
    },

    #[error("manifest missing file field for bundle `{bundle}`")]
    ManifestMissingFile {
        bundle: String,
    },

    #[error(
        "bundle `{bundle}` tampered with bundle_id: expected={expected:#x}, found={found:#x}"
    )]
    BundleTampered {
        bundle: String,
        expected: u64,
        found: u64,
    },
}
```

### Step 3.3: File `crates/polyplug_native/src/loader.rs`

**Action:** COMPLETE REWRITE

```rust
//! Native bundle loader — loads .so/.dll/.dylib plugins.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use polyplug::error::{LoaderError, RuntimeError};
use polyplug::loader::{BundleLoader, ManifestData};
use polyplug::{reload::wait_for_quiescence, Runtime};
use polyplug_abi::plugin::PluginContext;
use polyplug_abi::types::AbiError;
use polyplug_abi::{HostVTable, POLYPLUG_ABI_VERSION};
use polyplug_utils::BundleId;

use crate::config::NativeConfig;
use crate::error::NativeLoaderError;

/// Native (shared library) plugin loader.
///
/// Handles .so/.dll/.dylib bundles using dlopen/LoadLibrary.
/// Owns library handles internally — NOT stored in registry.
pub struct NativeLoader {
    config: NativeConfig,
    /// Active library handles, keyed by BundleId.
    libraries: Mutex<HashMap<BundleId, libloading::Library>>,
    /// Host vtable for plugin registration.
    host_vtable: &'static HostVTable,
}

impl NativeLoader {
    /// Create a new NativeLoader.
    pub fn new(config: NativeConfig, host_vtable: &'static HostVTable) -> Self {
        Self {
            config,
            libraries: Mutex::new(HashMap::new()),
            host_vtable,
        }
    }

    /// Internal: load a native bundle and return the library handle.
    ///
    /// Steps:
    /// 1. dlopen the library
    /// 2. Check ABI version
    /// 3. Resolve polyplug_init
    /// 4. Call init
    fn load_internal(
        &self,
        path: &std::path::Path,
        manifest: &ManifestData,
        runtime: &Runtime,
    ) -> Result<libloading::Library, NativeLoaderError> {
        let path_str: String = path.to_string_lossy().into_owned();

        // Step 1: dlopen the library
        // SAFETY: path points to a compiled plugin bundle; libloading validates the shared library.
        let library: libloading::Library = unsafe {
            libloading::Library::new(path).map_err(|e| NativeLoaderError::LoadFailed {
                path: path_str.clone(),
                source: e,
            })?
        };

        // Step 2: Check ABI version sentinel BEFORE calling init
        // SAFETY: polyplug_abi_version is a C function with signature `extern "C" fn() -> u32`.
        let abi_version_symbol: libloading::Symbol<'_, unsafe extern "C" fn() -> u32> = unsafe {
            library
                .get(b"polyplug_abi_version\0")
                .map_err(|_| NativeLoaderError::MissingSymbol {
                    bundle: path_str.clone(),
                    symbol: "polyplug_abi_version".to_owned(),
                })?
        };
        let found_version: u32 = unsafe { abi_version_symbol() };
        if found_version != POLYPLUG_ABI_VERSION {
            return Err(NativeLoaderError::AbiVersionMismatch {
                bundle: path_str,
                expected: POLYPLUG_ABI_VERSION,
                found: found_version,
            });
        }

        // Step 3: Resolve init symbol
        // SAFETY: polyplug_init is guaranteed by the plugin build process.
        let init_fn_ptr: unsafe extern "C" fn(
            *mut core::ffi::c_void,
            *const HostVTable,
            *const PluginContext,
        ) -> AbiError = {
            let sym: libloading::Symbol<
                '_,
                unsafe extern "C" fn(
                    *mut core::ffi::c_void,
                    *const HostVTable,
                    *const PluginContext,
                ) -> AbiError,
            > = unsafe {
                library.get(b"polyplug_init\0").map_err(|_| {
                    NativeLoaderError::MissingSymbol {
                        bundle: path_str.clone(),
                        symbol: "polyplug_init".to_owned(),
                    }
                })?
            };
            *sym
        };

        // Step 4: Create PluginContext
        let bundle_dir = path.parent().unwrap_or(std::path::Path::new("."));
        let ctx: PluginContext = PluginContext {
            bundle_id: BundleId::new(&manifest.name).id(),
            bundle_path: polyplug_abi::StringView {
                ptr: bundle_dir.as_os_str().as_encoded_bytes().as_ptr(),
                len: bundle_dir.as_os_str().as_encoded_bytes().len(),
            },
            host_abi_version: POLYPLUG_ABI_VERSION,
        };

        // Step 5: Create HostContext for dependency enforcement
        let expected_bundle_id: BundleId = BundleId::new(&manifest.name);
        let host_ctx: polyplug::runtime::HostContext = runtime.create_host_context(expected_bundle_id);

        // Step 6: Call init
        let rt_ctx: *mut core::ffi::c_void =
            &host_ctx as *const polyplug::runtime::HostContext as *mut core::ffi::c_void;
        let init_result: AbiError =
            unsafe { init_fn_ptr(rt_ctx, self.host_vtable as *const HostVTable, &ctx) };

        // Step 7: Verify bundle_id wasn't tampered
        let found_bundle_id: BundleId = runtime.get_host_context_bundle_id(&host_ctx);
        if found_bundle_id != expected_bundle_id {
            return Err(NativeLoaderError::BundleTampered {
                bundle: path_str,
                expected: expected_bundle_id.id(),
                found: found_bundle_id.id(),
            });
        }

        if init_result.code != polyplug_abi::types::AbiErrorCode::Ok {
            let error_msg: String = if init_result.message.ptr.is_null() {
                format!("init returned error code {}", init_result.code)
            } else {
                // SAFETY: ptr is non-null and points to valid UTF-8 bytes
                let bytes: &[u8] = unsafe {
                    core::slice::from_raw_parts(init_result.message.ptr, init_result.message.len)
                };
                String::from_utf8_lossy(bytes).into_owned()
            };
            return Err(NativeLoaderError::InitFailed {
                bundle: path_str,
                error: error_msg,
            });
        }

        Ok(library)
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

        let library: libloading::Library =
            self.load_internal(&bundle_path, manifest, runtime)
                .map_err(|e| RuntimeError::Loader(LoaderError::InitFailed {
                    bundle: manifest.name.clone(),
                    error: e.to_string(),
                }))?;

        // Store library handle
        let bundle_id: BundleId = BundleId::new(&manifest.name);
        self.libraries.lock().unwrap().insert(bundle_id, library);

        Ok(())
    }

    fn reload(&self, manifest: &ManifestData, runtime: &Runtime) -> Result<(), RuntimeError> {
        if !runtime.config().hot_reload_enabled {
            return Err(RuntimeError::HotReloadDisabled);
        }

        let bundle_id: BundleId = BundleId::new(&manifest.name);
        let bundle_path: PathBuf = manifest.path.join(&manifest.file);

        // Step 1: Load new library
        let new_library: libloading::Library =
            self.load_internal(&bundle_path, manifest, runtime)
                .map_err(|e| RuntimeError::Loader(LoaderError::InitFailed {
                    bundle: manifest.name.clone(),
                    error: e.to_string(),
                }))?;

        // Step 2: Wait for quiescence (no in-flight calls using old vtables)
        wait_for_quiescence(
            runtime.registry(),
            bundle_id,
            std::time::Duration::from_secs(5),
        )
        .map_err(|e| RuntimeError::from(e))?;

        // Step 3: Remove and DROP old library
        // SAFETY CONTRACT: Host must not have cached raw function pointers!
        // If they did, this will cause SIGSEGV - that's a HOST BUG.
        // The `on_reload_cb(ReloadPhase::Reloaded)` already fired, giving host a chance to clean up.
        if let Some(old_library) = self.libraries.lock().unwrap().remove(&bundle_id) {
            drop(old_library); // dlclose() - unmaps code pages
        }

        // Step 4: Store new library
        self.libraries.lock().unwrap().insert(bundle_id, new_library);

        Ok(())
    }
}
```

### Step 3.4: File `crates/polyplug_native/src/config.rs`

**Action:** KEEP AS IS - no changes needed

### Step 3.5: File `crates/polyplug_native/src/ffi.rs`

**Action:** KEEP AS IS - no changes needed (or remove if unused)

---

## Phase 4: Remove Native Coupling from Core

### Step 4.1: File `crates/polyplug/src/loader/mod.rs`

**Action:** REMOVE native-specific code

**Remove lines 40-93:** `NativeBundleLoader` struct and impl block

**Remove lines 112-286:** `load_bundle()` function

**Final file should look like:**
```rust
//! Loader — bundle loading via BundleLoader trait.
//!
//! The runtime dispatches each bundle to the loader whose `runtime_name()`
//! matches the `runtime` field in the bundle's `manifest.toml`.

mod bundle_loader;
mod loaded_bundle;
mod manifest;

pub use bundle_loader::BundleLoader;
pub use loaded_bundle::LoadedBundle;
pub use manifest::{ManifestData, ManifestDependency, RawManifestDependency};

// Re-export scanner for discovery
pub mod scanner;

// Note: parse_manifest is defined in manifest.rs
```

### Step 4.2: File `crates/polyplug/src/loader/loaded_bundle.rs`

**Action:** REMOVE `library` field

**Replace with:**
```rust
use std::path::PathBuf;

/// A successfully loaded bundle.
///
/// Note: Library handles are owned by the loader (e.g., NativeLoader),
/// not by this struct. The registry only stores vtable pointers.
pub struct LoadedBundle {
    pub path: PathBuf,
}
```

### Step 4.3: File `crates/polyplug/src/registry/plugin_registry.rs`

**Action:** REMOVE library storage

**Remove lines 82-85:**
```rust
// DELETE THIS:
loaded_libraries: Mutex<Vec<libloading::Library>>,
```

**Remove lines 99-114:** `push_library()` method

**Remove line in `new()` (around line 94):**
```rust
// DELETE THIS:
loaded_libraries: Mutex::new(Vec::new()),
```

**Remove from `clear_for_test()` (lines 600-606):**
```rust
// DELETE THIS:
self.loaded_libraries
    .lock()
    .unwrap_or_else(|e| {
        eprintln!("[polyplug] Mutex/RwLock poisoned, recovering: {}", e);
        e.into_inner()
    })
    .clear();
```

### Step 4.4: File `crates/polyplug/src/runtime.rs`

**Action:** REMOVE native-specific fields and methods

**Remove line 68:**
```rust
// DELETE THIS:
pub(crate) reload_libraries: Mutex<HashMap<u64, libloading::Library>>,
```

**Remove lines 417-530:** `watch_plugin_dir()` method (native-specific file watching)

**Update `reload_bundle()` method** - see Phase 2 for the new implementation

### Step 4.5: File `crates/polyplug/src/runtime_builder.rs`

**Action:** REMOVE auto-registration

**Remove lines 129-133:**
```rust
// DELETE THIS ENTIRE BLOCK:
if !loader_map.contains_key("native") {
    let native_loader: crate::loader::NativeBundleLoader =
        crate::loader::NativeBundleLoader::new(Arc::clone(&registry), host_vtable);
    loader_map.insert("native".to_owned(), Box::new(native_loader));
}
```

### Step 4.6: File `crates/polyplug/src/error.rs`

**Action:** MOVE native-specific error to polyplug_native

**Remove these variants from `LoaderError`:**
```rust
// DELETE THIS (move to polyplug_native/src/error.rs):
#[error("failed to load plugin bundle at `{path}`: {source}")]
LoadFailed {
    path: String,
    #[source]
    source: libloading::Error,
},

#[error("ABI version mismatch in `{bundle}`: expected={expected}, found={found}")]
AbiVersionMismatch {
    bundle: String,
    expected: u32,
    found: u32,
},

#[error("missing symbol `{symbol}` in bundle `{bundle}`")]
MissingSymbol { bundle: String, symbol: String },
```

**Keep these in `LoaderError` (generic, not native-specific):**
```rust
#[error("init failed for bundle `{bundle}`: {error}")]
InitFailed { bundle: String, error: String },

#[error("manifest parse error for `{path}`: {reason}")]
ManifestParse { path: String, reason: String },

#[error("duplicate loader for runtime \"{runtime_name}\"")]
DuplicateLoader { runtime_name: String },

#[error("no loader for runtime \"{runtime_name}\" in bundle \"{bundle}\"")]
NoLoaderForRuntime {
    bundle: String,
    runtime_name: String,
},

#[error("manifest missing file field for bundle `{bundle}`")]
ManifestMissingFile { bundle: String },

#[error("bundle `{bundle}` tampered with bundle_id: expected={expected:#x}, found={found:#x}")]
BundleTampered {
    bundle: String,
    expected: u64,
    found: u64,
},
```

---

## Phase 5: Require Explicit `runtime` in Manifest

### File: `crates/polyplug/src/loader/manifest.rs`

**Action:** REMOVE default runtime, require explicit value

**Remove lines 15-17:**
```rust
// DELETE THIS:
fn default_runtime() -> String {
    "native".to_owned()
}
```

**Change line 184 (approximately):**
```rust
// BEFORE:
#[serde(default = "default_runtime")]
pub runtime: String,

// AFTER:
#[serde(skip_serializing_if = "String::is_empty")]
pub runtime: String,
```

**Add validation method:**
```rust
impl ManifestData {
    /// Validate the manifest has all required fields.
    pub fn validate(&self) -> Result<(), crate::error::LoaderError> {
        if self.runtime.is_empty() {
            return Err(crate::error::LoaderError::ManifestParse {
                path: self.path.display().to_string(),
                reason: "runtime field is required but was empty".to_owned(),
            });
        }
        if self.name.is_empty() {
            return Err(crate::error::LoaderError::ManifestParse {
                path: self.path.display().to_string(),
                reason: "name field is required but was empty".to_owned(),
            });
        }
        if self.file.is_empty() {
            return Err(crate::error::LoaderError::ManifestMissingFile {
                bundle: self.name.clone(),
            });
        }
        Ok(())
    }
}
```

---

## Phase 6: Use Newtype IDs

### Step 6.1: Update imports across files

**Add to files that use bundle IDs:**
```rust
use polyplug_utils::BundleId;
use polyplug_utils::PluginContractId;
```

### Step 6.2: Update function signatures

**In `runtime.rs`:**
```rust
// BEFORE:
pub fn find_by_bundle(&self, bundle_id: u64, contract_id: u64, min_version: u32) -> ...

// AFTER:
pub fn find_by_bundle(&self, bundle_id: BundleId, contract_id: PluginContractId, min_version: u32) -> ...
```

**In `plugin_registry.rs`:**
```rust
// BEFORE:
pub fn find_slots_by_bundle(&self, bundle_id: u64) -> Vec<u32>

// AFTER:
pub fn find_slots_by_bundle(&self, bundle_id: BundleId) -> Vec<u32>
```

### Step 6.3: Update struct fields

**In `manifest.rs`:**
```rust
// Consider changing:
pub id: u64,
// To:
pub id: BundleId,
```

Note: This requires updating all code that reads/writes manifest.id

---

## Testing Checklist

After each phase, run:

```bash
# Check compilation
cargo check -p polyplug
cargo check -p polyplug_native

# Run tests
cargo test -p polyplug
cargo test -p polyplug_native
```

### Final Verification

```bash
# Verify libloading is NOT used in core
grep -r "libloading" crates/polyplug/src/
# Expected: no matches (except possibly in comments/docs)

# Verify libloading IS used in native
grep -r "libloading" crates/polyplug_native/src/
# Expected: matches found

# Verify no auto-registration
grep -r "NativeBundleLoader" crates/polyplug/src/
# Expected: no matches

# Full build
cargo build --workspace

# Run all tests
cargo test --workspace
```

---

## Safety Contract Documentation

Add this to `crates/polyplug/src/lib.rs` or create a separate `docs/HOT_RELOAD_SAFETY.md`:

```rust
//! # Hot-Reload Safety Contract
//!
//! ## For Host Applications
//!
//! When `on_reload_cb(ReloadPhase::Reloaded)` fires, the host MUST:
//! 1. Release ALL `PluginGuard` handles (automatic via Arc quiescence wait)
//! 2. NOT cache raw function pointers extracted from vtables beyond a single call scope
//!
//! If raw pointers are cached across reload, **CRASHES ARE POSSIBLE** (SIGSEGV/SIGBUS).
//! This is a HOST BUG, not a runtime bug.
//!
//! ### Correct Usage
//! ```rust
//! // CORRECT: Use via PluginGuard
//! let guard = runtime.resolve_plugin(handle)?;
//! guard.some_function(args);
//! ```
//!
//! ### Incorrect Usage
//! ```rust
//! // WRONG: Cache raw pointer
//! let fn_ptr = vtable.some_function;  // raw pointer extracted
//! store_for_later(fn_ptr);  // DANGEROUS - will crash after reload!
//! ```
//!
//! ## For Loader Implementers
//!
//! The `reload()` method must:
//! 1. Load/reload the bundle code (loader-specific)
//! 2. Call init to register new vtables
//! 3. Wait for quiescence (use `wait_for_quiescence()` utility)
//! 4. Clean up old resources AFTER quiescence
//!
//! ## Fail-Fast Philosophy
//!
//! This runtime uses fail-fast semantics:
//! - Stale raw pointers → SIGSEGV (host bug)
//! - Missing loader registration → runtime error (user bug)
//! - Failed quiescence → timeout error (potential leak)
//!
//! No internal safety nets — correct behavior is required.
```

---

## Common Mistakes

### 1. Forgetting to Register NativeLoader

```rust
// WRONG:
let rt = Runtime::builder().plugin_dir(path).build()?;
// Error: NoLoaderForRuntime for native bundles

// CORRECT:
let host_vtable = /* get from runtime */;
let rt = Runtime::builder()
    .plugin_dir(path)
    .loader(NativeLoader::new(NativeConfig::default(), host_vtable))
    .build()?;
```

### 2. Caching Raw Function Pointers

```rust
// WRONG:
let fn_ptr = vtable.some_function;  // raw pointer
store_for_later(fn_ptr);  // CRASH after reload!

// CORRECT:
let guard = runtime.resolve_plugin(handle)?;
guard.some_function(args);  // Use via guard
```

### 3. Missing `runtime` in Manifest

```toml
# WRONG:
name = "my-plugin"
file = "myplugin.so"

# CORRECT:
name = "my-plugin"
runtime = "native"  # REQUIRED
file = "myplugin.so"
```

---

## Summary

| Phase | Files Changed | Breaking? |
|-------|---------------|-----------|
| 1 | `loader/bundle_loader.rs` | Yes (trait change) |
| 2 | `reload.rs` | No (internal) |
| 3 | `polyplug_native/*` | No (new code) |
| 4 | `loader/mod.rs`, `registry.rs`, `runtime.rs`, etc. | Yes (removes API) |
| 5 | `loader/manifest.rs` | Yes (requires runtime) |
| 6 | Many files | Yes (type changes) |

**Total estimated effort:** 2-4 hours

**Result:** Core runtime is loader-agnostic, native code lives in `polyplug_native`.
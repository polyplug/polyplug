//! Reload — hot-reload logic for native plugin bundles.
//!
//! Implements vtable hot-swapping via `ArcSwap`, quiescence wait for in-flight calls,
//! and cascade re-init for bundles that depend on the reloaded bundle.

use core::hint::spin_loop;
use core::time::Duration;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use crate::error::PolyplugError;
use crate::loader::manifest::ManifestData;
use crate::registry::VTableSlot;
use crate::runtime::HostContext;
use crate::runtime::Runtime;

const QUIESCENCE_TIMEOUT: Duration = Duration::from_secs(5_u64);
const MAX_CASCADE_DEPTH: usize = 16_usize;

/// A raw pointer wrapper for PluginInterface that implements Send and Sync.
#[derive(Debug, Clone, Copy)]
pub(crate) struct VTablePtr(pub *const polyplug_abi::PluginInterface);

// SAFETY: VTablePtr wraps a raw pointer to a PluginInterface from a loaded library.
// The vtable remains valid for the lifetime of the loaded library, which is managed
// by the Runtime. During hot-reload, the old library is kept alive until quiescence
// is achieved, ensuring the vtable pointers remain valid.
unsafe impl Send for VTablePtr {}
// SAFETY: Same reasoning as Send — concurrent reads of a vtable pointer are safe
// because vtables are read-only after initialization.
unsafe impl Sync for VTablePtr {}

#[derive(Debug, Clone)]
pub struct ReloadEvent {
    pub bundle_name: String,
    pub bundle_path: String,
    pub old_version: String,
    pub new_version: String,
}

/// Phase of a hot-reload operation for notification callbacks.
#[derive(Debug, Clone)]
pub enum ReloadPhase {
    /// Bundle is being prepared for reload (before vtable swap).
    Preparing {
        bundle_id: u64,
        bundle_name: String,
        retry_count: u32,
    },
    /// Bundle has been successfully reloaded.
    Reloaded { bundle_id: u64, bundle_name: String },
    /// Bundle reload failed.
    Failed {
        bundle_id: u64,
        bundle_name: String,
        reason: String,
    },
}

/// Registrar callback used during reload to capture new vtable pointers.
///
/// # Safety
/// - `rt_ctx` must be a valid pointer to a HostContext
/// - `vtable` must be a valid `PluginInterface` pointer from the reloaded library's init.
pub(crate) unsafe extern "C" fn reload_register_callback(
    rt_ctx: *mut core::ffi::c_void,
    _descriptor: *const polyplug_abi::PluginDescriptor,
    vtable: *const polyplug_abi::PluginInterface,
) -> polyplug_abi::AbiError {
    if rt_ctx.is_null() || vtable.is_null() {
        return polyplug_abi::AbiError::ok();
    }
    // SAFETY: rt_ctx is a valid *mut HostContext passed by the reload code.
    let ctx: &HostContext = unsafe { &*(rt_ctx as *const HostContext) };
    // SAFETY: ctx.runtime is a valid pointer to a Runtime that is guaranteed to be live
    // during the reload operation.
    let runtime: &Runtime = unsafe { &*ctx.runtime };
    let mut guard: std::sync::MutexGuard<'_, Vec<VTablePtr>> =
        runtime.reload_captured_vtables.lock().unwrap_or_else(|e| {
            eprintln!("[polyplug] Mutex poisoned, recovering: {}", e);
            e.into_inner()
        });
    guard.push(VTablePtr(vtable));
    polyplug_abi::AbiError::ok()
}

/// Reload a bundle by path, with cascade depth tracking to prevent infinite loops.
pub(crate) fn reload_bundle_impl(
    runtime: &Runtime,
    path: &Path,
    cascade_depth: usize,
) -> Result<(), PolyplugError> {
    if !runtime.config().hot_reload_enabled {
        return Err(PolyplugError::HotReloadDisabled);
    }

    if cascade_depth >= MAX_CASCADE_DEPTH {
        return Err(PolyplugError::ReloadFailed {
            bundle: path.display().to_string(),
            reason: format!("cascade depth limit ({MAX_CASCADE_DEPTH}) exceeded"),
        });
    }

    // Determine bundle_dir and so_path:
    // - If path is a .so file (watcher-fired path), derive bundle_dir = path.parent()
    // - If path is a directory (cascade reload passes bundle dir), need to get so file from manifest
    let (bundle_dir_path, so_path): (PathBuf, PathBuf) = if path.is_dir() {
        // Path is already the bundle directory (cascade reload case).
        // Parse manifest first to discover the .so filename.
        let temp_manifest: ManifestData = crate::loader::parse_manifest(path)
            .map_err(|e: crate::error::LoaderError| PolyplugError::Loader(e))?;
        let so: PathBuf = path.join(&temp_manifest.file);
        (path.to_path_buf(), so)
    } else {
        // Path is the .so file (watcher path). Bundle dir is the parent.
        let dir: PathBuf = path.parent().unwrap_or(path).to_path_buf();
        (dir, path.to_path_buf())
    };

    let mut manifest: ManifestData = crate::loader::parse_manifest(&bundle_dir_path)
        .map_err(|e: crate::error::LoaderError| PolyplugError::Loader(e))?;
    if manifest.id == 0 {
        return Err(PolyplugError::ReloadFailed {
            bundle: path.display().to_string(),
            reason: "manifest.id is required but was 0 or missing".to_owned(),
        });
    }
    manifest.path = bundle_dir_path.clone();
    if manifest.runtime != "native" {
        runtime.emit_warning(&format!(
            "reload_bundle only supports native bundles; runtime={} path={}",
            manifest.runtime,
            path.display()
        ));
        return Err(PolyplugError::ReloadFailed {
            bundle: path.display().to_string(),
            reason: format!("runtime {} is not reloadable", manifest.runtime),
        });
    }

    let bundle_id_val: u64 = manifest.id;
    let slot_indices: Vec<u32> = runtime.registry().find_slots_by_bundle(bundle_id_val);
    if slot_indices.is_empty() {
        return Err(PolyplugError::ReloadFailed {
            bundle: path.display().to_string(),
            reason: "bundle is not loaded".to_owned(),
        });
    }

    let path_str: String = so_path.to_string_lossy().into_owned();
    let config: &crate::runtime::RuntimeConfig = runtime.config();
    let mut retry_count: u32 = 0_u32;

    // Result of library loading and initialization (computed inside retry loop)
    let new_library: libloading::Library;
    let new_vtable_map: HashMap<u64, *const polyplug_abi::PluginInterface>;

    // Retry loop wraps the entire reload process (library loading + quiescence waiting)
    loop {
        // Phase 1: Fire Preparing notification before any reload work
        if let Some(ref cb) = runtime.on_reload_cb {
            cb(ReloadPhase::Preparing {
                bundle_id: bundle_id_val,
                bundle_name: manifest.name.clone(),
                retry_count,
            });
        }

        // Try to load the new library
        // SAFETY: path points to a compiled plugin bundle; libloading validates the shared library.
        let load_result: Result<libloading::Library, PolyplugError> = unsafe {
            libloading::Library::new(&so_path).map_err(|e: libloading::Error| {
                PolyplugError::ReloadFailed {
                    bundle: path_str.clone(),
                    reason: format!("dlopen failed: {e}"),
                }
            })
        };

        let loaded_library: libloading::Library = match load_result {
            Ok(lib) => lib,
            Err(e) => {
                // Library load failed - check retry limits
                if retry_count >= config.hot_reload_max_retries
                    && config.hot_reload_abort_on_max_retries
                {
                    runtime.emit_warning(&format!(
                        "hot-reload: max retries ({}) exceeded for bundle {} - library load failed",
                        config.hot_reload_max_retries, manifest.name
                    ));
                    if let Some(ref cb) = runtime.on_reload_cb {
                        cb(ReloadPhase::Failed {
                            bundle_id: bundle_id_val,
                            bundle_name: manifest.name.clone(),
                            reason: format!("max retries exceeded: {}", e),
                        });
                    }
                    return Err(e);
                }
                // Retry
                std::thread::sleep(config.hot_reload_retry_interval);
                retry_count = retry_count.saturating_add(1_u32);
                continue;
            }
        };

        // SAFETY: Symbol lookup returns a valid function pointer for polyplug_abi_version.
        let abi_version_sym: libloading::Symbol<'_, unsafe extern "C" fn() -> u32> = unsafe {
            match loaded_library.get(b"polyplug_abi_version\0") {
                Ok(sym) => sym,
                Err(_) => {
                    // Symbol not found - check retry limits
                    if retry_count >= config.hot_reload_max_retries
                        && config.hot_reload_abort_on_max_retries
                    {
                        let err: PolyplugError = PolyplugError::ReloadFailed {
                            bundle: path_str.clone(),
                            reason: "missing symbol polyplug_abi_version".to_owned(),
                        };
                        if let Some(ref cb) = runtime.on_reload_cb {
                            cb(ReloadPhase::Failed {
                                bundle_id: bundle_id_val,
                                bundle_name: manifest.name.clone(),
                                reason: "max retries exceeded: missing symbol".to_owned(),
                            });
                        }
                        return Err(err);
                    }
                    std::thread::sleep(config.hot_reload_retry_interval);
                    retry_count = retry_count.saturating_add(1_u32);
                    continue;
                }
            }
        };
        // SAFETY: abi_version_sym is a valid function pointer just resolved from the library.
        let found_version: u32 = unsafe { abi_version_sym() };
        if found_version != polyplug_abi::POLYPLUG_ABI_VERSION {
            // ABI version mismatch - check retry limits
            if retry_count >= config.hot_reload_max_retries
                && config.hot_reload_abort_on_max_retries
            {
                let err: PolyplugError = PolyplugError::ReloadFailed {
                    bundle: path_str.clone(),
                    reason: format!(
                        "abi version mismatch: expected={}, found={}",
                        polyplug_abi::POLYPLUG_ABI_VERSION,
                        found_version
                    ),
                };
                if let Some(ref cb) = runtime.on_reload_cb {
                    cb(ReloadPhase::Failed {
                        bundle_id: bundle_id_val,
                        bundle_name: manifest.name.clone(),
                        reason: "max retries exceeded: abi version mismatch".to_owned(),
                    });
                }
                return Err(err);
            }
            std::thread::sleep(config.hot_reload_retry_interval);
            retry_count = retry_count.saturating_add(1_u32);
            continue;
        }

        let init_fn_ptr: unsafe extern "C" fn(
            *mut core::ffi::c_void,
            *const polyplug_abi::HostVTable,
            *const polyplug_abi::PluginContext,
        ) -> polyplug_abi::AbiError = {
            // SAFETY: Symbol lookup returns a valid function pointer on success.
            let init_sym: libloading::Symbol<
                '_,
                unsafe extern "C" fn(
                    *mut core::ffi::c_void,
                    *const polyplug_abi::HostVTable,
                    *const polyplug_abi::PluginContext,
                ) -> polyplug_abi::AbiError,
            > = unsafe {
                match loaded_library.get(b"polyplug_init\0") {
                    Ok(sym) => sym,
                    Err(_) => {
                        // Symbol not found - check retry limits
                        if retry_count >= config.hot_reload_max_retries
                            && config.hot_reload_abort_on_max_retries
                        {
                            let err: PolyplugError = PolyplugError::ReloadFailed {
                                bundle: path_str.clone(),
                                reason: "missing symbol polyplug_init".to_owned(),
                            };
                            if let Some(ref cb) = runtime.on_reload_cb {
                                cb(ReloadPhase::Failed {
                                    bundle_id: bundle_id_val,
                                    bundle_name: manifest.name.clone(),
                                    reason: "max retries exceeded: missing init symbol".to_owned(),
                                });
                            }
                            return Err(err);
                        }
                        // Signal retry needed
                        std::thread::sleep(config.hot_reload_retry_interval);
                        retry_count = retry_count.saturating_add(1_u32);
                        continue;
                    }
                }
            };
            *init_sym
        };

        runtime
            .reload_captured_vtables
            .lock()
            .unwrap_or_else(|e| {
                eprintln!("[polyplug] Mutex poisoned, recovering: {}", e);
                e.into_inner()
            })
            .clear();

        // Create PluginContext with bundle_id
        let bundle_path_sv: polyplug_abi::StringView = polyplug_abi::StringView {
            ptr: bundle_dir_path.as_os_str().as_encoded_bytes().as_ptr(),
            len: bundle_dir_path.as_os_str().as_encoded_bytes().len(),
        };
        let ctx: polyplug_abi::PluginContext = polyplug_abi::PluginContext {
            bundle_path: bundle_path_sv,
            host_abi_version: polyplug_abi::POLYPLUG_ABI_VERSION,
            bundle_id: manifest.id,
        };

        // Create HostContext on the stack for dependency enforcement
        let expected_bundle_id: u64 = manifest.id;
        let host_ctx: HostContext = HostContext {
            runtime: runtime as *const Runtime as *mut Runtime,
            bundle_id: expected_bundle_id,
        };

        // Build a temporary HostVTable with our capture callback
        let reload_host_vtable: polyplug_abi::HostVTable = polyplug_abi::HostVTable {
            register_plugin: reload_register_callback,
            alloc: crate::runtime::host_alloc,
            free: crate::runtime::host_free,
            find_by_contract: crate::runtime::host_find_by_contract,
            find_by_bundle: crate::runtime::host_find_by_bundle,
            find_all_by_contract: crate::runtime::host_find_all_by_contract,
            resolve_plugin: crate::runtime::host_resolve_plugin,
            get_extension: crate::runtime::host_get_extension,
        };

        let rt_ctx: *mut core::ffi::c_void =
            &host_ctx as *const HostContext as *mut core::ffi::c_void;
        // SAFETY: init_fn_ptr is resolved from loaded_library which remains alive for this call.
        // rt_ctx is a valid HostContext pointer, and reload_host_vtable is a valid HostVTable.
        let init_result: polyplug_abi::AbiError = unsafe {
            init_fn_ptr(
                rt_ctx,
                &reload_host_vtable as *const polyplug_abi::HostVTable,
                &ctx,
            )
        };

        // Verify bundle_id wasn't tampered with during init
        if host_ctx.bundle_id != expected_bundle_id {
            let err: PolyplugError =
                PolyplugError::Loader(crate::error::LoaderError::BundleTampered {
                    bundle: path_str.clone(),
                    expected: expected_bundle_id,
                    found: host_ctx.bundle_id,
                });
            if let Some(ref cb) = runtime.on_reload_cb {
                cb(ReloadPhase::Failed {
                    bundle_id: bundle_id_val,
                    bundle_name: manifest.name.clone(),
                    reason: "bundle tampered".to_owned(),
                });
            }
            return Err(err);
        }

        if init_result.code != polyplug_abi::ABI_OK {
            // Init failed - check retry limits
            if retry_count >= config.hot_reload_max_retries
                && config.hot_reload_abort_on_max_retries
            {
                let err: PolyplugError = PolyplugError::ReloadFailed {
                    bundle: path_str.clone(),
                    reason: format!("init failed with code {}", init_result.code),
                };
                if let Some(ref cb) = runtime.on_reload_cb {
                    cb(ReloadPhase::Failed {
                        bundle_id: bundle_id_val,
                        bundle_name: manifest.name.clone(),
                        reason: "max retries exceeded: init failed".to_owned(),
                    });
                }
                return Err(err);
            }
            std::thread::sleep(config.hot_reload_retry_interval);
            retry_count = retry_count.saturating_add(1_u32);
            continue;
        }

        let local_captured_vtables: Vec<VTablePtr> = runtime
            .reload_captured_vtables
            .lock()
            .unwrap_or_else(|e| {
                eprintln!("[polyplug] Mutex poisoned, recovering: {}", e);
                e.into_inner()
            })
            .clone();

        let mut local_vtable_map: HashMap<u64, *const polyplug_abi::PluginInterface> =
            HashMap::new();
        for vt_ptr in &local_captured_vtables {
            // SAFETY: vt_ptr.0 returned by init() is valid while loaded_library is alive.
            let contract_id: u64 = unsafe { (*vt_ptr.0).contract_id };
            local_vtable_map.insert(contract_id, vt_ptr.0);
        }

        // Wait for all slots to become quiescent (no active instances).
        // When we call get_vtable_arc(), it returns an Arc with count incremented by 1,
        // so we check for count == 2 (registry + our loaded Arc).
        // This wait uses a proper timeout instead of retry count to handle
        // sustained concurrent access from reader threads.
        let quiescence_start: Instant = Instant::now();
        loop {
            let mut all_slots_quiescent: bool = true;
            for &slot_idx in &slot_indices {
                let arc: Arc<VTableSlot> = match runtime.registry().get_vtable_arc(slot_idx) {
                    Some(a) => a,
                    None => continue,
                };
                if Arc::strong_count(&arc) > 2_usize {
                    all_slots_quiescent = false;
                    break;
                }
            }

            if all_slots_quiescent {
                break;
            }

            if quiescence_start.elapsed() > QUIESCENCE_TIMEOUT {
                runtime.emit_warning(&format!(
                    "hot-reload: quiescence timeout for bundle {} with active instances",
                    manifest.name
                ));
                if let Some(ref cb) = runtime.on_reload_cb {
                    cb(ReloadPhase::Failed {
                        bundle_id: bundle_id_val,
                        bundle_name: manifest.name.clone(),
                        reason: "quiescence timeout with active instances".to_owned(),
                    });
                }
                return Err(PolyplugError::QuiescenceTimeout {
                    bundle: manifest.name.clone(),
                });
            }

            std::thread::sleep(Duration::from_millis(1_u64));
            spin_loop();
        }

        // Success - store results and break out of retry loop
        new_library = loaded_library;
        new_vtable_map = local_vtable_map;
        break;
    }

    let mut old_arcs: Vec<Arc<VTableSlot>> = Vec::new();
    for &slot_idx in &slot_indices {
        let contract_id: u64 = match runtime.registry().get_slot_contract_id(slot_idx) {
            Some(id) => id,
            None => continue,
        };
        let new_vt_ptr: *const polyplug_abi::PluginInterface =
            match new_vtable_map.get(&contract_id) {
                Some(&ptr) => ptr,
                None => continue,
            };
        let new_arc: Arc<VTableSlot> = Arc::new(VTableSlot(new_vt_ptr));
        let old_arc: Arc<VTableSlot> = runtime
            .registry()
            .swap_vtable(slot_idx, new_arc)
            .map_err(|e: crate::error::RegistryError| PolyplugError::Registry(e))?;
        old_arcs.push(old_arc);
    }

    if let Some(ref cb) = runtime.on_reload_cb {
        cb(ReloadPhase::Reloaded {
            bundle_id: bundle_id_val,
            bundle_name: manifest.name.clone(),
        });
    }

    let quiescence_start: Instant = Instant::now();
    for old_arc in &old_arcs {
        loop {
            if Arc::strong_count(old_arc) == 1_usize {
                break;
            }
            if quiescence_start.elapsed() > QUIESCENCE_TIMEOUT {
                return Err(PolyplugError::QuiescenceTimeout {
                    bundle: manifest.name.clone(),
                });
            }
            std::thread::sleep(Duration::from_millis(1_u64));
            spin_loop();
        }
    }

    drop(old_arcs);
    let old_library: Option<libloading::Library> = runtime
        .reload_libraries
        .lock()
        .unwrap_or_else(|e| {
            eprintln!("[polyplug] Mutex poisoned, recovering: {}", e);
            e.into_inner()
        })
        .remove(&bundle_id_val);
    drop(old_library);

    runtime
        .reload_libraries
        .lock()
        .unwrap_or_else(|e| {
            eprintln!("[polyplug] Mutex poisoned, recovering: {}", e);
            e.into_inner()
        })
        .insert(bundle_id_val, new_library);
    runtime
        .bundle_manifests
        .lock()
        .unwrap_or_else(|e| {
            eprintln!("[polyplug] Mutex poisoned, recovering: {}", e);
            e.into_inner()
        })
        .insert(manifest.name.clone(), manifest.clone());

    let dependents: Vec<(String, PathBuf)> = {
        let manifests_guard: std::sync::MutexGuard<'_, HashMap<String, ManifestData>> =
            runtime.bundle_manifests.lock().unwrap_or_else(|e| {
                eprintln!("[polyplug] Mutex poisoned, recovering: {}", e);
                e.into_inner()
            });
        find_cascade_targets(&manifests_guard, &manifest.name)
    };
    for (_dep_name, dep_path) in dependents {
        reload_bundle_impl(runtime, &dep_path, cascade_depth + 1_usize)?;
    }
    Ok(())
}

/// Find bundles that depend on `reloaded_bundle_name` and need re-init.
pub(crate) fn find_cascade_targets(
    manifests: &HashMap<String, ManifestData>,
    reloaded_bundle_name: &str,
) -> Vec<(String, PathBuf)> {
    let mut targets: Vec<(String, PathBuf)> = Vec::new();
    for (name, manifest) in manifests {
        if !manifest.needs_reinit_on_dep_reload {
            continue;
        }
        let depends: bool = manifest
            .resolved_dependencies()
            .iter()
            .any(|dep| match dep {
                crate::loader::manifest::ManifestDependency::ByBundle { bundle, .. } => {
                    bundle.as_str() == reloaded_bundle_name
                }
                crate::loader::manifest::ManifestDependency::ByContract { .. } => false,
            });
        if depends {
            targets.push((name.clone(), manifest.path.clone()));
        }
    }
    targets
}

impl Runtime {
    pub fn reload_bundle(&self, path: &Path) -> Result<(), PolyplugError> {
        reload_bundle_impl(self, path, 0_usize)
    }

    pub fn refresh_handle(
        &self,
        contract_id: u64,
        min_version: u32,
    ) -> Result<polyplug_abi::PluginHandle, crate::error::RegistryError> {
        self.registry().find_by_contract(contract_id, min_version)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;
    use crate::runtime::RuntimeConfig;
    use core::time::Duration;

    // ─── ReloadPhase enum tests ─────────────────────────────────────────────────

    #[test]
    fn reload_phase_preparing_construction_and_field_access() {
        let bundle_id: u64 = 0xABCD_1234_u64;
        let bundle_name: String = "test_bundle".to_owned();
        let retry_count: u32 = 2_u32;

        let phase: ReloadPhase = ReloadPhase::Preparing {
            bundle_id,
            bundle_name: bundle_name.clone(),
            retry_count,
        };

        match phase {
            ReloadPhase::Preparing {
                bundle_id: id,
                bundle_name: name,
                retry_count: count,
            } => {
                assert_eq!(id, bundle_id);
                assert_eq!(name, bundle_name);
                assert_eq!(count, retry_count);
            }
            _ => panic!("expected Preparing variant"),
        }
    }

    #[test]
    fn reload_phase_reloaded_construction_and_field_access() {
        let bundle_id: u64 = 0xDEAD_BEEF_u64;
        let bundle_name: String = "reloaded_bundle".to_owned();

        let phase: ReloadPhase = ReloadPhase::Reloaded {
            bundle_id,
            bundle_name: bundle_name.clone(),
        };

        match phase {
            ReloadPhase::Reloaded {
                bundle_id: id,
                bundle_name: name,
            } => {
                assert_eq!(id, bundle_id);
                assert_eq!(name, bundle_name);
            }
            _ => panic!("expected Reloaded variant"),
        }
    }

    #[test]
    fn reload_phase_failed_construction_and_field_access() {
        let bundle_id: u64 = 0xCAFE_0001_u64;
        let bundle_name: String = "failed_bundle".to_owned();
        let reason: String = "max retries exceeded with active instances".to_owned();

        let phase: ReloadPhase = ReloadPhase::Failed {
            bundle_id,
            bundle_name: bundle_name.clone(),
            reason: reason.clone(),
        };

        match phase {
            ReloadPhase::Failed {
                bundle_id: id,
                bundle_name: name,
                reason: r,
            } => {
                assert_eq!(id, bundle_id);
                assert_eq!(name, bundle_name);
                assert_eq!(r, reason);
            }
            _ => panic!("expected Failed variant"),
        }
    }

    #[test]
    fn reload_phase_debug_impl() {
        let preparing: ReloadPhase = ReloadPhase::Preparing {
            bundle_id: 1_u64,
            bundle_name: "test".to_owned(),
            retry_count: 0_u32,
        };
        let debug_str: String = format!("{preparing:?}");
        assert!(debug_str.contains("Preparing"), "got: {debug_str}");

        let reloaded: ReloadPhase = ReloadPhase::Reloaded {
            bundle_id: 2_u64,
            bundle_name: "test".to_owned(),
        };
        let debug_str: String = format!("{reloaded:?}");
        assert!(debug_str.contains("Reloaded"), "got: {debug_str}");

        let failed: ReloadPhase = ReloadPhase::Failed {
            bundle_id: 3_u64,
            bundle_name: "test".to_owned(),
            reason: "error".to_owned(),
        };
        let debug_str: String = format!("{failed:?}");
        assert!(debug_str.contains("Failed"), "got: {debug_str}");
    }

    #[test]
    fn reload_phase_clone() {
        let original: ReloadPhase = ReloadPhase::Preparing {
            bundle_id: 42_u64,
            bundle_name: "clone_test".to_owned(),
            retry_count: 5_u32,
        };
        let cloned: ReloadPhase = original.clone();

        match (original, cloned) {
            (
                ReloadPhase::Preparing {
                    bundle_id: id1,
                    bundle_name: name1,
                    retry_count: count1,
                },
                ReloadPhase::Preparing {
                    bundle_id: id2,
                    bundle_name: name2,
                    retry_count: count2,
                },
            ) => {
                assert_eq!(id1, id2);
                assert_eq!(name1, name2);
                assert_eq!(count1, count2);
            }
            _ => panic!("both should be Preparing variant"),
        }
    }

    // ─── RuntimeConfig tests ────────────────────────────────────────────────────

    #[test]
    fn runtime_config_default_values() {
        let config: RuntimeConfig = RuntimeConfig::default();

        assert!(!config.hot_reload_enabled);
        assert_eq!(config.hot_reload_max_retries, 3_u32);
        assert_eq!(config.hot_reload_retry_interval, Duration::from_secs(1));
        assert!(config.hot_reload_abort_on_max_retries);
    }

    #[test]
    fn runtime_config_custom_values() {
        let config: RuntimeConfig = RuntimeConfig {
            hot_reload_enabled: true,
            hot_reload_max_retries: 10_u32,
            hot_reload_retry_interval: Duration::from_millis(500),
            hot_reload_abort_on_max_retries: false,
        };

        assert!(config.hot_reload_enabled);
        assert_eq!(config.hot_reload_max_retries, 10_u32);
        assert_eq!(config.hot_reload_retry_interval, Duration::from_millis(500));
        assert!(!config.hot_reload_abort_on_max_retries);
    }

    #[test]
    fn runtime_config_clone() {
        let original: RuntimeConfig = RuntimeConfig {
            hot_reload_enabled: true,
            hot_reload_max_retries: 7_u32,
            hot_reload_retry_interval: Duration::from_millis(250),
            hot_reload_abort_on_max_retries: true,
        };
        let cloned: RuntimeConfig = original.clone();

        assert_eq!(original.hot_reload_enabled, cloned.hot_reload_enabled);
        assert_eq!(
            original.hot_reload_max_retries,
            cloned.hot_reload_max_retries
        );
        assert_eq!(
            original.hot_reload_retry_interval,
            cloned.hot_reload_retry_interval
        );
        assert_eq!(
            original.hot_reload_abort_on_max_retries,
            cloned.hot_reload_abort_on_max_retries
        );
    }

    #[test]
    fn runtime_config_debug_impl() {
        let config: RuntimeConfig = RuntimeConfig::default();
        let debug_str: String = format!("{config:?}");

        assert!(debug_str.contains("RuntimeConfig"), "got: {debug_str}");
        assert!(
            debug_str.contains("hot_reload_max_retries"),
            "got: {debug_str}"
        );
        assert!(
            debug_str.contains("hot_reload_retry_interval"),
            "got: {debug_str}"
        );
        assert!(
            debug_str.contains("hot_reload_abort_on_max_retries"),
            "got: {debug_str}"
        );
    }

    // ─── Notification callback tests ────────────────────────────────────────────

    #[test]
    fn on_reload_callback_receives_preparing_phase() {
        use std::sync::{Arc, Mutex};

        let captured: Arc<Mutex<Option<ReloadPhase>>> = Arc::new(Mutex::new(None));
        let captured_clone: Arc<Mutex<Option<ReloadPhase>>> = Arc::clone(&captured);

        let callback = move |phase: ReloadPhase| {
            let mut guard: std::sync::MutexGuard<'_, Option<ReloadPhase>> =
                captured_clone.lock().unwrap_or_else(|e| {
                    eprintln!("[polyplug] Mutex poisoned, recovering: {}", e);
                    e.into_inner()
                });
            *guard = Some(phase);
        };

        // Simulate firing Preparing notification
        callback(ReloadPhase::Preparing {
            bundle_id: 123_u64,
            bundle_name: "test_bundle".to_owned(),
            retry_count: 0_u32,
        });

        let guard: std::sync::MutexGuard<'_, Option<ReloadPhase>> =
            captured.lock().unwrap_or_else(|e| {
                eprintln!("[polyplug] Mutex poisoned, recovering: {}", e);
                e.into_inner()
            });
        let phase: &Option<ReloadPhase> = &guard;
        match phase {
            Some(ReloadPhase::Preparing {
                bundle_id,
                bundle_name,
                retry_count,
            }) => {
                assert_eq!(*bundle_id, 123_u64);
                assert_eq!(*bundle_name, "test_bundle");
                assert_eq!(*retry_count, 0_u32);
            }
            _ => panic!("expected Preparing phase"),
        }
    }

    #[test]
    fn on_reload_callback_receives_reloaded_phase() {
        use std::sync::{Arc, Mutex};

        let captured: Arc<Mutex<Option<ReloadPhase>>> = Arc::new(Mutex::new(None));
        let captured_clone: Arc<Mutex<Option<ReloadPhase>>> = Arc::clone(&captured);

        let callback = move |phase: ReloadPhase| {
            let mut guard: std::sync::MutexGuard<'_, Option<ReloadPhase>> =
                captured_clone.lock().unwrap_or_else(|e| {
                    eprintln!("[polyplug] Mutex poisoned, recovering: {}", e);
                    e.into_inner()
                });
            *guard = Some(phase);
        };

        // Simulate firing Reloaded notification
        callback(ReloadPhase::Reloaded {
            bundle_id: 456_u64,
            bundle_name: "success_bundle".to_owned(),
        });

        let guard: std::sync::MutexGuard<'_, Option<ReloadPhase>> =
            captured.lock().unwrap_or_else(|e| {
                eprintln!("[polyplug] Mutex poisoned, recovering: {}", e);
                e.into_inner()
            });
        let phase: &Option<ReloadPhase> = &guard;
        match phase {
            Some(ReloadPhase::Reloaded {
                bundle_id,
                bundle_name,
            }) => {
                assert_eq!(*bundle_id, 456_u64);
                assert_eq!(*bundle_name, "success_bundle");
            }
            _ => panic!("expected Reloaded phase"),
        }
    }

    #[test]
    fn on_reload_callback_receives_failed_phase() {
        use std::sync::{Arc, Mutex};

        let captured: Arc<Mutex<Option<ReloadPhase>>> = Arc::new(Mutex::new(None));
        let captured_clone: Arc<Mutex<Option<ReloadPhase>>> = Arc::clone(&captured);

        let callback = move |phase: ReloadPhase| {
            let mut guard: std::sync::MutexGuard<'_, Option<ReloadPhase>> =
                captured_clone.lock().unwrap_or_else(|e| {
                    eprintln!("[polyplug] Mutex poisoned, recovering: {}", e);
                    e.into_inner()
                });
            *guard = Some(phase);
        };

        // Simulate firing Failed notification
        callback(ReloadPhase::Failed {
            bundle_id: 789_u64,
            bundle_name: "failed_bundle".to_owned(),
            reason: "max retries exceeded with active instances".to_owned(),
        });

        let guard: std::sync::MutexGuard<'_, Option<ReloadPhase>> =
            captured.lock().unwrap_or_else(|e| {
                eprintln!("[polyplug] Mutex poisoned, recovering: {}", e);
                e.into_inner()
            });
        let phase: &Option<ReloadPhase> = &guard;
        match phase {
            Some(ReloadPhase::Failed {
                bundle_id,
                bundle_name,
                reason,
            }) => {
                assert_eq!(*bundle_id, 789_u64);
                assert_eq!(*bundle_name, "failed_bundle");
                assert_eq!(*reason, "max retries exceeded with active instances");
            }
            _ => panic!("expected Failed phase"),
        }
    }

    #[test]
    fn notification_sequence_preparing_then_reloaded() {
        use std::sync::{Arc, Mutex};

        let captured: Arc<Mutex<Vec<ReloadPhase>>> = Arc::new(Mutex::new(Vec::new()));
        let captured_clone: Arc<Mutex<Vec<ReloadPhase>>> = Arc::clone(&captured);

        let callback = move |phase: ReloadPhase| {
            let mut guard: std::sync::MutexGuard<'_, Vec<ReloadPhase>> =
                captured_clone.lock().unwrap_or_else(|e| {
                    eprintln!("[polyplug] Mutex poisoned, recovering: {}", e);
                    e.into_inner()
                });
            guard.push(phase);
        };

        // Simulate successful reload sequence
        callback(ReloadPhase::Preparing {
            bundle_id: 100_u64,
            bundle_name: "seq_bundle".to_owned(),
            retry_count: 0_u32,
        });
        callback(ReloadPhase::Reloaded {
            bundle_id: 100_u64,
            bundle_name: "seq_bundle".to_owned(),
        });

        let guard: std::sync::MutexGuard<'_, Vec<ReloadPhase>> =
            captured.lock().unwrap_or_else(|e| {
                eprintln!("[polyplug] Mutex poisoned, recovering: {}", e);
                e.into_inner()
            });
        let phases: &Vec<ReloadPhase> = &guard;

        assert_eq!(phases.len(), 2);
        match &phases[0] {
            ReloadPhase::Preparing { .. } => {}
            _ => panic!("expected first phase to be Preparing"),
        }
        match &phases[1] {
            ReloadPhase::Reloaded { .. } => {}
            _ => panic!("expected second phase to be Reloaded"),
        }
    }

    #[test]
    fn notification_sequence_preparing_then_failed() {
        use std::sync::{Arc, Mutex};

        let captured: Arc<Mutex<Vec<ReloadPhase>>> = Arc::new(Mutex::new(Vec::new()));
        let captured_clone: Arc<Mutex<Vec<ReloadPhase>>> = Arc::clone(&captured);

        let callback = move |phase: ReloadPhase| {
            let mut guard: std::sync::MutexGuard<'_, Vec<ReloadPhase>> =
                captured_clone.lock().unwrap_or_else(|e| {
                    eprintln!("[polyplug] Mutex poisoned, recovering: {}", e);
                    e.into_inner()
                });
            guard.push(phase);
        };

        // Simulate failed reload sequence
        callback(ReloadPhase::Preparing {
            bundle_id: 200_u64,
            bundle_name: "fail_bundle".to_owned(),
            retry_count: 3_u32,
        });
        callback(ReloadPhase::Failed {
            bundle_id: 200_u64,
            bundle_name: "fail_bundle".to_owned(),
            reason: "max retries exceeded with active instances".to_owned(),
        });

        let guard: std::sync::MutexGuard<'_, Vec<ReloadPhase>> =
            captured.lock().unwrap_or_else(|e| {
                eprintln!("[polyplug] Mutex poisoned, recovering: {}", e);
                e.into_inner()
            });
        let phases: &Vec<ReloadPhase> = &guard;

        assert_eq!(phases.len(), 2);
        match &phases[0] {
            ReloadPhase::Preparing { retry_count, .. } => {
                assert_eq!(*retry_count, 3_u32);
            }
            _ => panic!("expected first phase to be Preparing"),
        }
        match &phases[1] {
            ReloadPhase::Failed { reason, .. } => {
                assert_eq!(*reason, "max retries exceeded with active instances");
            }
            _ => panic!("expected second phase to be Failed"),
        }
    }

    #[test]
    fn retry_count_increments_across_notifications() {
        use std::sync::{Arc, Mutex};

        let captured: Arc<Mutex<Vec<u32>>> = Arc::new(Mutex::new(Vec::new()));
        let captured_clone: Arc<Mutex<Vec<u32>>> = Arc::clone(&captured);

        let callback = move |phase: ReloadPhase| {
            if let ReloadPhase::Preparing { retry_count, .. } = phase {
                let mut guard: std::sync::MutexGuard<'_, Vec<u32>> =
                    captured_clone.lock().unwrap_or_else(|e| {
                        eprintln!("[polyplug] Mutex poisoned, recovering: {}", e);
                        e.into_inner()
                    });
                guard.push(retry_count);
            }
        };

        // Simulate retry mechanism: Preparing fires with incrementing retry_count
        for retry in 0_u32..=3_u32 {
            callback(ReloadPhase::Preparing {
                bundle_id: 300_u64,
                bundle_name: "retry_bundle".to_owned(),
                retry_count: retry,
            });
        }

        let guard: std::sync::MutexGuard<'_, Vec<u32>> = captured.lock().unwrap_or_else(|e| {
            eprintln!("[polyplug] Mutex poisoned, recovering: {}", e);
            e.into_inner()
        });
        let retry_counts: &Vec<u32> = &guard;

        assert_eq!(retry_counts, &[0_u32, 1_u32, 2_u32, 3_u32]);
    }

    // ─── ReloadEvent tests ──────────────────────────────────────────────────────

    #[test]
    fn reload_event_construction_and_field_access() {
        let event: ReloadEvent = ReloadEvent {
            bundle_name: "event_bundle".to_owned(),
            bundle_path: "/path/to/bundle".to_owned(),
            old_version: "1.0.0".to_owned(),
            new_version: "2.0.0".to_owned(),
        };

        assert_eq!(event.bundle_name, "event_bundle");
        assert_eq!(event.bundle_path, "/path/to/bundle");
        assert_eq!(event.old_version, "1.0.0");
        assert_eq!(event.new_version, "2.0.0");
    }

    #[test]
    fn reload_event_clone() {
        let original: ReloadEvent = ReloadEvent {
            bundle_name: "clone_event".to_owned(),
            bundle_path: "/clone/path".to_owned(),
            old_version: "0.1.0".to_owned(),
            new_version: "0.2.0".to_owned(),
        };
        let cloned: ReloadEvent = original.clone();

        assert_eq!(original.bundle_name, cloned.bundle_name);
        assert_eq!(original.bundle_path, cloned.bundle_path);
        assert_eq!(original.old_version, cloned.old_version);
        assert_eq!(original.new_version, cloned.new_version);
    }

    #[test]
    fn reload_event_debug_impl() {
        let event: ReloadEvent = ReloadEvent {
            bundle_name: "debug_event".to_owned(),
            bundle_path: "/debug/path".to_owned(),
            old_version: "1.0".to_owned(),
            new_version: "2.0".to_owned(),
        };
        let debug_str: String = format!("{event:?}");

        assert!(debug_str.contains("ReloadEvent"), "got: {debug_str}");
        assert!(debug_str.contains("debug_event"), "got: {debug_str}");
        assert!(debug_str.contains("/debug/path"), "got: {debug_str}");
    }
}

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
use crate::runtime::Runtime;

const QUIESCENCE_TIMEOUT: Duration = Duration::from_secs(5_u64);
const MAX_CASCADE_DEPTH: usize = 16_usize;

#[derive(Debug, Clone)]
pub struct ReloadEvent {
    pub bundle_name: String,
    pub bundle_path: String,
    pub old_version: String,
    pub new_version: String,
    pub affected_contract_ids: Vec<u64>,
}

thread_local! {
    static RELOAD_CAPTURED_VTABLES: core::cell::RefCell<Vec<*const crate::abi::PluginVTable>> =
        const { core::cell::RefCell::new(Vec::new()) };
}

pub(crate) unsafe extern "C" fn reload_registrar_callback(
    _registrar: *mut crate::abi::PluginRegistrar,
    _descriptor: *const crate::abi::PluginDescriptor,
    vtable: *const crate::abi::PluginVTable,
) -> crate::abi::AbiError {
    // SAFETY: vtable ptr comes from plugin init. Null check required before capture.
    if !vtable.is_null() {
        RELOAD_CAPTURED_VTABLES.with(
            |v: &core::cell::RefCell<Vec<*const crate::abi::PluginVTable>>| {
                v.borrow_mut().push(vtable);
            },
        );
    }
    crate::abi::AbiError::ok()
}

pub(crate) fn reload_bundle_impl(
    runtime: &Runtime,
    path: &Path,
    cascade_depth: usize,
) -> Result<(), PolyplugError> {
    if cascade_depth >= MAX_CASCADE_DEPTH {
        return Err(PolyplugError::ReloadFailed {
            bundle: path.display().to_string(),
            reason: format!("cascade depth limit ({MAX_CASCADE_DEPTH}) exceeded"),
        });
    }

    let mut manifest: ManifestData = crate::loader::parse_manifest(path)
        .map_err(|e: crate::error::LoaderError| PolyplugError::Loader(e))?;
    manifest.bundle_id = crate::abi::bundle_id(&manifest.bundle_name);
    manifest.path = path.to_path_buf();
    if manifest.runtime != "native" {
        crate::runtime::emit_warning(&format!(
            "reload_bundle only supports native bundles; runtime={} path={}",
            manifest.runtime,
            path.display()
        ));
        return Err(PolyplugError::ReloadFailed {
            bundle: path.display().to_string(),
            reason: format!("runtime {} is not reloadable", manifest.runtime),
        });
    }

    let bundle_id_val: u64 = manifest.bundle_id;
    let slot_indices: Vec<u32> = runtime.registry().find_slots_by_bundle(bundle_id_val);
    if slot_indices.is_empty() {
        return Err(PolyplugError::ReloadFailed {
            bundle: path.display().to_string(),
            reason: "bundle is not loaded".to_owned(),
        });
    }

    let path_str: String = path.to_string_lossy().into_owned();
    // SAFETY: path points to a compiled plugin bundle; libloading validates the shared library.
    let new_library: libloading::Library = unsafe {
        libloading::Library::new(path).map_err(|e: libloading::Error| {
            PolyplugError::ReloadFailed {
                bundle: path_str.clone(),
                reason: format!("dlopen failed: {e}"),
            }
        })?
    };
    // SAFETY: Symbol lookup returns a valid function pointer for polyplug_abi_version.
    let abi_version_sym: libloading::Symbol<'_, unsafe extern "C" fn() -> u32> = unsafe {
        new_library
            .get(b"polyplug_abi_version\0")
            .map_err(|_| PolyplugError::ReloadFailed {
                bundle: path_str.clone(),
                reason: "missing symbol polyplug_abi_version".to_owned(),
            })?
    };
    // SAFETY: abi_version_sym is a valid function pointer just resolved from the library.
    let found_version: u32 = unsafe { abi_version_sym() };
    if found_version != crate::abi::POLYPLUG_ABI_VERSION {
        return Err(PolyplugError::ReloadFailed {
            bundle: path_str.clone(),
            reason: format!(
                "abi version mismatch: expected={}, found={}",
                crate::abi::POLYPLUG_ABI_VERSION,
                found_version
            ),
        });
    }
    let init_fn_ptr: unsafe extern "C" fn(
        *mut crate::abi::PluginRegistrar,
    ) -> crate::abi::AbiError = {
        // SAFETY: Symbol lookup returns a valid function pointer on success.
        let init_sym: libloading::Symbol<
            '_,
            unsafe extern "C" fn(*mut crate::abi::PluginRegistrar) -> crate::abi::AbiError,
        > = unsafe {
            new_library
                .get(b"polyplug_init\0")
                .map_err(|_| PolyplugError::ReloadFailed {
                    bundle: path_str.clone(),
                    reason: "missing symbol polyplug_init".to_owned(),
                })?
        };
        *init_sym
    };

    RELOAD_CAPTURED_VTABLES.with(|v| v.borrow_mut().clear());
    let mut reload_registrar: crate::abi::PluginRegistrar = crate::abi::PluginRegistrar {
        register_plugin: reload_registrar_callback,
        host: runtime.host_vtable_ref() as *const crate::abi::HostVTable,
    };
    // SAFETY: init_fn_ptr is resolved from new_library which remains alive for this call.
    let init_result: crate::abi::AbiError =
        unsafe { init_fn_ptr(&mut reload_registrar as *mut crate::abi::PluginRegistrar) };
    if init_result.code != crate::abi::ABI_OK {
        return Err(PolyplugError::ReloadFailed {
            bundle: path_str.clone(),
            reason: format!("init failed with code {}", init_result.code),
        });
    }
    let captured_vtables: Vec<*const crate::abi::PluginVTable> =
        RELOAD_CAPTURED_VTABLES.with(|v| v.borrow().clone());

    let mut new_vtable_map: HashMap<u64, *const crate::abi::PluginVTable> = HashMap::new();
    for &vt_ptr in &captured_vtables {
        // SAFETY: vt_ptr returned by init() is valid while new_library is alive.
        let contract_id: u64 = unsafe { (*vt_ptr).contract_id };
        new_vtable_map.insert(contract_id, vt_ptr);
    }

    let mut old_arcs: Vec<Arc<VTableSlot>> = Vec::new();
    for &slot_idx in &slot_indices {
        let contract_id: u64 = match runtime.registry().get_slot_contract_id(slot_idx) {
            Some(id) => id,
            None => continue,
        };
        let new_vt_ptr: *const crate::abi::PluginVTable = match new_vtable_map.get(&contract_id) {
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

    let old_version: String = {
        let manifests_guard = runtime
            .bundle_manifests
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        manifests_guard
            .get(&manifest.bundle_name)
            .map(|m: &ManifestData| m.version.clone())
            .unwrap_or_default()
    };
    let event: ReloadEvent = ReloadEvent {
        bundle_name: manifest.bundle_name.clone(),
        bundle_path: path.display().to_string(),
        old_version,
        new_version: manifest.version.clone(),
        affected_contract_ids: new_vtable_map.keys().copied().collect::<Vec<u64>>(),
    };
    if let Some(ref cb) = runtime.on_reload_cb {
        cb(event);
    }

    let quiescence_start: Instant = Instant::now();
    for old_arc in &old_arcs {
        loop {
            if Arc::strong_count(old_arc) == 1_usize {
                break;
            }
            if quiescence_start.elapsed() > QUIESCENCE_TIMEOUT {
                return Err(PolyplugError::QuiescenceTimeout {
                    bundle: manifest.bundle_name.clone(),
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
        .unwrap_or_else(|e| e.into_inner())
        .remove(&bundle_id_val);
    drop(old_library);

    runtime
        .reload_libraries
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(bundle_id_val, new_library);
    runtime
        .bundle_manifests
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(manifest.bundle_name.clone(), manifest.clone());

    let dependents: Vec<(String, PathBuf)> = {
        let manifests_guard = runtime
            .bundle_manifests
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        find_cascade_targets(&manifests_guard, &manifest.bundle_name)
    };
    for (_dep_name, dep_path) in dependents {
        reload_bundle_impl(runtime, &dep_path, cascade_depth + 1_usize)?;
    }
    Ok(())
}

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
    ) -> Result<crate::abi::PluginHandle, crate::error::RegistryError> {
        self.registry().find_by_contract(contract_id, min_version)
    }
}

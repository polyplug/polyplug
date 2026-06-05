//! Reload — callback-based hot-reload framework for all loaders.
//!
//! Provides:
//! - Warning check for potential instance leaks after Preparing callback
//! - Explicit interface swap after init succeeds
//!
//! Hot-reload flow (callback-based model):
//! 1. Fire `ReloadPhase::preparing()` — host destroys all instances here
//! 2. Check Arc::strong_count — emit warning if refs remain (informational only)
//! 3. Call loader.reload() — load new library, init (registers new interfaces)
//! 4. Swap interfaces — for each slot, find new interface and swap atomically
//! 5. Fire `ReloadPhase::reloaded()` — host can create new instances
//!
//! If init fails: Fire `ReloadPhase::failed()`, no interface swap.
//!
//! Safety contract: Host MUST destroy all instances in Preparing callback.
//! Runtime emits warning if instances may remain but proceeds with reload.

use std::collections::HashSet;
use std::sync::Arc;

use polyplug_abi::runtime::ReloadPhase;
use polyplug_abi::types::StringView;
use polyplug_utils::{BundleId, GuestContractId};

use crate::error::RuntimeError;
use crate::loader::ManifestData;
use crate::runtime::Runtime;

/// Helper to create a StringView from a Rust string slice.
fn string_view(s: &str) -> StringView {
    StringView {
        ptr: s.as_ptr(),
        len: s.len(),
    }
}

/// Event describing a completed reload (for logging/telemetry).
#[derive(Debug, Clone)]
pub struct ReloadEvent {
    pub bundle_name: String,
    pub bundle_path: String,
    pub old_version: String,
    pub new_version: String,
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
    ///
    /// # Cascade
    /// After the primary bundle reloads successfully, any other loaded bundle that
    /// declared a dependency on one of this bundle's contracts and opted in via
    /// `needs_reinit_on_dep_reload = true` is reloaded automatically. Cascade
    /// reloads fire their own `Preparing`/`Reloaded`/`Failed` callbacks. A cascade
    /// failure does not fail the primary reload: it is logged as a warning and the
    /// caller still observes `Ok(())` for the primary bundle.
    pub fn reload_bundle(&self, path: &std::path::Path) -> Result<(), RuntimeError> {
        let mut visited: HashSet<BundleId> = HashSet::new();
        self.reload_bundle_with_visited(path, &mut visited)
    }

    /// Reload a bundle, tracking already-reloaded bundles in `visited` to break
    /// dependency cycles during cascade reloads.
    fn reload_bundle_with_visited(
        &self,
        path: &std::path::Path,
        visited: &mut HashSet<BundleId>,
    ) -> Result<(), RuntimeError> {
        if !self.config().hot_reload_enabled {
            return Err(RuntimeError::HotReloadDisabled);
        }

        // `path` points to the bundle's shared-library file; its parent directory
        // holds the manifest. A directory path is accepted as the bundle dir
        // directly (the loader resolves the .so from the manifest's `file`).
        let bundle_dir: &std::path::Path = if path.is_dir() {
            path
        } else {
            path.parent().unwrap_or(path)
        };

        let manifest: ManifestData =
            crate::loader::parse_manifest(bundle_dir).map_err(RuntimeError::Loader)?;

        // Find the loader (lock released before reload() runs — see `loader_for`).
        let loader: &dyn crate::loader::BundleLoader =
            self.loader_for(&manifest.runtime).ok_or_else(|| {
                RuntimeError::Loader(crate::error::LoaderError::NoLoaderForRuntime {
                    bundle: path.display().to_string(),
                    runtime_name: manifest.runtime.clone(),
                })
            })?;

        let bundle_id: BundleId = BundleId::new(&manifest.name);

        // Validate that the requested library file exists before doing any work.
        // A missing file is a reload failure that must fire the Failed callback so
        // the host learns the active version was kept.
        if !path.is_dir() && !path.exists() {
            let err: RuntimeError = RuntimeError::Loader(crate::error::LoaderError::InitFailed {
                bundle: manifest.name.clone(),
                error: format!("bundle library not found at {}", path.display()),
            });
            if let Some(cb) = self.on_reload_cb() {
                (cb.0)(
                    self.config().on_reload_user_data,
                    ReloadPhase::failed(
                        bundle_id,
                        string_view(&manifest.name),
                        string_view(&err.to_string()),
                    ),
                );
            }
            return Err(err);
        }

        // Store slot indices before reload (for warning check and interface swap)
        let slot_indices: Vec<u32> = self.registry.get_bundle_plugin_slots(bundle_id);

        // Fire Preparing callback
        if let Some(cb) = self.on_reload_cb() {
            (cb.0)(
                self.config().on_reload_user_data,
                ReloadPhase::preparing(bundle_id, string_view(&manifest.name)),
            );
        }

        // ─── Warning Check: Informational only, not blocking ─────────────────
        // Check Arc::strong_count after Preparing callback returned.
        // If > 1, host may not have destroyed all instances - emit UB warning.
        for slot_idx in &slot_indices {
            if let Some(arc) = self.registry.get_guest_contract_interface_arc(*slot_idx) {
                if Arc::strong_count(&arc) > 1 {
                    self.emit_warning(&format!(
                        "Potential UB: Arc refs still exist for bundle '{}' after Preparing callback. \
                         Host may not have destroyed all instances. Proceeding with reload anyway.",
                        manifest.name
                    ));
                    // Only emit once per bundle, not per slot
                    break;
                }
            }
        }

        // Open the reload window: interfaces registered during loader.reload() are
        // kept out of the find index (pending) so readers never see two live slots
        // per contract during the swap window. apply_reload_swap closes it on success;
        // the failure path closes it explicitly below.
        self.registry.begin_reload(bundle_id);

        // Call loader's reload() - this does load+init, registering new interfaces
        let result: Result<(), crate::error::RuntimeError> = loader.reload(&manifest, self);

        match result {
            Ok(()) => {
                // ─── Reconcile interfaces after init succeeds (HR-05) ─────────
                // loader.reload() called polyplug_init, which registered the new
                // version's interfaces into fresh slots (registration never
                // vacates the old slots). Move each new interface into its
                // pre-reload slot and retire the duplicate new slot, atomically.
                self.registry
                    .apply_reload_swap(bundle_id, &slot_indices)
                    .map_err(RuntimeError::Registry)?;

                // Mark this bundle visited before cascading so a dependency cycle
                // (A→B→A) terminates instead of recursing forever.
                visited.insert(bundle_id);

                // Cascade: reload dependents that opted in via
                // `needs_reinit_on_dep_reload` and depend on a contract this
                // bundle provides.
                self.cascade_reload_dependents(bundle_id, visited);

                // Fire Reloaded callback
                if let Some(cb) = self.on_reload_cb() {
                    (cb.0)(
                        self.config().on_reload_user_data,
                        ReloadPhase::reloaded(bundle_id, string_view(&manifest.name)),
                    );
                }
                Ok(())
            }
            Err(e) => {
                // Abort the reload window: init failed, so no swap happens. Purge any
                // pending slots the failed init registered (kept out of the find index)
                // so they do not accumulate across retries.
                self.registry.abort_reload(bundle_id, &slot_indices);
                // Fire Failed callback - NO interface swap on failure
                if let Some(cb) = self.on_reload_cb() {
                    (cb.0)(
                        self.config().on_reload_user_data,
                        ReloadPhase::failed(
                            bundle_id,
                            string_view(&manifest.name),
                            string_view(&e.to_string()),
                        ),
                    );
                }
                Err(e)
            }
        }
    }

    /// Reload bundles that depend on `reloaded_bundle_id` and opted in via
    /// `needs_reinit_on_dep_reload`.
    ///
    /// `visited` already contains the reloaded bundle (and any ancestor that
    /// triggered this cascade), so cycles terminate. Cascade failures are logged
    /// as warnings and never propagated — the primary reload already succeeded.
    fn cascade_reload_dependents(
        &self,
        reloaded_bundle_id: BundleId,
        visited: &mut HashSet<BundleId>,
    ) {
        // Step 1: contracts the reloaded bundle exports.
        let exported: HashSet<GuestContractId> = self
            .registry
            .bundle_exported_contracts(reloaded_bundle_id)
            .into_iter()
            .collect();
        if exported.is_empty() {
            return;
        }

        // Step 2: dependent bundles that opted into cascade reload. Collect their
        // names and paths while holding the manifest lock, then release it before
        // reloading (reload re-acquires the manifest lock).
        let dependent_ids: Vec<BundleId> = self.registry.bundles_depending_on_any(&exported);
        let mut candidates: Vec<(String, std::path::PathBuf)> = {
            let manifests: std::sync::MutexGuard<
                '_,
                std::collections::HashMap<String, ManifestData>,
            > = self.bundle_manifests.lock().unwrap_or_else(|e| {
                eprintln!("[polyplug] Mutex poisoned, recovering: {}", e);
                e.into_inner()
            });
            let mut collected: Vec<(String, std::path::PathBuf)> = Vec::new();
            for manifest in manifests.values() {
                let dep_bundle_id: BundleId = BundleId::new(&manifest.name);
                if dep_bundle_id == reloaded_bundle_id
                    || !dependent_ids.contains(&dep_bundle_id)
                    || !manifest.needs_reinit_on_dep_reload
                {
                    continue;
                }
                collected.push((manifest.name.clone(), manifest.path.clone()));
            }
            collected
        };

        // Step 3: reload each candidate in a deterministic order, skipping any
        // already visited (cycle detection).
        candidates.sort_by(|a, b| a.0.cmp(&b.0));
        for (dep_name, dep_path) in candidates {
            let dep_bundle_id: BundleId = BundleId::new(&dep_name);
            if visited.contains(&dep_bundle_id) {
                continue;
            }
            if let Err(e) = self.reload_bundle_with_visited(dep_path.as_path(), visited) {
                self.emit_warning(&format!(
                    "cascade reload of dependent bundle '{}' failed after '{}' reloaded: {}",
                    dep_name,
                    reloaded_bundle_id.id(),
                    e
                ));
            }
        }
    }

    /// Refresh a contract handle after reload.
    ///
    /// Returns a new handle for the contract.
    pub fn refresh_handle(
        &self,
        contract_id: u64,
        min_version: u32,
    ) -> Result<polyplug_abi::plugin::GuestContractHandle, crate::error::RegistryError> {
        self.registry()
            .find_guest_contract(GuestContractId::from_u64(contract_id), min_version)
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;
    use polyplug_abi::runtime::ReloadPhaseType;

    #[test]
    fn reload_phase_preparing_construction() {
        let bundle_id = BundleId::new("test-bundle");
        let name = StringView::from_static(b"test_bundle");
        let phase = ReloadPhase::preparing(bundle_id, name);

        assert_eq!(phase.phase_type, ReloadPhaseType::Preparing);
        assert_eq!(phase.bundle_id, bundle_id);
    }

    #[test]
    fn reload_phase_reloaded_construction() {
        let bundle_id = BundleId::new("test-bundle");
        let name = StringView::from_static(b"test_bundle");
        let phase = ReloadPhase::reloaded(bundle_id, name);

        assert_eq!(phase.phase_type, ReloadPhaseType::Reloaded);
        assert_eq!(phase.bundle_id, bundle_id);
    }

    #[test]
    fn reload_phase_failed_construction() {
        let bundle_id = BundleId::new("test-bundle");
        let name = StringView::from_static(b"test_bundle");
        let reason = StringView::from_static(b"init failed");
        let phase = ReloadPhase::failed(bundle_id, name, reason);

        assert_eq!(phase.phase_type, ReloadPhaseType::Failed);
        assert_eq!(phase.bundle_id, bundle_id);
        assert_eq!(phase.reason.len, 11);
    }

    #[test]
    fn reload_phase_clone() {
        let bundle_id = BundleId::new("test-bundle");
        let name = StringView::from_static(b"test");
        let original = ReloadPhase::preparing(bundle_id, name);
        let cloned: ReloadPhase = original;

        assert_eq!(original.phase_type, cloned.phase_type);
        assert_eq!(original.bundle_id, cloned.bundle_id);
    }

    #[test]
    fn reload_event_construction() {
        let event = ReloadEvent {
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

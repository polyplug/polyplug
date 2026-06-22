//! Reload — callback-based hot-reload framework for all loaders.
//!
//! Provides:
//! - Explicit interface swap after init succeeds
//!
//! Hot-reload flow (callback-based model):
//! 1. Fire `ReloadPhase::preparing()` — host destroys all instances here
//! 2. Call loader.reload() — load new library, init (registers new interfaces)
//! 3. Swap interfaces — for each slot, find new interface and swap atomically
//! 4. Fire `ReloadPhase::reloaded()` — host can create new instances
//!
//! If init fails: Fire `ReloadPhase::failed()`, no interface swap.
//!
//! Safety contract: Host MUST destroy all instances in Preparing callback.

use std::collections::HashSet;

use polyplug_abi::runtime::ReloadPhase;
use polyplug_abi::types::{LogLevel, StringView};
use polyplug_utils::{BundleId, GuestContractId};

use crate::error::RuntimeError;
use crate::loader::ManifestData;
use crate::logger::{RecoverPoisoned, RecoveringGuard};
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
    /// - `NoLoaderForName`: No loader registered for this loader name
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
    ///
    /// # Concurrency & reentrancy
    /// Reloads are serialized per `Runtime` via `reload_serialize`: concurrent
    /// reloads run one at a time so each one's snapshot↔swap is atomic. The lock is
    /// not reentrant, so a reload callback (`Preparing`/`Reloaded`/`Failed`) must
    /// not synchronously trigger another reload on the same runtime — doing so
    /// deadlocks. Cascade reloads are driven internally *after* the swap via the
    /// lock-free `reload_bundle_with_visited`, so they are unaffected.
    pub fn reload_bundle(&self, path: &std::path::Path) -> Result<(), RuntimeError> {
        // Serialize this reload against any other concurrent reload. A reload's
        // pre-reload slot snapshot and the `apply_reload_swap` that consumes it
        // straddle `loader.reload()`, so the registry `RwLock` (dropped between
        // those steps) does not make the sequence atomic. Without this guard, two
        // reloads of the same bundle can interleave so that one reload's snapshot
        // goes stale and its swap vacates a contract's only live slot — see the
        // `reload_serialize` field docs. The guard is held across the entire
        // cascade tree; the recursive `reload_bundle_with_visited` (also used for
        // cascade dependents) never re-acquires it, so cascades cannot self-deadlock.
        let _reload_guard: RecoveringGuard<std::sync::MutexGuard<'_, ()>> = self
            .reload_serialize
            .lock()
            .recover_poisoned(self.logger, "reload");
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

        let bundle_id: BundleId = BundleId::new(&manifest.name);

        // Validate the manifest before doing any work. This re-runs the identity
        // tamper check (id == FNV1a-64(name), TRUST_MODEL §2) on every reload — a
        // bundle whose on-disk manifest was swapped for one with a mismatched id must
        // be rejected, not silently reloaded. A validation failure is a reload
        // failure: fire the Failed callback so the host learns the active version was
        // kept, mirroring the missing-file and init-failure paths below.
        if let Err(e) = manifest.validate() {
            let err: RuntimeError = RuntimeError::Loader(e);
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

        // Find the loader (lock released before reload() runs — see `loader_for`).
        let loader: &dyn crate::loader::BundleLoader =
            self.loader_for(&manifest.loader).ok_or_else(|| {
                RuntimeError::Loader(crate::error::LoaderError::NoLoaderForName {
                    bundle: path.display().to_string(),
                    loader_name: manifest.loader.clone(),
                })
            })?;

        // A loader that does not support hot-reload (e.g. python, dotnet) gates the
        // same way config-disabled does: surface `HotReloadDisabled` and never call
        // `loader.reload()`. This is the single place the per-loader capability is
        // enforced — the loaders' `reload()` bodies no longer re-check it.
        if !loader.supports_hot_reload() {
            return Err(RuntimeError::HotReloadDisabled);
        }

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

        // Store slot indices before reload (for the interface swap)
        let slot_indices: Vec<u32> = self.registry.get_bundle_plugin_slots(bundle_id);

        // Fire Preparing callback
        if let Some(cb) = self.on_reload_cb() {
            (cb.0)(
                self.config().on_reload_user_data,
                ReloadPhase::preparing(bundle_id, string_view(&manifest.name)),
            );
        }

        // After the Preparing callback (the host's window to destroy instances),
        // warn if any of this bundle's contracts still have live guest instances.
        // A vacated interface keeps such instances valid today, but they must be
        // destroyed before the bundle is truly freed — surface the hazard, do not
        // block the reload (informational only).
        let exported: Vec<GuestContractId> = self.registry.bundle_exported_contracts(bundle_id);
        let live: u64 = self.live_instance_count_for_contracts(&exported);
        if live > 0 {
            let name: String = manifest.name.clone();
            self.logger.log(LogLevel::Warn, "reload", || {
                format!(
                    "reload: bundle '{name}' still has {live} live guest instance(s) across its \
                     contracts after the Preparing callback; destroy them before reload to avoid \
                     use-after-free. Proceeding anyway."
                )
            });
        }

        // Open the reload window: interfaces registered during loader.reload() are
        // kept out of the find index (pending) so readers never see two live slots
        // per contract during the swap window. apply_reload_swap closes it on success;
        // the failure path closes it explicitly below.
        self.registry.begin_reload(bundle_id);

        // Call loader's reload() - this does load+init, registering new interfaces
        let result: Result<(), crate::error::RuntimeError> =
            loader.reload(&manifest, self).map_err(RuntimeError::Loader);

        match result {
            Ok(()) => {
                // ─── Reconcile interfaces after init succeeds (HR-05) ─────────
                // loader.reload() called polyplug_init, which registered the new
                // version's interfaces into fresh slots (registration never
                // vacates the old slots). Move each new interface into its
                // pre-reload slot and vacate the duplicate new slot, atomically.
                //
                // A swap failure is a reload failure: fire the Failed callback (the
                // active version is kept) before propagating, mirroring the loader
                // failure path below — otherwise the host never learns the reload
                // aborted.
                if let Err(e) = self.registry.apply_reload_swap(bundle_id, &slot_indices) {
                    let err: RuntimeError = RuntimeError::Registry(e);
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

                // The swap reclaimed the previous interface and the guest-side state
                // of every instance it created: all pre-reload instances of this
                // bundle's contracts are now dead (a correct caller revalidates and
                // recreates its instance on the new interface). Reset their live
                // accounting so instances abandoned across the reload — never
                // destroyed through the vacated interface — do not inflate the
                // diagnostic count forever.
                self.reset_instance_counts_for_contracts(&exported);

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
            let manifests: RecoveringGuard<
                std::sync::MutexGuard<'_, std::collections::HashMap<String, ManifestData>>,
            > = self
                .bundle_manifests
                .lock()
                .recover_poisoned(self.logger, "reload");
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
                self.logger.log(LogLevel::Warn, "reload", || {
                    format!(
                        "cascade reload of dependent bundle '{}' failed after '{}' reloaded: {}",
                        dep_name,
                        reloaded_bundle_id.id(),
                        e
                    )
                });
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

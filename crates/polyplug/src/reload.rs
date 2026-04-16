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

use std::sync::Arc;

use polyplug_abi::guest::GuestContractInterface;
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
    pub fn reload_bundle(&self, path: &std::path::Path) -> Result<(), RuntimeError> {
        if !self.config().hot_reload_enabled {
            return Err(RuntimeError::HotReloadDisabled);
        }

        let bundle_dir: &std::path::Path = if path.is_file() {
            path.parent().unwrap_or(path)
        } else {
            path
        };

        let manifest: ManifestData =
            crate::loader::parse_manifest(bundle_dir).map_err(RuntimeError::Loader)?;

        // Find the loader
        let loader: &dyn crate::loader::BundleLoader = self
            .loaders
            .get(&manifest.runtime)
            .map(Box::as_ref)
            .ok_or_else(|| {
                RuntimeError::Loader(crate::error::LoaderError::NoLoaderForRuntime {
                    bundle: path.display().to_string(),
                    runtime_name: manifest.runtime.clone(),
                })
            })?;

        let bundle_id: BundleId = BundleId::new(&manifest.name);

        // Store slot indices before reload (for warning check and interface swap)
        let slot_indices: Vec<u32> = self.registry.get_bundle_plugin_slots(bundle_id);

        // Fire Preparing callback
        if let Some(cb) = self.on_reload_cb() {
            cb(ReloadPhase::preparing(
                bundle_id,
                string_view(&manifest.name),
            ));
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

        // Call loader's reload() - this does load+init, registering new interfaces
        let result: Result<(), crate::error::RuntimeError> = loader.reload(&manifest, self);

        match result {
            Ok(()) => {
                // ─── Swap interfaces after init succeeds (HR-05) ──────────────
                // New interfaces were registered during init inside loader.reload()
                // For each slot, find the NEW interface by contract_id and swap
                for slot_idx in &slot_indices {
                    // Get contract_id for this slot (stable across reload)
                    let contract_id: GuestContractId =
                        self.registry.get_slot_guest_contract_id(*slot_idx).ok_or({
                            RuntimeError::Registry(crate::error::RegistryError::InvalidHandle {
                                index: *slot_idx,
                            })
                        })?;

                    // Find NEW interface handle (registered during init)
                    let new_handle: polyplug_abi::plugin::GuestContractHandle =
                        self.registry.find_guest_contract(contract_id, 0)?;

                    // Get Arc to NEW interface
                    let new_interface: Arc<GuestContractInterface> = self
                        .registry
                        .get_guest_contract_interface_arc(new_handle.index)
                        .ok_or({
                            RuntimeError::Registry(crate::error::RegistryError::InvalidHandle {
                                index: new_handle.index,
                            })
                        })?;

                    // Atomic swap - old slot now points to new interface
                    self.registry
                        .swap_guest_contract_interface(*slot_idx, new_interface)?;
                }

                // Fire Reloaded callback
                if let Some(cb) = self.on_reload_cb() {
                    cb(ReloadPhase::reloaded(
                        bundle_id,
                        string_view(&manifest.name),
                    ));
                }
                Ok(())
            }
            Err(e) => {
                // Fire Failed callback - NO interface swap on failure
                if let Some(cb) = self.on_reload_cb() {
                    cb(ReloadPhase::failed(
                        bundle_id,
                        string_view(&manifest.name),
                        string_view(&e.to_string()),
                    ));
                }
                Err(e)
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
        let cloned = original.clone();

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

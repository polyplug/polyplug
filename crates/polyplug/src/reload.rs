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

use crate::error::{RuntimeError};
use crate::loader::ManifestData;
use crate::runtime::Runtime;

const QUIESCENCE_TIMEOUT: Duration = Duration::from_secs(5);

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
    let slot_indices: Vec<u32> = registry.find_slots_by_bundle(bundle_id);

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
                bundle: format!("bundle_id={}", bundle_id.id()),
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

        let manifest: ManifestData =
            crate::loader::parse_manifest(bundle_dir).map_err(|e| RuntimeError::Loader(e))?;

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
            ReloadPhase::Preparing {
                bundle_id: id,
                bundle_name,
                retry_count,
            } => {
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
            ReloadPhase::Reloaded {
                bundle_id: id,
                bundle_name,
            } => {
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
            ReloadPhase::Failed {
                bundle_id: id,
                bundle_name,
                reason,
            } => {
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
                ReloadPhase::Preparing {
                    bundle_id: id1,
                    retry_count: c1,
                    ..
                },
                ReloadPhase::Preparing {
                    bundle_id: id2,
                    retry_count: c2,
                    ..
                },
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

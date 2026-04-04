//! Reload — callback-based hot-reload framework for all loaders.
//!
//! Provides:
//! - `ReloadPhase` enum for notification callbacks
//! - Callback-based notification for host to destroy instances
//!
//! Each loader implements its own `reload()` method using these notifications.

use polyplug_utils::{BundleId, GuestContractId};

use crate::error::{RuntimeError};
use crate::loader::ManifestData;
use crate::runtime::Runtime;

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
        if let Some(cb) = self.on_reload_cb() {
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
                if let Some(cb) = self.on_reload_cb() {
                    cb(ReloadPhase::Reloaded {
                        bundle_id,
                        bundle_name: manifest.name.clone(),
                    });
                }
                Ok(())
            }
            Err(e) => {
                // Fire Failed callback
                if let Some(cb) = self.on_reload_cb() {
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
        self.registry().find_by_contract(GuestContractId::from_u64(contract_id), min_version)
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;

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

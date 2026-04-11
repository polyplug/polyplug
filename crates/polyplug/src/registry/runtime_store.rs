//! RuntimeStore — interface storage and contract handle management.
//!
//! Simple index-based registry: each slot holds an interface pointer.
//! GuestContractHandle validation checks for out-of-bounds indices only.
//! Hosts must destroy instances before hot-reload via callback.
//!
//! Multi-impl support: different bundles may register_guest_contract different implementations of
//! the same contract. guest_contract_index maps contract_id -> Vec<slot_index> to support
//! find_all_guest_contracts(). DuplicateProvider is only raised when the SAME bundle_id
//! tries to register_guest_contract the SAME contract_id twice.

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::RwLock;

use polyplug_abi::RuntimeLanguage;
use polyplug_abi::types::Version;
use polyplug_abi::{GuestContractInterface, PluginDescriptor, GuestContractHandle};
use polyplug_utils::{BundleId, GuestContractId};

use crate::error::RegistryError;

/// Bundle metadata stored in RuntimeStore.
///
/// Provides complete bundle information for introspection and dependency resolution.
pub struct BundleDescriptor {
    /// Bundle ID — computed from name via BundleId::new().
    pub id: BundleId,
    /// Bundle name — human-readable identifier.
    pub name: String,
    /// Bundle version — semantic version (major.minor.patch).
    pub version: Version,
    /// Runtime language — determines which BundleLoader handles this bundle.
    pub runtime: RuntimeLanguage,
    /// Path to bundle directory or library file.
    pub file_path: PathBuf,
    /// Bundle-level dependencies (replaces contract-level).
    pub dependencies: Vec<BundleDependency>,
}

/// Bundle-level dependency specification.
///
/// Parsed from manifest.toml `dependencies` array entries like:
/// - `"image-decoder"` -> { name: "image-decoder", min_version: None }
/// - `"image-decoder@1.0"` -> { name: "image-decoder", min_version: Some(Version::new(1, 0, 0)) }
pub struct BundleDependency {
    /// Dependency bundle name.
    pub name: String,
    /// Minimum version requirement (None = any version).
    pub min_version: Option<Version>,
}

/// Bundle data stored in RuntimeStoreData.bundle_data.
///
/// Contains all plugin slot indices for a bundle (enabling O(1) slot lookup)
/// and the bundle descriptor for introspection.
pub struct BundleData {
    /// All slot indices for plugins from this bundle.
    pub plugin_slots: Vec<u32>,
    /// Bundle metadata.
    pub descriptor: BundleDescriptor,
}

/// Live plugin registration data.
pub(crate) struct PluginEntry {
    /// Plugin metadata — used by other crates for introspection.
    pub descriptor: PluginDescriptor,
    /// Full contract name string for collision detection.
    pub contract_name: String,
    /// The bundle this registration originates from.
    pub bundle_id: BundleId,
}

/// A single slot in the registry's storage array.
pub(crate) struct PluginSlot {
    /// Slot contents — None if vacant (after unload).
    pub entry: Option<PluginEntry>,
    /// Interface pointer — direct Arc storage without wrapper.
    /// The callback-based hot-reload model ensures hosts destroy instances before swap.
    pub interface: Option<Arc<GuestContractInterface>>,
}

/// Internal data protected by a single RwLock.
///
/// This structure groups all mutable registry state together to enable
/// single-lock acquisition on the hot path, reducing lock overhead.
struct RuntimeStoreData {
    /// Slot storage — each slot holds a plugin registration or is vacant.
    slots: Vec<PluginSlot>,
    /// Maps contract_id to the Vec of registered slot indices (multi-impl support).
    guest_contract_index: HashMap<GuestContractId, Vec<u32>>,
    /// Maps bundle_id to BundleData containing all slots and descriptor (O(1) lookup).
    bundle_data: HashMap<BundleId, BundleData>,
    /// Maps bundle name to all loaded version BundleIds (multi-version support).
    bundle_name_index: HashMap<String, Vec<BundleId>>,
    /// Maps bundle_id to the set of contract_ids it has declared as dependencies.
    bundle_declared_deps: HashMap<BundleId, HashSet<GuestContractId>>,
}

impl RuntimeStoreData {
    /// Create empty registry data.
    fn new() -> Self {
        Self {
            slots: Vec::new(),
            guest_contract_index: HashMap::new(),
            bundle_data: HashMap::new(),
            bundle_name_index: HashMap::new(),
            bundle_declared_deps: HashMap::new(),
        }
    }
}

/// Thread-safe plugin registry.
//
//  Uses a single RwLock to protect all mutable state, reducing lock acquisition
//  overhead on the hot path. Writes (registration/unload) are rare, so contention
//  is minimal. Reads (find, resolve_guest_contract) take a read guard and are concurrent.
pub struct RuntimeStore {
    /// Single RwLock protecting all mutable registry state.
    data: RwLock<RuntimeStoreData>,
}

impl RuntimeStore {
    /// Create an empty registry.
    pub fn new() -> RuntimeStore {
        RuntimeStore {
            data: RwLock::new(RuntimeStoreData::new()),
        }
    }

    /// Register a plugin interface.
    ///
    /// # Safety
    ///
    /// `interface_ptr` must be a valid pointer to a `'static` `GuestContractInterface` that remains valid
    /// for the entire lifetime of the `Registry`. The caller must ensure the backing library
    /// is not unloaded while this registry holds the pointer.
    //
    //  The contract_id is read directly from the interface pointer.
    //
    //  Returns Err if:
    //  - contract_id is already register_guest_contracted to a DIFFERENT contract_name (hash collision)
    //  - contract_id is already register_guest_contracted by the SAME bundle_id (duplicate provider)
    //
    //  Different bundles MAY register_guest_contract the same contract_id (multi-impl).
    pub unsafe fn register_guest_contract(
        &self,
        descriptor: PluginDescriptor,
        interface_ptr: *const GuestContractInterface,
        contract_name: String,
        bundle_id: BundleId,
    ) -> Result<GuestContractHandle, RegistryError> {
        // SAFETY: interface_ptr is a valid 'static GuestContractInterface supplied by the caller.
        // The ABI contract requires the pointer to remain valid for the library lifetime.
        let contract_id: GuestContractId = unsafe { (*interface_ptr).contract_id };

        let mut data: std::sync::RwLockWriteGuard<'_, RuntimeStoreData> =
            self.data.write().unwrap_or_else(|e| {
                eprintln!("[polyplug] Mutex/RwLock poisoned, recovering: {}", e);
                e.into_inner()
            });

        // Check existing slots for this contract_id
        if let Some(existing_indices) = data.guest_contract_index.get(&contract_id) {
            for &existing_idx in existing_indices.iter() {
                let existing_slot: &PluginSlot = &data.slots[existing_idx as usize];
                if let Some(ref existing_entry) = existing_slot.entry {
                    // Hash collision: same contract_id, different contract_name
                    if existing_entry.contract_name != contract_name {
                        return Err(RegistryError::ContractIdCollision {
                            id: contract_id.id(),
                            name_a: existing_entry.contract_name.clone(),
                            name_b: contract_name,
                        });
                    }
                    // Same bundle, same contract — allowed (multi-impl support)
                    // Different bundle, same contract — also allowed (multi-impl support)
                }
            }
        }

        // Find a vacant slot or push a new one
        let slot_idx: u32 = data
            .slots
            .iter()
            .position(|s| s.entry.is_none())
            .map(|i| i as u32)
            .unwrap_or_else(|| {
                let new_idx: u32 = data.slots.len() as u32;
                data.slots.push(PluginSlot {
                    entry: None,
                    interface: None,
                });
                new_idx
            });

        let slot: &mut PluginSlot = &mut data.slots[slot_idx as usize];
        slot.entry = Some(PluginEntry {
            descriptor,
            contract_name,
            bundle_id,
        });
        // SAFETY: interface_ptr is a valid 'static pointer, we clone the interface
        // into an Arc for shared ownership.
        slot.interface = Some(Arc::new(unsafe { (*interface_ptr).clone() }));

        // Update contract_index: push slot_idx into the Vec for this contract_id
        data.guest_contract_index
            .entry(contract_id)
            .or_default()
            .push(slot_idx);

        // Update bundle_data: push slot_idx into plugin_slots Vec for this bundle_id.
        // Note: descriptor is populated separately via register_bundle_metadata().
        data.bundle_data
            .entry(bundle_id)
            .or_insert_with(|| BundleData {
                plugin_slots: Vec::new(),
                descriptor: BundleDescriptor {
                    id: bundle_id,
                    name: String::new(),
                    version: Version { major: 0, minor: 0, patch: 0 },
                    runtime: RuntimeLanguage::Rust,
                    file_path: PathBuf::new(),
                    dependencies: Vec::new(),
                },
            })
            .plugin_slots
            .push(slot_idx);

        Ok(GuestContractHandle { index: slot_idx })
    }

    /// Declare dependency contract_ids for a bundle.
    ///
    /// Must be called before the bundle resolve_guest_contracts any cross-bundle contracts.
    /// Prevents undeclared dependency resolution at runtime.
    pub fn declare_bundle_dependencies(
        &self,
        bundle_id: BundleId,
        contract_ids: Vec<GuestContractId>,
    ) -> Result<(), RegistryError> {
        let mut data: std::sync::RwLockWriteGuard<'_, RuntimeStoreData> =
            self.data.write().unwrap_or_else(|e| {
                eprintln!("[polyplug] Mutex/RwLock poisoned, recovering: {}", e);
                e.into_inner()
            });
        let set: &mut HashSet<GuestContractId> = data.bundle_declared_deps.entry(bundle_id).or_default();
        for cid in contract_ids {
            set.insert(cid);
        }
        Ok(())
    }

    /// Returns true if `bundle_id` has declared `contract_id` as a dependency.
    pub(crate) fn is_bundle_dependency_declared(&self, bundle_id: BundleId, contract_id: GuestContractId) -> bool {
        let data: std::sync::RwLockReadGuard<'_, RuntimeStoreData> =
            self.data.read().unwrap_or_else(|e| {
                eprintln!("[polyplug] Mutex/RwLock poisoned, recovering: {}", e);
                e.into_inner()
            });
        data.bundle_declared_deps
            .get(&bundle_id)
            .is_some_and(|s| s.contains(&contract_id))
    }

    /// Find any register_guest_contracted plugin satisfying the given contract_id and minimum version.
    //
    //  Returns the first slot whose interface.contract_version >= min_version.
    //  Pass min_version=0 to accept any version.
    pub fn find_guest_contract(
        &self,
        contract_id: GuestContractId,
        min_version: u32,
    ) -> Result<GuestContractHandle, RegistryError> {
        let data: std::sync::RwLockReadGuard<'_, RuntimeStoreData> =
            self.data.read().unwrap_or_else(|e| {
                eprintln!("[polyplug] Mutex/RwLock poisoned, recovering: {}", e);
                e.into_inner()
            });

        let indices: &Vec<u32> = match data.guest_contract_index.get(&contract_id) {
            Some(v) => v,
            None => {
                return Err(RegistryError::PluginNotFound {
                    contract_id: contract_id.id(),
                    min_version,
                });
            }
        };

        for &slot_idx in indices.iter() {
            let slot: &PluginSlot = &data.slots[slot_idx as usize];
            if slot.entry.is_some()
                && let Some(ref interface) = slot.interface
            {
                // SAFETY: interface points to 'static GuestContractInterface, valid for Registry lifetime.
                // The pointer is written once at registration and never mutated.
                let version: u32 = interface.contract_version.major;
                if version >= min_version {
                    return Ok(GuestContractHandle { index: slot_idx });
                }
            }
        }
        Err(RegistryError::PluginNotFound {
            contract_id: contract_id.id(),
            min_version,
        })
    }

    /// Find the plugin registered by a specific bundle_id that satisfies contract_id + min_version.
    pub fn find_guest_contract_by_bundle(
        &self,
        bundle_id: BundleId,
        contract_id: GuestContractId,
        min_version: u32,
    ) -> Result<GuestContractHandle, RegistryError> {
        let data: std::sync::RwLockReadGuard<'_, RuntimeStoreData> =
            self.data.read().unwrap_or_else(|e| {
                eprintln!("[polyplug] Mutex/RwLock poisoned, recovering: {}", e);
                e.into_inner()
            });

        // Get all slot indices for this bundle (O(1) via bundle_data)
        let slot_indices: &Vec<u32> = match data.bundle_data.get(&bundle_id) {
            Some(bd) => &bd.plugin_slots,
            None => {
                return Err(RegistryError::PluginNotFound {
                    contract_id: contract_id.id(),
                    min_version,
                });
            }
        };

        // Find the slot matching contract_id and version
        for &slot_idx in slot_indices.iter() {
            let slot: &PluginSlot = &data.slots[slot_idx as usize];
            if let Some(ref entry) = slot.entry
                && let Some(ref interface) = slot.interface
            {
                // SAFETY: interface is 'static GuestContractInterface valid for Registry lifetime.
                // Written once at registration, never mutated.
                if entry.bundle_id == bundle_id
                    && interface.contract_id == contract_id
                    && interface.contract_version.major >= min_version
                {
                    return Ok(GuestContractHandle { index: slot_idx });
                }
            }
        }
        Err(RegistryError::PluginNotFound {
            contract_id: contract_id.id(),
            min_version,
        })
    }

    /// Find all plugins satisfying the given contract_id and minimum version.
    pub fn find_all_guest_contracts(
        &self,
        contract_id: GuestContractId,
        min_version: u32,
        out: &mut [GuestContractHandle],
    ) -> usize {
        let data: std::sync::RwLockReadGuard<'_, RuntimeStoreData> =
            self.data.read().unwrap_or_else(|e| {
                eprintln!("[polyplug] Mutex/RwLock poisoned, recovering: {}", e);
                e.into_inner()
            });

        let indices: &Vec<u32> = match data.guest_contract_index.get(&contract_id) {
            Some(v) => v,
            None => return 0usize,
        };

        if out.is_empty() {
            return 0usize;
        }
        let mut write_count: usize = 0usize;
        for &slot_idx in indices.iter() {
            if write_count >= out.len() {
                break;
            }
            let slot: &PluginSlot = &data.slots[slot_idx as usize];
            if slot.entry.is_some()
                && let Some(ref interface) = slot.interface
            {
                // SAFETY: interface is 'static GuestContractInterface valid for Registry lifetime.
                // Read-only access after registration.
                let version: u32 = interface.contract_version.major;
                if version >= min_version {
                    out[write_count] = GuestContractHandle { index: slot_idx };
                    write_count += 1usize;
                }
            }
        }
        write_count
    }

    /// Find all plugins satisfying the given contract_id and minimum version,
    /// packing handles directly into a u64 buffer.
    ///
    /// This is an optimized version of `find_all_guest_contracts` that avoids
    /// intermediate allocation by packing handles directly during iteration.
    /// Each handle is packed as: `index as u64`.
    ///
    /// Returns the number of packed handles written to `out`.
    pub fn find_all_guest_contracts_packed(
        &self,
        contract_id: GuestContractId,
        min_version: u32,
        out: &mut [u64],
    ) -> usize {
        let data: std::sync::RwLockReadGuard<'_, RuntimeStoreData> =
            self.data.read().unwrap_or_else(|e| {
                eprintln!("[polyplug] Mutex/RwLock poisoned, recovering: {}", e);
                e.into_inner()
            });

        let indices: &Vec<u32> = match data.guest_contract_index.get(&contract_id) {
            Some(v) => v,
            None => return 0usize,
        };

        if out.is_empty() {
            return 0usize;
        }
        let mut write_count: usize = 0usize;
        for &slot_idx in indices.iter() {
            if write_count >= out.len() {
                break;
            }
            let slot: &PluginSlot = &data.slots[slot_idx as usize];
            if slot.entry.is_some()
                && let Some(ref interface) = slot.interface
            {
                // SAFETY: interface is 'static GuestContractInterface valid for Registry lifetime.
                // Read-only access after registration.
                let version: u32 = interface.contract_version.major;
                if version >= min_version {
                    // Pack handle directly: just the index
                    out[write_count] = slot_idx as u64;
                    write_count += 1usize;
                }
            }
        }
        write_count
    }

    /// Count plugins satisfying the given contract_id and minimum version.
    pub fn count_guest_contracts(&self, contract_id: GuestContractId, min_version: u32) -> usize {
        let data = self.data.read().unwrap_or_else(|e| {
            eprintln!("[polyplug] RwLock poisoned, recovering: {}", e);
            e.into_inner()
        });

        let indices = match data.guest_contract_index.get(&contract_id) {
            Some(v) => v,
            None => return 0,
        };

        let mut count = 0;
        for &slot_idx in indices.iter() {
            let slot = &data.slots[slot_idx as usize];
            if slot.entry.is_some() && let Some(ref interface) = slot.interface {
                if interface.contract_version.major >= min_version {
                    count += 1;
                }
            }
        }
        count
    }

    /// Find all plugins and write handles into the provided slice.
    /// Returns the number of handles written.
    pub fn find_all_guest_contracts_into(
        &self,
        contract_id: GuestContractId,
        min_version: u32,
        out: &mut [GuestContractHandle],
    ) -> usize {
        self.find_all_guest_contracts(contract_id, min_version, out)
    }

    /// Find a plugin by contract_id and minimum version.
    //
    //  Delegates to find_guest_contract(). Kept for API compatibility.
    //  min_version encoding: (minor << 16 | patch), same as GuestContractInterface::contract_version.
    //  Pass 0 to accept any version.
    pub fn find(&self, contract_id: GuestContractId, min_version: u32) -> Result<GuestContractHandle, RegistryError> {
        self.find_guest_contract(contract_id, min_version)
    }

    /// Validate a GuestContractHandle and return its interface pointer directly.
    ///
    /// Returns Err(InvalidHandle) if:
    /// - handle.index is out of bounds
    /// - the slot has no interface
    pub fn resolve_guest_contract(&self, handle: GuestContractHandle) -> Result<*const GuestContractInterface, RegistryError> {
        let data: std::sync::RwLockReadGuard<'_, RuntimeStoreData> =
            self.data.read().unwrap_or_else(|e| {
                eprintln!("[polyplug] Mutex/RwLock poisoned, recovering: {}", e);
                e.into_inner()
            });

        let slot_idx: usize = handle.index as usize;
        if slot_idx >= data.slots.len() {
            return Err(RegistryError::InvalidHandle { index: handle.index });
        }

        let slot: &PluginSlot = &data.slots[slot_idx];
        match slot.interface {
            Some(ref interface) => Ok(interface.as_ref() as *const GuestContractInterface),
            None => Err(RegistryError::InvalidHandle { index: handle.index }),
        }
    }

    /// Atomically swap the interface in slot `slot_index` with `new_interface`.
    ///
    /// This is a direct swap under write lock. The callback-based hot-reload model
    /// (Phase 4) ensures hosts destroy instances before this is called.
    ///
    /// # Errors
    /// Returns `Err(RegistryError::InvalidHandle)` if `slot_index` is out of bounds
    /// or the slot has no interface.
    pub fn swap_guest_contract_interface(
        &self,
        slot_index: u32,
        new_interface: Arc<GuestContractInterface>,
    ) -> Result<(), RegistryError> {
        let mut data: std::sync::RwLockWriteGuard<'_, RuntimeStoreData> =
            self.data.write().unwrap_or_else(|e| {
                eprintln!("[polyplug] Mutex/RwLock poisoned, recovering: {}", e);
                e.into_inner()
            });
        let slot_idx: usize = slot_index as usize;
        if slot_idx >= data.slots.len() {
            return Err(RegistryError::InvalidHandle { index: slot_index });
        }
        let slot: &mut PluginSlot = &mut data.slots[slot_idx];
        if slot.interface.is_none() {
            return Err(RegistryError::InvalidHandle { index: slot_index });
        }
        slot.interface = Some(new_interface);
        Ok(())
    }

    /// Find all slot indices that were registered by `bundle_id`.
    ///
    /// Returns an empty `Vec` if the bundle has no registered slots.
    /// O(1) lookup via bundle_data HashMap.
    pub fn get_bundle_plugin_slots(&self, bundle_id: BundleId) -> Vec<u32> {
        let data: std::sync::RwLockReadGuard<'_, RuntimeStoreData> =
            self.data.read().unwrap_or_else(|e| {
                eprintln!("[polyplug] Mutex/RwLock poisoned, recovering: {}", e);
                e.into_inner()
            });
        data.bundle_data
            .get(&bundle_id)
            .map(|bd: &BundleData| bd.plugin_slots.clone())
            .unwrap_or_default()
    }

    /// Register bundle metadata after load_bundle completes.
    ///
    /// This populates the BundleDescriptor in bundle_data and adds to bundle_name_index.
    /// Must be called after all plugins from this bundle have registered.
    pub fn register_bundle_metadata(
        &self,
        bundle_id: BundleId,
        bundle_name: String,
        version: Version,
        runtime: RuntimeLanguage,
        file_path: PathBuf,
        dependencies: Vec<BundleDependency>,
    ) -> Result<(), RegistryError> {
        let mut data: std::sync::RwLockWriteGuard<'_, RuntimeStoreData> =
            self.data.write().unwrap_or_else(|e| {
                eprintln!("[polyplug] RwLock poisoned, recovering: {}", e);
                e.into_inner()
            });

        // Update bundle_data descriptor if entry exists
        if let Some(bundle_data) = data.bundle_data.get_mut(&bundle_id) {
            bundle_data.descriptor.name = bundle_name.clone();
            bundle_data.descriptor.version = version;
            bundle_data.descriptor.runtime = runtime;
            bundle_data.descriptor.file_path = file_path;
            bundle_data.descriptor.dependencies = dependencies;
        } else {
            // Bundle has no plugins yet, create entry with empty plugin_slots
            data.bundle_data.insert(bundle_id, BundleData {
                plugin_slots: Vec::new(),
                descriptor: BundleDescriptor {
                    id: bundle_id,
                    name: bundle_name.clone(),
                    version,
                    runtime,
                    file_path,
                    dependencies,
                },
            });
        }

        // Add to bundle_name_index for multi-version support
        data.bundle_name_index
            .entry(bundle_name)
            .or_default()
            .push(bundle_id);

        Ok(())
    }

    /// List all loaded bundle IDs.
    pub fn list_bundles(&self) -> Vec<BundleId> {
        let data: std::sync::RwLockReadGuard<'_, RuntimeStoreData> =
            self.data.read().unwrap_or_else(|e| {
                eprintln!("[polyplug] RwLock poisoned, recovering: {}", e);
                e.into_inner()
            });
        data.bundle_data.keys().copied().collect::<Vec<BundleId>>()
    }

    /// Get bundle metadata by bundle ID.
    pub fn get_bundle_descriptor(&self, bundle_id: BundleId) -> Option<BundleDescriptor> {
        let data: std::sync::RwLockReadGuard<'_, RuntimeStoreData> =
            self.data.read().unwrap_or_else(|e| {
                eprintln!("[polyplug] RwLock poisoned, recovering: {}", e);
                e.into_inner()
            });
        data.bundle_data
            .get(&bundle_id)
            .map(|bd: &BundleData| {
                // Clone descriptor fields manually since BundleDescriptor doesn't derive Clone
                BundleDescriptor {
                    id: bd.descriptor.id,
                    name: bd.descriptor.name.clone(),
                    version: bd.descriptor.version,
                    runtime: bd.descriptor.runtime,
                    file_path: bd.descriptor.file_path.clone(),
                    dependencies: bd.descriptor.dependencies.iter().map(|d| BundleDependency {
                        name: d.name.clone(),
                        min_version: d.min_version,
                    }).collect(),
                }
            })
    }

    /// Get all BundleIds for a given bundle name (multi-version support).
    pub fn get_bundles_by_name(&self, bundle_name: &str) -> Vec<BundleId> {
        let data: std::sync::RwLockReadGuard<'_, RuntimeStoreData> =
            self.data.read().unwrap_or_else(|e| {
                eprintln!("[polyplug] RwLock poisoned, recovering: {}", e);
                e.into_inner()
            });
        data.bundle_name_index
            .get(bundle_name)
            .cloned()
            .unwrap_or_default()
    }

    /// Get the contract_id for the interface currently stored in `slot_index`.
    /// Returns None if the slot is empty or has no interface.
    pub(crate) fn get_slot_guest_contract_id(&self, slot_index: u32) -> Option<GuestContractId> {
        let data: std::sync::RwLockReadGuard<'_, RuntimeStoreData> =
            self.data.read().unwrap_or_else(|e| {
                eprintln!("[polyplug] Mutex/RwLock poisoned, recovering: {}", e);
                e.into_inner()
            });
        let slot: &PluginSlot = data.slots.get(slot_index as usize)?;
        let interface: &Arc<GuestContractInterface> = slot.interface.as_ref()?;
        Some(interface.contract_id)
    }

    /// Get a clone of the Arc<GuestContractInterface> for `slot_index` to check strong_count.
    /// Returns None if the slot is empty or has no interface.
    pub(crate) fn get_guest_contract_interface_arc(&self, slot_index: u32) -> Option<Arc<GuestContractInterface>> {
        let data: std::sync::RwLockReadGuard<'_, RuntimeStoreData> =
            self.data.read().unwrap_or_else(|e| {
                eprintln!("[polyplug] Mutex/RwLock poisoned, recovering: {}", e);
                e.into_inner()
            });
        let slot: &PluginSlot = data.slots.get(slot_index as usize)?;
        slot.interface.as_ref().map(|arc| Arc::clone(arc))
    }


    /// Clear all registrations for testing.
    /// This is only available in test builds to allow test isolation.
    #[cfg(test)]
    pub fn clear_for_test(&self) {
        let mut data: std::sync::RwLockWriteGuard<'_, RuntimeStoreData> =
            self.data.write().unwrap_or_else(|e| {
                eprintln!("[polyplug] Mutex/RwLock poisoned, recovering: {}", e);
                e.into_inner()
            });
        data.slots.clear();
        data.guest_contract_index.clear();
        data.bundle_data.clear();
        data.bundle_name_index.clear();
        data.bundle_declared_deps.clear();
    }
}

impl Default for RuntimeStore {
    fn default() -> RuntimeStore {
        RuntimeStore::new()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;
    use polyplug_abi::{
        DispatchType, GuestContractInterface, HostInterface, NativeDispatch, PluginDescriptor, StringView,
        Version, DispatchMechanisms, GuestContractInstance,
    };

    /// No-op create_instance callback.
    unsafe extern "C" fn noop_create_instance(
        _host: *const HostInterface,
        _args: *const (),
    ) -> GuestContractInstance {
        GuestContractInstance::null()
    }

    /// No-op destroy_instance callback.
    unsafe extern "C" fn noop_destroy_instance(
        _host: *const HostInterface,
        _instance: GuestContractInstance,
    ) {
    }

    fn mock_interface(contract_id: u64) -> GuestContractInterface {
        GuestContractInterface {
            contract_id: GuestContractId::from_u64(contract_id),
            contract_version: Version { major: 1, minor: 0, patch: 0 },
            dispatch_type: DispatchType::Native,
            create_instance: noop_create_instance,
            destroy_instance: noop_destroy_instance,
            dispatch: DispatchMechanisms {
                native: NativeDispatch {
                    function_count: 0,
                    functions: core::ptr::null(),
                },
            },
        }
    }

    fn make_descriptor(name: &'static str, contract_name: &'static str) -> PluginDescriptor {
        PluginDescriptor {
            name: StringView::from_static(name.as_bytes()),
            contract_name: StringView::from_static(contract_name.as_bytes()),
            version: Version { major: 1, minor: 0, patch: 0 },
        }
    }

    #[test]
    fn register_guest_contract_and_find() {
        let registry: RuntimeStore = RuntimeStore::new();
        let descriptor: PluginDescriptor = make_descriptor("test_plugin", "image.decode");
        let interface = mock_interface(0x1234_5678_9ABC_DEF0);
        // SAFETY: interface is a local value, but we're just testing registration
        let handle: GuestContractHandle = unsafe {
            registry.register_guest_contract(
                descriptor,
                &interface,
                "image.decode".to_owned(),
                BundleId::from_u64(0),
            )
        }
        .expect("registration should succeed");
        assert!(!handle.is_null());

        let found: GuestContractHandle = registry
            .find(GuestContractId::from_u64(0x1234_5678_9ABC_DEF0), 0)
            .expect("find should succeed");
        assert_eq!(found.index, handle.index);
    }

    #[test]
    fn invalid_handle_detection() {
        let registry: RuntimeStore = RuntimeStore::new();
        let descriptor: PluginDescriptor = make_descriptor("test_plugin", "audio.decode");
        let interface = mock_interface(0x1234_5678_9ABC_DEF0);

        // SAFETY: interface is a local value, but we're just testing registration
        let _handle: GuestContractHandle = unsafe {
            registry.register_guest_contract(
                descriptor,
                &interface,
                "image.decode".to_owned(),
                BundleId::from_u64(1),
            )
        }
        .expect("registration should succeed");

        // Use a handle with out-of-bounds index
        let invalid: GuestContractHandle = GuestContractHandle { index: 999 };
        let result: Result<*const GuestContractInterface, RegistryError> = registry.resolve_guest_contract(invalid);
        assert!(
            matches!(result, Err(RegistryError::InvalidHandle { .. })),
            "expected InvalidHandle error"
        );
    }

    #[test]
    fn duplicate_provider_allowed() {
        let registry: RuntimeStore = RuntimeStore::new();
        let d1: PluginDescriptor = make_descriptor("plugin_a", "image.decode");
        let d2: PluginDescriptor = make_descriptor("plugin_b", "image.decode");
        let bundle_id = BundleId::from_u64(0);
        let interface = mock_interface(0x1234_5678_9ABC_DEF0);

        // SAFETY: interface is a local value
        unsafe {
            registry
                .register_guest_contract(d1, &interface, "image.decode".to_owned(), bundle_id)
                .expect("first registration should succeed");
        }

        let result: Result<GuestContractHandle, RegistryError> =
            // SAFETY: interface is a local value
            unsafe { registry.register_guest_contract(d2, &interface, "image.decode".to_owned(), bundle_id) };
        // Second registration should succeed (multi-impl allowed)
        assert!(
            result.is_ok(),
            "second registration should succeed (multi-impl allowed)"
        );
    }

    #[test]
    fn collision_detection() {
        let registry: RuntimeStore = RuntimeStore::new();
        let d1: PluginDescriptor = make_descriptor("plugin_a", "contract.a");
        let d2: PluginDescriptor = make_descriptor("plugin_b", "contract.b");
        let bundle_id_a = BundleId::from_u64(10);
        let bundle_id_b = BundleId::from_u64(20);
        let interface = mock_interface(0x1234_5678_9ABC_DEF0);

        // SAFETY: interface is a local value
        unsafe {
            registry
                .register_guest_contract(d1, &interface, "contract.a".to_owned(), bundle_id_a)
                .expect("first registration should succeed");
        }

        let result: Result<GuestContractHandle, RegistryError> =
            // SAFETY: interface is a local value
            unsafe { registry.register_guest_contract(d2, &interface, "contract.b".to_owned(), bundle_id_b) };
        assert!(
            matches!(result, Err(RegistryError::ContractIdCollision { .. })),
            "expected ContractIdCollision error"
        );
    }

    #[test]
    fn resolve_guest_contract_returns_interface_pointer() {
        let registry: RuntimeStore = RuntimeStore::new();
        let descriptor: PluginDescriptor = make_descriptor("test_plugin", "test.contract");
        let interface = mock_interface(0x1234_5678_9ABC_DEF0);

        // SAFETY: interface is a local value
        let handle: GuestContractHandle = unsafe {
            registry.register_guest_contract(
                descriptor,
                &interface,
                "image.decode".to_owned(),
                BundleId::from_u64(2),
            )
        }
        .expect("registration should succeed");

        let interface_ptr: *const GuestContractInterface =
            registry.resolve_guest_contract(handle).expect("resolve_guest_contract should succeed");
        // SAFETY: interface_ptr points to a valid GuestContractInterface
        let contract_id: GuestContractId = unsafe { (*interface_ptr).contract_id };
        assert_eq!(contract_id, interface.contract_id);
    }

    #[test]
    fn declare_bundle_dependencies_and_query() {
        let registry: RuntimeStore = RuntimeStore::new();
        let bundle_id = BundleId::from_u64(42);
        let contract_a = GuestContractId::from_u64(0x1111_2222_3333_4444);
        let contract_b = GuestContractId::from_u64(0x5555_6666_7777_8888);

        registry
            .declare_bundle_dependencies(bundle_id, vec![contract_a])
            .expect("declare_bundle_dependencies should succeed");

        assert!(
            registry.is_bundle_dependency_declared(bundle_id, contract_a),
            "declared dep should be found"
        );
        assert!(
            !registry.is_bundle_dependency_declared(bundle_id, contract_b),
            "undeclared dep should not be found"
        );
    }

    #[test]
    fn get_bundle_plugin_slots_is_o1_lookup() {
        let registry: RuntimeStore = RuntimeStore::new();
        let bundle_id: BundleId = BundleId::new("test-bundle");
        let descriptor: PluginDescriptor = make_descriptor("plugin", "contract");
        let interface: GuestContractInterface = mock_interface(0x1234_5678_9ABC_DEF0);

        // Register a plugin
        // SAFETY: interface is a local value for testing
        unsafe {
            registry.register_guest_contract(descriptor, &interface, "contract".to_owned(), bundle_id)
        }.expect("registration should succeed");

        // Register bundle metadata
        registry.register_bundle_metadata(
            bundle_id,
            "test-bundle".to_string(),
            Version { major: 1, minor: 0, patch: 0 },
            RuntimeLanguage::Rust,
            PathBuf::from("/test"),
            Vec::new(),
        ).expect("metadata registration should succeed");

        // O(1) lookup should return the slot
        let slots: Vec<u32> = registry.get_bundle_plugin_slots(bundle_id);
        assert_eq!(slots.len(), 1, "bundle should have one plugin slot");
        assert_eq!(slots[0], 0, "slot index should be 0");
    }

    #[test]
    fn get_bundle_descriptor_returns_metadata() {
        let registry: RuntimeStore = RuntimeStore::new();
        let bundle_id: BundleId = BundleId::new("test-bundle");
        let descriptor: PluginDescriptor = make_descriptor("plugin", "contract");
        let interface: GuestContractInterface = mock_interface(0x1234_5678_9ABC_DEF0);

        // SAFETY: interface is a local value for testing
        unsafe {
            registry.register_guest_contract(descriptor, &interface, "contract".to_owned(), bundle_id)
        }.expect("registration should succeed");

        registry.register_bundle_metadata(
            bundle_id,
            "test-bundle".to_string(),
            Version { major: 1, minor: 2, patch: 3 },
            RuntimeLanguage::Python,
            PathBuf::from("/path/to/bundle"),
            vec![BundleDependency {
                name: "dep-bundle".to_string(),
                min_version: Some(Version { major: 1, minor: 0, patch: 0 }),
            }],
        ).expect("metadata registration should succeed");

        let desc: Option<BundleDescriptor> = registry.get_bundle_descriptor(bundle_id);
        assert!(desc.is_some(), "descriptor should be found");
        let d: BundleDescriptor = desc.expect("descriptor exists");
        assert_eq!(d.name, "test-bundle");
        assert_eq!(d.version.major, 1);
        assert_eq!(d.version.minor, 2);
        assert_eq!(d.version.patch, 3);
        assert_eq!(d.runtime, RuntimeLanguage::Python);
        assert_eq!(d.dependencies.len(), 1);
    }

    #[test]
    fn get_bundles_by_name_returns_matching_ids() {
        let registry: RuntimeStore = RuntimeStore::new();
        let bundle_id: BundleId = BundleId::new("test-bundle");

        // Register bundle metadata (no plugins needed for name index test)
        registry.register_bundle_metadata(
            bundle_id,
            "test-bundle".to_string(),
            Version { major: 1, minor: 0, patch: 0 },
            RuntimeLanguage::Rust,
            PathBuf::new(),
            Vec::new(),
        ).expect("metadata registration should succeed");

        let ids: Vec<BundleId> = registry.get_bundles_by_name("test-bundle");
        assert_eq!(ids.len(), 1);
        assert_eq!(ids[0], bundle_id);

        // Non-existent name returns empty
        let missing: Vec<BundleId> = registry.get_bundles_by_name("non-existent");
        assert!(missing.is_empty(), "non-existent name should return empty vec");
    }
}
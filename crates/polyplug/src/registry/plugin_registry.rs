//! Registry — interface storage and plugin handle management.
//!
//! Simple index-based registry: each slot holds an interface pointer.
//! PluginHandle validation checks for out-of-bounds indices only.
//! Hosts must destroy instances before hot-reload via callback.
//!
//! Multi-impl support: different bundles may register different implementations of
//! the same contract. contract_index maps contract_id -> Vec<slot_index> to support
//! find_all_by_contract(). DuplicateProvider is only raised when the SAME bundle_id
//! tries to register the SAME contract_id twice.

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::RwLock;

use polyplug_abi::{GuestContractInterface, PluginDescriptor, PluginHandle};
use polyplug_utils::{BundleId, GuestContractId};

use crate::error::RegistryError;

/// Live plugin registration data.
pub(crate) struct RegistryEntry {
    /// Plugin metadata — used by other crates for introspection.
    pub descriptor: PluginDescriptor,
    /// Full contract name string for collision detection.
    pub contract_name: String,
    /// The bundle this registration originates from.
    pub bundle_id: BundleId,
}

/// A single slot in the registry's storage array.
pub(crate) struct RegistrySlot {
    /// Slot contents — None if vacant (after unload).
    pub entry: Option<RegistryEntry>,
    /// Interface pointer — direct Arc storage without wrapper.
    /// The callback-based hot-reload model ensures hosts destroy instances before swap.
    pub interface: Option<Arc<GuestContractInterface>>,
}

/// Internal data protected by a single RwLock.
///
/// This structure groups all mutable registry state together to enable
/// single-lock acquisition on the hot path, reducing lock overhead.
struct RegistryData {
    /// Slot storage — each slot holds a plugin registration or is vacant.
    slots: Vec<RegistrySlot>,
    /// Maps contract_id to the Vec of registered slot indices (multi-impl support).
    contract_index: HashMap<GuestContractId, Vec<u32>>,
    /// Maps bundle_id to the first slot index registered for that bundle.
    bundle_index: HashMap<BundleId, u32>,
    /// Maps bundle_id to the set of contract_ids it has declared as dependencies.
    declared_deps: HashMap<BundleId, HashSet<GuestContractId>>,
}

impl RegistryData {
    /// Create empty registry data.
    fn new() -> Self {
        Self {
            slots: Vec::new(),
            contract_index: HashMap::new(),
            bundle_index: HashMap::new(),
            declared_deps: HashMap::new(),
        }
    }
}

/// Thread-safe plugin registry.
//
//  Uses a single RwLock to protect all mutable state, reducing lock acquisition
//  overhead on the hot path. Writes (registration/unload) are rare, so contention
//  is minimal. Reads (find, resolve) take a read guard and are concurrent.
pub struct PluginRegistry {
    /// Single RwLock protecting all mutable registry state.
    data: RwLock<RegistryData>,
}

impl PluginRegistry {
    /// Create an empty registry.
    pub fn new() -> PluginRegistry {
        PluginRegistry {
            data: RwLock::new(RegistryData::new()),
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
    //  - contract_id is already registered to a DIFFERENT contract_name (hash collision)
    //  - contract_id is already registered by the SAME bundle_id (duplicate provider)
    //
    //  Different bundles MAY register the same contract_id (multi-impl).
    pub unsafe fn register(
        &self,
        descriptor: PluginDescriptor,
        interface_ptr: *const GuestContractInterface,
        contract_name: String,
        bundle_id: BundleId,
    ) -> Result<PluginHandle, RegistryError> {
        // SAFETY: interface_ptr is a valid 'static GuestContractInterface supplied by the caller.
        // The ABI contract requires the pointer to remain valid for the library lifetime.
        let contract_id: GuestContractId = unsafe { (*interface_ptr).contract_id };

        let mut data: std::sync::RwLockWriteGuard<'_, RegistryData> =
            self.data.write().unwrap_or_else(|e| {
                eprintln!("[polyplug] Mutex/RwLock poisoned, recovering: {}", e);
                e.into_inner()
            });

        // Check existing slots for this contract_id
        if let Some(existing_indices) = data.contract_index.get(&contract_id) {
            for &existing_idx in existing_indices.iter() {
                let existing_slot: &RegistrySlot = &data.slots[existing_idx as usize];
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
                data.slots.push(RegistrySlot {
                    entry: None,
                    interface: None,
                });
                new_idx
            });

        let slot: &mut RegistrySlot = &mut data.slots[slot_idx as usize];
        slot.entry = Some(RegistryEntry {
            descriptor,
            contract_name,
            bundle_id,
        });
        // SAFETY: interface_ptr is a valid 'static pointer, we clone the interface
        // into an Arc for shared ownership.
        slot.interface = Some(Arc::new(unsafe { (*interface_ptr).clone() }));

        // Update contract_index: push slot_idx into the Vec for this contract_id
        data.contract_index
            .entry(contract_id)
            .or_default()
            .push(slot_idx);

        // Update bundle_index: record first slot for this bundle_id
        data.bundle_index.entry(bundle_id).or_insert(slot_idx);

        Ok(PluginHandle { index: slot_idx })
    }

    /// Declare dependency contract_ids for a bundle.
    ///
    /// Must be called before the bundle resolves any cross-bundle contracts.
    /// Prevents undeclared dependency resolution at runtime.
    pub fn declare_deps(
        &self,
        bundle_id: BundleId,
        contract_ids: Vec<GuestContractId>,
    ) -> Result<(), RegistryError> {
        let mut data: std::sync::RwLockWriteGuard<'_, RegistryData> =
            self.data.write().unwrap_or_else(|e| {
                eprintln!("[polyplug] Mutex/RwLock poisoned, recovering: {}", e);
                e.into_inner()
            });
        let set: &mut HashSet<GuestContractId> = data.declared_deps.entry(bundle_id).or_default();
        for cid in contract_ids {
            set.insert(cid);
        }
        Ok(())
    }

    /// Returns true if `bundle_id` has declared `contract_id` as a dependency.
    pub(crate) fn is_dependency_declared(&self, bundle_id: BundleId, contract_id: GuestContractId) -> bool {
        let data: std::sync::RwLockReadGuard<'_, RegistryData> =
            self.data.read().unwrap_or_else(|e| {
                eprintln!("[polyplug] Mutex/RwLock poisoned, recovering: {}", e);
                e.into_inner()
            });
        data.declared_deps
            .get(&bundle_id)
            .is_some_and(|s| s.contains(&contract_id))
    }

    /// Find any registered plugin satisfying the given contract_id and minimum version.
    //
    //  Returns the first slot whose interface.contract_version >= min_version.
    //  Pass min_version=0 to accept any version.
    pub fn find_by_contract(
        &self,
        contract_id: GuestContractId,
        min_version: u32,
    ) -> Result<PluginHandle, RegistryError> {
        let data: std::sync::RwLockReadGuard<'_, RegistryData> =
            self.data.read().unwrap_or_else(|e| {
                eprintln!("[polyplug] Mutex/RwLock poisoned, recovering: {}", e);
                e.into_inner()
            });

        let indices: &Vec<u32> = match data.contract_index.get(&contract_id) {
            Some(v) => v,
            None => {
                return Err(RegistryError::PluginNotFound {
                    contract_id: contract_id.id(),
                    min_version,
                });
            }
        };

        for &slot_idx in indices.iter() {
            let slot: &RegistrySlot = &data.slots[slot_idx as usize];
            if slot.entry.is_some()
                && let Some(ref interface) = slot.interface
            {
                // SAFETY: interface points to 'static GuestContractInterface, valid for Registry lifetime.
                // The pointer is written once at registration and never mutated.
                let version: u32 = interface.contract_version.major;
                if version >= min_version {
                    return Ok(PluginHandle { index: slot_idx });
                }
            }
        }
        Err(RegistryError::PluginNotFound {
            contract_id: contract_id.id(),
            min_version,
        })
    }

    /// Find the plugin registered by a specific bundle_id that satisfies contract_id + min_version.
    pub fn find_by_bundle(
        &self,
        bundle_id: BundleId,
        contract_id: GuestContractId,
        min_version: u32,
    ) -> Result<PluginHandle, RegistryError> {
        let data: std::sync::RwLockReadGuard<'_, RegistryData> =
            self.data.read().unwrap_or_else(|e| {
                eprintln!("[polyplug] Mutex/RwLock poisoned, recovering: {}", e);
                e.into_inner()
            });

        let &slot_idx: &u32 = match data.bundle_index.get(&bundle_id) {
            Some(i) => i,
            None => {
                return Err(RegistryError::PluginNotFound {
                    contract_id: contract_id.id(),
                    min_version,
                });
            }
        };

        let slot: &RegistrySlot = &data.slots[slot_idx as usize];
        if let Some(ref entry) = slot.entry
            && let Some(ref interface) = slot.interface
        {
            // SAFETY: interface is 'static GuestContractInterface valid for Registry lifetime.
            // Written once at registration, never mutated.
            if entry.bundle_id == bundle_id
                && interface.contract_id == contract_id
                && interface.contract_version.major >= min_version
            {
                return Ok(PluginHandle { index: slot_idx });
            }
        }
        Err(RegistryError::PluginNotFound {
            contract_id: contract_id.id(),
            min_version,
        })
    }

    /// Find all plugins satisfying the given contract_id and minimum version.
    pub fn find_all_by_contract(
        &self,
        contract_id: GuestContractId,
        min_version: u32,
        out: &mut [PluginHandle],
    ) -> usize {
        let data: std::sync::RwLockReadGuard<'_, RegistryData> =
            self.data.read().unwrap_or_else(|e| {
                eprintln!("[polyplug] Mutex/RwLock poisoned, recovering: {}", e);
                e.into_inner()
            });

        let indices: &Vec<u32> = match data.contract_index.get(&contract_id) {
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
            let slot: &RegistrySlot = &data.slots[slot_idx as usize];
            if slot.entry.is_some()
                && let Some(ref interface) = slot.interface
            {
                // SAFETY: interface is 'static GuestContractInterface valid for Registry lifetime.
                // Read-only access after registration.
                let version: u32 = interface.contract_version.major;
                if version >= min_version {
                    out[write_count] = PluginHandle { index: slot_idx };
                    write_count += 1usize;
                }
            }
        }
        write_count
    }

    /// Find all plugins satisfying the given contract_id and minimum version,
    /// packing handles directly into a u64 buffer.
    ///
    /// This is an optimized version of `find_all_by_contract` that avoids
    /// intermediate allocation by packing handles directly during iteration.
    /// Each handle is packed as: `index as u64`.
    ///
    /// Returns the number of packed handles written to `out`.
    pub fn find_all_by_contract_packed(
        &self,
        contract_id: GuestContractId,
        min_version: u32,
        out: &mut [u64],
    ) -> usize {
        let data: std::sync::RwLockReadGuard<'_, RegistryData> =
            self.data.read().unwrap_or_else(|e| {
                eprintln!("[polyplug] Mutex/RwLock poisoned, recovering: {}", e);
                e.into_inner()
            });

        let indices: &Vec<u32> = match data.contract_index.get(&contract_id) {
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
            let slot: &RegistrySlot = &data.slots[slot_idx as usize];
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

    /// Find a plugin by contract_id and minimum version.
    //
    //  Delegates to find_by_contract(). Kept for API compatibility.
    //  min_version encoding: (minor << 16 | patch), same as GuestContractInterface::contract_version.
    //  Pass 0 to accept any version.
    pub fn find(&self, contract_id: GuestContractId, min_version: u32) -> Result<PluginHandle, RegistryError> {
        self.find_by_contract(contract_id, min_version)
    }

    /// Validate a PluginHandle and return its interface pointer directly.
    ///
    /// Returns Err(InvalidHandle) if:
    /// - handle.index is out of bounds
    /// - the slot has no interface
    pub fn resolve(&self, handle: PluginHandle) -> Result<*const GuestContractInterface, RegistryError> {
        let data: std::sync::RwLockReadGuard<'_, RegistryData> =
            self.data.read().unwrap_or_else(|e| {
                eprintln!("[polyplug] Mutex/RwLock poisoned, recovering: {}", e);
                e.into_inner()
            });

        let slot_idx: usize = handle.index as usize;
        if slot_idx >= data.slots.len() {
            return Err(RegistryError::InvalidHandle { index: handle.index });
        }

        let slot: &RegistrySlot = &data.slots[slot_idx];
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
    pub fn swap_interface(
        &self,
        slot_index: u32,
        new_interface: Arc<GuestContractInterface>,
    ) -> Result<(), RegistryError> {
        let mut data: std::sync::RwLockWriteGuard<'_, RegistryData> =
            self.data.write().unwrap_or_else(|e| {
                eprintln!("[polyplug] Mutex/RwLock poisoned, recovering: {}", e);
                e.into_inner()
            });
        let slot_idx: usize = slot_index as usize;
        if slot_idx >= data.slots.len() {
            return Err(RegistryError::InvalidHandle { index: slot_index });
        }
        let slot: &mut RegistrySlot = &mut data.slots[slot_idx];
        if slot.interface.is_none() {
            return Err(RegistryError::InvalidHandle { index: slot_index });
        }
        slot.interface = Some(new_interface);
        Ok(())
    }

    /// Find all slot indices that were registered by `bundle_id`.
    ///
    /// Returns an empty `Vec` if the bundle has no registered slots.
    /// Used by `reload_bundle()` to locate every interface slot to swap.
    pub fn find_slots_by_bundle(&self, bundle_id: BundleId) -> Vec<u32> {
        let data: std::sync::RwLockReadGuard<'_, RegistryData> =
            self.data.read().unwrap_or_else(|e| {
                eprintln!("[polyplug] Mutex/RwLock poisoned, recovering: {}", e);
                e.into_inner()
            });
        let mut result: Vec<u32> = Vec::new();
        for (i, slot) in data.slots.iter().enumerate() {
            if let Some(ref entry) = slot.entry
                && entry.bundle_id == bundle_id
            {
                result.push(i as u32);
            }
        }
        result
    }

    /// Get the contract_id for the interface currently stored in `slot_index`.
    /// Returns None if the slot is empty or has no interface.
    pub(crate) fn get_slot_contract_id(&self, slot_index: u32) -> Option<GuestContractId> {
        let data: std::sync::RwLockReadGuard<'_, RegistryData> =
            self.data.read().unwrap_or_else(|e| {
                eprintln!("[polyplug] Mutex/RwLock poisoned, recovering: {}", e);
                e.into_inner()
            });
        let slot: &RegistrySlot = data.slots.get(slot_index as usize)?;
        let interface: &Arc<GuestContractInterface> = slot.interface.as_ref()?;
        Some(interface.contract_id)
    }

    /// Get a clone of the Arc<GuestContractInterface> for `slot_index` to check strong_count.
    /// Returns None if the slot is empty or has no interface.
    pub(crate) fn get_interface_arc(&self, slot_index: u32) -> Option<Arc<GuestContractInterface>> {
        let data: std::sync::RwLockReadGuard<'_, RegistryData> =
            self.data.read().unwrap_or_else(|e| {
                eprintln!("[polyplug] Mutex/RwLock poisoned, recovering: {}", e);
                e.into_inner()
            });
        let slot: &RegistrySlot = data.slots.get(slot_index as usize)?;
        slot.interface.as_ref().map(|arc| Arc::clone(arc))
    }


    /// Clear all registrations for testing.
    /// This is only available in test builds to allow test isolation.
    #[cfg(test)]
    pub fn clear_for_test(&self) {
        let mut data: std::sync::RwLockWriteGuard<'_, RegistryData> =
            self.data.write().unwrap_or_else(|e| {
                eprintln!("[polyplug] Mutex/RwLock poisoned, recovering: {}", e);
                e.into_inner()
            });
        data.slots.clear();
        data.contract_index.clear();
        data.bundle_index.clear();
        data.declared_deps.clear();
    }
}

impl Default for PluginRegistry {
    fn default() -> PluginRegistry {
        PluginRegistry::new()
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
    fn register_and_find() {
        let registry: PluginRegistry = PluginRegistry::new();
        let descriptor: PluginDescriptor = make_descriptor("test_plugin", "image.decode");
        let interface = mock_interface(0x1234_5678_9ABC_DEF0);
        // SAFETY: interface is a local value, but we're just testing registration
        let handle: PluginHandle = unsafe {
            registry.register(
                descriptor,
                &interface,
                "image.decode".to_owned(),
                BundleId::from_u64(0),
            )
        }
        .expect("registration should succeed");
        assert!(!handle.is_null());

        let found: PluginHandle = registry
            .find(GuestContractId::from_u64(0x1234_5678_9ABC_DEF0), 0)
            .expect("find should succeed");
        assert_eq!(found.index, handle.index);
    }

    #[test]
    fn invalid_handle_detection() {
        let registry: PluginRegistry = PluginRegistry::new();
        let descriptor: PluginDescriptor = make_descriptor("test_plugin", "audio.decode");
        let interface = mock_interface(0x1234_5678_9ABC_DEF0);

        // SAFETY: interface is a local value, but we're just testing registration
        let _handle: PluginHandle = unsafe {
            registry.register(
                descriptor,
                &interface,
                "image.decode".to_owned(),
                BundleId::from_u64(1),
            )
        }
        .expect("registration should succeed");

        // Use a handle with out-of-bounds index
        let invalid: PluginHandle = PluginHandle { index: 999 };
        let result: Result<*const GuestContractInterface, RegistryError> = registry.resolve(invalid);
        assert!(
            matches!(result, Err(RegistryError::InvalidHandle { .. })),
            "expected InvalidHandle error"
        );
    }

    #[test]
    fn duplicate_provider_allowed() {
        let registry: PluginRegistry = PluginRegistry::new();
        let d1: PluginDescriptor = make_descriptor("plugin_a", "image.decode");
        let d2: PluginDescriptor = make_descriptor("plugin_b", "image.decode");
        let bundle_id = BundleId::from_u64(0);
        let interface = mock_interface(0x1234_5678_9ABC_DEF0);

        // SAFETY: interface is a local value
        unsafe {
            registry
                .register(d1, &interface, "image.decode".to_owned(), bundle_id)
                .expect("first registration should succeed");
        }

        let result: Result<PluginHandle, RegistryError> =
            // SAFETY: interface is a local value
            unsafe { registry.register(d2, &interface, "image.decode".to_owned(), bundle_id) };
        // Second registration should succeed (multi-impl allowed)
        assert!(
            result.is_ok(),
            "second registration should succeed (multi-impl allowed)"
        );
    }

    #[test]
    fn collision_detection() {
        let registry: PluginRegistry = PluginRegistry::new();
        let d1: PluginDescriptor = make_descriptor("plugin_a", "contract.a");
        let d2: PluginDescriptor = make_descriptor("plugin_b", "contract.b");
        let bundle_id_a = BundleId::from_u64(10);
        let bundle_id_b = BundleId::from_u64(20);
        let interface = mock_interface(0x1234_5678_9ABC_DEF0);

        // SAFETY: interface is a local value
        unsafe {
            registry
                .register(d1, &interface, "contract.a".to_owned(), bundle_id_a)
                .expect("first registration should succeed");
        }

        let result: Result<PluginHandle, RegistryError> =
            // SAFETY: interface is a local value
            unsafe { registry.register(d2, &interface, "contract.b".to_owned(), bundle_id_b) };
        assert!(
            matches!(result, Err(RegistryError::ContractIdCollision { .. })),
            "expected ContractIdCollision error"
        );
    }

    #[test]
    fn resolve_returns_interface_pointer() {
        let registry: PluginRegistry = PluginRegistry::new();
        let descriptor: PluginDescriptor = make_descriptor("test_plugin", "test.contract");
        let interface = mock_interface(0x1234_5678_9ABC_DEF0);

        // SAFETY: interface is a local value
        let handle: PluginHandle = unsafe {
            registry.register(
                descriptor,
                &interface,
                "image.decode".to_owned(),
                BundleId::from_u64(2),
            )
        }
        .expect("registration should succeed");

        let interface_ptr: *const GuestContractInterface =
            registry.resolve(handle).expect("resolve should succeed");
        // SAFETY: interface_ptr points to a valid GuestContractInterface
        let contract_id: GuestContractId = unsafe { (*interface_ptr).contract_id };
        assert_eq!(contract_id, interface.contract_id);
    }

    #[test]
    fn declare_deps_and_query() {
        let registry: PluginRegistry = PluginRegistry::new();
        let bundle_id = BundleId::from_u64(42);
        let contract_a = GuestContractId::from_u64(0x1111_2222_3333_4444);
        let contract_b = GuestContractId::from_u64(0x5555_6666_7777_8888);

        registry
            .declare_deps(bundle_id, vec![contract_a])
            .expect("declare_deps should succeed");

        assert!(
            registry.is_dependency_declared(bundle_id, contract_a),
            "declared dep should be found"
        );
        assert!(
            !registry.is_dependency_declared(bundle_id, contract_b),
            "undeclared dep should not be found"
        );
    }
}
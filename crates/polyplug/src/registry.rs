//! Registry — VTable storage and plugin handle management.
//!
//! Implements a generational index registry: each slot holds a generation counter.
//! PluginHandle validation compares handle.generation against slot.generation to
//! detect use-after-unload (stale handle detection).
//!
//! Multi-impl support: different bundles may register different implementations of
//! the same contract. contract_index maps contract_id -> Vec<slot_index> to support
//! find_all_by_contract(). DuplicateProvider is only raised when the SAME bundle_id
//! tries to register the SAME contract_id twice.

use core::sync::atomic::{AtomicU32, Ordering};
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::RwLock;

use arc_swap::ArcSwap;

use crate::error::RegistryError;
use polyplug_abi::PluginDescriptor;
use polyplug_abi::PluginHandle;
use polyplug_abi::PluginInterface;

/// A `Send + Sync` wrapper around a raw interface pointer.
/// The pointer is guaranteed to point to `'static` data that is never mutated after registration.
pub struct VTableSlot(pub *const PluginInterface);

// SAFETY: *const PluginInterface points to 'static plugin data. Once registered, the data is never
// mutated. The pointer remains valid for the lifetime of the loaded library. Aliasing is safe
// because all access is read-only through PluginVTableGuard.
unsafe impl Send for VTableSlot {}
// SAFETY: Same reasoning as Send above — read-only access to 'static data.
unsafe impl Sync for VTableSlot {}

/// An Arc-backed guard that keeps a vtable slot alive.
/// This is Rust-only and never crosses the C ABI boundary.
/// Intentionally NOT Send: the guard must be used on the same thread that called
/// resolve_guard(), or re-resolved per-call from a new thread.
pub struct PluginVTableGuard {
    pub(crate) slot: Arc<VTableSlot>,
    /// Opt-out of Send. Cell<()> is !Send, so PluginVTableGuard becomes !Send automatically.
    _not_send: core::marker::PhantomData<core::cell::Cell<()>>,
}

impl PluginVTableGuard {
    /// Construct a new guard wrapping the given slot.
    pub(crate) fn new(slot: Arc<VTableSlot>) -> Self {
        Self {
            slot,
            _not_send: core::marker::PhantomData,
        }
    }

    /// Returns the raw interface pointer. The pointer is valid as long as this guard is alive.
    pub fn vtable(&self) -> *const PluginInterface {
        self.slot.0
    }
}

/// A single slot in the registry's storage array.
//
//  Implements the generational index pattern.
//  generation is incremented each time the slot is vacated (plugin unloaded).
pub(crate) struct RegistrySlot {
    /// Current generation counter. Compared against PluginHandle::generation.
    /// Uses AtomicU32 for lock-free reads during handle validation.
    pub generation: AtomicU32,
    /// Slot contents — None if vacant (after unload).
    pub entry: Option<RegistryEntry>,
    /// ArcSwap vtable — allows lock-free hot-swapping of implementations.
    pub vtable: Option<ArcSwap<VTableSlot>>,
}

/// Live plugin registration data.
pub(crate) struct RegistryEntry {
    /// Plugin metadata — used by other crates for introspection.
    #[allow(dead_code)]
    pub descriptor: PluginDescriptor,
    /// Full contract name string for collision detection.
    pub contract_name: String,
    /// The bundle this registration originates from.
    pub bundle_id: u64,
}

/// Internal data protected by a single RwLock.
///
/// This structure groups all mutable registry state together to enable
/// single-lock acquisition on the hot path, reducing lock overhead.
struct RegistryData {
    /// Slot storage — each slot holds a plugin registration or is vacant.
    slots: Vec<RegistrySlot>,
    /// Maps contract_id (FNV-1a u64) to the Vec of registered slot indices (multi-impl support).
    contract_index: HashMap<u64, Vec<u32>>,
    /// Maps bundle_id to the first slot index registered for that bundle.
    bundle_index: HashMap<u64, u32>,
    /// Maps bundle_id to the set of contract_ids it has declared as dependencies.
    declared_deps: HashMap<u64, HashSet<u64>>,
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
pub struct Registry {
    /// Library handles for all loaded native bundles.
    /// Declared FIRST so they drop LAST (Rust drops fields in reverse order).
    /// This ensures vtable pointers in `slots` are never dangling during drop.
    loaded_libraries: Mutex<Vec<libloading::Library>>,
    /// Single RwLock protecting all mutable registry state.
    data: RwLock<RegistryData>,
}

impl Registry {
    /// Create an empty registry.
    pub fn new() -> Registry {
        Registry {
            loaded_libraries: Mutex::new(Vec::new()),
            data: RwLock::new(RegistryData::new()),
        }
    }

    /// Store a loaded native library handle, keeping it alive until this Registry drops.
    ///
    /// Called by `load_bundle()` after successfully extracting vtable pointers from
    /// the library. The handle must outlive the Registry to prevent `dlclose()` from
    /// unmapping code pages that vtable function pointers point into.
    ///
    /// `loaded_libraries` is declared as the first field in `Registry`, so it drops
    /// last during `Registry` drop — after all `RegistrySlot` vtable pointers are gone.
    pub fn push_library(&self, library: libloading::Library) {
        self.loaded_libraries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(library);
    }

    /// Register a plugin interface.
    ///
    /// # Safety
    ///
    /// `interface_ptr` must be a valid pointer to a `'static` `PluginInterface` that remains valid
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
        interface_ptr: *const PluginInterface,
        contract_name: String,
        bundle_id: u64,
    ) -> Result<PluginHandle, RegistryError> {
        // SAFETY: interface_ptr is a valid 'static PluginInterface supplied by the caller.
        // The ABI contract requires the pointer to remain valid for the library lifetime.
        let contract_id: u64 = unsafe { (*interface_ptr).contract_id };

        let mut data: std::sync::RwLockWriteGuard<'_, RegistryData> =
            self.data.write().unwrap_or_else(|e| e.into_inner());

        // Check existing slots for this contract_id
        if let Some(existing_indices) = data.contract_index.get(&contract_id) {
            for &existing_idx in existing_indices.iter() {
                let existing_slot: &RegistrySlot = &data.slots[existing_idx as usize];
                if let Some(ref existing_entry) = existing_slot.entry {
                    // Hash collision: same contract_id, different contract_name
                    if existing_entry.contract_name != contract_name {
                        return Err(RegistryError::ContractIdCollision {
                            id: contract_id,
                            name_a: existing_entry.contract_name.clone(),
                            name_b: contract_name,
                        });
                    }
                    // Duplicate provider: same bundle_id registering same contract_id again
                    if existing_entry.bundle_id == bundle_id {
                        return Err(RegistryError::DuplicateProvider {
                            contract: contract_name,
                            existing: existing_entry.contract_name.clone(),
                        });
                    }
                    // Different bundle, same contract — allowed (multi-impl), keep scanning
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
                    generation: AtomicU32::new(0_u32),
                    entry: None,
                    vtable: None,
                });
                new_idx
            });

        let slot: &mut RegistrySlot = &mut data.slots[slot_idx as usize];
        let generation: u32 = slot.generation.load(Ordering::Acquire);
        slot.entry = Some(RegistryEntry {
            descriptor,
            contract_name,
            bundle_id,
        });
        slot.vtable = Some(ArcSwap::new(Arc::new(VTableSlot(interface_ptr))));

        // Update contract_index: push slot_idx into the Vec for this contract_id
        data.contract_index
            .entry(contract_id)
            .or_default()
            .push(slot_idx);

        // Update bundle_index: record first slot for this bundle_id
        data.bundle_index.entry(bundle_id).or_insert(slot_idx);

        Ok(PluginHandle {
            index: slot_idx,
            generation,
        })
    }

    /// Declare dependency contract_ids for a bundle.
    ///
    /// Must be called before the bundle resolves any cross-bundle contracts.
    /// Prevents undeclared dependency resolution at runtime.
    pub fn declare_deps(
        &self,
        bundle_id: u64,
        contract_ids: Vec<u64>,
    ) -> Result<(), RegistryError> {
        let mut data: std::sync::RwLockWriteGuard<'_, RegistryData> =
            self.data.write().unwrap_or_else(|e| e.into_inner());
        let set: &mut HashSet<u64> = data.declared_deps.entry(bundle_id).or_default();
        for cid in contract_ids {
            set.insert(cid);
        }
        Ok(())
    }

    /// Returns true if `bundle_id` has declared `contract_id` as a dependency.
    pub(crate) fn is_dependency_declared(&self, bundle_id: u64, contract_id: u64) -> bool {
        let data: std::sync::RwLockReadGuard<'_, RegistryData> =
            self.data.read().unwrap_or_else(|e| e.into_inner());
        data.declared_deps
            .get(&bundle_id)
            .is_some_and(|s| s.contains(&contract_id))
    }

    /// Find any registered plugin satisfying the given contract_id and minimum version.
    //
    //  Returns the first slot whose vtable.contract_version >= min_version.
    //  Pass min_version=0 to accept any version.
    pub fn find_by_contract(
        &self,
        contract_id: u64,
        min_version: u32,
    ) -> Result<PluginHandle, RegistryError> {
        let data: std::sync::RwLockReadGuard<'_, RegistryData> =
            self.data.read().unwrap_or_else(|e| e.into_inner());

        let indices: &Vec<u32> = match data.contract_index.get(&contract_id) {
            Some(v) => v,
            None => {
                return Err(RegistryError::PluginNotFound {
                    contract_id,
                    min_version,
                });
            }
        };

        for &slot_idx in indices.iter() {
            let slot: &RegistrySlot = &data.slots[slot_idx as usize];
            if slot.entry.is_some()
                && let Some(ref arc_vtable) = slot.vtable
            {
                let guard: arc_swap::Guard<Arc<VTableSlot>> = arc_vtable.load();
                // SAFETY: VTableSlot.0 points to 'static PluginInterface, valid for Registry lifetime.
                // The pointer is written once at registration and never mutated.
                let version: u32 = unsafe { (*guard.0).contract_version };
                if version >= min_version {
                    return Ok(PluginHandle {
                        index: slot_idx,
                        generation: slot.generation.load(Ordering::Acquire),
                    });
                }
            }
        }
        Err(RegistryError::PluginNotFound {
            contract_id,
            min_version,
        })
    }

    /// Find the plugin registered by a specific bundle_id that satisfies contract_id + min_version.
    pub fn find_by_bundle(
        &self,
        bundle_id: u64,
        contract_id: u64,
        min_version: u32,
    ) -> Result<PluginHandle, RegistryError> {
        let data: std::sync::RwLockReadGuard<'_, RegistryData> =
            self.data.read().unwrap_or_else(|e| e.into_inner());

        let &slot_idx: &u32 = match data.bundle_index.get(&bundle_id) {
            Some(i) => i,
            None => {
                return Err(RegistryError::PluginNotFound {
                    contract_id,
                    min_version,
                });
            }
        };

        let slot: &RegistrySlot = &data.slots[slot_idx as usize];
        if let Some(ref entry) = slot.entry
            && let Some(ref arc_vtable) = slot.vtable
        {
            let guard: arc_swap::Guard<Arc<VTableSlot>> = arc_vtable.load();
            // SAFETY: VTableSlot.0 is 'static PluginInterface valid for Registry lifetime.
            // Written once at registration, never mutated.
            let vtable_ref: &PluginInterface = unsafe { &*guard.0 };
            if entry.bundle_id == bundle_id
                && vtable_ref.contract_id == contract_id
                && vtable_ref.contract_version >= min_version
            {
                return Ok(PluginHandle {
                    index: slot_idx,
                    generation: slot.generation.load(Ordering::Acquire),
                });
            }
        }
        Err(RegistryError::PluginNotFound {
            contract_id,
            min_version,
        })
    }

    /// Find all plugins satisfying the given contract_id and minimum version.
    pub fn find_all_by_contract(
        &self,
        contract_id: u64,
        min_version: u32,
        out: &mut [PluginHandle],
    ) -> usize {
        let data: std::sync::RwLockReadGuard<'_, RegistryData> =
            self.data.read().unwrap_or_else(|e| e.into_inner());

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
                && let Some(ref arc_vtable) = slot.vtable
            {
                let guard: arc_swap::Guard<Arc<VTableSlot>> = arc_vtable.load();
                // SAFETY: VTableSlot.0 is 'static PluginInterface valid for Registry lifetime.
                // Read-only access after registration.
                let version: u32 = unsafe { (*guard.0).contract_version };
                if version >= min_version {
                    out[write_count] = PluginHandle {
                        index: slot_idx,
                        generation: slot.generation.load(Ordering::Acquire),
                    };
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
    /// Each handle is packed as: `(generation as u64) << 32 | index as u64`.
    ///
    /// Returns the number of packed handles written to `out`.
    pub fn find_all_by_contract_packed(
        &self,
        contract_id: u64,
        min_version: u32,
        out: &mut [u64],
    ) -> usize {
        let data: std::sync::RwLockReadGuard<'_, RegistryData> =
            self.data.read().unwrap_or_else(|e| e.into_inner());

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
                && let Some(ref arc_vtable) = slot.vtable
            {
                let guard: arc_swap::Guard<Arc<VTableSlot>> = arc_vtable.load();
                // SAFETY: VTableSlot.0 is 'static PluginInterface valid for Registry lifetime.
                // Read-only access after registration.
                let version: u32 = unsafe { (*guard.0).contract_version };
                if version >= min_version {
                    // Pack handle directly: (generation << 32) | index
                    out[write_count] =
                        (slot.generation.load(Ordering::Acquire) as u64) << 32 | slot_idx as u64;
                    write_count += 1usize;
                }
            }
        }
        write_count
    }

    /// Validate a PluginHandle and return an Arc-backed vtable guard.
    //
    //  Returns Err(StaleHandle) if the handle's generation doesn't match the slot.
    //  Returns Err(StaleHandle) if the slot is vacant or has no vtable.
    pub fn resolve_guard(&self, handle: PluginHandle) -> Result<PluginVTableGuard, RegistryError> {
        let data: std::sync::RwLockReadGuard<'_, RegistryData> =
            self.data.read().unwrap_or_else(|e| e.into_inner());

        let slot_idx: usize = handle.index as usize;
        if slot_idx >= data.slots.len() {
            return Err(RegistryError::StaleHandle {
                index: handle.index,
                expected: handle.generation,
                found: 0,
            });
        }

        let slot: &RegistrySlot = &data.slots[slot_idx];
        let slot_generation: u32 = slot.generation.load(Ordering::Acquire);
        if slot_generation != handle.generation {
            return Err(RegistryError::StaleHandle {
                index: handle.index,
                expected: handle.generation,
                found: slot_generation,
            });
        }

        match slot.vtable {
            Some(ref arc_vtable) => {
                let arc: Arc<VTableSlot> = arc_vtable.load_full();
                Ok(PluginVTableGuard::new(arc))
            }
            None => Err(RegistryError::StaleHandle {
                index: handle.index,
                expected: handle.generation,
                found: slot_generation,
            }),
        }
    }

    /// Find a plugin by contract_id and minimum version.
    //
    //  Delegates to find_by_contract(). Kept for API compatibility.
    //  min_version encoding: (minor << 16 | patch), same as PluginVTable::contract_version.
    //  Pass 0 to accept any version.
    pub fn find(&self, contract_id: u64, min_version: u32) -> Result<PluginHandle, RegistryError> {
        self.find_by_contract(contract_id, min_version)
    }

    /// Validate a PluginHandle and return its interface pointer.
    //
    //  Delegates to resolve_guard(). Kept for API compatibility.
    //  Returns Err(StaleHandle) if the handle's generation doesn't match the slot.
    pub fn resolve(&self, handle: PluginHandle) -> Result<*const PluginInterface, RegistryError> {
        let guard: PluginVTableGuard = self.resolve_guard(handle)?;
        Ok(guard.vtable())
    }

    /// Atomically swap the vtable in slot `slot_index` with `new_vtable`.
    ///
    /// Returns the old `Arc<VTableSlot>` — the caller holds it alive during quiescence
    /// then drops it after `Arc::strong_count` reaches 1 (all in-flight calls done).
    /// Bumps `slot.generation` so stale `PluginHandle`s from before reload are detected.
    ///
    /// # Errors
    /// Returns `Err(RegistryError::StaleHandle)` if `slot_index` is out of bounds
    /// or the slot has no vtable.
    pub fn swap_vtable(
        &self,
        slot_index: u32,
        new_vtable: Arc<VTableSlot>,
    ) -> Result<Arc<VTableSlot>, RegistryError> {
        let mut data: std::sync::RwLockWriteGuard<'_, RegistryData> =
            self.data.write().unwrap_or_else(|e| e.into_inner());
        let slot_idx: usize = slot_index as usize;
        if slot_idx >= data.slots.len() {
            return Err(RegistryError::StaleHandle {
                index: slot_index,
                expected: 0_u32,
                found: 0_u32,
            });
        }
        let slot: &mut RegistrySlot = &mut data.slots[slot_idx];
        match slot.vtable {
            Some(ref arc_swap) => {
                let old_arc: Arc<VTableSlot> = arc_swap.swap(new_vtable);
                // Bump generation so stale PluginHandles from before reload are detected.
                slot.generation.fetch_add(1_u32, Ordering::AcqRel);
                Ok(old_arc)
            }
            None => Err(RegistryError::StaleHandle {
                index: slot_index,
                expected: 0_u32,
                found: slot.generation.load(Ordering::Acquire),
            }),
        }
    }

    /// Find all slot indices that were registered by `bundle_id`.
    ///
    /// Returns an empty `Vec` if the bundle has no registered slots.
    /// Used by `reload_bundle()` to locate every vtable slot to swap.
    pub fn find_slots_by_bundle(&self, bundle_id: u64) -> Vec<u32> {
        let data: std::sync::RwLockReadGuard<'_, RegistryData> =
            self.data.read().unwrap_or_else(|e| e.into_inner());
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

    /// Get the contract_id for the vtable currently stored in `slot_index`.
    /// Returns None if the slot is empty or has no vtable.
    pub(crate) fn get_slot_contract_id(&self, slot_index: u32) -> Option<u64> {
        let data: std::sync::RwLockReadGuard<'_, RegistryData> =
            self.data.read().unwrap_or_else(|e| e.into_inner());
        let slot: &RegistrySlot = data.slots.get(slot_index as usize)?;
        let arc_swap: &arc_swap::ArcSwap<VTableSlot> = slot.vtable.as_ref()?;
        let guard: arc_swap::Guard<Arc<VTableSlot>> = arc_swap.load();
        // SAFETY: VTableSlot.0 is a valid 'static PluginInterface written at registration.
        Some(unsafe { (*guard.0).contract_id })
    }

    /// Get a clone of the Arc<VTableSlot> for `slot_index` to check strong_count.
    /// Returns None if the slot is empty or has no vtable.
    pub(crate) fn get_vtable_arc(&self, slot_index: u32) -> Option<Arc<VTableSlot>> {
        let data: std::sync::RwLockReadGuard<'_, RegistryData> =
            self.data.read().unwrap_or_else(|e| e.into_inner());
        let slot: &RegistrySlot = data.slots.get(slot_index as usize)?;
        let arc_swap: &arc_swap::ArcSwap<VTableSlot> = slot.vtable.as_ref()?;
        Some(arc_swap.load_full())
    }

    /// Clear all registrations for testing.
    /// This is only available in test builds to allow test isolation.
    #[cfg(test)]
    pub fn clear_for_test(&self) {
        let mut data: std::sync::RwLockWriteGuard<'_, RegistryData> =
            self.data.write().unwrap_or_else(|e| e.into_inner());
        data.slots.clear();
        data.contract_index.clear();
        data.bundle_index.clear();
        data.declared_deps.clear();
        self.loaded_libraries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }
}

impl Default for Registry {
    fn default() -> Registry {
        Registry::new()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;
    use polyplug_abi::{
        DispatchType, NativeDispatch, PluginDescriptor, PluginDispatch, StringView,
    };

    const MOCK_FNS: [*const (); 0] = [];

    static MOCK_INTERFACE: PluginInterface = PluginInterface {
        rt_ctx: core::ptr::null(),
        contract_id: 0x1234_5678_9ABC_DEF0,
        contract_version: (1 << 16),
        function_count: 0,
        dispatch_type: DispatchType::Native,
        dispatch: PluginDispatch {
            native: NativeDispatch {
                functions: MOCK_FNS.as_ptr(),
            },
        },
    };

    fn make_descriptor(name: &'static str, contract_name: &'static str) -> PluginDescriptor {
        PluginDescriptor {
            name: StringView::from_static(name.as_bytes()),
            contract_name: StringView::from_static(contract_name.as_bytes()),
            version_major: 1,
            version_minor: 0,
            version_patch: 0,
        }
    }

    #[test]
    fn register_and_find() {
        let registry: Registry = Registry::new();
        let descriptor: PluginDescriptor = make_descriptor("test_plugin", "image.decode");
        // contract_id comes from MOCK_INTERFACE.contract_id (0x1234_5678_9ABC_DEF0)
        // SAFETY: MOCK_INTERFACE is 'static, pointer is valid for Registry lifetime.
        let handle: PluginHandle = unsafe {
            registry.register(
                descriptor,
                &MOCK_INTERFACE,
                "image.decode".to_owned(),
                0u64, // bundle_id
            )
        }
        .expect("registration should succeed");
        assert!(!handle.is_null());

        let found: PluginHandle = registry
            .find(0x1234_5678_9ABC_DEF0, 0)
            .expect("find should succeed");
        assert_eq!(found.index, handle.index);
    }

    #[test]
    fn stale_handle_detection() {
        let registry: Registry = Registry::new();
        let descriptor: PluginDescriptor = make_descriptor("test_plugin", "audio.decode");

        // We need an interface whose contract_id differs from MOCK_INTERFACE to avoid collision
        // with the image.decode test. Use a separate static with same contract_id here
        // since each test gets its own Registry instance.
        // SAFETY: MOCK_INTERFACE is 'static, pointer is valid for Registry lifetime.
        let handle: PluginHandle = unsafe {
            registry.register(
                descriptor,
                &MOCK_INTERFACE,
                "image.decode".to_owned(),
                1u64, // bundle_id
            )
        }
        .expect("registration should succeed");

        // Use a handle with wrong generation
        let stale: PluginHandle = PluginHandle {
            index: handle.index,
            generation: handle.generation + 1,
        };
        let result: Result<*const PluginInterface, RegistryError> = registry.resolve(stale);
        assert!(
            matches!(result, Err(RegistryError::StaleHandle { .. })),
            "expected StaleHandle error"
        );
    }

    #[test]
    fn duplicate_provider_rejected() {
        let registry: Registry = Registry::new();
        let d1: PluginDescriptor = make_descriptor("plugin_a", "image.decode");
        let d2: PluginDescriptor = make_descriptor("plugin_b", "image.decode");
        // Same bundle_id = duplicate provider (same bundle can't register same contract twice)
        let bundle_id: u64 = 0u64;

        // SAFETY: MOCK_INTERFACE is 'static, pointer is valid for Registry lifetime.
        unsafe {
            registry
                .register(d1, &MOCK_INTERFACE, "image.decode".to_owned(), bundle_id)
                .expect("first registration should succeed");
        }

        let result: Result<PluginHandle, RegistryError> =
            // SAFETY: MOCK_INTERFACE is 'static, pointer is valid.
            unsafe { registry.register(d2, &MOCK_INTERFACE, "image.decode".to_owned(), bundle_id) };
        assert!(
            matches!(result, Err(RegistryError::DuplicateProvider { .. })),
            "expected DuplicateProvider error"
        );
    }

    #[test]
    fn collision_detection() {
        let registry: Registry = Registry::new();
        let d1: PluginDescriptor = make_descriptor("plugin_a", "contract.a");
        let d2: PluginDescriptor = make_descriptor("plugin_b", "contract.b");
        // Different bundle_ids: collision is about hash collision on contract_id (same id, different name)
        // MOCK_INTERFACE.contract_id will be read, so we register with the same interface but
        // different contract_name strings — this simulates a hash collision scenario.
        let bundle_id_a: u64 = 10u64;
        let bundle_id_b: u64 = 20u64;

        // SAFETY: MOCK_INTERFACE is 'static, pointer is valid for Registry lifetime.
        unsafe {
            registry
                .register(d1, &MOCK_INTERFACE, "contract.a".to_owned(), bundle_id_a)
                .expect("first registration should succeed");
        }

        let result: Result<PluginHandle, RegistryError> =
            // SAFETY: MOCK_INTERFACE is 'static, pointer is valid.
            unsafe { registry.register(d2, &MOCK_INTERFACE, "contract.b".to_owned(), bundle_id_b) };
        assert!(
            matches!(result, Err(RegistryError::ContractIdCollision { .. })),
            "expected ContractIdCollision error"
        );
    }

    #[test]
    fn resolve_returns_interface_pointer() {
        let registry: Registry = Registry::new();
        let descriptor: PluginDescriptor = make_descriptor("test_plugin", "test.contract");
        // SAFETY: MOCK_INTERFACE is 'static, pointer is valid for Registry lifetime.
        let handle: PluginHandle = unsafe {
            registry.register(
                descriptor,
                &MOCK_INTERFACE,
                "image.decode".to_owned(), // must match MOCK_INTERFACE's implied contract name
                2u64,                      // bundle_id
            )
        }
        .expect("registration should succeed");

        let interface_ptr: *const PluginInterface =
            registry.resolve(handle).expect("resolve should succeed");
        // SAFETY: interface_ptr points to MOCK_INTERFACE which is 'static.
        let contract_id: u64 = unsafe { (*interface_ptr).contract_id };
        assert_eq!(contract_id, MOCK_INTERFACE.contract_id);
    }

    #[test]
    fn multi_impl_different_bundles() {
        // Two different bundles may register the same contract_id.
        // Both should succeed; find_all_by_contract should return both.
        static INTERFACE_A: PluginInterface = PluginInterface {
            rt_ctx: core::ptr::null(),
            contract_id: 0xAAAA_BBBB_CCCC_DDDD,
            contract_version: (1 << 16),
            function_count: 0,
            dispatch_type: DispatchType::Native,
            dispatch: PluginDispatch {
                native: NativeDispatch {
                    functions: MOCK_FNS.as_ptr(),
                },
            },
        };
        static INTERFACE_B: PluginInterface = PluginInterface {
            rt_ctx: core::ptr::null(),
            contract_id: 0xAAAA_BBBB_CCCC_DDDD,
            contract_version: (2 << 16),
            function_count: 0,
            dispatch_type: DispatchType::Native,
            dispatch: PluginDispatch {
                native: NativeDispatch {
                    functions: MOCK_FNS.as_ptr(),
                },
            },
        };

        let registry: Registry = Registry::new();
        let d1: PluginDescriptor = make_descriptor("bundle_a_plugin", "multi.contract");
        let d2: PluginDescriptor = make_descriptor("bundle_b_plugin", "multi.contract");

        // SAFETY: INTERFACE_A is 'static, pointer is valid for Registry lifetime.
        let handle_a: PluginHandle =
            unsafe { registry.register(d1, &INTERFACE_A, "multi.contract".to_owned(), 100u64) }
                .expect("bundle_a registration should succeed");
        // SAFETY: INTERFACE_B is 'static, pointer is valid for Registry lifetime.
        let handle_b: PluginHandle =
            unsafe { registry.register(d2, &INTERFACE_B, "multi.contract".to_owned(), 200u64) }
                .expect("bundle_b registration should succeed");

        assert_ne!(
            handle_a.index, handle_b.index,
            "each bundle gets its own slot"
        );

        let mut handles: [PluginHandle; 4] = [PluginHandle {
            index: 0u32,
            generation: 0u32,
        }; 4];
        let count: usize = registry.find_all_by_contract(0xAAAA_BBBB_CCCC_DDDD, 0, &mut handles);
        assert_eq!(count, 2, "both implementations should be found");
    }

    #[test]
    fn declare_deps_and_query() {
        let registry: Registry = Registry::new();
        let bundle_id: u64 = 42u64;
        let contract_a: u64 = 0x1111_2222_3333_4444;
        let contract_b: u64 = 0x5555_6666_7777_8888;

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
    #[test]
    fn swap_vtable_returns_old_arc_and_bumps_generation() {
        static NEW_INTERFACE: PluginInterface = PluginInterface {
            rt_ctx: core::ptr::null(),
            contract_id: 0x1234_5678_9ABC_DEF0,
            contract_version: (2 << 16),
            function_count: 0,
            dispatch_type: DispatchType::Native,
            dispatch: PluginDispatch {
                native: NativeDispatch {
                    functions: MOCK_FNS.as_ptr(),
                },
            },
        };

        let registry: Registry = Registry::new();
        let descriptor: PluginDescriptor = make_descriptor("swap_plugin", "swap.contract");
        // SAFETY: MOCK_INTERFACE is 'static, pointer is valid for Registry lifetime.
        let handle: PluginHandle = unsafe {
            registry.register(
                descriptor,
                &MOCK_INTERFACE,
                "swap.contract".to_owned(),
                50u64,
            )
        }
        .expect("registration should succeed");

        let gen_before: u32 = handle.generation;
        let new_arc: Arc<VTableSlot> = Arc::new(VTableSlot(&NEW_INTERFACE));
        let old_arc: Arc<VTableSlot> = registry
            .swap_vtable(handle.index, new_arc)
            .expect("swap_vtable should succeed");

        // old_arc should point to the original MOCK_INTERFACE
        // SAFETY: old_arc.0 is MOCK_INTERFACE which is 'static.
        let old_contract_id: u64 = unsafe { (*old_arc.0).contract_id };
        assert_eq!(old_contract_id, MOCK_INTERFACE.contract_id);

        // Verify generation was bumped
        let data: std::sync::RwLockReadGuard<'_, RegistryData> =
            registry.data.read().unwrap_or_else(|e| e.into_inner());
        let new_gen: u32 = data.slots[handle.index as usize]
            .generation
            .load(Ordering::Acquire);
        assert_eq!(new_gen, gen_before.wrapping_add(1_u32));
    }

    #[test]
    fn find_slots_by_bundle_returns_all_slots() {
        static INTERFACE_X: PluginInterface = PluginInterface {
            rt_ctx: core::ptr::null(),
            contract_id: 0xDEAD_BEEF_0000_0001,
            contract_version: (1 << 16),
            function_count: 0,
            dispatch_type: DispatchType::Native,
            dispatch: PluginDispatch {
                native: NativeDispatch {
                    functions: MOCK_FNS.as_ptr(),
                },
            },
        };
        static INTERFACE_Y: PluginInterface = PluginInterface {
            rt_ctx: core::ptr::null(),
            contract_id: 0xDEAD_BEEF_0000_0002,
            contract_version: (1 << 16),
            function_count: 0,
            dispatch_type: DispatchType::Native,
            dispatch: PluginDispatch {
                native: NativeDispatch {
                    functions: MOCK_FNS.as_ptr(),
                },
            },
        };

        let registry: Registry = Registry::new();
        let bundle_id: u64 = 999u64;
        let d1: PluginDescriptor = make_descriptor("bundle_plugin_x", "bundle.contract.x");
        let d2: PluginDescriptor = make_descriptor("bundle_plugin_y", "bundle.contract.y");

        // SAFETY: INTERFACE_X and INTERFACE_Y are 'static, pointers are valid for Registry lifetime.
        unsafe {
            registry
                .register(d1, &INTERFACE_X, "bundle.contract.x".to_owned(), bundle_id)
                .expect("first registration should succeed");
            registry
                .register(d2, &INTERFACE_Y, "bundle.contract.y".to_owned(), bundle_id)
                .expect("second registration should succeed");
        }

        let slots: Vec<u32> = registry.find_slots_by_bundle(bundle_id);
        assert_eq!(slots.len(), 2, "both slots should be found for the bundle");
    }

    #[test]
    fn concurrent_generation_reads() {
        let registry: Registry = Registry::new();
        let descriptor: PluginDescriptor =
            make_descriptor("concurrent_test", "concurrent.contract");
        // SAFETY: MOCK_INTERFACE is 'static, pointer is valid for Registry lifetime.
        let handle: PluginHandle = unsafe {
            registry.register(
                descriptor,
                &MOCK_INTERFACE,
                "concurrent.contract".to_owned(),
                999u64,
            )
        }
        .expect("registration should succeed");

        let data: std::sync::RwLockReadGuard<'_, RegistryData> =
            registry.data.read().unwrap_or_else(|e| e.into_inner());
        let gen1: u32 = data.slots[handle.index as usize]
            .generation
            .load(Ordering::Acquire);
        let gen2: u32 = data.slots[handle.index as usize]
            .generation
            .load(Ordering::Acquire);
        assert_eq!(gen1, gen2, "consecutive loads should return same value");
        assert_eq!(
            gen1, handle.generation,
            "loaded generation should match handle"
        );
    }

    #[test]
    fn generation_increment_during_swap() {
        let registry: Registry = Registry::new();
        let descriptor: PluginDescriptor = make_descriptor("swap_test", "swap.test.contract");
        // SAFETY: MOCK_INTERFACE is 'static, pointer is valid for Registry lifetime.
        let handle: PluginHandle = unsafe {
            registry.register(
                descriptor,
                &MOCK_INTERFACE,
                "swap.test.contract".to_owned(),
                888u64,
            )
        }
        .expect("registration should succeed");

        static NEW_INTERFACE: PluginInterface = PluginInterface {
            rt_ctx: core::ptr::null(),
            contract_id: 0x1234_5678_9ABC_DEF0,
            contract_version: (3 << 16),
            function_count: 0,
            dispatch_type: DispatchType::Native,
            dispatch: PluginDispatch {
                native: NativeDispatch {
                    functions: MOCK_FNS.as_ptr(),
                },
            },
        };

        let original_gen: u32 = handle.generation;
        let new_arc: Arc<VTableSlot> = Arc::new(VTableSlot(&NEW_INTERFACE));
        let _: Arc<VTableSlot> = registry
            .swap_vtable(handle.index, new_arc)
            .expect("swap_vtable should succeed");

        let result: Result<*const PluginInterface, RegistryError> = registry.resolve(handle);
        assert!(
            matches!(result, Err(RegistryError::StaleHandle { .. })),
            "old handle should be stale after generation bump"
        );

        let data: std::sync::RwLockReadGuard<'_, RegistryData> =
            registry.data.read().unwrap_or_else(|e| e.into_inner());
        let new_gen: u32 = data.slots[handle.index as usize]
            .generation
            .load(Ordering::Acquire);
        assert_eq!(
            new_gen,
            original_gen.wrapping_add(1_u32),
            "generation should increment by 1"
        );
    }
}

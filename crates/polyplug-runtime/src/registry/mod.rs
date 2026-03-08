//! Registry — VTable storage and plugin handle management.
//!
//! Implements a generational index registry: each slot holds a generation counter.
//! PluginHandle validation compares handle.generation against slot.generation to
//! detect use-after-unload (stale handle detection).

use std::collections::HashMap;
use std::sync::RwLock;

use crate::abi::PluginDescriptor;
use crate::abi::PluginHandle;
use crate::abi::PluginVTable;
use crate::error::RegistryError;

/// A single slot in the registry's storage array.
//
//  Implements the generational index pattern.
//  generation is incremented each time the slot is vacated (plugin unloaded).
pub(crate) struct RegistrySlot {
    /// Current generation counter. Compared against PluginHandle::generation.
    pub generation: u32,
    /// Slot contents — None if vacant (after unload).
    pub entry: Option<RegistryEntry>,
}

/// Live plugin registration data.
#[allow(dead_code)]
pub(crate) struct RegistryEntry {
    /// Plugin metadata. StringView fields are 'static (Library is never dropped).
    pub descriptor: PluginDescriptor,
    /// Pointer to the plugin's vtable. 'static, never freed.
    pub vtable: *const PluginVTable,
    /// Full contract name string for collision detection.
    pub contract_name: String,
}

// SAFETY: RegistryEntry contains raw pointers into library memory that is 'static
// (the library is never dropped per the never-drop invariant). These pointers are
// only written once (at registration) and only read after that, making concurrent
// access safe once the write lock is released.
unsafe impl Send for RegistryEntry {}
// SAFETY: RegistryEntry contains raw pointers into library memory that is 'static
// (the library is never dropped per the never-drop invariant). These pointers are
// only written once (at registration) and only read after that, making concurrent
// access safe once the write lock is released.
unsafe impl Sync for RegistryEntry {}

/// Thread-safe plugin registry.
//
//  slots: RwLock protects all writes (registration/unload).
//  Reads (find_plugin, call_plugin) take a read guard and are concurrent.
//  contract_index maps contract_id -> slot index for O(1) lookup.
pub struct Registry {
    slots: RwLock<Vec<RegistrySlot>>,
    /// Maps contract_id (FNV-1a u64) to the index of the registered slot.
    contract_index: RwLock<HashMap<u64, u32>>,
}

// SAFETY: Registry uses RwLock internally, making it safe to share across threads.
unsafe impl Send for Registry {}
// SAFETY: Registry uses RwLock internally, making it safe to share across threads.
// All interior mutability is protected by the RwLock.
unsafe impl Sync for Registry {}

impl Registry {
    /// Create an empty registry.
    pub fn new() -> Registry {
        Registry {
            slots: RwLock::new(Vec::new()),
            contract_index: RwLock::new(HashMap::new()),
        }
    }

    /// Register a plugin vtable.
    //
    //  Returns Err if:
    //  - contract_id is already registered to a DIFFERENT contract_name (collision)
    //  - contract_id is already registered to the SAME contract_name (duplicate provider)
    pub fn register(
        &self,
        descriptor: PluginDescriptor,
        vtable: *const PluginVTable,
        contract_name: String,
        contract_id: u64,
    ) -> Result<PluginHandle, RegistryError> {
        let mut slots: std::sync::RwLockWriteGuard<'_, Vec<RegistrySlot>> =
            self.slots.write().unwrap_or_else(|e| e.into_inner());
        let mut index_map: std::sync::RwLockWriteGuard<'_, HashMap<u64, u32>> = self
            .contract_index
            .write()
            .unwrap_or_else(|e| e.into_inner());

        // Check for contract_id collision or duplicate provider
        if let Some(&existing_idx) = index_map.get(&contract_id) {
            let existing_slot: &RegistrySlot = &slots[existing_idx as usize];
            if let Some(ref existing_entry) = existing_slot.entry {
                if existing_entry.contract_name != contract_name {
                    return Err(RegistryError::ContractIdCollision {
                        id: contract_id,
                        name_a: existing_entry.contract_name.clone(),
                        name_b: contract_name,
                    });
                } else {
                    return Err(RegistryError::DuplicateProvider {
                        contract: contract_name,
                        existing: existing_entry.contract_name.clone(),
                    });
                }
            }
        }

        // Find a vacant slot or push a new one
        let slot_idx: u32 = slots
            .iter()
            .position(|s| s.entry.is_none())
            .map(|i| i as u32)
            .unwrap_or_else(|| {
                let new_idx: u32 = slots.len() as u32;
                slots.push(RegistrySlot {
                    generation: 0,
                    entry: None,
                });
                new_idx
            });

        let slot: &mut RegistrySlot = &mut slots[slot_idx as usize];
        let generation: u32 = slot.generation;
        slot.entry = Some(RegistryEntry {
            descriptor,
            vtable,
            contract_name,
        });
        index_map.insert(contract_id, slot_idx);

        Ok(PluginHandle {
            index: slot_idx,
            generation,
        })
    }

    /// Find a plugin by contract_id and minimum version.
    //
    //  min_version encoding: (minor << 16 | patch), same as PluginVTable::contract_version.
    //  Pass 0 to accept any version.
    //
    //  Returns Err(PluginNotFound) if no plugin satisfies the contract + version.
    pub fn find(&self, contract_id: u64, min_version: u32) -> Result<PluginHandle, RegistryError> {
        let slots: std::sync::RwLockReadGuard<'_, Vec<RegistrySlot>> =
            self.slots.read().unwrap_or_else(|e| e.into_inner());
        let index_map: std::sync::RwLockReadGuard<'_, HashMap<u64, u32>> = self
            .contract_index
            .read()
            .unwrap_or_else(|e| e.into_inner());

        let slot_idx: u32 = match index_map.get(&contract_id) {
            Some(&idx) => idx,
            None => {
                return Err(RegistryError::PluginNotFound {
                    contract_id,
                    min_version,
                });
            }
        };

        let slot: &RegistrySlot = &slots[slot_idx as usize];
        let entry: &RegistryEntry = match slot.entry {
            Some(ref e) => e,
            None => {
                return Err(RegistryError::PluginNotFound {
                    contract_id,
                    min_version,
                });
            }
        };

        // Check version constraint
        // SAFETY: entry.vtable points to a 'static PluginVTable (never dropped, per §7.3).
        let vtable_version: u32 = unsafe { (*entry.vtable).contract_version };
        if vtable_version < min_version {
            return Err(RegistryError::PluginNotFound {
                contract_id,
                min_version,
            });
        }

        Ok(PluginHandle {
            index: slot_idx,
            generation: slot.generation,
        })
    }

    /// Validate a PluginHandle and return its vtable pointer.
    //
    //  Returns Err(StaleHandle) if the handle's generation doesn't match the slot.
    //  Returns Err(PluginNotFound) if the slot is vacant.
    pub fn resolve(&self, handle: PluginHandle) -> Result<*const PluginVTable, RegistryError> {
        let slots: std::sync::RwLockReadGuard<'_, Vec<RegistrySlot>> =
            self.slots.read().unwrap_or_else(|e| e.into_inner());

        let slot_idx: usize = handle.index as usize;
        if slot_idx >= slots.len() {
            return Err(RegistryError::StaleHandle {
                index: handle.index,
                expected: handle.generation,
                found: 0,
            });
        }

        let slot: &RegistrySlot = &slots[slot_idx];
        if slot.generation != handle.generation {
            return Err(RegistryError::StaleHandle {
                index: handle.index,
                expected: handle.generation,
                found: slot.generation,
            });
        }

        match slot.entry {
            Some(ref entry) => Ok(entry.vtable),
            None => Err(RegistryError::StaleHandle {
                index: handle.index,
                expected: handle.generation,
                found: slot.generation,
            }),
        }
    }
}

impl Default for Registry {
    fn default() -> Registry {
        Registry::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::ABI_OK;
    use crate::abi::PluginVTable;
    use crate::abi::StringView;

    const MOCK_FNS: [*const (); 0] = [];

    static MOCK_VTABLE: PluginVTable = PluginVTable {
        contract_id: 0x1234_5678_9ABC_DEF0,
        contract_version: (1 << 16) | 0, // minor=1, patch=0
        function_count: 0,
        functions: MOCK_FNS.as_ptr(),
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
        let handle: PluginHandle = registry
            .register(
                descriptor,
                &MOCK_VTABLE,
                "image.decode".to_owned(),
                0x1234_5678_9ABC_DEF0,
            )
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
        let handle: PluginHandle = registry
            .register(
                descriptor,
                &MOCK_VTABLE,
                "audio.decode".to_owned(),
                0xDEAD_BEEF_1234_5678,
            )
            .expect("registration should succeed");

        // Use a handle with wrong generation
        let stale: PluginHandle = PluginHandle {
            index: handle.index,
            generation: handle.generation + 1,
        };
        let result: Result<*const PluginVTable, RegistryError> = registry.resolve(stale);
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
        let contract_id: u64 = 0xAAAA_BBBB_CCCC_DDDD;

        registry
            .register(d1, &MOCK_VTABLE, "image.decode".to_owned(), contract_id)
            .expect("first registration should succeed");

        let result: Result<PluginHandle, RegistryError> =
            registry.register(d2, &MOCK_VTABLE, "image.decode".to_owned(), contract_id);
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
        // Same hash, different names (simulated collision)
        let contract_id: u64 = 0xC0FFEE_BABE_1234;

        registry
            .register(d1, &MOCK_VTABLE, "contract.a".to_owned(), contract_id)
            .expect("first registration should succeed");

        let result: Result<PluginHandle, RegistryError> =
            registry.register(d2, &MOCK_VTABLE, "contract.b".to_owned(), contract_id);
        assert!(
            matches!(result, Err(RegistryError::ContractIdCollision { .. })),
            "expected ContractIdCollision error"
        );
    }

    #[test]
    fn resolve_returns_vtable_pointer() {
        let registry: Registry = Registry::new();
        let descriptor: PluginDescriptor = make_descriptor("test_plugin", "test.contract");
        let handle: PluginHandle = registry
            .register(
                descriptor,
                &MOCK_VTABLE,
                "test.contract".to_owned(),
                0x1111_2222_3333_4444,
            )
            .expect("registration should succeed");

        let vtable_ptr: *const PluginVTable =
            registry.resolve(handle).expect("resolve should succeed");
        // SAFETY: vtable_ptr points to MOCK_VTABLE which is 'static.
        let contract_id: u64 = unsafe { (*vtable_ptr).contract_id };
        assert_eq!(contract_id, MOCK_VTABLE.contract_id);
        // Suppress unused import warning from ABI_OK in scope
        let _: u32 = ABI_OK;
    }
}

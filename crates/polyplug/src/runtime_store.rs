//! RuntimeStore — interface storage and contract handle management.
//!
//! Simple index-based registry: each slot holds an interface pointer.
//! GuestContractHandle validation checks for out-of-bounds indices only.
//! Hosts must destroy instances before hot-reload via callback.
//!
//! Multi-impl support: different bundles may register different implementations of
//! the same contract. guest_contract_index maps contract_id -> Vec<slot_index> to support
//! find_all_guest_contracts(). DuplicateProvider is only raised when the SAME bundle_id
//! tries to register the SAME contract_id twice (outside a reload window).

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::RwLock;

use polyplug_abi::RuntimeLanguage;
use polyplug_abi::dispatch::dispatch_type::DispatchType;
use polyplug_abi::types::Version;
use polyplug_abi::{
    GuestContractHandle, GuestContractInstance, GuestContractInterface, HostApi, PluginDescriptor,
};
use polyplug_utils::{BundleId, GuestContractId};

use crate::error::RegistryError;
use crate::logger::{LoggerHandle, RecoverPoisoned, RecoveringGuard};

/// Outcome of [`RuntimeStore::resolve_single_provider`] — the single-read-guard
/// primitive behind the `call_guest_method` HostApi cross-dispatch path.
///
/// The cross-call must, under ONE read guard, (1) count live providers for the
/// contract, (2) reject ambiguous multi-provider routing, and (3) resolve the
/// sole provider's interface pointer. Splitting that across `count` + `find` +
/// `resolve` took three separate read-guard acquisitions; this enum lets the
/// store do all three under a single guard and hand the caller exactly the
/// information it needs to reproduce the original observable behaviour.
pub enum SingleProviderResolution {
    /// No live provider matched the contract id at the requested version floor.
    /// The caller maps this to the same `NotFound` outcome the former
    /// `find_guest_contract` not-found path produced.
    NotFound,
    /// More than one live provider is registered for the contract. Routing keys
    /// solely on `contract_id`, so the target is ambiguous and the caller must
    /// refuse with `DuplicateProvider` (it never dispatches in this case).
    Multiple,
    /// Exactly one live provider matched; the contained pointer is its interface,
    /// borrowed from the slot's `Arc` (valid for the runtime lifetime under the
    /// retire-not-drop model, exactly as `resolve_guest_contract` returns).
    Resolved(*const GuestContractInterface),
}

/// Built-in stateless `create_instance` stub.
///
/// Some guest runtimes cannot express a function pointer that returns the
/// 16-byte `GuestContractInstance` by value — notably the Python (ctypes) guest
/// generator, whose callbacks may not return a struct by value. Such generators
/// emit a null `create_instance` pointer. The registry substitutes this stub so
/// every host caller can safely call `create_instance` on any registered
/// interface. It returns the canonical stateless dispatch token (null data).
unsafe extern "C" fn stateless_create_instance(
    _host: *const HostApi,
    _args: *const (),
) -> GuestContractInstance {
    GuestContractInstance::null()
}

/// Built-in no-op `destroy_instance` stub, paired with `stateless_create_instance`.
unsafe extern "C" fn stateless_destroy_instance(
    _host: *const HostApi,
    _instance: GuestContractInstance,
) {
}

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

/// Copy a `StringView`'s bytes into an owned, strictly-validated UTF-8 `String`.
///
/// Plugin-provided names key the registry, so a lossy conversion could silently
/// replace invalid bytes with U+FFFD and alias two distinct names. Invalid UTF-8
/// is therefore rejected with [`RegistryError::InvalidUtf8`] rather than coerced.
///
/// # Safety
/// `sv.ptr` must be valid for `sv.len` bytes for the duration of this call, or be null.
unsafe fn string_view_to_owned_string(
    sv: &polyplug_abi::types::StringView,
    context: &str,
) -> Result<String, RegistryError> {
    if sv.ptr.is_null() || sv.len == 0 {
        return Ok(String::new());
    }
    // SAFETY: caller guarantees ptr/len describe a valid byte range for this call.
    let bytes: &[u8] = unsafe { core::slice::from_raw_parts(sv.ptr, sv.len) };
    match core::str::from_utf8(bytes) {
        Ok(s) => Ok(s.to_owned()),
        Err(_) => Err(RegistryError::InvalidUtf8 {
            context: context.to_owned(),
        }),
    }
}

/// Owned copy of a plugin's [`PluginDescriptor`] for the registry to retain.
///
/// The ABI [`PluginDescriptor`] carries borrowed `StringView`s (`name`,
/// `contract_name`) that point into the plugin's transient init-time buffers —
/// some generators (e.g. C#) free those buffers as soon as `register_guest_contract`
/// returns. Retaining the raw descriptor would therefore dangle. This struct
/// owns the string data so introspection stays valid for the registry's lifetime.
#[derive(Debug, Clone)]
pub struct OwnedPluginDescriptor {
    /// Human-readable plugin name (owned copy of `PluginDescriptor.name`).
    pub name: String,
    /// Full contract name (owned copy of `PluginDescriptor.contract_name`).
    pub contract_name: String,
    /// Plugin version (a plain `Copy` value — no borrowed data).
    pub version: Version,
}

/// Live plugin registration data.
pub(crate) struct PluginEntry {
    /// Plugin metadata — owned so no borrowed `StringView`s are retained.
    /// Used for registry introspection.
    pub descriptor: OwnedPluginDescriptor,
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
    /// Monotonic generation counter for stale-handle detection.
    ///
    /// Stamped into every [`GuestContractHandle`] minted against this slot. Server-side
    /// state only — never part of the ABI. The unload feature bumps this whenever the
    /// slot is retired so a handle minted against an earlier generation is recognised as
    /// stale even after the index is recycled by a later registration. Starts at 0.
    pub generation: u32,
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
    /// Interface `Arc`s retired by hot-reload swaps, kept alive for the lifetime
    /// of the runtime.
    ///
    /// `resolve_guest_contract` hands out a raw `*const GuestContractInterface`
    /// borrowed from the slot's `Arc`. A concurrent reload that swaps the slot
    /// would otherwise drop the old `Arc` and free the interface struct while a
    /// reader still dereferences that pointer — a use-after-free. Retaining the
    /// old `Arc` here keeps the interface memory valid for any in-flight reader.
    /// This mirrors the documented hot-reload guarantee that the old vtable is
    /// held alive until all in-flight calls complete (TRUST_MODEL.md §Hot-Reload
    /// Safety Guarantees).
    retired_interfaces: Vec<Arc<GuestContractInterface>>,
    /// Bundles currently inside a hot-reload's re-init phase.
    ///
    /// While a bundle id is present here, contracts registered by that bundle are
    /// "pending": their slot is created (and tracked in `bundle_data.plugin_slots`)
    /// but NOT published into `guest_contract_index`, so `find`/`find_all` readers
    /// never observe a transient second live slot per contract during the window
    /// between `loader.reload()` (which re-runs init, registering fresh slots) and
    /// `apply_reload_swap` (which reconciles). `apply_reload_swap` moves each new
    /// interface into the already-published old slot and retires the pending slot,
    /// so the contract stays single-slot throughout.
    reloading_bundles: HashSet<BundleId>,
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
            retired_interfaces: Vec::new(),
            reloading_bundles: HashSet::new(),
        }
    }

    /// Tear down a single published slot — the one canonical teardown atom.
    ///
    /// This is the SOURCE OF TRUTH for retiring a slot. Both `invalidate_bundle`
    /// (unload) and the dropped-contract branch of `apply_reload_swap` (when the
    /// reloaded version no longer provides a contract the old version had) route
    /// through here so the teardown semantics stay identical (DRY).
    ///
    /// For the slot at `slot_idx` it:
    /// - bumps `slot.generation` (so every handle minted against the old generation
    ///   now resolves to [`RegistryError::StaleHandle`]);
    /// - **retires** the slot's interface `Arc` into `retired_interfaces` rather than
    ///   dropping it (retire-not-drop), so any raw `*const GuestContractInterface`
    ///   already handed out by `resolve_guest_contract` stays valid for the runtime
    ///   lifetime;
    /// - clears the slot `entry`;
    /// - removes the slot index from `guest_contract_index` for its contract_id,
    ///   dropping the now-empty key.
    ///
    /// It deliberately does NOT touch `bundle_data` — bundle-level bookkeeping is the
    /// caller's responsibility because it differs per caller (unload removes the whole
    /// bundle entry, reload removes a single slot index). An out-of-bounds `slot_idx`
    /// is a no-op.
    ///
    /// Note: reload's surviving-contract path performs an in-place interface swap
    /// instead of calling this helper — that path keeps the slot live (same
    /// generation) so a handle stays resolvable to the new interface. Routing it
    /// through `retire_slot` would break that continuity guarantee.
    ///
    /// Returns `Some(Arc::clone(&retired))` — an extra reference to the interface
    /// `Arc` that was just retired — so callers (notably `invalidate_bundle`) can
    /// inspect its `Arc::strong_count` to decide whether the underlying library can
    /// be safely reclaimed. Returns `None` when the slot was out of bounds or already
    /// held no interface.
    fn retire_slot(&mut self, slot_idx: u32) -> Option<Arc<GuestContractInterface>> {
        let slot_idx_usize: usize = slot_idx as usize;
        if slot_idx_usize >= self.slots.len() {
            return None;
        }

        // Read contract_id before clearing the slot.
        let contract_id: Option<GuestContractId> = self.slots[slot_idx_usize]
            .interface
            .as_ref()
            .map(|arc: &Arc<GuestContractInterface>| arc.contract_id);

        // Remove the slot index from guest_contract_index, dropping now-empty keys.
        if let Some(cid) = contract_id
            && let Some(indices) = self.guest_contract_index.get_mut(&cid)
        {
            indices.retain(|&idx| idx != slot_idx);
            if indices.is_empty() {
                self.guest_contract_index.remove(&cid);
            }
        }

        // Bump generation so old handles go stale, retire (never drop) the interface
        // so raw pointers stay valid, and vacate the slot entry.
        self.slots[slot_idx_usize].generation =
            self.slots[slot_idx_usize].generation.wrapping_add(1);
        let retired: Option<Arc<GuestContractInterface>> =
            self.slots[slot_idx_usize].interface.take();
        self.slots[slot_idx_usize].entry = None;
        match retired {
            Some(arc) => {
                // Hand the caller an extra reference (for strong_count inspection)
                // while the original is held alive in `retired_interfaces`.
                let returned: Arc<GuestContractInterface> = Arc::clone(&arc);
                self.retired_interfaces.push(arc);
                Some(returned)
            }
            None => None,
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
    /// Instance-owned copy of the host logging configuration. The store has no
    /// back-reference to its `Runtime`, so the handle is copied in at
    /// construction (Rule 12: instance state, no globals).
    logger: LoggerHandle,
}

impl RuntimeStore {
    /// Create an empty registry with the default (stderr Error/Warn) logger.
    pub fn new() -> RuntimeStore {
        RuntimeStore {
            data: RwLock::new(RuntimeStoreData::new()),
            logger: LoggerHandle::default_stderr(),
        }
    }

    /// Create an empty registry that logs through the given handle.
    pub(crate) fn with_logger(logger: LoggerHandle) -> RuntimeStore {
        RuntimeStore {
            data: RwLock::new(RuntimeStoreData::new()),
            logger,
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
    //  - contract_id is already registered by the SAME bundle_id and the bundle is not
    //    mid-reload (duplicate provider)
    //
    //  Different bundles MAY register the same contract_id (multi-impl).
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

        // The plugin is untrusted: `dispatch_type` is a `#[repr(u32)]` enum but the
        // plugin-provided struct can hold any 32-bit pattern. Materializing an
        // out-of-range value as the enum would be UB, so read the field as a raw
        // `u32` and validate it via the total `DispatchType::from_u32` before use.
        let dispatch_type: DispatchType = {
            // SAFETY: interface_ptr is a valid 'static GuestContractInterface (ABI
            // contract). We read the 4-byte `dispatch_type` field as a raw `u32`
            // (never as the typed enum) so an out-of-range value is observed soundly.
            let raw: u32 = unsafe {
                core::ptr::read(core::ptr::addr_of!((*interface_ptr).dispatch_type) as *const u32)
            };
            match DispatchType::from_u32(raw) {
                Some(dt) => dt,
                None => return Err(RegistryError::InvalidDispatchType { value: raw }),
            }
        };

        // Validate and copy the descriptor's borrowed `name` into an owned String
        // BEFORE touching any registry state, so a rejected (invalid UTF-8) name
        // leaves no slot behind. The StringView is valid for the whole call (the
        // plugin owns the backing buffer during polyplug_init); we only read it.
        // SAFETY: `descriptor.name` describes a valid byte range for this call.
        let owned_name: String =
            unsafe { string_view_to_owned_string(&descriptor.name, "PluginDescriptor.name")? };

        let mut data: RecoveringGuard<std::sync::RwLockWriteGuard<'_, RuntimeStoreData>> =
            self.data.write().recover_poisoned(self.logger, "store");

        // During a reload window the bundle legitimately re-registers its own
        // contracts into fresh (pending) slots; that is re-init, not a duplicate.
        // Outside the window, a second registration of the SAME contract_id by the
        // SAME bundle_id is the DuplicateProvider case the module contract promises
        // to reject.
        let is_reloading: bool = data.reloading_bundles.contains(&bundle_id);

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
                    // Same bundle, same contract, NOT mid-reload — duplicate provider.
                    if existing_entry.bundle_id == bundle_id && !is_reloading {
                        return Err(RegistryError::DuplicateProvider {
                            contract: contract_name,
                            existing: existing_entry.descriptor.name.clone(),
                        });
                    }
                    // Different bundle, same contract — allowed (multi-impl support).
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
                    generation: 0,
                });
                new_idx
            });

        let owned_descriptor: OwnedPluginDescriptor = OwnedPluginDescriptor {
            name: owned_name,
            contract_name: contract_name.clone(),
            version: descriptor.version,
        };

        let slot: &mut PluginSlot = &mut data.slots[slot_idx as usize];
        let slot_generation: u32 = slot.generation;
        slot.entry = Some(PluginEntry {
            descriptor: owned_descriptor,
            contract_name,
            bundle_id,
        });
        // Copy the interface into an Arc for shared ownership, substituting
        // built-in stateless stubs for any null instance-lifecycle pointer.
        //
        // Guest generators that cannot emit a struct-returning callback (e.g.
        // Python/ctypes, whose callbacks may not return the 16-byte
        // GuestContractInstance by value) leave create_instance / destroy_instance
        // as null. The ABI field type is a *non-nullable* `fn` pointer, so a null
        // must never be materialized as a typed `fn` value (that would be UB).
        // The lifecycle pointers are therefore read as raw `*const ()` directly
        // from the source struct, checked for null, and only then transmuted back
        // to typed fn pointers.
        let interface: GuestContractInterface = unsafe {
            // SAFETY: interface_ptr is a valid 'static GuestContractInterface.
            // We read the two function-pointer fields as raw pointers (never as
            // typed `fn`) so a null value is observed soundly.
            let create_raw: *const () = core::ptr::read(core::ptr::addr_of!(
                (*interface_ptr).create_instance
            ) as *const *const ());
            let destroy_raw: *const () = core::ptr::read(core::ptr::addr_of!(
                (*interface_ptr).destroy_instance
            ) as *const *const ());

            let create_instance: unsafe extern "C" fn(
                *const HostApi,
                *const (),
            ) -> GuestContractInstance = if create_raw.is_null() {
                stateless_create_instance
            } else {
                // SAFETY: non-null pointer to a valid create_instance per ABI.
                core::mem::transmute::<
                    *const (),
                    unsafe extern "C" fn(*const HostApi, *const ()) -> GuestContractInstance,
                >(create_raw)
            };
            let destroy_instance: unsafe extern "C" fn(*const HostApi, GuestContractInstance) =
                if destroy_raw.is_null() {
                    stateless_destroy_instance
                } else {
                    // SAFETY: non-null pointer to a valid destroy_instance per ABI.
                    core::mem::transmute::<
                        *const (),
                        unsafe extern "C" fn(*const HostApi, GuestContractInstance),
                    >(destroy_raw)
                };

            // SAFETY: the remaining POD fields are read from the same valid
            // struct; the (possibly substituted) lifecycle fns are sound.
            GuestContractInterface {
                contract_id: (*interface_ptr).contract_id,
                contract_version: (*interface_ptr).contract_version,
                dispatch_type,
                create_instance,
                destroy_instance,
                dispatch: core::ptr::read(core::ptr::addr_of!((*interface_ptr).dispatch)),
            }
        };
        slot.interface = Some(Arc::new(interface));

        // Publish into guest_contract_index UNLESS this bundle is mid-reload. During a
        // reload the freshly-registered slot is "pending": keeping it out of the find
        // index prevents readers from transiently seeing two live slots per contract.
        // apply_reload_swap later moves the interface into the already-published old
        // slot and retires this pending slot, so the index is never double-populated.
        // `is_reloading` was computed above (the reload set is not mutated in between).
        if !is_reloading {
            data.guest_contract_index
                .entry(contract_id)
                .or_default()
                .push(slot_idx);
        }

        // Update bundle_data: push slot_idx into plugin_slots Vec for this bundle_id.
        // Note: descriptor is populated separately via register_bundle_metadata().
        data.bundle_data
            .entry(bundle_id)
            .or_insert_with(|| BundleData {
                plugin_slots: Vec::new(),
                descriptor: BundleDescriptor {
                    id: bundle_id,
                    name: String::new(),
                    version: Version {
                        major: 0,
                        minor: 0,
                        patch: 0,
                    },
                    runtime: RuntimeLanguage::Rust,
                    file_path: PathBuf::new(),
                    dependencies: Vec::new(),
                },
            })
            .plugin_slots
            .push(slot_idx);

        Ok(GuestContractHandle {
            index: slot_idx,
            generation: slot_generation,
        })
    }

    /// Declare dependency contract_ids for a bundle.
    ///
    /// Must be called before the bundle resolves any cross-bundle contracts.
    /// Prevents undeclared dependency resolution at runtime.
    pub fn declare_bundle_dependencies(
        &self,
        bundle_id: BundleId,
        contract_ids: Vec<GuestContractId>,
    ) -> Result<(), RegistryError> {
        let mut data: RecoveringGuard<std::sync::RwLockWriteGuard<'_, RuntimeStoreData>> =
            self.data.write().recover_poisoned(self.logger, "store");
        let set: &mut HashSet<GuestContractId> =
            data.bundle_declared_deps.entry(bundle_id).or_default();
        for cid in contract_ids {
            set.insert(cid);
        }
        Ok(())
    }

    /// Returns true if `bundle_id` has declared `contract_id` as a dependency.
    pub(crate) fn is_bundle_dependency_declared(
        &self,
        bundle_id: BundleId,
        contract_id: GuestContractId,
    ) -> bool {
        let data: RecoveringGuard<std::sync::RwLockReadGuard<'_, RuntimeStoreData>> =
            self.data.read().recover_poisoned(self.logger, "store");
        data.bundle_declared_deps
            .get(&bundle_id)
            .is_some_and(|s| s.contains(&contract_id))
    }

    /// Find any registered plugin satisfying the given contract_id and minimum version.
    //
    //  `min_version` is a MAJOR-version floor: returns the first slot whose
    //  `interface.contract_version.major >= min_version`. Pass min_version=0 to accept
    //  any version. (The contract_id hash already pins the major; minor-level
    //  requirements are enforced at manifest validation, not here.)
    pub fn find_guest_contract(
        &self,
        contract_id: GuestContractId,
        min_version: u32,
    ) -> Result<GuestContractHandle, RegistryError> {
        let data: RecoveringGuard<std::sync::RwLockReadGuard<'_, RuntimeStoreData>> =
            self.data.read().recover_poisoned(self.logger, "store");

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
                    return Ok(GuestContractHandle {
                        index: slot_idx,
                        generation: slot.generation,
                    });
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
        let data: RecoveringGuard<std::sync::RwLockReadGuard<'_, RuntimeStoreData>> =
            self.data.read().recover_poisoned(self.logger, "store");

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

        // While the bundle is mid-reload, freshly-registered slots are "pending":
        // they live in `plugin_slots` but are deliberately kept out of
        // `guest_contract_index` until `apply_reload_swap` reconciles them. Minting a
        // handle to a pending slot would be unsound — `abort_reload` drops that slot's
        // Arc (it was never published), so a previously-resolved raw pointer would
        // dangle. During the window, therefore, only match slot indices that are
        // actually published in `guest_contract_index` for this contract.
        let is_reloading: bool = data.reloading_bundles.contains(&bundle_id);
        let published_for_contract: Option<&Vec<u32>> = data.guest_contract_index.get(&contract_id);

        // Find the slot matching contract_id and version
        for &slot_idx in slot_indices.iter() {
            if is_reloading
                && !published_for_contract.is_some_and(|published| published.contains(&slot_idx))
            {
                // Pending (unpublished) slot during a reload window — skip it.
                continue;
            }
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
                    return Ok(GuestContractHandle {
                        index: slot_idx,
                        generation: slot.generation,
                    });
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
        let data: RecoveringGuard<std::sync::RwLockReadGuard<'_, RuntimeStoreData>> =
            self.data.read().recover_poisoned(self.logger, "store");

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
                    out[write_count] = GuestContractHandle {
                        index: slot_idx,
                        generation: slot.generation,
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
    /// This is an optimized version of `find_all_guest_contracts` that avoids
    /// intermediate allocation by packing handles directly during iteration.
    /// Each handle is packed as: `(generation as u64) << 32 | index as u64`,
    /// matching [`GuestContractHandle::pack`].
    ///
    /// Returns the number of packed handles written to `out`.
    pub fn find_all_guest_contracts_packed(
        &self,
        contract_id: GuestContractId,
        min_version: u32,
        out: &mut [u64],
    ) -> usize {
        let data: RecoveringGuard<std::sync::RwLockReadGuard<'_, RuntimeStoreData>> =
            self.data.read().recover_poisoned(self.logger, "store");

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
                    // Pack handle directly: generation in the high 32 bits, index low.
                    out[write_count] = ((slot.generation as u64) << 32) | (slot_idx as u64);
                    write_count += 1usize;
                }
            }
        }
        write_count
    }

    /// Count plugins satisfying the given contract_id and minimum version.
    pub fn count_guest_contracts(&self, contract_id: GuestContractId, min_version: u32) -> usize {
        let data: RecoveringGuard<std::sync::RwLockReadGuard<'_, RuntimeStoreData>> =
            self.data.read().recover_poisoned(self.logger, "store");

        let indices = match data.guest_contract_index.get(&contract_id) {
            Some(v) => v,
            None => return 0,
        };

        let mut count = 0;
        for &slot_idx in indices.iter() {
            let slot = &data.slots[slot_idx as usize];
            if slot.entry.is_some()
                && let Some(ref interface) = slot.interface
                && interface.contract_version.major >= min_version
            {
                count += 1;
            }
        }
        count
    }

    /// Count live providers for `contract_id` (a MAJOR-version floor) and, when
    /// exactly one exists, resolve its interface pointer — all under a SINGLE read
    /// guard.
    ///
    /// This is the cross-call primitive behind the `call_guest_method` HostApi
    /// callback. It folds what was previously three separate read-guard
    /// acquisitions (`count_guest_contracts` + `find_guest_contract` +
    /// `resolve_guest_contract`) into one, eliminating two lock round-trips per
    /// cross-call while preserving the exact observable outcomes:
    /// - **0 live providers** → [`SingleProviderResolution::NotFound`] (the same
    ///   outcome the old `find_guest_contract` not-found path produced).
    /// - **>1 live provider** → [`SingleProviderResolution::Multiple`]: routing keys
    ///   only on `contract_id`, so the target is ambiguous; the caller refuses.
    /// - **exactly 1** → [`SingleProviderResolution::Resolved`] with the sole
    ///   provider's interface pointer, identical to resolving the handle the old
    ///   `find_guest_contract` returned (the first live matching slot).
    ///
    /// The liveness + version filtering mirrors `count_guest_contracts` /
    /// `find_guest_contract` exactly (slot entry present, interface present,
    /// `interface.contract_version.major >= min_version`). The returned pointer is
    /// borrowed from the slot's `Arc`; under the retire-not-drop model it stays valid
    /// for the runtime lifetime even across a concurrent reload, matching
    /// `resolve_guest_contract`'s guarantee.
    pub fn resolve_single_provider(
        &self,
        contract_id: GuestContractId,
        min_version: u32,
    ) -> SingleProviderResolution {
        let data: RecoveringGuard<std::sync::RwLockReadGuard<'_, RuntimeStoreData>> =
            self.data.read().recover_poisoned(self.logger, "store");

        let indices: &Vec<u32> = match data.guest_contract_index.get(&contract_id) {
            Some(v) => v,
            None => return SingleProviderResolution::NotFound,
        };

        // Single pass: count live matches and remember the first one's interface
        // pointer. `count_guest_contracts` and `find_guest_contract` (min_version=0)
        // both walk `indices` with the same liveness + version filter, so iterating
        // once here is behaviour-identical to running them in sequence — the first
        // match is exactly the slot `find_guest_contract` would have returned.
        let mut count: usize = 0;
        let mut first_interface: *const GuestContractInterface = core::ptr::null();
        for &slot_idx in indices.iter() {
            let slot: &PluginSlot = &data.slots[slot_idx as usize];
            if slot.entry.is_some()
                && let Some(ref interface) = slot.interface
                && interface.contract_version.major >= min_version
            {
                if count == 0 {
                    first_interface = interface.as_ref() as *const GuestContractInterface;
                }
                count += 1;
                if count > 1 {
                    // Ambiguous: no need to scan further, the cross-call is refused.
                    return SingleProviderResolution::Multiple;
                }
            }
        }

        if count == 1 {
            SingleProviderResolution::Resolved(first_interface)
        } else {
            SingleProviderResolution::NotFound
        }
    }

    /// Count AND collect every live provider for `contract_id` at or above
    /// `min_version` (a MAJOR-version floor) under a SINGLE read guard.
    ///
    /// This is the allocation-safe primitive behind the `find_all_guest_contracts`
    /// HostApi callback. Counting and collecting in two separate guards is unsound:
    /// a concurrent unload that shrinks the registry between the two acquisitions
    /// would leave the caller allocating for a stale count but filling fewer
    /// handles, so the returned `Array.len` would disagree with the allocation
    /// size — and the SDK-side free (`len * sizeof(T)`) would then deallocate with
    /// a layout that differs from the allocation, which is undefined behaviour.
    /// Collecting under one guard makes `vec.len()` the single source of truth for
    /// both the allocation size and the `Array.len`.
    ///
    /// Returns an empty `Vec` when nothing matches — callers must allocate nothing
    /// in that case.
    pub fn collect_guest_contracts(
        &self,
        contract_id: GuestContractId,
        min_version: u32,
    ) -> Vec<GuestContractHandle> {
        let data: RecoveringGuard<std::sync::RwLockReadGuard<'_, RuntimeStoreData>> =
            self.data.read().recover_poisoned(self.logger, "store");

        let indices: &Vec<u32> = match data.guest_contract_index.get(&contract_id) {
            Some(v) => v,
            None => return Vec::new(),
        };

        let mut collected: Vec<GuestContractHandle> = Vec::with_capacity(indices.len());
        for &slot_idx in indices.iter() {
            let slot: &PluginSlot = &data.slots[slot_idx as usize];
            if slot.entry.is_some()
                && let Some(ref interface) = slot.interface
                && interface.contract_version.major >= min_version
            {
                collected.push(GuestContractHandle {
                    index: slot_idx,
                    generation: slot.generation,
                });
            }
        }
        collected
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
    //  Delegates to find_guest_contract(). `min_version` is the minimum MAJOR version:
    //  a provider matches when `interface.contract_version.major >= min_version`. The
    //  contract_id already pins the major via its hash, and minor-level requirements are
    //  enforced at manifest validation — there is no packed (minor/patch) encoding here.
    //  Pass 0 to accept any version. The plain-major comparison is load-bearing: all six
    //  code generators pass the plain major, so changing the encoding would require a
    //  coordinated change across every generator.
    pub fn find(
        &self,
        contract_id: GuestContractId,
        min_version: u32,
    ) -> Result<GuestContractHandle, RegistryError> {
        self.find_guest_contract(contract_id, min_version)
    }

    /// Validate a GuestContractHandle and return its interface pointer directly.
    ///
    /// Returns Err(InvalidHandle) if:
    /// - handle.index is out of bounds
    /// - the slot is live at the handle's generation but currently holds no interface
    ///
    /// Returns Err(StaleHandle) if the handle's generation no longer matches the
    /// slot's generation — the slot was retired by an unload (and possibly reused by
    /// a later registration) after this handle was minted. The generation is checked
    /// before the interface presence so a handle whose bundle was unloaded resolves
    /// to StaleHandle even though the slot was vacated.
    pub fn resolve_guest_contract(
        &self,
        handle: GuestContractHandle,
    ) -> Result<*const GuestContractInterface, RegistryError> {
        let data: RecoveringGuard<std::sync::RwLockReadGuard<'_, RuntimeStoreData>> =
            self.data.read().recover_poisoned(self.logger, "store");

        let slot_idx: usize = handle.index as usize;
        if slot_idx >= data.slots.len() {
            return Err(RegistryError::InvalidHandle {
                index: handle.index,
            });
        }

        let slot: &PluginSlot = &data.slots[slot_idx];
        if handle.generation != slot.generation {
            return Err(RegistryError::StaleHandle {
                index: handle.index,
            });
        }
        match slot.interface {
            Some(ref interface) => Ok(interface.as_ref() as *const GuestContractInterface),
            None => Err(RegistryError::InvalidHandle {
                index: handle.index,
            }),
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
        let mut data: RecoveringGuard<std::sync::RwLockWriteGuard<'_, RuntimeStoreData>> =
            self.data.write().recover_poisoned(self.logger, "store");
        let slot_idx: usize = slot_index as usize;
        if slot_idx >= data.slots.len() {
            return Err(RegistryError::InvalidHandle { index: slot_index });
        }
        let slot: &mut PluginSlot = &mut data.slots[slot_idx];
        let old_interface: Arc<GuestContractInterface> = match slot.interface.replace(new_interface)
        {
            Some(old) => old,
            None => {
                // Slot had no interface — restore the empty state and report.
                data.slots[slot_idx].interface = None;
                return Err(RegistryError::InvalidHandle { index: slot_index });
            }
        };
        // Retire (do not drop) the old interface so any in-flight reader holding a
        // raw pointer into it stays valid.
        data.retired_interfaces.push(old_interface);
        Ok(())
    }

    /// Apply a reload swap for `bundle_id`, moving freshly-registered interfaces
    /// into the bundle's pre-reload slots and retiring the duplicate new slots.
    ///
    /// During a reload, the loader calls `polyplug_init`, which registers the new
    /// version's interfaces into brand-new slots (the existing slots are never
    /// vacated by registration). This method reconciles that: for each pre-reload
    /// slot in `old_slots`, it finds the matching newly-registered slot by
    /// `contract_id`, moves the new interface `Arc` into the old slot, and then
    /// retires the new slot (clears its entry/interface and removes it from the
    /// `guest_contract_index` and the bundle's `plugin_slots`).
    ///
    /// The whole operation runs under a single write lock so that concurrent
    /// readers always observe either the complete old state or the complete new
    /// state — never a half-swapped registry with duplicate or orphaned slots.
    ///
    /// # Dropped contracts (retire-not-drop)
    /// If a contract that the old version provided is no longer provided by the new
    /// version (no newly-registered slot matches its `contract_id`), the reload does
    /// NOT fail. Instead the old slot is retired: its interface `Arc` is moved into
    /// `retired_interfaces` (kept alive for in-flight readers holding raw pointers),
    /// and the slot is removed from the find index so the dropped contract becomes
    /// unresolvable via `find_*`. Previously-resolved raw pointers stay valid; new
    /// lookups for the dropped contract simply return nothing.
    ///
    /// # Errors
    /// Returns `Err(RegistryError::InvalidHandle)` if any old slot has no interface.
    /// Enter the reload window for `bundle_id`.
    ///
    /// Contracts registered by this bundle while the window is open are kept out of
    /// `guest_contract_index` (pending) so concurrent `find`/`find_all` readers do
    /// not observe a second live slot per contract. Pair with `apply_reload_swap`
    /// (which closes the window) or `abort_reload` on the failure path.
    pub(crate) fn begin_reload(&self, bundle_id: BundleId) {
        let mut data: RecoveringGuard<std::sync::RwLockWriteGuard<'_, RuntimeStoreData>> =
            self.data.write().recover_poisoned(self.logger, "store");
        data.reloading_bundles.insert(bundle_id);
    }

    /// Abort the reload window for `bundle_id`, purging any pending slots.
    ///
    /// Used on the reload failure path (init failed). Closes the window and removes
    /// the slots that `loader.reload()` registered before failing — i.e. the current
    /// bundle plugin slots that are NOT in `old_slots` (the pre-reload set). These
    /// pending slots were never published into `guest_contract_index`, so they are
    /// never-visible; purging them prevents unbounded accumulation across retries.
    ///
    /// Retire-not-drop is respected: only pending (never-visible) slots are purged.
    /// Pre-reload (live) and previously-retired slots are left untouched, so any
    /// raw pointer a caller already resolved stays valid.
    pub(crate) fn abort_reload(&self, bundle_id: BundleId, old_slots: &[u32]) {
        let mut data: RecoveringGuard<std::sync::RwLockWriteGuard<'_, RuntimeStoreData>> =
            self.data.write().recover_poisoned(self.logger, "store");
        data.reloading_bundles.remove(&bundle_id);

        // Pending slots = current bundle slots minus the pre-reload set.
        let pending_slots: Vec<u32> = match data.bundle_data.get(&bundle_id) {
            Some(bd) => bd
                .plugin_slots
                .iter()
                .copied()
                .filter(|idx| !old_slots.contains(idx))
                .collect(),
            None => Vec::new(),
        };

        for pending_idx in pending_slots {
            let contract_id: Option<GuestContractId> = data
                .slots
                .get(pending_idx as usize)
                .and_then(|s| s.interface.as_ref())
                .map(|i| i.contract_id);

            // Clear the pending slot. Its interface Arc was never handed out (the
            // slot was never published), so dropping it here is sound.
            if let Some(slot) = data.slots.get_mut(pending_idx as usize) {
                slot.entry = None;
                slot.interface = None;
            }

            // Defensively drop the slot from the find index. Pending slots are kept
            // out of the index during the window, but a brand-new contract path must
            // never leave a dangling index entry.
            if let Some(cid) = contract_id
                && let Some(indices) = data.guest_contract_index.get_mut(&cid)
            {
                indices.retain(|&idx| idx != pending_idx);
                if indices.is_empty() {
                    data.guest_contract_index.remove(&cid);
                }
            }

            if let Some(bd) = data.bundle_data.get_mut(&bundle_id) {
                bd.plugin_slots.retain(|&idx| idx != pending_idx);
            }
        }
    }

    pub(crate) fn apply_reload_swap(
        &self,
        bundle_id: BundleId,
        old_slots: &[u32],
    ) -> Result<(), RegistryError> {
        let mut data: RecoveringGuard<std::sync::RwLockWriteGuard<'_, RuntimeStoreData>> =
            self.data.write().recover_poisoned(self.logger, "store");
        // Close the reload window: subsequent registrations (if any) publish normally,
        // and the swap below makes the new interfaces visible via the old slots.
        data.reloading_bundles.remove(&bundle_id);

        // Slots registered during this reload = current bundle slots minus old slots.
        let new_slots: Vec<u32> = match data.bundle_data.get(&bundle_id) {
            Some(bd) => bd
                .plugin_slots
                .iter()
                .copied()
                .filter(|idx| !old_slots.contains(idx))
                .collect(),
            None => Vec::new(),
        };

        for &old_idx in old_slots {
            let old_contract_id: GuestContractId = data
                .slots
                .get(old_idx as usize)
                .and_then(|s| s.interface.as_ref())
                .map(|i| i.contract_id)
                .ok_or(RegistryError::InvalidHandle { index: old_idx })?;

            // Find the newly-registered slot for the same contract_id.
            let matching_new_idx: Option<u32> = new_slots.iter().copied().find(|&idx| {
                data.slots
                    .get(idx as usize)
                    .and_then(|s| s.interface.as_ref())
                    .is_some_and(|i| i.contract_id == old_contract_id)
            });

            let new_idx: u32 = match matching_new_idx {
                Some(idx) => idx,
                None => {
                    // Retire-not-drop: the new bundle version no longer provides this
                    // contract. Do NOT fail the whole reload. Route through the canonical
                    // teardown atom so the dropped contract is retired with identical
                    // unload semantics — generation bumped (old handles go stale),
                    // interface retired (in-flight raw pointers stay valid), and the slot
                    // removed from the find index so the contract is no longer resolvable.
                    // The returned extra Arc reference is irrelevant to reload (only
                    // unload's reclaim decision inspects it), so it is discarded.
                    let _discarded: Option<Arc<GuestContractInterface>> = data.retire_slot(old_idx);
                    if let Some(bd) = data.bundle_data.get_mut(&bundle_id) {
                        bd.plugin_slots.retain(|&idx| idx != old_idx);
                    }
                    continue;
                }
            };

            // Move the new interface into the old slot, retiring (not dropping)
            // the old interface so any in-flight reader holding a raw pointer
            // into it stays valid.
            let new_interface: Arc<GuestContractInterface> = data.slots[new_idx as usize]
                .interface
                .take()
                .ok_or(RegistryError::InvalidHandle { index: new_idx })?;
            if let Some(old_interface) = data.slots[old_idx as usize]
                .interface
                .replace(new_interface)
            {
                data.retired_interfaces.push(old_interface);
            }

            // Vacate the now-orphaned new slot. Its interface Arc was already moved
            // out into the old slot above, so there is nothing to retire here (do NOT
            // push to `retired_interfaces`). Bump the generation so any handle minted
            // against this slot during the reload window — e.g. a pending handle the
            // registration returned — is recognised as stale once the index is recycled
            // by a later registration (ABA protection), mirroring `retire_slot`'s
            // generation bump without the (now absent) Arc retirement.
            data.slots[new_idx as usize].generation =
                data.slots[new_idx as usize].generation.wrapping_add(1);
            data.slots[new_idx as usize].entry = None;
            if let Some(indices) = data.guest_contract_index.get_mut(&old_contract_id) {
                indices.retain(|&idx| idx != new_idx);
            }
            if let Some(bd) = data.bundle_data.get_mut(&bundle_id) {
                bd.plugin_slots.retain(|&idx| idx != new_idx);
            }
        }

        // Publish any pending new slots that were NOT consumed by a swap above — these
        // are brand-new contracts the reloaded version introduced (no matching old
        // slot). They were registered pending (kept out of the find index during the
        // window); now that reload is reconciled, make them discoverable.
        for &new_idx in &new_slots {
            // Determine the contract_id of a still-live pending slot, then drop the
            // immutable borrow before mutating the index.
            let contract_id: Option<GuestContractId> = match data.slots.get(new_idx as usize) {
                // A consumed/retired slot has its entry cleared; skip those.
                Some(slot) if slot.entry.is_some() => {
                    slot.interface.as_ref().map(|i| i.contract_id)
                }
                _ => None,
            };
            if let Some(cid) = contract_id {
                let indices: &mut Vec<u32> = data.guest_contract_index.entry(cid).or_default();
                if !indices.contains(&new_idx) {
                    indices.push(new_idx);
                }
            }
        }

        Ok(())
    }

    /// Return the owned descriptor for the plugin registered at `handle`'s slot.
    ///
    /// Exposes the registry's owned copy of the plugin's `PluginDescriptor` (name,
    /// contract_name, version) for introspection. Returns `None` if the handle is
    /// out of bounds, its slot is vacant, or the handle is stale (its generation no
    /// longer matches the slot's). The generation check is what prevents a handle
    /// minted against a retired slot from observing the descriptor of a later
    /// occupant after the slot index is recycled. The returned data is owned (no
    /// borrowed `StringView`s), so it stays valid independently of the plugin's
    /// transient init-time buffers.
    pub fn get_guest_contract_descriptor(
        &self,
        handle: GuestContractHandle,
    ) -> Option<OwnedPluginDescriptor> {
        let data: RecoveringGuard<std::sync::RwLockReadGuard<'_, RuntimeStoreData>> =
            self.data.read().recover_poisoned(self.logger, "store");
        if handle.is_null() {
            return None;
        }
        let slot_idx: usize = handle.index as usize;
        let slot: &PluginSlot = data.slots.get(slot_idx)?;
        // Reject stale handles: a recycled slot carries a bumped generation, so a
        // handle minted against the prior occupant must not see the new one.
        if handle.generation != slot.generation {
            return None;
        }
        slot.entry
            .as_ref()
            .map(|entry: &PluginEntry| entry.descriptor.clone())
    }

    /// Find all slot indices that were registered by `bundle_id`.
    ///
    /// Returns an empty `Vec` if the bundle has no registered slots.
    /// O(1) lookup via bundle_data HashMap.
    pub fn get_bundle_plugin_slots(&self, bundle_id: BundleId) -> Vec<u32> {
        let data: RecoveringGuard<std::sync::RwLockReadGuard<'_, RuntimeStoreData>> =
            self.data.read().recover_poisoned(self.logger, "store");
        data.bundle_data
            .get(&bundle_id)
            .map(|bd: &BundleData| bd.plugin_slots.clone())
            .unwrap_or_default()
    }

    /// Collect, for each slot registered by `bundle_id`, the registered contract
    /// name, the interface's major version, and the actual native function count.
    ///
    /// The native function count is `Some(n)` only for `Native`-dispatch interfaces
    /// (read from `dispatch.native.function_count`). For VM-dispatch interfaces the
    /// count is `None` — the ABI does not expose a function count for VM dispatch,
    /// so there is nothing to compare against the manifest.
    pub fn bundle_native_function_counts(
        &self,
        bundle_id: BundleId,
    ) -> Vec<(String, u32, Option<u32>)> {
        let data: RecoveringGuard<std::sync::RwLockReadGuard<'_, RuntimeStoreData>> =
            self.data.read().recover_poisoned(self.logger, "store");
        let slots: &Vec<u32> = match data.bundle_data.get(&bundle_id) {
            Some(bd) => &bd.plugin_slots,
            None => return Vec::new(),
        };
        let mut result: Vec<(String, u32, Option<u32>)> = Vec::new();
        for &slot_idx in slots {
            let slot: &PluginSlot = match data.slots.get(slot_idx as usize) {
                Some(s) => s,
                None => continue,
            };
            let entry: &PluginEntry = match slot.entry.as_ref() {
                Some(e) => e,
                None => continue,
            };
            let interface: &Arc<GuestContractInterface> = match slot.interface.as_ref() {
                Some(i) => i,
                None => continue,
            };
            let major: u32 = interface.contract_version.major;
            // Read the native function count only when dispatch is Native.
            let native_count: Option<u32> = if interface.dispatch_type == DispatchType::Native {
                // SAFETY: dispatch_type == Native means the `native` arm of the
                // `dispatch` union is the active member (per the ABI contract), so
                // reading `dispatch.native.function_count` is sound.
                Some(unsafe { interface.dispatch.native.function_count })
            } else {
                None
            };
            result.push((entry.contract_name.clone(), major, native_count));
        }
        result
    }

    /// Collect the distinct contract IDs exported by `bundle_id`.
    ///
    /// Cross-references the bundle's plugin slots against `guest_contract_index`:
    /// a contract is exported by this bundle when at least one of its registered
    /// slot indices belongs to the bundle. Used by cascade reload to determine
    /// which contracts a freshly-reloaded bundle provides.
    pub(crate) fn bundle_exported_contracts(&self, bundle_id: BundleId) -> Vec<GuestContractId> {
        let data: RecoveringGuard<std::sync::RwLockReadGuard<'_, RuntimeStoreData>> =
            self.data.read().recover_poisoned(self.logger, "store");
        let bundle_slots: &Vec<u32> = match data.bundle_data.get(&bundle_id) {
            Some(bd) => &bd.plugin_slots,
            None => return Vec::new(),
        };
        let mut exported: Vec<GuestContractId> = Vec::new();
        for (contract_id, slot_indices) in data.guest_contract_index.iter() {
            if slot_indices.iter().any(|idx| bundle_slots.contains(idx)) {
                exported.push(*contract_id);
            }
        }
        exported
    }

    /// Return the bundle IDs that declared a dependency on any contract in `contracts`.
    ///
    /// Iterates `bundle_declared_deps`, returning each `bundle_id` whose declared
    /// dependency set intersects `contracts`. Used by cascade reload to find
    /// bundles that must be re-initialized when one of their dependencies reloads.
    pub(crate) fn bundles_depending_on_any(
        &self,
        contracts: &HashSet<GuestContractId>,
    ) -> Vec<BundleId> {
        let data: RecoveringGuard<std::sync::RwLockReadGuard<'_, RuntimeStoreData>> =
            self.data.read().recover_poisoned(self.logger, "store");
        data.bundle_declared_deps
            .iter()
            .filter(|(_, declared)| !declared.is_disjoint(contracts))
            .map(|(bundle_id, _)| *bundle_id)
            .collect()
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
        let mut data: RecoveringGuard<std::sync::RwLockWriteGuard<'_, RuntimeStoreData>> =
            self.data.write().recover_poisoned(self.logger, "store");

        // Update bundle_data descriptor if entry exists
        if let Some(bundle_data) = data.bundle_data.get_mut(&bundle_id) {
            bundle_data.descriptor.name = bundle_name.clone();
            bundle_data.descriptor.version = version;
            bundle_data.descriptor.runtime = runtime;
            bundle_data.descriptor.file_path = file_path;
            bundle_data.descriptor.dependencies = dependencies;
        } else {
            // Bundle has no plugins yet, create entry with empty plugin_slots
            data.bundle_data.insert(
                bundle_id,
                BundleData {
                    plugin_slots: Vec::new(),
                    descriptor: BundleDescriptor {
                        id: bundle_id,
                        name: bundle_name.clone(),
                        version,
                        runtime,
                        file_path,
                        dependencies,
                    },
                },
            );
        }

        // Add to bundle_name_index for multi-version support
        // Avoid duplicates when re-registering (e.g., during hot-reload).
        let name_entries = data.bundle_name_index.entry(bundle_name).or_default();
        if !name_entries.contains(&bundle_id) {
            name_entries.push(bundle_id);
        }

        Ok(())
    }

    /// Invalidate a bundle: retire its interfaces, bump generations, and remove it
    /// from every index structure.
    ///
    /// This is the invalidate-only unload primitive (retire-not-drop). Each owned slot
    /// is torn down via the canonical [`RuntimeStoreData::retire_slot`] helper, which:
    /// - bumps `slot.generation` (so every handle minted against the old generation now
    ///   resolves to [`RegistryError::StaleHandle`]);
    /// - **retires** the slot's interface `Arc` into `retired_interfaces` rather than
    ///   dropping it, so any raw `*const GuestContractInterface` already handed out by
    ///   `resolve_guest_contract` stays valid for the lifetime of the runtime;
    /// - clears the slot `entry` and removes the slot index from `guest_contract_index`.
    ///
    /// It then removes the bundle from `bundle_data`, `bundle_name_index`, and
    /// `bundle_declared_deps`, so `find_*` and `list_bundles` no longer observe it.
    ///
    /// The combination of retire-not-drop and the generation bump is what makes unload
    /// sound: old raw pointers remain dereferenceable (the memory is never freed here),
    /// while every old handle is recognised as stale on its next resolve.
    ///
    /// Returns `(count, retired_arcs)` where `count` is the number of slots that were
    /// invalidated and `retired_arcs` holds one extra `Arc<GuestContractInterface>`
    /// reference per retired interface. The runtime inspects `Arc::strong_count` on
    /// these to decide whether a loader may reclaim (e.g. `dlclose`) the bundle's
    /// backing library or must defer (retire) it — see [`crate::runtime::Runtime::unload_bundle`].
    pub fn invalidate_bundle(
        &self,
        bundle_id: BundleId,
    ) -> Result<(u32, Vec<Arc<GuestContractInterface>>), RegistryError> {
        let mut data: RecoveringGuard<std::sync::RwLockWriteGuard<'_, RuntimeStoreData>> =
            self.data.write().recover_poisoned(self.logger, "store");

        // Collect slot indices and bundle name before mutating.
        let (slot_indices, bundle_name): (Vec<u32>, String) = match data.bundle_data.get(&bundle_id)
        {
            Some(bd) => (bd.plugin_slots.clone(), bd.descriptor.name.clone()),
            None => return Ok((0, Vec::new())),
        };

        // Tear down each slot via the canonical teardown atom, collecting each retired
        // interface's extra Arc reference so the runtime can inspect strong_count.
        // Bundle-level metadata is removed in bulk below (no per-slot plugin_slots
        // edit needed here).
        let mut retired_arcs: Vec<Arc<GuestContractInterface>> = Vec::new();
        for slot_idx in &slot_indices {
            if let Some(arc) = data.retire_slot(*slot_idx) {
                retired_arcs.push(arc);
            }
        }

        // Remove from bundle_data.
        data.bundle_data.remove(&bundle_id);

        // Remove from bundle_name_index, dropping the now-empty name key.
        if let Some(ids) = data.bundle_name_index.get_mut(&bundle_name) {
            ids.retain(|id| *id != bundle_id);
            if ids.is_empty() {
                data.bundle_name_index.remove(&bundle_name);
            }
        }

        // Remove from bundle_declared_deps.
        data.bundle_declared_deps.remove(&bundle_id);

        Ok((slot_indices.len() as u32, retired_arcs))
    }

    /// List all loaded bundle IDs.
    pub fn list_bundles(&self) -> Vec<BundleId> {
        let data: RecoveringGuard<std::sync::RwLockReadGuard<'_, RuntimeStoreData>> =
            self.data.read().recover_poisoned(self.logger, "store");
        data.bundle_data.keys().copied().collect::<Vec<BundleId>>()
    }

    /// Get bundle metadata by bundle ID.
    pub fn get_bundle_descriptor(&self, bundle_id: BundleId) -> Option<BundleDescriptor> {
        let data: RecoveringGuard<std::sync::RwLockReadGuard<'_, RuntimeStoreData>> =
            self.data.read().recover_poisoned(self.logger, "store");
        data.bundle_data.get(&bundle_id).map(|bd: &BundleData| {
            // Clone descriptor fields manually since BundleDescriptor doesn't derive Clone
            BundleDescriptor {
                id: bd.descriptor.id,
                name: bd.descriptor.name.clone(),
                version: bd.descriptor.version,
                runtime: bd.descriptor.runtime,
                file_path: bd.descriptor.file_path.clone(),
                dependencies: bd
                    .descriptor
                    .dependencies
                    .iter()
                    .map(|d| BundleDependency {
                        name: d.name.clone(),
                        min_version: d.min_version,
                    })
                    .collect(),
            }
        })
    }

    /// Get all BundleIds for a given bundle name (multi-version support).
    pub fn get_bundles_by_name(&self, bundle_name: &str) -> Vec<BundleId> {
        let data: RecoveringGuard<std::sync::RwLockReadGuard<'_, RuntimeStoreData>> =
            self.data.read().recover_poisoned(self.logger, "store");
        data.bundle_name_index
            .get(bundle_name)
            .cloned()
            .unwrap_or_default()
    }

    /// Get a clone of the Arc<GuestContractInterface> for `slot_index` to check strong_count.
    /// Returns None if the slot is empty or has no interface.
    pub(crate) fn get_guest_contract_interface_arc(
        &self,
        slot_index: u32,
    ) -> Option<Arc<GuestContractInterface>> {
        let data: RecoveringGuard<std::sync::RwLockReadGuard<'_, RuntimeStoreData>> =
            self.data.read().recover_poisoned(self.logger, "store");
        let slot: &PluginSlot = data.slots.get(slot_index as usize)?;
        slot.interface.as_ref().map(Arc::clone)
    }

    /// Clear all registrations for testing.
    /// This is only available in test builds to allow test isolation.
    #[cfg(test)]
    pub fn clear_for_test(&self) {
        let mut data: RecoveringGuard<std::sync::RwLockWriteGuard<'_, RuntimeStoreData>> =
            self.data.write().recover_poisoned(self.logger, "store");
        data.slots.clear();
        data.guest_contract_index.clear();
        data.bundle_data.clear();
        data.bundle_name_index.clear();
        data.bundle_declared_deps.clear();
        data.reloading_bundles.clear();
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
        DispatchMechanisms, DispatchType, GuestContractInstance, GuestContractInterface, HostApi,
        NativeDispatch, PluginDescriptor, StringView, Version,
    };

    /// No-op create_instance callback.
    unsafe extern "C" fn noop_create_instance(
        _host: *const HostApi,
        _args: *const (),
    ) -> GuestContractInstance {
        GuestContractInstance::null()
    }

    /// No-op destroy_instance callback.
    unsafe extern "C" fn noop_destroy_instance(
        _host: *const HostApi,
        _instance: GuestContractInstance,
    ) {
    }

    fn mock_interface(contract_id: u64) -> GuestContractInterface {
        GuestContractInterface {
            contract_id: GuestContractId::from_u64(contract_id),
            contract_version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
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
            version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
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
        let invalid: GuestContractHandle = GuestContractHandle {
            index: 999,
            generation: 0,
        };
        let result: Result<*const GuestContractInterface, RegistryError> =
            registry.resolve_guest_contract(invalid);
        assert!(
            matches!(result, Err(RegistryError::InvalidHandle { .. })),
            "expected InvalidHandle error"
        );
    }

    #[test]
    fn same_bundle_same_contract_is_duplicate_provider() {
        let registry: RuntimeStore = RuntimeStore::new();
        let d1: PluginDescriptor = make_descriptor("plugin_a", "image.decode");
        let d2: PluginDescriptor = make_descriptor("plugin_b", "image.decode");
        let bundle_id = BundleId::from_u64(0);
        let other_bundle = BundleId::from_u64(1);
        let interface = mock_interface(0x1234_5678_9ABC_DEF0);

        // SAFETY: interface is a local value
        unsafe {
            registry
                .register_guest_contract(d1, &interface, "image.decode".to_owned(), bundle_id)
                .expect("first registration should succeed");
        }

        // Same bundle re-registering the same contract (no reload window) is rejected.
        let dup: PluginDescriptor = make_descriptor("plugin_a2", "image.decode");
        let result: Result<GuestContractHandle, RegistryError> =
            // SAFETY: interface is a local value
            unsafe { registry.register_guest_contract(dup, &interface, "image.decode".to_owned(), bundle_id) };
        assert!(
            matches!(result, Err(RegistryError::DuplicateProvider { .. })),
            "same bundle + same contract must be DuplicateProvider, got {result:?}"
        );

        // A DIFFERENT bundle registering the same contract is still allowed (multi-impl).
        let other: Result<GuestContractHandle, RegistryError> =
            // SAFETY: interface is a local value
            unsafe { registry.register_guest_contract(d2, &interface, "image.decode".to_owned(), other_bundle) };
        assert!(
            other.is_ok(),
            "different bundle + same contract must be allowed (multi-impl), got {other:?}"
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
    fn register_invalid_utf8_name_rejected_runtime_unaffected() {
        let registry: RuntimeStore = RuntimeStore::new();
        // 0xFF/0xFE are invalid UTF-8 lead bytes.
        let bad_name: &[u8] = &[0xFF_u8, 0xFE_u8, b'x'];
        let descriptor: PluginDescriptor = PluginDescriptor {
            name: StringView {
                ptr: bad_name.as_ptr(),
                len: bad_name.len(),
            },
            contract_name: StringView::from_static(b"image.decode"),
            version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
        };
        let interface = mock_interface(0x1234_5678_9ABC_DEF0);

        // SAFETY: interface and bad_name are local values valid for this call.
        let result: Result<GuestContractHandle, RegistryError> = unsafe {
            registry.register_guest_contract(
                descriptor,
                &interface,
                "image.decode".to_owned(),
                BundleId::from_u64(0),
            )
        };
        assert!(
            matches!(result, Err(RegistryError::InvalidUtf8 { .. })),
            "invalid UTF-8 name must be rejected with InvalidUtf8, got: {result:?}"
        );

        // The runtime is unaffected: the rejected registration left no slot behind,
        // and a subsequent valid registration still succeeds and is findable.
        let good: PluginDescriptor = make_descriptor("ok_plugin", "image.decode");
        // SAFETY: interface is a local value valid for this call.
        let handle: GuestContractHandle = unsafe {
            registry.register_guest_contract(
                good,
                &interface,
                "image.decode".to_owned(),
                BundleId::from_u64(0),
            )
        }
        .expect("valid registration after rejection should succeed");
        let found: GuestContractHandle = registry
            .find(GuestContractId::from_u64(0x1234_5678_9ABC_DEF0), 0)
            .expect("find should succeed after recovery");
        assert_eq!(found.index, handle.index);
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

        let interface_ptr: *const GuestContractInterface = registry
            .resolve_guest_contract(handle)
            .expect("resolve_guest_contract should succeed");
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
            registry.register_guest_contract(
                descriptor,
                &interface,
                "contract".to_owned(),
                bundle_id,
            )
        }
        .expect("registration should succeed");

        // Register bundle metadata
        registry
            .register_bundle_metadata(
                bundle_id,
                "test-bundle".to_string(),
                Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
                RuntimeLanguage::Rust,
                PathBuf::from("/test"),
                Vec::new(),
            )
            .expect("metadata registration should succeed");

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
            registry.register_guest_contract(
                descriptor,
                &interface,
                "contract".to_owned(),
                bundle_id,
            )
        }
        .expect("registration should succeed");

        registry
            .register_bundle_metadata(
                bundle_id,
                "test-bundle".to_string(),
                Version {
                    major: 1,
                    minor: 2,
                    patch: 3,
                },
                RuntimeLanguage::Python,
                PathBuf::from("/path/to/bundle"),
                vec![BundleDependency {
                    name: "dep-bundle".to_string(),
                    min_version: Some(Version {
                        major: 1,
                        minor: 0,
                        patch: 0,
                    }),
                }],
            )
            .expect("metadata registration should succeed");

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
        registry
            .register_bundle_metadata(
                bundle_id,
                "test-bundle".to_string(),
                Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
                RuntimeLanguage::Rust,
                PathBuf::new(),
                Vec::new(),
            )
            .expect("metadata registration should succeed");

        let ids: Vec<BundleId> = registry.get_bundles_by_name("test-bundle");
        assert_eq!(ids.len(), 1);
        assert_eq!(ids[0], bundle_id);

        // Non-existent name returns empty
        let missing: Vec<BundleId> = registry.get_bundles_by_name("non-existent");
        assert!(
            missing.is_empty(),
            "non-existent name should return empty vec"
        );
    }

    /// Item 7: during a reload, find_all must report exactly one slot per contract
    /// throughout — never the transient two-live-slots window.
    #[test]
    fn reload_window_keeps_single_slot_per_contract() {
        const CID: u64 = 0x1111_2222_3333_4444_u64;
        let registry: RuntimeStore = RuntimeStore::new();
        let bundle_id: BundleId = BundleId::from_u64(0xAAAA_u64);
        let contract_id: GuestContractId = GuestContractId::from_u64(CID);

        let descriptor_v1: PluginDescriptor = make_descriptor("plugin", "reload.contract");
        let iface_v1: GuestContractInterface = mock_interface(CID);
        // SAFETY: iface_v1 is a local 'static-shaped value used only for this test's lifetime.
        let _h1: GuestContractHandle = unsafe {
            registry.register_guest_contract(
                descriptor_v1,
                &iface_v1,
                "reload.contract".to_owned(),
                bundle_id,
            )
        }
        .expect("v1 registration");

        let old_slots: Vec<u32> = registry.get_bundle_plugin_slots(bundle_id);
        assert_eq!(old_slots.len(), 1, "one slot before reload");

        let mut out: [GuestContractHandle; 8] = [GuestContractHandle::null(); 8];
        assert_eq!(
            registry.find_all_guest_contracts(contract_id, 0, &mut out),
            1,
            "exactly one provider before reload"
        );

        // Begin the reload window and register the new version (pending).
        registry.begin_reload(bundle_id);
        let descriptor_v2: PluginDescriptor = make_descriptor("plugin", "reload.contract");
        let iface_v2: GuestContractInterface = mock_interface(CID);
        // SAFETY: iface_v2 is a local value valid for this test's lifetime.
        let _h2: GuestContractHandle = unsafe {
            registry.register_guest_contract(
                descriptor_v2,
                &iface_v2,
                "reload.contract".to_owned(),
                bundle_id,
            )
        }
        .expect("v2 registration");

        // CRITICAL: even though a second slot now exists, find_all must still see one.
        assert_eq!(
            registry.find_all_guest_contracts(contract_id, 0, &mut out),
            1,
            "exactly one provider DURING the reload window (no duplicate slot)"
        );

        // Reconcile: swap and close the window.
        registry
            .apply_reload_swap(bundle_id, &old_slots)
            .expect("apply_reload_swap");

        assert_eq!(
            registry.find_all_guest_contracts(contract_id, 0, &mut out),
            1,
            "exactly one provider after reload"
        );
    }

    /// Item 8: when the reloaded version drops a previously-provided contract,
    /// apply_reload_swap must retire the old slot (not error), making the contract
    /// unresolvable while not failing the reload.
    #[test]
    fn reload_dropping_contract_retires_old_slot() {
        const CID: u64 = 0x5555_6666_7777_8888_u64;
        let registry: RuntimeStore = RuntimeStore::new();
        let bundle_id: BundleId = BundleId::from_u64(0xBBBB_u64);
        let contract_id: GuestContractId = GuestContractId::from_u64(CID);

        let descriptor: PluginDescriptor = make_descriptor("plugin", "dropped.contract");
        let iface: GuestContractInterface = mock_interface(CID);
        // SAFETY: iface is a local value valid for this test's lifetime.
        let _h: GuestContractHandle = unsafe {
            registry.register_guest_contract(
                descriptor,
                &iface,
                "dropped.contract".to_owned(),
                bundle_id,
            )
        }
        .expect("registration");

        let old_slots: Vec<u32> = registry.get_bundle_plugin_slots(bundle_id);
        assert_eq!(old_slots.len(), 1);

        // Begin a reload that registers NO new slot for this contract (it is dropped).
        registry.begin_reload(bundle_id);
        // apply_reload_swap must succeed (retire-not-drop), not error.
        registry
            .apply_reload_swap(bundle_id, &old_slots)
            .expect("apply_reload_swap must not fail when a contract is dropped");

        let mut out: [GuestContractHandle; 4] = [GuestContractHandle::null(); 4];
        assert_eq!(
            registry.find_all_guest_contracts(contract_id, 0, &mut out),
            0,
            "dropped contract must be unresolvable after reload"
        );
        assert!(
            registry.find(contract_id, 0).is_err(),
            "find must fail for a dropped contract"
        );
    }

    /// Finding 2: during a reload window the pending (unpublished) slot must NOT
    /// be returned by `find_guest_contract_by_bundle`, and after `abort_reload`
    /// the previously-published handle still resolves (nothing dangles).
    #[test]
    fn pending_reload_slot_not_returned_by_find_by_bundle() {
        const CID: u64 = 0x2222_0000_0000_0001_u64;
        let registry: RuntimeStore = RuntimeStore::new();
        let bundle_id: BundleId = BundleId::from_u64(0x7777);
        let contract_id: GuestContractId = GuestContractId::from_u64(CID);

        let iface_v1: GuestContractInterface = mock_interface(CID);
        // SAFETY: iface_v1 is a local value valid for this test's lifetime.
        let h1: GuestContractHandle = unsafe {
            registry.register_guest_contract(
                make_descriptor("v1", "reload.contract"),
                &iface_v1,
                "reload.contract".to_owned(),
                bundle_id,
            )
        }
        .expect("v1 register");

        let old_slots: Vec<u32> = registry.get_bundle_plugin_slots(bundle_id);
        assert_eq!(old_slots.len(), 1, "one published slot before reload");

        // Open the reload window and register the new version (pending slot).
        registry.begin_reload(bundle_id);
        let iface_v2: GuestContractInterface = mock_interface(CID);
        // SAFETY: iface_v2 is a local value valid for this test's lifetime.
        let _h2: GuestContractHandle = unsafe {
            registry.register_guest_contract(
                make_descriptor("v2", "reload.contract"),
                &iface_v2,
                "reload.contract".to_owned(),
                bundle_id,
            )
        }
        .expect("v2 register (pending)");

        // The pending slot is now in plugin_slots, but find_by_bundle must return
        // only the PUBLISHED slot, never the pending one.
        let found: GuestContractHandle = registry
            .find_guest_contract_by_bundle(bundle_id, contract_id, 0)
            .expect("must resolve to the published slot, not the pending one");
        assert_eq!(
            found.index, h1.index,
            "find_by_bundle must return the published slot during a reload window"
        );

        // Abort the reload — the pending slot is purged; the published handle still
        // resolves (nothing the caller resolved dangles).
        registry.abort_reload(bundle_id, &old_slots);
        let still_live: *const GuestContractInterface = registry
            .resolve_guest_contract(h1)
            .expect("published handle stays valid after abort");
        assert!(!still_live.is_null());
    }

    /// Finding 2 (ABA): `apply_reload_swap` must bump the generation of the
    /// consumed new slot so a handle that captured its pre-swap generation goes
    /// StaleHandle once the recycled slot index is reused by a later registration.
    #[test]
    fn apply_reload_swap_bumps_consumed_new_slot_generation() {
        const CID: u64 = 0x3333_0000_0000_0001_u64;
        let registry: RuntimeStore = RuntimeStore::new();
        let bundle_id: BundleId = BundleId::from_u64(0x8888);

        let iface_v1: GuestContractInterface = mock_interface(CID);
        // SAFETY: local value valid for this test's lifetime.
        let _h1: GuestContractHandle = unsafe {
            registry.register_guest_contract(
                make_descriptor("v1", "aba.contract"),
                &iface_v1,
                "aba.contract".to_owned(),
                bundle_id,
            )
        }
        .expect("v1 register");
        let old_slots: Vec<u32> = registry.get_bundle_plugin_slots(bundle_id);

        // Reload: register a new version into a fresh (pending) slot.
        registry.begin_reload(bundle_id);
        let iface_v2: GuestContractInterface = mock_interface(CID);
        // SAFETY: local value valid for this test's lifetime.
        let h_new: GuestContractHandle = unsafe {
            registry.register_guest_contract(
                make_descriptor("v2", "aba.contract"),
                &iface_v2,
                "aba.contract".to_owned(),
                bundle_id,
            )
        }
        .expect("v2 register (pending)");
        let consumed_slot_index: u32 = h_new.index;
        let captured_generation: u32 = h_new.generation;
        assert_ne!(
            consumed_slot_index, old_slots[0],
            "the new version registers into a distinct slot"
        );

        // Swap: the new interface moves into the old slot; the new slot is vacated.
        registry
            .apply_reload_swap(bundle_id, &old_slots)
            .expect("apply_reload_swap");

        // Recycle the vacated index with a brand-new registration.
        let recycle_bundle: BundleId = BundleId::from_u64(0x8889);
        let iface_recycle: GuestContractInterface = mock_interface(0x3333_0000_0000_0002_u64);
        // SAFETY: local value valid for this test's lifetime.
        let h_recycle: GuestContractHandle = unsafe {
            registry.register_guest_contract(
                make_descriptor("recycled", "recycle.contract"),
                &iface_recycle,
                "recycle.contract".to_owned(),
                recycle_bundle,
            )
        }
        .expect("recycled register");
        assert_eq!(
            h_recycle.index, consumed_slot_index,
            "the vacated new slot index must be recycled by the next registration"
        );
        assert_ne!(
            h_recycle.generation, captured_generation,
            "recycled slot must carry a bumped generation (ABA protection)"
        );

        // The stale handle (vacated index + captured generation) must NOT resolve.
        let stale: GuestContractHandle = GuestContractHandle {
            index: consumed_slot_index,
            generation: captured_generation,
        };
        let result: Result<*const GuestContractInterface, RegistryError> =
            registry.resolve_guest_contract(stale);
        assert!(
            matches!(result, Err(RegistryError::StaleHandle { .. })),
            "handle capturing the pre-swap generation must resolve StaleHandle, got {result:?}"
        );
    }

    #[test]
    fn resolve_after_unload_returns_stale_handle() {
        let registry: RuntimeStore = RuntimeStore::new();
        let descriptor: PluginDescriptor = make_descriptor("unload_plugin", "image.decode");
        let interface: GuestContractInterface = mock_interface(0x0BAD_F00D_0000_0001);
        let bundle_id: BundleId = BundleId::from_u64(0x11);

        // SAFETY: interface is a valid local GuestContractInterface for this test.
        let handle: GuestContractHandle = unsafe {
            registry.register_guest_contract(
                descriptor,
                &interface,
                "image.decode".to_owned(),
                bundle_id,
            )
        }
        .expect("registration should succeed");

        registry
            .invalidate_bundle(bundle_id)
            .expect("invalidate should succeed");

        let result: Result<*const GuestContractInterface, RegistryError> =
            registry.resolve_guest_contract(handle);
        assert!(
            matches!(result, Err(RegistryError::StaleHandle { .. })),
            "handle minted before unload must resolve to StaleHandle, got {result:?}"
        );
    }

    #[test]
    fn find_stops_returning_after_unload() {
        let registry: RuntimeStore = RuntimeStore::new();
        let descriptor: PluginDescriptor = make_descriptor("unload_plugin", "audio.decode");
        let interface: GuestContractInterface = mock_interface(0x0BAD_F00D_0000_0002);
        let bundle_id: BundleId = BundleId::from_u64(0x12);
        let contract_id: GuestContractId = GuestContractId::from_u64(0x0BAD_F00D_0000_0002);

        // SAFETY: interface is a valid local GuestContractInterface for this test.
        unsafe {
            registry.register_guest_contract(
                descriptor,
                &interface,
                "audio.decode".to_owned(),
                bundle_id,
            )
        }
        .expect("registration should succeed");

        registry
            .invalidate_bundle(bundle_id)
            .expect("invalidate should succeed");

        let result: Result<GuestContractHandle, RegistryError> = registry.find(contract_id, 0);
        assert!(
            matches!(result, Err(RegistryError::PluginNotFound { .. })),
            "find must report PluginNotFound after unload, got {result:?}"
        );
    }

    #[test]
    fn unload_retires_interface_keeping_pointer_valid() {
        let registry: RuntimeStore = RuntimeStore::new();
        let descriptor: PluginDescriptor = make_descriptor("unload_plugin", "render.frame");
        let raw_contract_id: u64 = 0x0BAD_F00D_0000_0003;
        let interface: GuestContractInterface = mock_interface(raw_contract_id);
        let bundle_id: BundleId = BundleId::from_u64(0x13);

        // SAFETY: interface is a valid local GuestContractInterface for this test.
        let handle: GuestContractHandle = unsafe {
            registry.register_guest_contract(
                descriptor,
                &interface,
                "render.frame".to_owned(),
                bundle_id,
            )
        }
        .expect("registration should succeed");

        // Resolve a raw pointer BEFORE unload. Retire-not-drop guarantees it stays
        // valid afterwards because the interface Arc is moved to retire storage.
        let resolved: *const GuestContractInterface = registry
            .resolve_guest_contract(handle)
            .expect("resolve before unload should succeed");

        registry
            .invalidate_bundle(bundle_id)
            .expect("invalidate should succeed");

        // SAFETY: `resolved` was obtained before unload; invalidate retires (does not
        // drop) the interface Arc, so the memory remains valid for this read.
        let observed_id: GuestContractId = unsafe { (*resolved).contract_id };
        assert_eq!(
            observed_id,
            GuestContractId::from_u64(raw_contract_id),
            "retired interface must still be readable through the pre-unload pointer"
        );
    }
}

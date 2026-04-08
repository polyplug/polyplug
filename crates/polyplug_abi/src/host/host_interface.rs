//! Host Interface — function table passed to guests during initialization.
//!
//! This module defines `HostInterface`, the primary interface guests use to
//! interact with the runtime. Guests receive this interface in `polyplug_init()`.
//!
//! # Who provides
//! The runtime creates this struct and passes it to `polyplug_init()`.
//!
//! # Who calls
//! Guest (plugin) code calls these functions to interact with the runtime.
//!
//! # Ownership
//! The struct is statically allocated by the runtime. The pointer is valid
//! until the runtime is destroyed. Guest must NOT free this pointer.
//!
//! # Lifetime
//! Lives as long as the runtime that created it.
//!
//! # Thread Safety
//! All functions are safe to call from any thread. The runtime handles
//! internal synchronization.
//!
//! # Self-Passing Pattern
//! All functions take `self: *const HostInterface` as the first parameter.
//! SDKs hide this detail from users, automatically passing the interface pointer.

use core::ffi::c_void;

use polyplug_utils::BundleId;

use crate::{
    guest::{GuestContractInterface, GuestContractInstance},
    plugin::{PluginDescriptor, PluginHandle},
    types::{AbiError, Array, DependencyInfo},
};

/// Type alias for backward compatibility during transition.
/// Will be replaced with ContractHandle in Phase 2.
pub type ContractHandle = PluginHandle;

/// Host Interface — function table passed to guests during initialization.
///
/// Contains an opaque runtime pointer and function pointers for guest calls.
/// All functions use self-passing pattern (receive HostInterface pointer as first parameter).
///
/// # Who provides
/// The runtime creates this struct and passes it to `polyplug_init()`.
/// The struct is allocated using `Box::leak()` for `'static` lifetime.
///
/// # Who calls
/// Guest (plugin) code calls these functions to interact with the runtime.
/// SDK-generated wrappers handle the self-passing pattern automatically.
///
/// # Ownership
/// The struct is statically allocated by the runtime. The pointer is valid
/// until the runtime is destroyed. Guest must NOT free this pointer.
///
/// # Lifetime
/// Lives as long as the runtime that created it.
///
/// # Thread Safety
/// All functions are safe to call from any thread. The runtime uses
/// internal synchronization (RwLock/Mutex) for shared state.
///
/// # Self-passing pattern
/// Each function receives the interface pointer as its first parameter,
/// allowing guests to call: `host->find_by_contract(host, id, ver)`
/// SDKs hide this pattern: `host.find_by_contract(id, ver)`
#[repr(C)]
pub struct HostInterface {
    /// Opaque pointer to Runtime.
    ///
    /// Set during interface creation. Provides access to runtime state
    /// for dependency enforcement and resource management.
    ///
    /// # Ownership
    /// Owned by the runtime. Guests must NOT free or modify this pointer.
    pub runtime: *mut c_void,
    /// Register a guest contract implementation.
    ///
    /// Called by plugins during `polyplug_init()` to register their contracts.
    /// Returns error if contract_id collision detected or ABI version mismatch.
    ///
    /// # Arguments
    /// - `this`: HostInterface pointer (self-passing)
    /// - `descriptor`: Plugin descriptor with contract metadata
    /// - `interface`: GuestContractInterface to register
    ///
    /// # Returns
    /// AbiError::OK on success, error code on failure.
    pub register_contract: unsafe extern "C" fn(
        this: *const HostInterface,
        descriptor: *const PluginDescriptor,
        interface: *const GuestContractInterface,
    ) -> AbiError,
    /// Allocate memory using the host allocator.
    ///
    /// Memory allocated here must be freed via `free`.
    /// Returns null on allocation failure.
    ///
    /// # Arguments
    /// - `this`: HostInterface pointer (self-passing)
    /// - `size`: Number of bytes to allocate
    /// - `align`: Alignment requirement (must be power of 2)
    ///
    /// # Returns
    /// Pointer to allocated memory, or null on failure.
    pub alloc: unsafe extern "C" fn(this: *const HostInterface, size: usize, align: usize) -> *mut u8,
    /// Free memory allocated via `alloc`.
    ///
    /// Must pass the same size and align used for allocation.
    ///
    /// # Arguments
    /// - `this`: HostInterface pointer (self-passing)
    /// - `ptr`: Pointer to memory to free
    /// - `size`: Size used for allocation
    /// - `align`: Alignment used for allocation
    pub free: unsafe extern "C" fn(this: *const HostInterface, ptr: *mut u8, size: usize, align: usize),
    /// Find a guest contract by contract_id and minimum version.
    ///
    /// Returns a ContractHandle that can be resolved to an interface.
    /// Returns null handle if no matching contract found.
    ///
    /// # Arguments
    /// - `this`: HostInterface pointer (self-passing)
    /// - `contract_id`: Contract identifier hash
    /// - `min_version`: Minimum version required
    ///
    /// # Returns
    /// ContractHandle for the first matching contract, or null handle.
    pub find_by_contract: unsafe extern "C" fn(
        this: *const HostInterface,
        contract_id: u64,
        min_version: u32,
    ) -> ContractHandle,
    /// Find all guest contracts matching contract_id and minimum version.
    ///
    /// Returns an Array of ContractHandle. Caller must free via `host->free`.
    /// Use when multiple implementations of the same contract may exist.
    ///
    /// # Arguments
    /// - `this`: HostInterface pointer (self-passing)
    /// - `contract_id`: Contract identifier hash
    /// - `min_version`: Minimum version required
    ///
    /// # Returns
    /// Array of ContractHandle. Caller owns and must free.
    pub find_all_by_contract: unsafe extern "C" fn(
        this: *const HostInterface,
        contract_id: u64,
        min_version: u32,
    ) -> Array<ContractHandle>,
    /// Resolve a ContractHandle to a GuestContractInterface pointer.
    ///
    /// Returns null if the handle is invalid or contract was unloaded.
    ///
    /// # Arguments
    /// - `this`: HostInterface pointer (self-passing)
    /// - `handle`: ContractHandle from find_by_contract
    ///
    /// # Returns
    /// Pointer to GuestContractInterface, or null if invalid/stale.
    pub resolve_contract: unsafe extern "C" fn(
        this: *const HostInterface,
        handle: ContractHandle,
    ) -> *const GuestContractInterface,
    /// Call a method on a guest contract instance.
    ///
    /// This is the cross-dispatch mechanism for calling methods across
    /// different dispatch types (Native vs VM).
    ///
    /// # Arguments
    /// - `this`: HostInterface pointer (self-passing)
    /// - `instance`: GuestContractInstance with contract_id for dispatch
    /// - `method_id`: Method index within the contract
    /// - `args`: Pointer to packed arguments (contract-specific layout)
    /// - `out`: Pointer to output buffer for return value
    ///
    /// # Returns
    /// AbiError::OK on success, error code on failure.
    pub call_guest_method: unsafe extern "C" fn(
        this: *const HostInterface,
        instance: GuestContractInstance,
        method_id: u32,
        args: *const (),
        out: *mut (),
    ) -> AbiError,
    /// Get a host contract instance by contract_id and minimum version.
    ///
    /// For singleton host contracts, returns the same instance every time.
    /// For multi-instance host contracts, returns a new instance each time.
    ///
    /// # Arguments
    /// - `this`: HostInterface pointer (self-passing)
    /// - `contract_id`: Host contract identifier hash
    /// - `min_version`: Minimum version required
    ///
    /// # Returns
    /// HostContractInstance for the contract.
    pub get_host_contract: unsafe extern "C" fn(
        this: *const HostInterface,
        contract_id: u64,
        min_version: u32,
    ) -> crate::host::HostContractInstance,
    /// Resolve a host contract interface by contract_id and minimum version.
    ///
    /// Returns the HostContractInterface pointer for the contract.
    /// This is needed to access dispatch metadata (dispatch_type, function_count, functions).
    /// Returns null if no matching contract found.
    ///
    /// # Arguments
    /// - `this`: HostInterface pointer (self-passing)
    /// - `contract_id`: Host contract identifier hash
    /// - `min_version`: Minimum version required
    ///
    /// # Returns
    /// Pointer to HostContractInterface, or null if invalid/not found.
    pub resolve_host_contract_interface: unsafe extern "C" fn(
        this: *const HostInterface,
        contract_id: u64,
        min_version: u32,
    ) -> *const crate::host::HostContractInterface,
    /// List all loaded bundles.
    ///
    /// Returns an Array of BundleId. Caller must free via `host->free`.
    /// Bundle IDs are stable for the lifetime of the runtime.
    ///
    /// # Arguments
    /// - `this`: HostInterface pointer (self-passing)
    ///
    /// # Returns
    /// Array of BundleId. Caller owns and must free.
    pub list_bundles: unsafe extern "C" fn(
        this: *const HostInterface,
    ) -> Array<BundleId>,
    /// Get dependencies for the calling bundle.
    ///
    /// Uses bundle_id from current PluginContext (TLS) to look up declared deps.
    /// Returns an Array of DependencyInfo. Caller must free via `host->free`.
    ///
    /// # Arguments
    /// - `this`: HostInterface pointer (self-passing)
    ///
    /// # Returns
    /// Array of DependencyInfo. Caller owns and must free.
    /// Returns empty array if called outside bundle init context.
    pub get_dependencies: unsafe extern "C" fn(
        this: *const HostInterface,
    ) -> Array<DependencyInfo>,
}

// SAFETY: HostInterface contains an opaque pointer and function pointers.
// The opaque pointer is managed by the runtime.
// Function pointers are inherently thread-safe to call from any thread
// (the functions themselves must handle their own synchronization).
unsafe impl Send for HostInterface {}

// SAFETY: HostInterface contains an opaque pointer and function pointers.
// Concurrent calls to the same interface are safe because the runtime
// handles internal synchronization.
unsafe impl Sync for HostInterface {}

#[cfg(test)]
mod tests {
    use core::mem::{align_of, offset_of, size_of};

    use crate::host::host_interface::HostInterface;

    #[test]
    fn layout_host_interface() {
        // HostInterface: runtime pointer (8 bytes) + 11 extern "C" fn pointers (88 bytes).
        // Fields: register_contract, alloc, free, find_by_contract, find_all_by_contract,
        //         resolve_contract, call_guest_method, get_host_contract,
        //         resolve_host_contract_interface, list_bundles, get_dependencies
        assert_eq!(size_of::<HostInterface>(), 96);
        assert_eq!(align_of::<HostInterface>(), 8);
        assert_eq!(offset_of!(HostInterface, runtime), 0);
        assert_eq!(offset_of!(HostInterface, register_contract), 8);
        assert_eq!(offset_of!(HostInterface, alloc), 16);
        assert_eq!(offset_of!(HostInterface, free), 24);
        assert_eq!(offset_of!(HostInterface, find_by_contract), 32);
        assert_eq!(offset_of!(HostInterface, find_all_by_contract), 40);
        assert_eq!(offset_of!(HostInterface, resolve_contract), 48);
        assert_eq!(offset_of!(HostInterface, call_guest_method), 56);
        assert_eq!(offset_of!(HostInterface, get_host_contract), 64);
        assert_eq!(offset_of!(HostInterface, resolve_host_contract_interface), 72);
        assert_eq!(offset_of!(HostInterface, list_bundles), 80);
        assert_eq!(offset_of!(HostInterface, get_dependencies), 88);
    }

    /// Verify HostInterface has runtime: *mut c_void field at offset 0.
    #[test]
    fn host_interface_has_runtime_field() {
        assert_eq!(offset_of!(HostInterface, runtime), 0);
        assert_eq!(size_of::<*mut core::ffi::c_void>(), 8);
    }

    /// Verify list_bundles field exists.
    #[test]
    fn list_bundles_field_exists() {
        assert_eq!(offset_of!(HostInterface, list_bundles), 80);
    }

    /// Verify get_dependencies field exists.
    #[test]
    fn get_dependencies_field_exists() {
        assert_eq!(offset_of!(HostInterface, get_dependencies), 88);
    }
}
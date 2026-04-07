//! Runtime Interface — function table returned to host from polyplug_runtime_create().
//!
//! # Who provides
//! The runtime creates this struct and returns it from `polyplug_runtime_create()`.
//!
//! # Who calls
//! Host application code calls these functions to interact with the runtime.
//!
//! # Ownership
//! The struct is allocated by `polyplug_runtime_create()`. The host owns the pointer
//! and must call `destroy()` to free the runtime and the interface.
//!
//! # Lifetime
//! Lives until `destroy()` is called.
//!
//! # Thread Safety
//! All functions are safe to call from any thread. The runtime handles
//! internal synchronization.

use core::ffi::{c_char, c_void};

use crate::{
    guest::GuestContractInterface,
    host::HostContractInstance,
    plugin::PluginHandle,
    types::{AbiError, Array, DependencyInfo, StringView},
};

use polyplug_utils::BundleId;

/// Type alias for backward compatibility during transition.
pub type ContractHandle = PluginHandle;

/// Runtime Interface — function table returned to host from polyplug_runtime_create().
///
/// Contains an opaque runtime pointer and function pointers for host calls.
/// All functions take `*const RuntimeInterface` as first parameter.
///
/// # Self-passing pattern
/// Each function receives the interface pointer as its first parameter,
/// allowing hosts to call: `rt->load_bundle(rt, path)`
/// SDKs hide this pattern: `rt.load_bundle(path)`
#[repr(C)]
pub struct RuntimeInterface {
    /// Opaque pointer to Runtime.
    ///
    /// Set during interface creation. Provides access to runtime state.
    pub runtime: *mut c_void,
    /// Load a plugin bundle from the given path.
    ///
    /// # Arguments
    /// - `this`: RuntimeInterface pointer
    /// - `path`: Path to the bundle directory or file
    ///
    /// # Returns
    /// AbiError::OK on success, error code on failure.
    pub load_bundle: unsafe extern "C" fn(this: *const RuntimeInterface, path: *const c_char) -> AbiError,
    /// Reload a bundle (hot-reload).
    ///
    /// # Arguments
    /// - `this`: RuntimeInterface pointer
    /// - `bundle_id`: ID of the bundle to reload
    ///
    /// # Returns
    /// AbiError::OK on success, error code on failure.
    pub reload_bundle: unsafe extern "C" fn(this: *const RuntimeInterface, bundle_id: BundleId) -> AbiError,
    /// Unload a bundle.
    ///
    /// # Arguments
    /// - `this`: RuntimeInterface pointer
    /// - `bundle_id`: ID of the bundle to unload
    ///
    /// # Returns
    /// AbiError::OK on success, error code on failure.
    pub unload_bundle: unsafe extern "C" fn(this: *const RuntimeInterface, bundle_id: BundleId) -> AbiError,
    /// Find a guest contract by contract_id and minimum version.
    ///
    /// # Arguments
    /// - `this`: RuntimeInterface pointer
    /// - `contract_id`: Contract identifier
    /// - `min_version`: Minimum version required
    ///
    /// # Returns
    /// ContractHandle that can be resolved to an interface.
    pub find_by_contract: unsafe extern "C" fn(
        this: *const RuntimeInterface,
        contract_id: u64,
        min_version: u32,
    ) -> ContractHandle,
    /// Find all guest contracts matching contract_id and minimum version.
    ///
    /// Returns an Array of ContractHandle. Caller must free via host->free.
    pub find_all_by_contract: unsafe extern "C" fn(
        this: *const RuntimeInterface,
        contract_id: u64,
        min_version: u32,
    ) -> Array<ContractHandle>,
    /// Resolve a ContractHandle to a GuestContractInterface pointer.
    ///
    /// # Arguments
    /// - `this`: RuntimeInterface pointer
    /// - `handle`: Handle from find_by_contract
    ///
    /// # Returns
    /// Pointer to GuestContractInterface, or null if invalid/stale.
    pub resolve_contract: unsafe extern "C" fn(
        this: *const RuntimeInterface,
        handle: ContractHandle,
    ) -> *const GuestContractInterface,
    /// Get a host contract instance by contract_id and minimum version.
    ///
    /// # Arguments
    /// - `this`: RuntimeInterface pointer
    /// - `contract_id`: Contract identifier
    /// - `min_version`: Minimum version required
    ///
    /// # Returns
    /// HostContractInstance for the contract.
    pub get_host_contract: unsafe extern "C" fn(
        this: *const RuntimeInterface,
        contract_id: u64,
        min_version: u32,
    ) -> HostContractInstance,
    /// Get the last error message.
    ///
    /// # Arguments
    /// - `this`: RuntimeInterface pointer
    ///
    /// # Returns
    /// StringView containing the error message, or empty string if no error.
    pub get_last_error: unsafe extern "C" fn(this: *const RuntimeInterface) -> StringView,
    /// List all loaded bundles.
    ///
    /// Returns an Array of BundleId values. Caller must free via host->free.
    pub list_bundles: unsafe extern "C" fn(
        this: *const RuntimeInterface,
    ) -> Array<BundleId>,
    /// Get dependencies (returns empty array for host context).
    ///
    /// Returns an Array of DependencyInfo. Caller must free via host->free.
    pub get_dependencies: unsafe extern "C" fn(
        this: *const RuntimeInterface,
    ) -> Array<DependencyInfo>,
    /// Destroy the runtime and free this interface.
    ///
    /// # Arguments
    /// - `this`: RuntimeInterface pointer
    ///
    /// # Safety
    /// After calling destroy, the pointer is invalid and must not be used.
    pub destroy: unsafe extern "C" fn(this: *const RuntimeInterface),
}

// SAFETY: RuntimeInterface contains an opaque pointer and function pointers.
// The opaque pointer is managed by the runtime.
// Function pointers are inherently thread-safe to call from any thread
// (the functions themselves must handle their own synchronization).
unsafe impl Send for RuntimeInterface {}

// SAFETY: RuntimeInterface contains an opaque pointer and function pointers.
// Concurrent calls to the same interface are safe because the runtime
// handles internal synchronization.
unsafe impl Sync for RuntimeInterface {}

#[cfg(test)]
mod tests {
    use core::mem::{align_of, offset_of, size_of};

    use crate::host::runtime_interface::RuntimeInterface;

    #[test]
    fn layout_runtime_interface() {
        // RuntimeInterface: runtime pointer (8 bytes) + 11 extern "C" fn pointers (88 bytes).
        // Fields: load_bundle, reload_bundle, unload_bundle, find_by_contract, find_all_by_contract,
        //         resolve_contract, get_host_contract, get_last_error, list_bundles, get_dependencies, destroy
        assert_eq!(size_of::<RuntimeInterface>(), 96);
        assert_eq!(align_of::<RuntimeInterface>(), 8);
        assert_eq!(offset_of!(RuntimeInterface, runtime), 0);
        assert_eq!(offset_of!(RuntimeInterface, load_bundle), 8);
        assert_eq!(offset_of!(RuntimeInterface, reload_bundle), 16);
        assert_eq!(offset_of!(RuntimeInterface, unload_bundle), 24);
        assert_eq!(offset_of!(RuntimeInterface, find_by_contract), 32);
        assert_eq!(offset_of!(RuntimeInterface, find_all_by_contract), 40);
        assert_eq!(offset_of!(RuntimeInterface, resolve_contract), 48);
        assert_eq!(offset_of!(RuntimeInterface, get_host_contract), 56);
        assert_eq!(offset_of!(RuntimeInterface, get_last_error), 64);
        assert_eq!(offset_of!(RuntimeInterface, list_bundles), 72);
        assert_eq!(offset_of!(RuntimeInterface, get_dependencies), 80);
        assert_eq!(offset_of!(RuntimeInterface, destroy), 88);
    }

    /// Verify RuntimeInterface has runtime: *mut c_void field at offset 0.
    #[test]
    fn runtime_interface_has_runtime_field() {
        assert_eq!(offset_of!(RuntimeInterface, runtime), 0);
        assert_eq!(size_of::<*mut core::ffi::c_void>(), 8);
    }

    /// Verify list_bundles field exists.
    #[test]
    fn list_bundles_field_exists() {
        assert_eq!(offset_of!(RuntimeInterface, list_bundles), 72);
    }

    /// Verify get_dependencies field exists.
    #[test]
    fn get_dependencies_field_exists() {
        assert_eq!(offset_of!(RuntimeInterface, get_dependencies), 80);
    }
}
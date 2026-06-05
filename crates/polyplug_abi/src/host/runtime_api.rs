//! Runtime Interface — function table returned to host from polyplug_runtime_create().
//!
//! This module defines `RuntimeApi`, the interface hosts use to interact
//! with the runtime. Hosts receive this interface when creating a runtime.
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
//!
//! # Self-Passing Pattern
//! All functions take `self: *const RuntimeApi` as the first parameter.
//! SDKs hide this detail from users, automatically passing the interface pointer.

use core::ffi::c_void;

use crate::{
    guest::GuestContractInterface,
    host::HostContractInstance,
    plugin::GuestContractHandle,
    types::{AbiError, Array, DependencyInfo, StringView},
};

use polyplug_utils::BundleId;

/// Runtime Interface — function table returned to host from polyplug_runtime_create().
///
/// Contains an opaque runtime pointer and function pointers for host calls.
/// All functions take `*const RuntimeApi` as first parameter.
///
/// # Who provides
/// The runtime creates this struct and returns it from `polyplug_runtime_create()`.
/// The struct is heap-allocated and owned by the host.
///
/// # Who calls
/// Host application code calls these functions to interact with the runtime.
/// SDK-generated wrappers handle the self-passing pattern automatically.
///
/// # Ownership
/// The struct is allocated by `polyplug_runtime_create()`. The host owns
/// the pointer and must call `destroy()` to free the runtime and interface.
///
/// # Lifetime
/// Lives until `destroy()` is called. After destroy, the pointer is invalid.
///
/// # Thread Safety
/// All functions are safe to call from any thread. The runtime uses
/// internal synchronization for shared state.
///
/// # Self-passing pattern
/// Each function receives the interface pointer as its first parameter,
/// allowing hosts to call: `rt->load_bundle(rt, path)`
/// SDKs hide this pattern: `rt.load_bundle(path)`
#[repr(C)]
pub struct RuntimeApi {
    /// Opaque pointer to Runtime.
    ///
    /// Set during interface creation. Provides access to runtime state.
    ///
    /// # Ownership
    /// Owned by the runtime. Host must call `destroy()` to free.
    pub runtime: *mut c_void,
    /// Load a plugin bundle from the given path.
    ///
    /// Loads the bundle and initializes all its guest contracts.
    /// Dependencies are resolved in topological order.
    ///
    /// # Arguments
    /// - `this`: RuntimeApi pointer (self-passing)
    /// - `path`: UTF-8 path to the bundle directory or manifest file (not null-terminated)
    ///
    /// # Returns
    /// AbiError::OK on success, error code on failure.
    /// Use `get_last_error()` for detailed error message.
    pub load_bundle: unsafe extern "C" fn(this: *const RuntimeApi, path: StringView) -> AbiError,
    /// Reload a bundle (hot-reload).
    ///
    /// Triggers hot-reload of the specified bundle. The runtime will:
    /// 1. Call pre-reload callbacks to notify hosts
    /// 2. Wait for all instances to be destroyed
    /// 3. Unload the old bundle
    /// 4. Load the new bundle
    /// 5. Call post-reload callbacks
    ///
    /// # Arguments
    /// - `this`: RuntimeApi pointer (self-passing)
    /// - `bundle_id`: ID of the bundle to reload
    ///
    /// # Returns
    /// AbiError::OK on success, error code on failure.
    pub reload_bundle:
        unsafe extern "C" fn(this: *const RuntimeApi, bundle_id: BundleId) -> AbiError,
    /// Unload a bundle.
    ///
    /// Removes the bundle and all its guest contracts from the registry.
    /// Host must destroy all instances before unloading.
    ///
    /// # Arguments
    /// - `this`: RuntimeApi pointer (self-passing)
    /// - `bundle_id`: ID of the bundle to unload
    ///
    /// # Returns
    /// AbiError::OK on success, error code on failure.
    pub unload_bundle:
        unsafe extern "C" fn(this: *const RuntimeApi, bundle_id: BundleId) -> AbiError,
    /// Find a guest contract by contract_id and minimum version.
    ///
    /// Returns a GuestContractHandle that can be resolved to an interface.
    /// Returns null handle if no matching contract found.
    ///
    /// # Arguments
    /// - `this`: RuntimeApi pointer (self-passing)
    /// - `contract_id`: Contract identifier hash
    /// - `min_version`: Minimum version required
    ///
    /// # Returns
    /// GuestContractHandle for the first matching contract, or null handle.
    pub find_guest_contract: unsafe extern "C" fn(
        this: *const RuntimeApi,
        contract_id: u64,
        min_version: u32,
    ) -> GuestContractHandle,
    /// Find all guest contracts matching contract_id and minimum version.
    ///
    /// Returns an Array of GuestContractHandle. Caller must free via host->free.
    /// Use when multiple implementations of the same contract may exist.
    ///
    /// # Arguments
    /// - `this`: RuntimeApi pointer (self-passing)
    /// - `contract_id`: Contract identifier hash
    /// - `min_version`: Minimum version required
    ///
    /// # Returns
    /// Array of GuestContractHandle. Caller owns and must free.
    pub find_all_by_contract: unsafe extern "C" fn(
        this: *const RuntimeApi,
        contract_id: u64,
        min_version: u32,
    ) -> Array<GuestContractHandle>,
    /// Resolve a GuestContractHandle to a GuestContractInterface pointer.
    ///
    /// Returns null if the handle is invalid or contract was unloaded.
    ///
    /// # Arguments
    /// - `this`: RuntimeApi pointer (self-passing)
    /// - `handle`: GuestContractHandle from find_guest_contract
    ///
    /// # Returns
    /// Pointer to GuestContractInterface, or null if invalid/stale.
    pub resolve_guest_contract: unsafe extern "C" fn(
        this: *const RuntimeApi,
        handle: GuestContractHandle,
    ) -> *const GuestContractInterface,
    /// Get a host contract instance by contract_id and minimum version.
    ///
    /// For singleton host contracts, returns the same instance every time.
    /// For multi-instance host contracts, returns a new instance each time.
    ///
    /// # Arguments
    /// - `this`: RuntimeApi pointer (self-passing)
    /// - `contract_id`: Host contract identifier hash
    /// - `min_version`: Minimum version required
    ///
    /// # Returns
    /// HostContractInstance for the contract.
    pub get_host_contract: unsafe extern "C" fn(
        this: *const RuntimeApi,
        contract_id: u64,
        min_version: u32,
    ) -> HostContractInstance,
    /// Get the last error message.
    ///
    /// Returns detailed error message for the most recent failed operation.
    /// Message is valid until the next operation is performed.
    ///
    /// # Arguments
    /// - `this`: RuntimeApi pointer (self-passing)
    ///
    /// # Returns
    /// StringView containing the error message, or empty string if no error.
    pub get_last_error: unsafe extern "C" fn(this: *const RuntimeApi) -> StringView,
    /// List all loaded bundles.
    ///
    /// Returns an Array of BundleId. Caller must free via host->free.
    /// Bundle IDs are stable for the lifetime of the runtime.
    ///
    /// # Arguments
    /// - `this`: RuntimeApi pointer (self-passing)
    ///
    /// # Returns
    /// Array of BundleId. Caller owns and must free.
    pub list_bundles: unsafe extern "C" fn(this: *const RuntimeApi) -> Array<BundleId>,
    /// Get dependencies (returns empty array for host context).
    ///
    /// Hosts have no bundle dependencies, so this returns an empty array.
    /// Guests use HostApi::get_dependencies for their actual deps.
    ///
    /// # Arguments
    /// - `this`: RuntimeApi pointer (self-passing)
    ///
    /// # Returns
    /// Empty Array of DependencyInfo. Caller owns and must free.
    pub get_dependencies: unsafe extern "C" fn(this: *const RuntimeApi) -> Array<DependencyInfo>,
    /// Destroy the runtime and free this interface.
    ///
    /// # Arguments
    /// - `this`: RuntimeApi pointer (self-passing)
    ///
    /// # Safety
    /// After calling destroy, the pointer is invalid and must not be used.
    /// All instances must be destroyed before calling this.
    pub destroy: unsafe extern "C" fn(this: *const RuntimeApi),
}

// SAFETY: RuntimeApi contains an opaque pointer and function pointers.
// The opaque pointer is managed by the runtime.
// Function pointers are inherently thread-safe to call from any thread
// (the functions themselves must handle their own synchronization).
unsafe impl Send for RuntimeApi {}

// SAFETY: RuntimeApi contains an opaque pointer and function pointers.
// Concurrent calls to the same interface are safe because the runtime
// handles internal synchronization.
unsafe impl Sync for RuntimeApi {}

#[cfg(test)]
mod tests {
    use core::mem::{align_of, offset_of, size_of};

    use crate::host::runtime_api::RuntimeApi;

    #[test]
    fn layout_runtime_api() {
        // RuntimeApi: runtime pointer (8 bytes) + 11 extern "C" fn pointers (88 bytes).
        // Fields: load_bundle, reload_bundle, unload_bundle, find_guest_contract, find_all_by_contract,
        //         resolve_guest_contract, get_host_contract, get_last_error, list_bundles, get_dependencies, destroy
        assert_eq!(size_of::<RuntimeApi>(), 96);
        assert_eq!(align_of::<RuntimeApi>(), 8);
        assert_eq!(offset_of!(RuntimeApi, runtime), 0);
        assert_eq!(offset_of!(RuntimeApi, load_bundle), 8);
        assert_eq!(offset_of!(RuntimeApi, reload_bundle), 16);
        assert_eq!(offset_of!(RuntimeApi, unload_bundle), 24);
        assert_eq!(offset_of!(RuntimeApi, find_guest_contract), 32);
        assert_eq!(offset_of!(RuntimeApi, find_all_by_contract), 40);
        assert_eq!(offset_of!(RuntimeApi, resolve_guest_contract), 48);
        assert_eq!(offset_of!(RuntimeApi, get_host_contract), 56);
        assert_eq!(offset_of!(RuntimeApi, get_last_error), 64);
        assert_eq!(offset_of!(RuntimeApi, list_bundles), 72);
        assert_eq!(offset_of!(RuntimeApi, get_dependencies), 80);
        assert_eq!(offset_of!(RuntimeApi, destroy), 88);
    }

    /// Verify RuntimeApi has runtime: *mut c_void field at offset 0.
    #[test]
    fn runtime_api_has_runtime_field() {
        assert_eq!(offset_of!(RuntimeApi, runtime), 0);
        assert_eq!(size_of::<*mut core::ffi::c_void>(), 8);
    }

    /// Verify list_bundles field exists.
    #[test]
    fn list_bundles_field_exists() {
        assert_eq!(offset_of!(RuntimeApi, list_bundles), 72);
    }

    /// Verify get_dependencies field exists.
    #[test]
    fn get_dependencies_field_exists() {
        assert_eq!(offset_of!(RuntimeApi, get_dependencies), 80);
    }
}

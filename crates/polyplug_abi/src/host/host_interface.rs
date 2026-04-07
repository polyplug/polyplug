//! Host Interface — function table passed to guests during initialization.
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

use core::ffi::c_void;

use crate::{
    guest::{GuestContractInterface, GuestContractInstance},
    plugin::{PluginDescriptor, PluginHandle},
    types::AbiError,
};

/// Type alias for backward compatibility during transition.
/// Will be replaced with ContractHandle in Phase 2.
pub type ContractHandle = PluginHandle;

/// Host Interface — function table passed to guests during initialization.
///
/// Contains an opaque runtime pointer and function pointers for guest calls.
/// All functions use self-passing pattern (receive HostInterface pointer as first parameter).
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
    pub runtime: *mut c_void,
    /// Register a guest contract implementation.
    ///
    /// Called by plugins during `polyplug_init` to register their contracts.
    pub register_contract: unsafe extern "C" fn(
        this: *const HostInterface,
        descriptor: *const PluginDescriptor,
        interface: *const GuestContractInterface,
    ) -> AbiError,
    /// Allocate memory using the host allocator.
    pub alloc: unsafe extern "C" fn(this: *const HostInterface, size: usize, align: usize) -> *mut u8,
    /// Free memory using the host allocator.
    pub free: unsafe extern "C" fn(this: *const HostInterface, ptr: *mut u8, size: usize, align: usize),
    /// Find a guest contract by contract_id and minimum version.
    ///
    /// Returns a ContractHandle that can be resolved to an interface.
    pub find_by_contract: unsafe extern "C" fn(
        this: *const HostInterface,
        contract_id: u64,
        min_version: u32,
    ) -> ContractHandle,
    /// Find all guest contracts matching contract_id and minimum version.
    ///
    /// Returns the number of handles written to the output buffer.
    /// Wave 5 will change this to return `Array<ContractHandle>`.
    pub find_all_by_contract: unsafe extern "C" fn(
        this: *const HostInterface,
        contract_id: u64,
        min_version: u32,
        out: *mut ContractHandle,
        out_cap: usize,
    ) -> usize,
    /// Resolve a ContractHandle to a GuestContractInterface pointer.
    ///
    /// Returns null if the handle is invalid or stale.
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
    /// - `this`: HostInterface pointer
    /// - `instance`: The guest contract instance
    /// - `method_id`: Method index within the contract
    /// - `args`: Pointer to packed arguments
    /// - `out`: Pointer to output buffer for return value
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
    pub get_host_contract: unsafe extern "C" fn(
        this: *const HostInterface,
        contract_id: u64,
        min_version: u32,
    ) -> crate::host::HostContractInstance,
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
        // HostInterface: runtime pointer (8 bytes) + 8 extern "C" fn pointers (64 bytes).
        assert_eq!(size_of::<HostInterface>(), 72);
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
    }

    /// Verify HostInterface has runtime: *mut c_void field at offset 0.
    #[test]
    fn host_interface_has_runtime_field() {
        assert_eq!(offset_of!(HostInterface, runtime), 0);
        assert_eq!(size_of::<*mut core::ffi::c_void>(), 8);
    }

    /// Verify call_guest_method exists (renamed from call_method).
    #[test]
    fn call_guest_method_field_exists() {
        // This is a compile-time verification - the struct must have call_guest_method.
        // offset_of! will fail to compile if the field doesn't exist.
        assert_eq!(offset_of!(HostInterface, call_guest_method), 56);
    }
}
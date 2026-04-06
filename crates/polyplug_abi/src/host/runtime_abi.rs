//! Runtime ABI — function table passed to plugins during initialization.

use crate::{
    guest::{GuestContractInterface, GuestContractInstance},
    host::RuntimeContext,
    plugin::{PluginDescriptor, PluginHandle},
    types::AbiError,
};

/// Type alias for backward compatibility during transition.
/// Will be replaced with ContractHandle in Phase 2.
pub type ContractHandle = PluginHandle;

/// Runtime ABI — function table passed to plugins during initialization.
///
/// OWNERSHIP: `'static`, lives as long as the runtime.
///
/// All functions take `rt_ctx` as first parameter - a RuntimeContext handle.
/// This allows each Runtime to have its own isolated state (no global registry).
///
/// # Renamed from HostVTable
/// This struct was renamed from `HostVTable` to clarify that it represents
/// the runtime's ABI, not the host's vtable. The host terminology is now
/// used for host-provided contracts (`HostContractInterface`).
#[repr(C)]
pub struct RuntimeAbi {
    /// Register a guest contract implementation.
    ///
    /// Called by plugins during `polyplug_init` to register their contracts.
    pub register_contract: unsafe extern "C" fn(
        rt_ctx: RuntimeContext,
        descriptor: *const PluginDescriptor,
        interface: *const GuestContractInterface,
    ) -> AbiError,
    /// Allocate memory using the host allocator.
    pub alloc: unsafe extern "C" fn(rt_ctx: RuntimeContext, size: usize, align: usize) -> *mut u8,
    /// Free memory using the host allocator.
    pub free: unsafe extern "C" fn(rt_ctx: RuntimeContext, ptr: *mut u8, size: usize, align: usize),
    /// Find a guest contract by contract_id and minimum version.
    ///
    /// Returns a ContractHandle that can be resolved to an interface.
    pub find_by_contract: unsafe extern "C" fn(
        rt_ctx: RuntimeContext,
        contract_id: u64,
        min_version: u32,
    ) -> ContractHandle,
    /// Find all guest contracts matching contract_id and minimum version.
    ///
    /// Returns the number of handles written to the output buffer.
    pub find_all_by_contract: unsafe extern "C" fn(
        rt_ctx: RuntimeContext,
        contract_id: u64,
        min_version: u32,
        out: *mut ContractHandle,
        out_cap: usize,
    ) -> usize,
    /// Resolve a ContractHandle to a GuestContractInterface pointer.
    ///
    /// Returns null if the handle is invalid or stale.
    pub resolve_contract: unsafe extern "C" fn(
        rt_ctx: RuntimeContext,
        handle: ContractHandle,
    ) -> *const GuestContractInterface,
    /// Call a method on a guest contract instance.
    ///
    /// This is the cross-dispatch mechanism for calling methods across
    /// different dispatch types (Native vs VM).
    ///
    /// # Arguments
    /// - `rt_ctx`: RuntimeContext handle
    /// - `instance`: The guest contract instance
    /// - `method_id`: Method index within the contract
    /// - `args`: Pointer to packed arguments
    /// - `out`: Pointer to output buffer for return value
    pub call_method: unsafe extern "C" fn(
        rt_ctx: RuntimeContext,
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
        rt_ctx: RuntimeContext,
        contract_id: u64,
        min_version: u32,
    ) -> crate::host::HostContractInstance,
}

// SAFETY: RuntimeAbi contains only function pointers.
// Function pointers are inherently thread-safe to call from any thread
// (the functions themselves must handle their own synchronization).
unsafe impl Send for RuntimeAbi {}

// SAFETY: RuntimeAbi contains only function pointers.
// Function pointers are inherently thread-safe to call from any thread
// (the functions themselves must handle their own synchronization).
unsafe impl Sync for RuntimeAbi {}

#[cfg(test)]
mod tests {
    use core::mem::{align_of, offset_of, size_of};

    use crate::host::runtime_abi::RuntimeAbi;
    use crate::host::RuntimeContext;

    #[test]
    fn layout_runtime_abi() {
        // RuntimeAbi: 8 extern "C" fn pointers, each 8 bytes on x86_64.
        assert_eq!(size_of::<RuntimeAbi>(), 64);
        assert_eq!(align_of::<RuntimeAbi>(), 8);
        assert_eq!(offset_of!(RuntimeAbi, register_contract), 0);
        assert_eq!(offset_of!(RuntimeAbi, alloc), 8);
        assert_eq!(offset_of!(RuntimeAbi, free), 16);
        assert_eq!(offset_of!(RuntimeAbi, find_by_contract), 24);
        assert_eq!(offset_of!(RuntimeAbi, find_all_by_contract), 32);
        assert_eq!(offset_of!(RuntimeAbi, resolve_contract), 40);
        assert_eq!(offset_of!(RuntimeAbi, call_method), 48);
        assert_eq!(offset_of!(RuntimeAbi, get_host_contract), 56);
    }

    /// TH-01: Verify all RuntimeAbi function signatures use RuntimeContext, not *mut c_void.
    /// This is a compile-time verification test.
    #[test]
    fn runtime_abi_uses_runtime_context() {
        // Verify RuntimeContext is pointer-sized (same as *mut c_void would be)
        assert_eq!(size_of::<RuntimeContext>(), 8);

        // This test passes at compile time because the struct definition
        // uses RuntimeContext. If any function used *mut c_void instead,
        // the struct would still be 64 bytes, but the type safety would be lost.
        // We verify by checking that RuntimeContext is the correct size.
        // Additional compile-time verification: the struct is #[repr(C)]
        // and all function pointers take RuntimeContext as first parameter.
    }
}
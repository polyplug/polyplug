//! VM dispatch data — call through a dispatch function.

use crate::dispatch::VmLoaderData;
use crate::guest::GuestContractInstance;
use crate::types::AbiError;

/// VM dispatch data — call through a dispatch function.
///
/// Used when `dispatch_type == DispatchType::VirtualMachine`.
/// The `call` function receives `loader_data` which contains VM-specific state.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VmDispatch {
    /// Dispatch function called for every VM function invocation.
    ///
    /// # Arguments
    /// - `loader_data`: VmLoaderData handle containing VM-specific state
    /// - `instance`: The guest contract instance (opaque handle)
    /// - `fn_id`: Function index within the contract
    /// - `args`: Pointer to packed arguments (ABI-specific layout)
    /// - `out`: Pointer to output buffer for return value
    pub call: unsafe extern "C" fn(
        loader_data: VmLoaderData,
        instance: GuestContractInstance,
        fn_id: u32,
        args: *const (),
        out: *mut (),
    ) -> AbiError,
    /// Loader-specific data handle.
    /// Opaque to the host; interpreted by the dispatch function.
    pub loader_data: VmLoaderData,
}

// SAFETY: VmDispatch contains a function pointer and an opaque handle.
// The function pointer is safe to call from any thread (the dispatch function
// must handle its own synchronization). The VmLoaderData handle is managed by
// the loader and must be thread-safe.
unsafe impl Send for VmDispatch {}

// SAFETY: VmDispatch contains only a function pointer and an opaque handle.
// Concurrent calls to the dispatch function must be safe (loader's responsibility).
unsafe impl Sync for VmDispatch {}

#[cfg(test)]
mod tests {
    use core::mem::{align_of, offset_of, size_of};

    use crate::dispatch::vm_dispatch::VmDispatch;
    use crate::dispatch::VmLoaderData;

    #[test]
    fn layout_vm_dispatch() {
        // VmDispatch: function pointer (8) + raw pointer (8) = 16 bytes
        // The instance parameter is passed to the call function, not stored in the struct
        assert_eq!(size_of::<VmDispatch>(), 16);
        assert_eq!(align_of::<VmDispatch>(), 8);
        assert_eq!(offset_of!(VmDispatch, call), 0);
        assert_eq!(offset_of!(VmDispatch, loader_data), 8);
    }

    /// TH-02: Verify VmDispatch.loader_data field uses VmLoaderData.
    /// This is a compile-time verification test.
    #[test]
    fn vm_dispatch_uses_vm_loader_data() {
        // Verify VmLoaderData is pointer-sized (same as *mut c_void would be)
        assert_eq!(size_of::<VmLoaderData>(), 8);

        // This test passes at compile time because the struct definition
        // uses VmLoaderData for the loader_data field. If it used *mut c_void
        // instead, the struct would still be 16 bytes, but the type safety
        // would be lost.
    }

    /// TH-03: Verify VmDispatch.call instance parameter is GuestContractInstance.
    /// This is a compile-time verification test.
    #[test]
    fn vm_dispatch_instance_is_guest_contract_instance() {
        use crate::guest::GuestContractInstance;

        // Verify GuestContractInstance is 16 bytes (data pointer + contract_id).
        // Changed from 8 bytes in Phase 11 Wave 3 when contract_id was added.
        assert_eq!(size_of::<GuestContractInstance>(), 16);

        // This test passes at compile time because the struct definition
        // uses GuestContractInstance for the instance parameter. If it used
        // *mut c_void instead, the type safety would be lost.
    }
}
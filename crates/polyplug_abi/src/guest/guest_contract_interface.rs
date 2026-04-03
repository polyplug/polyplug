//! Guest Contract Interface — one per contract implemented by a guest (plugin).

use core::ffi::c_void;
use polyplug_utils::GuestContractId;

use crate::{
    dispatch::{dispatch_mechanisms::DispatchMechanisms, dispatch_type::DispatchType},
    guest::GuestContractInstance,
    types::Version,
};

/// Guest Contract Interface — one per contract implemented by a guest (plugin).
///
/// OWNERSHIP: Must be `'static` or intentionally leaked.
/// Never stack-allocated. Never freed while runtime lives.
///
/// # Instance Lifecycle
/// - `create_instance`: Factory function to create new instances
/// - `destroy_instance`: Destructor to clean up instances before hot-reload
///
/// # Dispatch
/// - `dispatch_type == Native`: Call via `dispatch.native.functions[fn_id](instance, args, out)`
/// - `dispatch_type == VirtualMachine`: Call via `dispatch.vm.call(loader_data, instance, fn_id, args, out)`
#[repr(C)]
pub struct GuestContractInterface {
    /// FNV-1a hash of "guest_contract:name@major_version".
    pub contract_id: GuestContractId,
    /// Contract version.
    pub contract_version: Version,
    /// Dispatch mechanism type (Native or VirtualMachine).
    pub dispatch_type: DispatchType,
    /// Create a new instance of this contract.
    ///
    /// # Arguments
    /// - `rt_ctx`: Runtime context (opaque pointer to Runtime)
    /// - `args`: Optional initialization arguments (contract-specific)
    ///
    /// # Returns
    /// Opaque instance handle, or null handle on failure.
    pub create_instance: unsafe extern "C" fn(
        rt_ctx: *mut c_void,
        args: *const (),
    ) -> GuestContractInstance,
    /// Destroy an instance of this contract.
    ///
    /// MUST be called before hot-reload for all instances.
    ///
    /// # Arguments
    /// - `rt_ctx`: Runtime context
    /// - `instance`: Instance handle to destroy
    pub destroy_instance: unsafe extern "C" fn(
        rt_ctx: *mut c_void,
        instance: GuestContractInstance,
    ),
    /// Union of dispatch mechanisms — access based on dispatch_type.
    pub dispatch: DispatchMechanisms,
}

#[cfg(test)]
mod tests {
    use core::mem::{align_of, offset_of, size_of};
    use super::GuestContractInterface;

    #[test]
    fn layout_guest_contract_interface() {
        // GuestContractInterface layout:
        //   contract_id (GuestContractId/u64): 8 bytes @ offset 0
        //   contract_version (Version/3xu32): 12 bytes @ offset 8
        //   dispatch_type (DispatchType/u32): 4 bytes @ offset 20
        //   [padding 4 bytes for alignment]
        //   create_instance (fn ptr): 8 bytes @ offset 24
        //   destroy_instance (fn ptr): 8 bytes @ offset 32
        //   dispatch (union): 16 bytes @ offset 40
        // Total: 56 bytes
        assert_eq!(size_of::<GuestContractInterface>(), 56);
        assert_eq!(align_of::<GuestContractInterface>(), 8);
        assert_eq!(offset_of!(GuestContractInterface, contract_id), 0);
        assert_eq!(offset_of!(GuestContractInterface, contract_version), 8);
        assert_eq!(offset_of!(GuestContractInterface, dispatch_type), 20);
        assert_eq!(offset_of!(GuestContractInterface, create_instance), 24);
        assert_eq!(offset_of!(GuestContractInterface, destroy_instance), 32);
        assert_eq!(offset_of!(GuestContractInterface, dispatch), 40);
    }
}
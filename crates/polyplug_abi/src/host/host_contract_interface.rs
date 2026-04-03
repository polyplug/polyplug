//! Host Contract Interface — for host-provided services.

use core::ffi::c_void;
use polyplug_utils::HostContractId;

use crate::{
    dispatch::{DispatchMechanisms, DispatchType},
    host::HostContractInstance,
    types::Version,
};

/// Host Contract Interface — for host-provided services.
///
/// Host contracts are services provided by the host application to plugins.
///
/// # Singleton Mode
/// - `singleton == true`: Same instance returned for all callers
/// - `singleton == false`: New instance per caller
#[repr(C)]
pub struct HostContractInterface {
    /// FNV-1a hash of "host_contract:name@major_version".
    pub contract_id: HostContractId,
    /// Contract version.
    pub contract_version: Version,
    /// Whether this contract provides a singleton instance.
    /// If true, `get_host_contract` returns the same instance for all callers.
    pub singleton: bool,
    /// Dispatch mechanism type (Native or VirtualMachine).
    pub dispatch_type: DispatchType,
    /// Create a new instance of this host contract.
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
    ) -> HostContractInstance,
    /// Destroy an instance of this host contract.
    ///
    /// For singleton contracts, this is typically a no-op.
    /// For multi-instance contracts, caller must destroy after use.
    pub destroy_instance: unsafe extern "C" fn(
        rt_ctx: *mut c_void,
        instance: HostContractInstance,
    ),
    /// Union of dispatch mechanisms — access based on dispatch_type.
    pub dispatch: DispatchMechanisms,
}

#[cfg(test)]
mod tests {
    use core::mem::{align_of, offset_of, size_of};
    use super::HostContractInterface;

    #[test]
    fn layout_host_contract_interface() {
        // Layout:
        //   contract_id (HostContractId/u64): 8 bytes at offset 0
        //   contract_version (Version): 6 bytes at offset 8
        //   singleton (bool): 1 byte at offset 14
        //   dispatch_type (DispatchType/u32): 4 bytes at offset 15
        //   [padding 1 byte for alignment to 8]
        //   create_instance (fn ptr): 8 bytes at offset 20
        //   destroy_instance (fn ptr): 8 bytes at offset 28
        //   dispatch (union): 16 bytes at offset 36
        // [padding 4 bytes at end for alignment]
        // Total: 56 bytes
        assert_eq!(size_of::<HostContractInterface>(), 56);
        assert_eq!(align_of::<HostContractInterface>(), 8);
        assert_eq!(offset_of!(HostContractInterface, contract_id), 0);
        assert_eq!(offset_of!(HostContractInterface, contract_version), 8);
        assert_eq!(offset_of!(HostContractInterface, singleton), 14);
        assert_eq!(offset_of!(HostContractInterface, dispatch_type), 15);
        assert_eq!(offset_of!(HostContractInterface, create_instance), 16);
        assert_eq!(offset_of!(HostContractInterface, destroy_instance), 24);
        assert_eq!(offset_of!(HostContractInterface, dispatch), 32);
    }
}
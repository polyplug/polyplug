use polyplug_utils::PluginContractId;

use crate::{
    dispatch::{dispatch_mechanisms::DispatchMechanisms, dispatch_type::DispatchType},
    types::Version,
};

/// Plugin Interface — one per contract implemented by a plugin.
///
/// OWNERSHIP: Must be `'static` or intentionally leaked.
/// Never stack-allocated. Never freed while runtime lives.
///
/// # Dispatch
/// - `dispatch_type == Native`: Call via `dispatch.native.functions[fn_id]`
/// - `dispatch_type == VirtualMachine`: Call via `dispatch.vm.call(...)`
#[repr(C)]
pub struct PluginInterface {
    /// FNV-1a hash of "contract_name@major_version".
    pub contract_id: PluginContractId,
    /// Contract version.
    pub contract_version: Version,
    /// Dispatch mechanism type (Native or VirtualMachine).
    pub dispatch_type: DispatchType,
    /// Union of dispatch mechanisms — access based on dispatch_type.
    pub dispatch: DispatchMechanisms,
}

#[cfg(test)]
mod tests {
    use core::mem::{align_of, offset_of, size_of};

    use crate::plugin::plugin_interface::PluginInterface;

    #[test]
    fn layout_plugin_interface() {
        // PluginInterface layout:
        //   contract_id (PluginContractId/u64): 8 bytes @ offset 0
        //   contract_version (Version/3xu32): 12 bytes @ offset 8
        //   dispatch_type (DispatchType/u32): 4 bytes @ offset 20
        //   [padding 4 bytes for alignment]
        //   dispatch (union): 16 bytes @ offset 24
        // Total: 40 bytes
        assert_eq!(size_of::<PluginInterface>(), 40);
        assert_eq!(align_of::<PluginInterface>(), 8);
        assert_eq!(offset_of!(PluginInterface, contract_id), 0);
        assert_eq!(offset_of!(PluginInterface, contract_version), 8);
        assert_eq!(offset_of!(PluginInterface, dispatch_type), 20);
        assert_eq!(offset_of!(PluginInterface, dispatch), 24);
    }
}
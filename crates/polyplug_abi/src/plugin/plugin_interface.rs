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
    pub contract_id: u64,
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
        assert_eq!(size_of::<PluginInterface>(), 40);
        assert_eq!(align_of::<PluginInterface>(), 8);
        assert_eq!(offset_of!(PluginInterface, contract_id), 0);
        assert_eq!(offset_of!(PluginInterface, contract_version), 8);
        assert_eq!(offset_of!(PluginInterface, dispatch_type), 14);
        assert_eq!(offset_of!(PluginInterface, dispatch), 18);
    }
}
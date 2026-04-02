use crate::dispatch::{native_dispatch::NativeDispatch, vm_dispatch::VmDispatch};

/// Union of dispatch mechanisms — use based on `dispatch_type`.
///
/// # Safety
/// Access the correct variant based on `PluginInterface::dispatch_type`:
/// - `dispatch_type == Native` → access `.native`
/// - `dispatch_type == VirtualMachine` → access `.vm`
#[repr(C)]
pub union DispatchMechanisms {
    /// Native dispatch data (when dispatch_type == Native).
    pub native: NativeDispatch,
    /// VM dispatch data (when dispatch_type == VirtualMachine).
    pub vm: VmDispatch,
}

#[cfg(test)]
mod tests {
    use core::mem::{align_of, size_of};

    use crate::dispatch::dispatch_mechanisms::DispatchMechanisms;

    #[test]
    fn layout_plugin_dispatch() {
        assert_eq!(size_of::<DispatchMechanisms>(), 16);
        assert_eq!(align_of::<DispatchMechanisms>(), 8);
    }
}
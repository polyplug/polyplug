use core::ffi::c_void;

use crate::{
    plugin::{
        plugin_descriptor::PluginDescriptor, plugin_handle::PluginHandle,
        plugin_interface::PluginInterface,
    },
    types::abi_error::AbiError,
};

/// Host capabilities passed to every plugin at init time.
///
/// OWNERSHIP: `'static`, lives as long as the runtime.
///
/// All functions take `rt_ctx` as first parameter - an opaque pointer to the Runtime.
/// This allows each Runtime to have its own isolated state (no global registry).
#[repr(C)]
pub struct HostVTable {
    pub register_plugin: unsafe extern "C" fn(
        rt_ctx: *mut c_void,
        descriptor: *const PluginDescriptor,
        vtable: *const PluginInterface,
    ) -> AbiError,
    pub alloc: unsafe extern "C" fn(rt_ctx: *mut c_void, size: usize, align: usize) -> *mut u8,
    pub free: unsafe extern "C" fn(rt_ctx: *mut c_void, ptr: *mut u8, size: usize, align: usize),
    pub find_by_contract: unsafe extern "C" fn(
        rt_ctx: *mut c_void,
        contract_id: u64,
        min_version: u32,
    ) -> PluginHandle,
    pub find_by_bundle: unsafe extern "C" fn(
        rt_ctx: *mut c_void,
        bundle_id: u64,
        contract_id: u64,
        min_version: u32,
    ) -> PluginHandle,
    pub find_all_by_contract: unsafe extern "C" fn(
        rt_ctx: *mut c_void,
        contract_id: u64,
        min_version: u32,
        out: *mut PluginHandle,
        out_cap: usize,
    ) -> usize,
    pub resolve_plugin:
        unsafe extern "C" fn(rt_ctx: *mut c_void, handle: PluginHandle) -> *const PluginInterface,
    // /// Get host contract vtable by contract_id and minimum version.
    // /// Returns null if no host contract matches the criteria.
    // pub get_host_contract: unsafe extern "C" fn(
    //     rt_ctx: *mut c_void,
    //     contract_id: u64,
    //     min_version: u32,
    // ) -> *const HostContractVTable,
}

// SAFETY: HostVTable contains only function pointers.
// Function pointers are inherently thread-safe to call from any thread
// (the functions themselves must handle their own synchronization).
unsafe impl Send for HostVTable {}

// SAFETY: HostVTable contains only function pointers.
// Function pointers are inherently thread-safe to call from any thread
// (the functions themselves must handle their own synchronization).
unsafe impl Sync for HostVTable {}

#[cfg(test)]
mod tests {
    use core::mem::{align_of, offset_of, size_of};

    use crate::host::host_vtable::HostVTable;

    #[test]
    fn layout_host_vtable() {
        // HostVTable: 8 extern "C" fn pointers, each 8 bytes on x86_64.
        assert_eq!(size_of::<HostVTable>(), 64);
        assert_eq!(align_of::<HostVTable>(), 8);
        assert_eq!(offset_of!(HostVTable, register_plugin), 0);
        assert_eq!(offset_of!(HostVTable, alloc), 8);
        assert_eq!(offset_of!(HostVTable, free), 16);
        assert_eq!(offset_of!(HostVTable, find_by_contract), 24);
        assert_eq!(offset_of!(HostVTable, find_by_bundle), 32);
        assert_eq!(offset_of!(HostVTable, find_all_by_contract), 40);
        assert_eq!(offset_of!(HostVTable, resolve_plugin), 48);
        // assert_eq!(offset_of!(HostVTable, get_host_contract), 56);
    }
}

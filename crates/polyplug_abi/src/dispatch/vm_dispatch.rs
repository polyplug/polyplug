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
    /// - `loader_data`: VM-specific data (cast from `*mut c_void`)
    /// - `fn_id`: Function index within the contract
    /// - `args`: Pointer to packed arguments (ABI-specific layout)
    /// - `out`: Pointer to output buffer for return value
    pub call: unsafe extern "C" fn(
        loader_data: *mut core::ffi::c_void,
        fn_id: u32,
        args: *const (),
        out: *mut (),
    ) -> AbiError,
    /// Loader-specific data (e.g., LuaLoaderData, JsLoaderData).
    /// Opaque to the host; interpreted by the dispatch function.
    pub loader_data: *mut core::ffi::c_void,
}

// SAFETY: VmDispatch contains a function pointer and a raw pointer.
// The function pointer is safe to call from any thread (the dispatch function
// must handle its own synchronization). The loader_data pointer is owned by
// the loader and must be thread-safe.
unsafe impl Send for VmDispatch {}

// SAFETY: VmDispatch contains only a function pointer and a raw pointer.
// Concurrent calls to the dispatch function must be safe (loader's responsibility).
unsafe impl Sync for VmDispatch {}

#[cfg(test)]
mod tests {
    use core::mem::{align_of, offset_of, size_of};

    use crate::dispatch::vm_dispatch::VmDispatch;

    #[test]
    fn layout_vm_dispatch() {
        assert_eq!(size_of::<VmDispatch>(), 16);
        assert_eq!(align_of::<VmDispatch>(), 8);
        assert_eq!(offset_of!(VmDispatch, call), 0);
        assert_eq!(offset_of!(VmDispatch, loader_data), 8);
    }
}

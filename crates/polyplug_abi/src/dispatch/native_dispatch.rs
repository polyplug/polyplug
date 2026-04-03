/// Native dispatch data — direct function pointer array.
///
/// Used when `dispatch_type == DispatchType::Native`.
/// The `functions` array contains `function_count` function pointers.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct NativeDispatch {
    /// Number of valid entries in the dispatch array.
    pub function_count: u32,
    /// Pointer to a static array of function pointers, indexed by function_id.
    pub functions: *const *const (),
}

// SAFETY: NativeDispatch contains only a pointer to static data.
// The function pointers are 'static and safe to call from any thread.
unsafe impl Send for NativeDispatch {}

// SAFETY: NativeDispatch contains only a pointer to static data.
// Concurrent reads of the pointer are safe.
unsafe impl Sync for NativeDispatch {}

#[cfg(test)]
mod tests {
    use core::mem::{align_of, offset_of, size_of};

    use crate::dispatch::native_dispatch::NativeDispatch;

    #[test]
    fn layout_native_dispatch() {
        // NativeDispatch: u32 (4) + pointer (8) + padding (4) = 16 bytes
        assert_eq!(size_of::<NativeDispatch>(), 16);
        assert_eq!(align_of::<NativeDispatch>(), 8);
        assert_eq!(offset_of!(NativeDispatch, function_count), 0);
        assert_eq!(offset_of!(NativeDispatch, functions), 8);
    }
}

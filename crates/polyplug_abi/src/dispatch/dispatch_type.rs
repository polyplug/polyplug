
/// Dispatch mechanism type — determines how function calls are routed.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchType {
    /// Native dispatch: direct function pointer calls (zero overhead).
    Native = 0,
    /// VM dispatch: call through a dispatch function with loader_data.
    VirtualMachine = 1,
}

#[cfg(test)]
mod tests {
    use core::mem::{align_of, size_of};

    use crate::dispatch::dispatch_type::DispatchType;

    #[test]
    fn layout_dispatch_type() {
        assert_eq!(size_of::<DispatchType>(), 4);
        assert_eq!(align_of::<DispatchType>(), 4);
    }
}
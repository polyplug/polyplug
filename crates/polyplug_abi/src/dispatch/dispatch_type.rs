/// Dispatch mechanism type — determines how function calls are routed.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchType {
    /// Native dispatch: direct function pointer calls (zero overhead).
    Native = 0,
    /// VM dispatch: call through a dispatch function with loader_data.
    VirtualMachine = 1,
}

impl DispatchType {
    /// Convert from a raw `u32` arriving across the C ABI.
    ///
    /// Plugins are untrusted: a plugin-provided interface struct can carry any
    /// 32-bit pattern in its `dispatch_type` field, including values that are
    /// not declared discriminants of this `#[repr(u32)]` enum. Reinterpreting
    /// such a value as the enum would be undefined behaviour, so registration
    /// boundaries read the field as a raw `u32` and convert it here. The
    /// conversion is TOTAL and SAFE: `0` and `1` map to their variants, and
    /// every other value yields `None` so the caller can reject the interface.
    #[inline]
    pub const fn from_u32(value: u32) -> Option<DispatchType> {
        match value {
            0 => Some(DispatchType::Native),
            1 => Some(DispatchType::VirtualMachine),
            _ => None,
        }
    }
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

    #[test]
    fn from_u32_maps_known_and_rejects_unknown() {
        assert_eq!(DispatchType::from_u32(0), Some(DispatchType::Native));
        assert_eq!(
            DispatchType::from_u32(1),
            Some(DispatchType::VirtualMachine)
        );
        // Untrusted plugins may supply any pattern; unknown values are rejected
        // rather than materialized as an invalid enum discriminant (UB).
        assert_eq!(DispatchType::from_u32(2), None);
        assert_eq!(DispatchType::from_u32(u32::MAX), None);
    }
}

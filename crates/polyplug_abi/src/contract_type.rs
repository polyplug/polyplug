// Side of a contract relationship at the ABI boundary.
//
// `Host` denotes a contract the host exposes to plugins; `Plugin` denotes a contract
// a plugin exposes to the host. The type is reserved ABI surface: it is emitted into
// the generated SDK `abi` files for every language, but no Rust call site consumes it
// yet. The owner should decide whether to wire it into registration/dispatch or remove
// it. (Kept as a non-doc comment so the SDK abi files stay byte-identical — the build
// extractor only emits `///` doc comments into generated SDKs.)
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContractType {
    // A contract the host exposes to plugins.
    Host = 0,
    // A contract a plugin exposes to the host.
    Plugin = 1,
}

impl ContractType {
    /// Convert from a raw `u32` arriving across the C ABI.
    ///
    /// Mirrors `DispatchType::from_u32`: the conversion is TOTAL and SAFE. `0` and `1`
    /// map to their variants; every other value yields `None` so an untrusted producer
    /// cannot materialize an invalid `#[repr(u32)]` discriminant (which would be UB).
    #[inline]
    pub const fn from_u32(value: u32) -> Option<ContractType> {
        match value {
            0 => Some(ContractType::Host),
            1 => Some(ContractType::Plugin),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::contract_type::ContractType;

    #[test]
    fn from_u32_maps_known_and_rejects_unknown() {
        assert_eq!(ContractType::from_u32(0), Some(ContractType::Host));
        assert_eq!(ContractType::from_u32(1), Some(ContractType::Plugin));
        assert_eq!(ContractType::from_u32(2), None);
        assert_eq!(ContractType::from_u32(u32::MAX), None);
    }
}

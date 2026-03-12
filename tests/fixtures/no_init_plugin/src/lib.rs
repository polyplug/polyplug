//! Minimal plugin fixture that exports polyplug_abi_version but NOT polyplug_init.
//! Used to test LoaderError::MissingSymbol { symbol: "polyplug_init" }.

/// ABI version constant — makes this a recognisable polyplug plugin binary.
#[unsafe(no_mangle)]
pub extern "C" fn polyplug_abi_version() -> u32 {
    1_u32
}

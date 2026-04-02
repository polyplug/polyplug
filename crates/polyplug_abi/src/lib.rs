//! ABI — `#[repr(C)]` types, constants, and FNV-1a hashing for the polyplug ABI boundary.
//!
//! Type definitions are sourced from `abi.toml` in this crate's root.

pub mod compatibility;
pub mod contract_type;
pub mod dispatch;
pub mod ffi;
pub mod host;
pub mod plugin;
pub mod runtime_config;
pub mod runtime_language;
pub mod tracking;
pub mod types;

// ABI version sentinel — all bundles must export a function returning this value.
pub const POLYPLUG_ABI_VERSION: u32 = 1;

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use crate::types::{abi_error::AbiError, error_code::AbiErrorCode};

    #[test]
    fn test_abi_error_ok() {
        let e: AbiError = AbiError::ok();
        assert!(e.is_ok());
        assert_eq!(e.code, AbiErrorCode::Ok as u32);
    }
}

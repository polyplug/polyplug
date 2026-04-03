//! ABI — `#[repr(C)]` types, constants, and FNV-1a hashing for the polyplug ABI boundary.
//!
//! Type definitions are sourced from `abi.toml` in this crate's root.

pub mod contract_type;
pub mod dispatch;
pub mod ffi;
pub mod guest;
pub mod host;
pub mod plugin;
pub mod runtime;
mod runtime_language;
pub mod tracking;
pub mod types;

pub use runtime_language::RuntimeLanguage;

// ─── Runtime exports ──────────────────────────────────────────────────────────

pub use runtime::{Compatibility, RuntimeConfig, ReloadPhaseData, ReloadPhaseType};

// ─── Type exports ───────────────────────────────────────────────────────────

pub use types::AbiErrorCode;

// ─── FFI Helper Function exports ────────────────────────────────────────────

pub use types::{abi_error_ok, string_view_null, string_view_from_static};

// ─── New exports from guest module ───────────────────────────────────────────

pub use guest::{GuestContractInterface, GuestContractInstance};

// ─── New exports from host module ────────────────────────────────────────────

pub use host::{HostContractInterface, HostContractInstance, RuntimeAbi};

// ─── Legacy aliases for transition (Phase 6 removes these) ────────────────────

/// Legacy alias for GuestContractInterface during transition.
/// Will be removed in Phase 6.
pub type PluginInterface = GuestContractInterface;

/// Legacy alias for RuntimeAbi during transition.
/// Will be removed in Phase 6.
pub type HostVTable = RuntimeAbi;

pub use plugin::{PluginHandle, PluginDescriptor, PluginContext};

// ─── ABI version sentinel ────────────────────────────────────────────────────

/// ABI version sentinel — all bundles must export a function returning this value.
pub const POLYPLUG_ABI_VERSION: u32 = 1;

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use crate::types::{AbiError, AbiErrorCode};

    #[test]
    fn test_abi_error_ok() {
        let e: AbiError = AbiError::ok();
        assert!(e.is_ok());
        assert_eq!(e.code, AbiErrorCode::Ok);
    }
}
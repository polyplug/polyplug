//! ABI — `#[repr(C)]` types, constants, and FNV-1a hashing for the polyplug ABI boundary.
//!
//! The `#[repr(C)]` type definitions in the submodules below are the single
//! source of truth for the ABI; the per-language SDK type files (abi.lua,
//! abi.py, abi.hpp, …) are generated to match them.

pub mod contract_type;
pub mod dispatch;
pub mod ffi;
pub mod guest;
pub mod host;
pub mod plugin;
pub mod runtime;
mod runtime_language;
#[cfg(feature = "tracking")]
pub mod tracking;
pub mod types;

pub use runtime_language::RuntimeLanguage;

// ─── Runtime exports ──────────────────────────────────────────────────────────

pub use runtime::{Compatibility, ReloadPhase, ReloadPhaseType, RuntimeConfig};

// ─── Type exports ───────────────────────────────────────────────────────────

pub use types::{
    AbiError, AbiErrorCode, ArenaOverflowBlock, Array, Buffer, CallArena, DependencyInfo, LogLevel,
    StringView, Version,
};

// ─── Dispatch exports ────────────────────────────────────────────────────────

pub use dispatch::{DispatchMechanisms, DispatchType, NativeDispatch, VmDispatch, VmLoaderData};

// ─── FFI Helper Function exports ────────────────────────────────────────────

pub use types::{abi_error_ok, string_view_from_static, string_view_null};

// ─── New exports from guest module ───────────────────────────────────────────

pub use guest::{GuestContractInstance, GuestContractInterface};

// ─── New exports from host module ────────────────────────────────────────────

pub use host::{HostApi, HostContractInstance, HostContractInterface};

pub use plugin::{BundleInitContext, GuestContractHandle, PluginDescriptor};

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
        assert_eq!(e.code, AbiErrorCode::Ok as u32);
    }
}

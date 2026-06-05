//! RuntimeLanguageBridge — trait for bridging between runtime and VM-based hosts.
//!
//! This trait defines the interface that VM-based host implementations (Python, Lua, JavaScript)
//! must provide to enable host contract registration and dispatch. The bridge allows VM-based
//! hosts to register host contract implementations and route calls through VM-specific dispatch
//! mechanisms.
//!
//! # Architecture
//!
//! The polyplug runtime supports two dispatch modes for host contracts:
//! - **Native dispatch**: Direct function pointer calls (zero overhead) — used by Rust hosts
//! - **VM dispatch**: Call through a dispatch function with bridge_data — used by VM-based hosts
//!
//! VM-based hosts (Python, Lua, JavaScript) implement this trait to:
//! 1. Register host contract implementations in their VM's native format
//! 2. Provide a dispatch function that translates ABI calls to VM calls
//!
//! # Thread Safety
//!
//! All implementations must be `Send + Sync` because:
//! - The runtime may be accessed from multiple threads concurrently
//! - Host contract calls can occur from any plugin thread
//! - The bridge must handle its own synchronization for VM state

use core::any::Any;

use polyplug_abi::{RuntimeLanguage, types::AbiError};

/// Errors from the host runtime bridge operations.
#[derive(Debug, thiserror::Error)]
pub enum BridgeError {
    /// A host contract with this ID is already registered.
    #[error("duplicate host contract registration: contract_id=0x{contract_id:016X}")]
    DuplicateContract { contract_id: u64 },

    /// The specified host contract was not found.
    #[error("host contract not found: contract_id=0x{contract_id:016X}")]
    ContractNotFound { contract_id: u64 },

    /// The implementation type does not match the expected type for this contract.
    #[error(
        "implementation type mismatch for contract_id=0x{contract_id:016X}: expected {expected}, got {got}"
    )]
    TypeMismatch {
        contract_id: u64,
        expected: String,
        got: String,
    },

    /// The VM failed to register the host contract implementation.
    #[error("VM registration failed for contract_id=0x{contract_id:016X}: {reason}")]
    VmRegistrationFailed { contract_id: u64, reason: String },

    /// The VM dispatch function failed to execute the host contract call.
    #[error("VM dispatch failed for contract_id=0x{contract_id:016X} fn_id={fn_id}: {reason}")]
    VmDispatchFailed {
        contract_id: u64,
        fn_id: u32,
        reason: String,
    },

    /// The bridge is not initialized or has been shut down.
    #[error("host runtime bridge not initialized for runtime={runtime:?}")]
    BridgeNotInitialized { runtime: RuntimeLanguage },
}

/// Trait that bridges between the polyplug runtime and VM-based hosts.
///
/// This trait is implemented by VM-specific bridges (Python, Lua, JavaScript) to enable
/// host contract registration and dispatch. The bridge translates between the ABI-level
/// host contract interface and the VM's native calling conventions.
///
/// # Implementation Requirements
///
/// Implementations must:
/// - Be `Send + Sync` for thread-safe access
/// - Store registered implementations in a thread-safe manner
/// - Handle VM-specific type conversions at the ABI boundary
/// - Provide proper error handling for VM exceptions
///
/// # Usage
///
/// The runtime uses this bridge when:
/// - A VM-based host registers a host contract implementation
/// - A plugin calls a host contract function through VM dispatch
///
/// # Example
///
/// ```rust,ignore
/// use polyplug::host_bridge::{RuntimeLanguageBridge, BridgeError};
/// use polyplug_abi::{RuntimeLanguage, AbiError};
///
/// struct PythonBridge {
///     // Python VM state and registered contracts
/// }
///
/// impl RuntimeLanguageBridge for PythonBridge {
///     fn runtime_type(&self) -> RuntimeLanguage {
///         RuntimeLanguage::Python
///     }
///
///     fn register_host_contract(
///         &mut self,
///         contract_id: u64,
///         implementation: Box<dyn Any>,
///     ) -> Result<(), BridgeError> {
///         // Convert implementation to Python object and register
///         // ...
///     }
///
///     fn call_host_contract(
///         &self,
///         contract_id: u64,
///         fn_id: u32,
///         args: *const (),
///         out: *mut (),
///     ) -> AbiError {
///         // Dispatch call to Python implementation
///         // ...
///     }
/// }
/// ```
pub trait RuntimeLanguageBridge: Send + Sync {
    /// Returns the host runtime type this bridge supports.
    ///
    /// This identifies whether the bridge handles Python, Lua, JavaScript, or Rust.
    fn runtime_type(&self) -> RuntimeLanguage;

    /// Register a host contract implementation.
    ///
    /// The `implementation` is a VM-specific boxed type that the bridge interprets
    /// according to its VM's conventions. For example:
    /// - Python bridge: `Box<dyn Any>` contains a Python object reference
    /// - Lua bridge: `Box<dyn Any>` contains a Lua table/function reference
    /// - JavaScript bridge: `Box<dyn Any>` contains a JS object reference
    ///
    /// # Arguments
    ///
    /// - `contract_id`: The FNV-1a hash of `"host_contract:name@major"`
    /// - `implementation`: VM-specific implementation object
    ///
    /// # Errors
    ///
    /// Returns `BridgeError::DuplicateContract` if a contract with this ID
    /// is already registered.
    ///
    /// Returns `BridgeError::TypeMismatch` if the implementation type does not
    /// match the expected type for this contract.
    ///
    /// Returns `BridgeError::VmRegistrationFailed` if the VM failed to register
    /// the implementation.
    fn register_host_contract(
        &mut self,
        contract_id: u64,
        implementation: Box<dyn Any>,
    ) -> Result<(), BridgeError>;

    /// Call a host contract function.
    ///
    /// This method is invoked when a plugin calls a host contract function through
    /// VM dispatch. The bridge must:
    /// 1. Look up the registered implementation for `contract_id`
    /// 2. Convert ABI-level arguments to VM-native types
    /// 3. Invoke the function identified by `fn_id`
    /// 4. Convert the result back to ABI format and write to `out`
    ///
    /// # Arguments
    ///
    /// - `contract_id`: The contract ID to look up
    /// - `fn_id`: Function index within the contract (0-based)
    /// - `args`: Pointer to packed ABI arguments (layout defined by contract)
    /// - `out`: Pointer to output buffer for return value
    ///
    /// # Returns
    ///
    /// - `abi_error_ok()` on success
    /// - `AbiError { code: HostContractNotFound, ... }` if contract not found
    /// - `AbiError { code: HostContractCallFailed, ... }` if dispatch failed
    ///
    /// # Safety
    ///
    /// This method is inherently unsafe because it deals with raw pointers:
    /// - `args` must point to valid ABI-packed arguments for the contract
    /// - `out` must point to a valid buffer sized for the return type
    /// - The caller must ensure proper alignment of both pointers
    unsafe fn call_host_contract(
        &self,
        contract_id: u64,
        fn_id: u32,
        args: *const (),
        out: *mut (),
    ) -> AbiError;
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;

    // --- BridgeError Display Tests ---

    #[test]
    fn bridge_error_duplicate_contract_display() {
        let err: BridgeError = BridgeError::DuplicateContract {
            contract_id: 0xABCD_EF01_2345_6789,
        };
        let s: String = err.to_string();
        assert!(s.contains("duplicate"), "got: {s}");
        assert!(s.to_lowercase().contains("abcdef01"), "got: {s}");
    }

    #[test]
    fn bridge_error_contract_not_found_display() {
        let err: BridgeError = BridgeError::ContractNotFound {
            contract_id: 0x1234_5678_9ABC_DEF0,
        };
        let s: String = err.to_string();
        assert!(s.contains("not found"), "got: {s}");
        assert!(s.to_lowercase().contains("12345678"), "got: {s}");
    }

    #[test]
    fn bridge_error_type_mismatch_display() {
        let err: BridgeError = BridgeError::TypeMismatch {
            contract_id: 0xCAFE_BABE_0000_0001,
            expected: "PyObject".to_owned(),
            got: "LuaTable".to_owned(),
        };
        let s: String = err.to_string();
        assert!(s.contains("type mismatch"), "got: {s}");
        assert!(s.contains("PyObject"), "got: {s}");
        assert!(s.contains("LuaTable"), "got: {s}");
    }

    #[test]
    fn bridge_error_vm_registration_failed_display() {
        let err: BridgeError = BridgeError::VmRegistrationFailed {
            contract_id: 0xDEAD_BEEF_0000_0001,
            reason: "interpreter shutdown".to_owned(),
        };
        let s: String = err.to_string();
        assert!(s.contains("registration failed"), "got: {s}");
        assert!(s.contains("interpreter shutdown"), "got: {s}");
    }

    #[test]
    fn bridge_error_vm_dispatch_failed_display() {
        let err: BridgeError = BridgeError::VmDispatchFailed {
            contract_id: 0xF00D_0000_0001,
            fn_id: 3,
            reason: "Python exception raised".to_owned(),
        };
        let s: String = err.to_string();
        assert!(s.contains("dispatch failed"), "got: {s}");
        assert!(s.contains('3'), "got: {s}");
        assert!(s.contains("Python exception"), "got: {s}");
    }

    #[test]
    fn bridge_error_bridge_not_initialized_display() {
        let err: BridgeError = BridgeError::BridgeNotInitialized {
            runtime: RuntimeLanguage::Python,
        };
        let s: String = err.to_string();
        assert!(s.contains("not initialized"), "got: {s}");
        assert!(s.contains("Python"), "got: {s}");
    }
}

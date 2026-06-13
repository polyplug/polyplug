//! LuaHostBridge — Bridge for Lua hosts implementing host contracts.
//!
//! This module provides the bridge that allows Lua hosts to implement
//! host contracts. The bridge stores Lua callable functions and dispatches
//! calls through the LuaJIT VM.
//!
//! # Architecture
//!
//! When a Lua host registers a host contract implementation:
//! 1. The Lua callable is stored in a HashMap keyed by contract_id
//! 2. When a plugin calls a host contract function, the bridge:
//!    - Looks up the Lua callable
//!    - Invokes it with converted arguments
//!    - Converts the result back to ABI format
//!
//! # Thread Safety
//!
//! The bridge uses `RwLock` to protect the contracts HashMap because:
//! - Registration happens during initialization (write lock)
//! - Calls happen during plugin execution (read lock)
//! - mlua's `send` feature makes `Lua` and `Function` Send/Sync safe
//!
//! # Lua VM Ownership
//!
//! The bridge owns its own `Lua` instance for complete isolation from
//! plugin Lua VMs. This ensures:
//! - Host contract implementations don't interfere with plugin state
//! - Multiple Runtime instances can have isolated Lua host bridges

use std::collections::HashMap;
use std::sync::RwLock;

use mlua::Function;
use mlua::Lua;

use polyplug::host_bridge::BridgeError;
use polyplug::host_bridge::RuntimeLanguageBridge;
use polyplug_abi::AbiError;
use polyplug_abi::AbiErrorCode;
use polyplug_abi::StringView;
use polyplug_abi::SupportedLanguage;

/// Bridge for Lua hosts implementing host contracts.
///
/// This bridge stores Lua callable functions and dispatches calls through
/// the LuaJIT VM. The bridge handles:
/// - Thread-safe storage of registered implementations
/// - Lua error handling and conversion to AbiError
/// - Per-bridge Lua VM isolation
///
/// # Example
///
/// ```rust,ignore
/// use polyplug_lua::bridge::LuaHostBridge;
/// use polyplug::host_bridge::RuntimeLanguageBridge;
///
/// let bridge = LuaHostBridge::new();
///
/// // Register a Lua implementation
/// let lua = bridge.lua();
/// let callable = lua.load("function(fn_id, args, out) return fn_id end").eval::<Function>().unwrap();
/// bridge.register_host_contract(1234, Box::new(callable));
///
/// // Call through the bridge
/// let result = bridge.call_host_contract(1234, 0, args_ptr, out_ptr);
/// ```
pub struct LuaHostBridge {
    /// The Lua VM owned by this bridge.
    /// SAFETY: mlua's `send` feature makes Lua Send + Sync.
    lua: Lua,

    /// Registered host contract implementations.
    /// Key: contract_id (FNV-1a hash of "host_contract:name@major")
    /// Value: Lua callable function (Function)
    contracts: RwLock<HashMap<u64, Function>>,
}

impl LuaHostBridge {
    /// Create a new LuaHostBridge with a fresh LuaJIT VM.
    ///
    /// # Safety
    ///
    /// Uses `Lua::unsafe_new()` to enable the LuaJIT FFI module, which is
    /// required for host contract implementations that need to interact with
    /// ABI structures (struct layout, pointer casts).
    ///
    /// We trust the Lua scripts registered through this bridge.
    pub fn new() -> LuaHostBridge {
        // SAFETY: We trust the Lua scripts loaded through this bridge.
        // The LuaJIT FFI is required for host contract implementations that
        // need to interact with ABI structures.
        let lua: Lua = unsafe { Lua::unsafe_new() };

        LuaHostBridge {
            lua,
            contracts: RwLock::new(HashMap::new()),
        }
    }

    /// Create a new LuaHostBridge with pre-allocated capacity.
    pub fn with_capacity(capacity: usize) -> LuaHostBridge {
        // SAFETY: Same as new() — we trust the Lua scripts.
        let lua: Lua = unsafe { Lua::unsafe_new() };

        LuaHostBridge {
            lua,
            contracts: RwLock::new(HashMap::with_capacity(capacity)),
        }
    }

    /// Get a reference to the underlying Lua VM.
    ///
    /// This allows host code to create Lua functions for registration.
    pub fn lua(&self) -> &Lua {
        &self.lua
    }
}

impl Default for LuaHostBridge {
    fn default() -> LuaHostBridge {
        LuaHostBridge::new()
    }
}

impl RuntimeLanguageBridge for LuaHostBridge {
    /// Returns `SupportedLanguage::Lua` to identify this as a Lua bridge.
    fn runtime_type(&self) -> SupportedLanguage {
        SupportedLanguage::Lua
    }

    /// Register a Lua callable as a host contract implementation.
    ///
    /// The `implementation` must be a `Box<Function>` containing a Lua
    /// callable function. The bridge stores this function for later dispatch.
    ///
    /// # Arguments
    ///
    /// - `contract_id`: The FNV-1a hash of `"host_contract:name@major"`
    /// - `implementation`: A boxed Lua function (`Function`)
    ///
    /// # Errors
    ///
    /// - `BridgeError::DuplicateContract`: Contract already registered
    /// - `BridgeError::TypeMismatch`: Implementation is not a `Function`
    fn register_host_contract(
        &mut self,
        contract_id: u64,
        implementation: Box<dyn core::any::Any>,
    ) -> Result<(), BridgeError> {
        // Attempt to downcast to Function
        let callable: Function = implementation
            .downcast::<Function>()
            .map_err(|_| BridgeError::TypeMismatch {
                contract_id,
                expected: "Function".to_owned(),
                got: "unknown type".to_owned(),
            })
            .map(|boxed| *boxed)?;

        // Acquire write lock and insert
        let mut contracts: std::sync::RwLockWriteGuard<'_, HashMap<u64, Function>> = self
            .contracts
            .write()
            .map_err(|_| BridgeError::VmRegistrationFailed {
                contract_id,
                reason: "failed to acquire write lock on contracts map".to_owned(),
            })?;

        if contracts.contains_key(&contract_id) {
            return Err(BridgeError::DuplicateContract { contract_id });
        }

        contracts.insert(contract_id, callable);
        Ok(())
    }

    /// Call a host contract function through Lua dispatch.
    ///
    /// This method:
    /// 1. Looks up the registered Lua callable
    /// 2. Calls the function with converted arguments
    /// 3. Returns the result or an error
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
    /// - `AbiError::ok()` on success
    /// - `AbiError { code: AbiErrorCode::HostContractNotFound, ... }` if contract not found
    /// - `AbiError { code: AbiErrorCode::HostContractCallFailed, ... }` if dispatch failed
    ///
    /// # Safety
    ///
    /// This method is inherently unsafe because it deals with raw pointers:
    /// - `args` must point to valid ABI-packed arguments for the contract
    /// - `out` must point to a valid buffer sized for the return type
    /// - The caller must ensure proper alignment of both pointers
    ///
    /// # Note
    ///
    /// For MVP, this implementation provides basic dispatch functionality.
    /// Full type marshaling for all primitive types will be added in future tasks.
    unsafe fn call_host_contract(
        &self,
        contract_id: u64,
        fn_id: u32,
        args: *const (),
        out: *mut (),
    ) -> AbiError {
        // Step 1: Look up the registered callable
        let contracts_guard: std::sync::RwLockReadGuard<'_, HashMap<u64, Function>> =
            match self.contracts.read() {
                Ok(guard) => guard,
                Err(_) => {
                    return AbiError {
                        code: AbiErrorCode::HostContractCallFailed as u32,
                        message: StringView::from_static(
                            b"failed to acquire read lock on contracts map",
                        ),
                    };
                }
            };

        let callable: &Function = match contracts_guard.get(&contract_id) {
            Some(f) => f,
            None => {
                return AbiError {
                    code: AbiErrorCode::HostContractNotFound as u32,
                    message: StringView::from_static(b"host contract not found"),
                };
            }
        };

        // Step 2: Call the function
        // For MVP, we pass fn_id as the first argument and args/out as opaque pointers
        // Full type marshaling will be implemented in future tasks
        //
        // Pass pointers as i64 to preserve full 64-bit precision on LuaJIT.
        // LuaJIT lua_Integer is int64_t — safe for pointer-width integers.
        let fn_id_arg: u32 = fn_id;
        let args_ptr: i64 = args as usize as i64;
        let out_ptr: i64 = out as usize as i64;

        let call_result: Result<(), mlua::Error> =
            callable.call::<()>((fn_id_arg, args_ptr, out_ptr));

        match call_result {
            Ok(()) => AbiError::ok(),
            Err(e) => {
                // The full Lua error detail propagates to the caller in the
                // AbiError message below; no side-channel print.
                let message: String = format!("Lua exception: {}", e);
                // SAFETY: We leak the message string to create a 'static StringView.
                // This is acceptable because:
                // 1. Error messages are small and short-lived
                // 2. The alternative would require host_alloc which we don't have here
                // 3. This matches the pattern used in other loaders
                let message_static: &'static str = Box::leak(message.into_boxed_str());
                AbiError {
                    code: AbiErrorCode::HostContractCallFailed as u32,
                    message: StringView {
                        ptr: message_static.as_ptr(),
                        len: message_static.len(),
                    },
                }
            }
        }
    }
}

// SAFETY: LuaHostBridge is Send because:
// - RwLock<HashMap<u64, Function>> is Send (RwLock is Send, HashMap is Send)
// - Lua is Send when compiled with mlua's `send` feature (which we use)
// - Function is Send when compiled with mlua's `send` feature
// - All operations that access Lua objects are thread-safe due to mlua's internal synchronization
unsafe impl Send for LuaHostBridge {}

// SAFETY: LuaHostBridge is Sync because:
// - RwLock provides synchronization for the contracts HashMap
// - Lua is Sync when compiled with mlua's `send` feature (which we use)
// - mlua's internal mutex provides synchronization for Lua state access
// - Concurrent reads are safe (read lock + mlua's internal sync)
// - Concurrent writes are serialized (write lock + mlua's internal sync)
unsafe impl Sync for LuaHostBridge {}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn bridge_new_creates_empty_bridge() {
        let bridge: LuaHostBridge = LuaHostBridge::new();
        let contracts: std::sync::RwLockReadGuard<'_, HashMap<u64, Function>> =
            bridge.contracts.read().expect("read lock");
        assert!(contracts.is_empty());
    }

    #[test]
    fn bridge_default_creates_empty_bridge() {
        let bridge: LuaHostBridge = LuaHostBridge::default();
        let contracts: std::sync::RwLockReadGuard<'_, HashMap<u64, Function>> =
            bridge.contracts.read().expect("read lock");
        assert!(contracts.is_empty());
    }

    #[test]
    fn bridge_with_capacity_creates_empty_bridge() {
        let bridge: LuaHostBridge = LuaHostBridge::with_capacity(10);
        let contracts: std::sync::RwLockReadGuard<'_, HashMap<u64, Function>> =
            bridge.contracts.read().expect("read lock");
        assert!(contracts.is_empty());
    }

    #[test]
    fn bridge_runtime_type_returns_lua() {
        let bridge: LuaHostBridge = LuaHostBridge::new();
        assert_eq!(bridge.runtime_type(), SupportedLanguage::Lua);
    }

    #[test]
    fn bridge_lua_returns_reference() {
        let bridge: LuaHostBridge = LuaHostBridge::new();
        let lua: &Lua = bridge.lua();
        // Verify we can use the Lua reference
        let result: i64 = lua.load("return 42").eval::<i64>().expect("eval");
        assert_eq!(result, 42);
    }

    #[test]
    fn bridge_register_host_contract_success() {
        let mut bridge: LuaHostBridge = LuaHostBridge::new();

        // Create a simple Lua callable
        let callable: Function = bridge
            .lua()
            .load("function(fn_id, args, out) return fn_id end")
            .eval::<Function>()
            .expect("eval function");

        // Register it
        let result: Result<(), BridgeError> =
            bridge.register_host_contract(1234, Box::new(callable));
        assert!(result.is_ok());

        // Verify it's stored
        let contracts: std::sync::RwLockReadGuard<'_, HashMap<u64, Function>> =
            bridge.contracts.read().expect("read lock");
        assert!(contracts.contains_key(&1234));
    }

    #[test]
    fn bridge_register_host_contract_duplicate_fails() {
        let mut bridge: LuaHostBridge = LuaHostBridge::new();

        // Create a simple Lua callable
        let callable: Function = bridge
            .lua()
            .load("function(fn_id, args, out) return fn_id end")
            .eval::<Function>()
            .expect("eval function");

        // Register it twice
        let callable2: Function = bridge
            .lua()
            .load("function(fn_id, args, out) return fn_id * 2 end")
            .eval::<Function>()
            .expect("eval function 2");

        let result1: Result<(), BridgeError> =
            bridge.register_host_contract(1234, Box::new(callable));
        assert!(result1.is_ok());

        let result2: Result<(), BridgeError> =
            bridge.register_host_contract(1234, Box::new(callable2));
        assert!(result2.is_err());
        let err: BridgeError = result2.expect_err("should fail");
        assert!(matches!(
            err,
            BridgeError::DuplicateContract { contract_id: 1234 }
        ));
    }

    #[test]
    fn bridge_register_host_contract_type_mismatch_fails() {
        let mut bridge: LuaHostBridge = LuaHostBridge::new();

        // Try to register a non-Function
        let result: Result<(), BridgeError> = bridge.register_host_contract(1234, Box::new(42i32));
        assert!(result.is_err());
        let err: BridgeError = result.expect_err("should fail");
        assert!(matches!(
            err,
            BridgeError::TypeMismatch {
                contract_id: 1234,
                ..
            }
        ));
    }

    #[test]
    fn bridge_call_host_contract_not_found() {
        let bridge: LuaHostBridge = LuaHostBridge::new();

        // SAFETY: null pointers are valid here — the lookup fails before any
        // pointer is dereferenced (contract 9999 is not registered).
        let result: AbiError =
            unsafe { bridge.call_host_contract(9999, 0, core::ptr::null(), core::ptr::null_mut()) };
        assert_eq!(result.code, AbiErrorCode::HostContractNotFound as u32);
    }

    #[test]
    fn bridge_call_host_contract_success() {
        let mut bridge: LuaHostBridge = LuaHostBridge::new();

        // Create a simple Lua callable that returns fn_id
        let callable: Function = bridge
            .lua()
            .load("function(fn_id, args, out) return fn_id end")
            .eval::<Function>()
            .expect("eval function");

        // Register it
        bridge
            .register_host_contract(1234, Box::new(callable))
            .expect("register");

        // Call it
        // SAFETY: the registered Lua function treats args/out as opaque integers and
        // never dereferences them, so passing null pointers is sound here.
        let result: AbiError =
            unsafe { bridge.call_host_contract(1234, 5, core::ptr::null(), core::ptr::null_mut()) };
        assert!(result.is_ok());
    }

    #[test]
    fn bridge_call_host_contract_exception() {
        let mut bridge: LuaHostBridge = LuaHostBridge::new();

        // Create a Lua callable that raises an error
        let callable: Function = bridge
            .lua()
            .load("function(fn_id, args, out) error('test error') end")
            .eval::<Function>()
            .expect("eval function");

        // Register it
        bridge
            .register_host_contract(1234, Box::new(callable))
            .expect("register");

        // Call it - should return error
        // SAFETY: the registered Lua function raises before touching args/out, so
        // passing null pointers is sound here.
        let result: AbiError =
            unsafe { bridge.call_host_contract(1234, 0, core::ptr::null(), core::ptr::null_mut()) };
        assert_eq!(result.code, AbiErrorCode::HostContractCallFailed as u32);
    }
}

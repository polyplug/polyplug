//! JsHostBridge — Bridge for JavaScript hosts implementing host contracts.
//!
//! This module provides the bridge that allows JavaScript hosts to implement
//! host contracts. The bridge stores JavaScript callable functions and dispatches
//! calls through the QuickJS VM.
//!
//! # Architecture
//!
//! When a JavaScript host registers a host contract implementation:
//! 1. The JS callable is stored in a HashMap keyed by contract_id
//! 2. When a plugin calls a host contract function, the bridge:
//!    - Looks up the JS callable
//!    - Invokes it with converted arguments
//!    - Converts the result back to ABI format
//!
//! # Thread Safety
//!
//! The bridge uses `RwLock` to protect the contracts HashMap because:
//! - Registration happens during initialization (write lock)
//! - Calls happen during plugin execution (read lock)
//! - rquickjs's `parallel` feature makes `Runtime`, `Context`, and `Persistent<Function>` Send/Sync safe
//!
//! # QuickJS VM Ownership
//!
//! The bridge owns its own QuickJS Runtime and Context for complete isolation from
//! plugin QuickJS VMs. This ensures:
//! - Host contract implementations don't interfere with plugin state
//! - Multiple Runtime instances can have isolated JS host bridges

use std::collections::HashMap;
use std::sync::RwLock;

use rquickjs::Context;
use rquickjs::Function;
use rquickjs::Persistent;
use rquickjs::Runtime;

use polyplug::host_bridge::BridgeError;
use polyplug::host_bridge::HostRuntimeBridge;
use polyplug_abi::ABI_HOST_CONTRACT_CALL_FAILED;
use polyplug_abi::ABI_HOST_CONTRACT_NOT_FOUND;
use polyplug_abi::AbiError;
use polyplug_abi::HostRuntime;
use polyplug_abi::StringView;

/// Errors that can occur when creating a JsHostBridge.
#[derive(Debug, thiserror::Error)]
pub enum JsBridgeError {
    /// Failed to create the QuickJS runtime.
    #[error("QuickJS runtime creation failed: {0}")]
    RuntimeCreationFailed(String),

    /// Failed to create the QuickJS context.
    #[error("QuickJS context creation failed: {0}")]
    ContextCreationFailed(String),
}

/// Type alias for a persistent JS function stored across scope boundaries.
type PersistentFunction = Persistent<Function<'static>>;

/// Bridge for JavaScript hosts implementing host contracts.
///
/// This bridge stores JavaScript callable functions and dispatches calls through
/// the QuickJS VM. The bridge handles:
/// - Thread-safe storage of registered implementations
/// - JavaScript error handling and conversion to AbiError
/// - Per-bridge QuickJS VM isolation
///
/// # Example
///
/// ```rust,ignore
/// use polyplug_js::bridge::JsHostBridge;
/// use polyplug::host_bridge::HostRuntimeBridge;
///
/// let bridge = JsHostBridge::new();
///
/// // Register a JS implementation
/// let ctx = bridge.context();
/// ctx.with(|ctx| {
///     let callable = ctx.eval::<Function>("function(fn_id, args, out) { return fn_id; }").unwrap();
///     let persistent = Persistent::save(&ctx, callable);
///     bridge.register_host_contract(1234, Box::new(persistent));
/// });
///
/// // Call through the bridge
/// let result = bridge.call_host_contract(1234, 0, args_ptr, out_ptr);
/// ```
pub struct JsHostBridge {
    /// The QuickJS Runtime owned by this bridge.
    /// SAFETY: rquickjs's `parallel` feature makes Runtime Send + Sync.
    runtime: Runtime,

    /// The QuickJS Context owned by this bridge.
    /// SAFETY: rquickjs's `parallel` feature makes Context Send + Sync.
    context: Context,

    /// Registered host contract implementations.
    /// Key: contract_id (FNV-1a hash of "host_contract:name@major")
    /// Value: JavaScript callable function (Persistent<Function<'static>>)
    contracts: RwLock<HashMap<u64, PersistentFunction>>,
}

impl JsHostBridge {
    /// Create a new JsHostBridge with a fresh QuickJS VM.
    ///
    /// # Errors
    ///
    /// Returns `JsBridgeError::RuntimeCreationFailed` if the QuickJS runtime
    /// cannot be created.
    ///
    /// Returns `JsBridgeError::ContextCreationFailed` if the QuickJS context
    /// cannot be created.
    pub fn new() -> Result<JsHostBridge, JsBridgeError> {
        let runtime: Runtime = Runtime::new()
            .map_err(|e: rquickjs::Error| JsBridgeError::RuntimeCreationFailed(e.to_string()))?;
        let context: Context = Context::full(&runtime)
            .map_err(|e: rquickjs::Error| JsBridgeError::ContextCreationFailed(e.to_string()))?;

        Ok(JsHostBridge {
            runtime,
            context,
            contracts: RwLock::new(HashMap::new()),
        })
    }

    /// Create a new JsHostBridge with pre-allocated capacity.
    ///
    /// # Errors
    ///
    /// Returns `JsBridgeError::RuntimeCreationFailed` if the QuickJS runtime
    /// cannot be created.
    ///
    /// Returns `JsBridgeError::ContextCreationFailed` if the QuickJS context
    /// cannot be created.
    pub fn with_capacity(capacity: usize) -> Result<JsHostBridge, JsBridgeError> {
        let runtime: Runtime = Runtime::new()
            .map_err(|e: rquickjs::Error| JsBridgeError::RuntimeCreationFailed(e.to_string()))?;
        let context: Context = Context::full(&runtime)
            .map_err(|e: rquickjs::Error| JsBridgeError::ContextCreationFailed(e.to_string()))?;

        Ok(JsHostBridge {
            runtime,
            context,
            contracts: RwLock::new(HashMap::with_capacity(capacity)),
        })
    }

    /// Get a reference to the underlying QuickJS Context.
    ///
    /// This allows host code to create JS functions for registration.
    pub fn context(&self) -> &Context {
        &self.context
    }

    /// Get a reference to the underlying QuickJS Runtime.
    ///
    /// This allows host code to access runtime-level features.
    pub fn runtime(&self) -> &Runtime {
        &self.runtime
    }
}

impl Drop for JsHostBridge {
    fn drop(&mut self) {
        // Clear the contracts HashMap before dropping the Runtime.
        // This ensures all Persistent<Function<'static>> references are released
        // before the QuickJS Runtime is destroyed, preventing the GC assertion failure.
        if let Ok(mut contracts) = self.contracts.write() {
            contracts.clear();
        }
    }
}

impl HostRuntimeBridge for JsHostBridge {
    /// Returns `HostRuntime::JavaScript` to identify this as a JavaScript bridge.
    fn runtime_type(&self) -> HostRuntime {
        HostRuntime::JavaScript
    }

    /// Register a JavaScript callable as a host contract implementation.
    ///
    /// The `implementation` must be a `Box<Persistent<Function<'static>>>` containing a
    /// JavaScript callable function. The bridge stores this function for later dispatch.
    ///
    /// # Arguments
    ///
    /// - `contract_id`: The FNV-1a hash of `"host_contract:name@major"`
    /// - `implementation`: A boxed persistent JS function (`Persistent<Function<'static>>`)
    ///
    /// # Errors
    ///
    /// - `BridgeError::DuplicateContract`: Contract already registered
    /// - `BridgeError::TypeMismatch`: Implementation is not a `Persistent<Function<'static>>`
    fn register_host_contract(
        &mut self,
        contract_id: u64,
        implementation: Box<dyn core::any::Any>,
    ) -> Result<(), BridgeError> {
        // Attempt to downcast to Persistent<Function<'static>>
        let callable: PersistentFunction = implementation
            .downcast::<PersistentFunction>()
            .map_err(|_| BridgeError::TypeMismatch {
                contract_id,
                expected: "Persistent<Function<'static>>".to_owned(),
                got: "unknown type".to_owned(),
            })
            .map(|boxed| *boxed)?;

        // Acquire write lock and insert
        let mut contracts: std::sync::RwLockWriteGuard<'_, HashMap<u64, PersistentFunction>> = self
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

    /// Call a host contract function through JavaScript dispatch.
    ///
    /// This method:
    /// 1. Looks up the registered JavaScript callable
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
    /// - `AbiError { code: ABI_HOST_CONTRACT_NOT_FOUND, ... }` if contract not found
    /// - `AbiError { code: ABI_HOST_CONTRACT_CALL_FAILED, ... }` if dispatch failed
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
    fn call_host_contract(
        &self,
        contract_id: u64,
        fn_id: u32,
        args: *const (),
        out: *mut (),
    ) -> AbiError {
        // Step 1: Look up the registered callable
        let contracts_guard: std::sync::RwLockReadGuard<'_, HashMap<u64, PersistentFunction>> =
            match self.contracts.read() {
                Ok(guard) => guard,
                Err(_) => {
                    return AbiError {
                        code: ABI_HOST_CONTRACT_CALL_FAILED,
                        message: StringView::from_static(
                            b"failed to acquire read lock on contracts map",
                        ),
                    };
                }
            };

        let callable: &PersistentFunction = match contracts_guard.get(&contract_id) {
            Some(f) => f,
            None => {
                return AbiError {
                    code: ABI_HOST_CONTRACT_NOT_FOUND,
                    message: StringView::from_static(b"host contract not found"),
                };
            }
        };

        // Step 2: Call the function within the QuickJS context
        // For MVP, we pass fn_id as the first argument and args/out as opaque pointers
        // Full type marshaling will be implemented in future tasks
        //
        // Pass pointers as BigInt to preserve full 64-bit precision.
        // QuickJS BigInt can hold full 64-bit integers.
        let fn_id_arg: u32 = fn_id;
        let args_ptr: i64 = args as usize as i64;
        let out_ptr: i64 = out as usize as i64;

        let call_result: Result<i32, rquickjs::Error> = self.context.with(|ctx| {
            // Restore the persistent function to a usable Function<'_> in this context
            let js_fn: Function<'_> = callable.clone().restore(&ctx)?;

            // Create BigInt values for pointer arguments
            let args_bigint: rquickjs::BigInt<'_> =
                rquickjs::BigInt::from_i64(ctx.clone(), args_ptr)?;
            let out_bigint: rquickjs::BigInt<'_> =
                rquickjs::BigInt::from_i64(ctx.clone(), out_ptr)?;

            // Call the JS function with (fn_id, args_ptr, out_ptr)
            let result: i32 = js_fn
                .call::<(u32, rquickjs::BigInt<'_>, rquickjs::BigInt<'_>), i32>((
                    fn_id_arg,
                    args_bigint,
                    out_bigint,
                ))?;

            Ok(result)
        });

        match call_result {
            Ok(0) => AbiError::ok(),
            Ok(code) => AbiError {
                code: code as u32,
                message: StringView::null(),
            },
            Err(e) => {
                // Print JS error for debugging
                eprintln!("[polyplug_js] JS host contract call failed: {}", e);

                // Return error with message
                let message: String = format!("JavaScript exception: {}", e);
                // SAFETY: We leak the message string to create a 'static StringView.
                // This is acceptable because:
                // 1. Error messages are small and short-lived
                // 2. The alternative would require host_alloc which we don't have here
                // 3. This matches the pattern used in other loaders
                let message_static: &'static str = Box::leak(message.into_boxed_str());
                AbiError {
                    code: ABI_HOST_CONTRACT_CALL_FAILED,
                    message: StringView {
                        ptr: message_static.as_ptr(),
                        len: message_static.len(),
                    },
                }
            }
        }
    }
}

// SAFETY: JsHostBridge is Send because:
// - RwLock<HashMap<u64, PersistentFunction>> is Send (RwLock is Send, HashMap is Send)
// - Runtime is Send when compiled with rquickjs's `parallel` feature (which we use)
// - Context is Send when compiled with rquickjs's `parallel` feature
// - Persistent<Function<'static>> is Send when compiled with rquickjs's `parallel` feature
// - All operations that access QuickJS objects are thread-safe due to rquickjs's internal synchronization
unsafe impl Send for JsHostBridge {}

// SAFETY: JsHostBridge is Sync because:
// - RwLock provides synchronization for the contracts HashMap
// - Runtime is Sync when compiled with rquickjs's `parallel` feature (which we use)
// - Context is Sync when compiled with rquickjs's `parallel` feature
// - rquickjs's internal mutex provides synchronization for QuickJS state access
// - Concurrent reads are safe (read lock + rquickjs's internal sync)
// - Concurrent writes are serialized (write lock + rquickjs's internal sync)
unsafe impl Sync for JsHostBridge {}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use polyplug_abi::abi_error_is_ok;

    use super::*;

    #[test]
    fn bridge_new_creates_empty_bridge() {
        let bridge: JsHostBridge = JsHostBridge::new().expect("bridge creation");
        let contracts: std::sync::RwLockReadGuard<'_, HashMap<u64, PersistentFunction>> =
            bridge.contracts.read().expect("read lock");
        assert!(contracts.is_empty());
    }

    #[test]
    fn bridge_with_capacity_creates_empty_bridge() {
        let bridge: JsHostBridge = JsHostBridge::with_capacity(10).expect("bridge creation");
        let contracts: std::sync::RwLockReadGuard<'_, HashMap<u64, PersistentFunction>> =
            bridge.contracts.read().expect("read lock");
        assert!(contracts.is_empty());
    }

    #[test]
    fn bridge_runtime_type_returns_javascript() {
        let bridge: JsHostBridge = JsHostBridge::new().expect("bridge creation");
        assert_eq!(bridge.runtime_type(), HostRuntime::JavaScript);
    }

    #[test]
    fn bridge_context_returns_reference() {
        let bridge: JsHostBridge = JsHostBridge::new().expect("bridge creation");
        let ctx: &Context = bridge.context();
        let result: i32 = ctx.with(|ctx| ctx.eval::<i32, _>("42")).expect("eval");
        assert_eq!(result, 42);
    }

    #[test]
    fn bridge_runtime_returns_reference() {
        let bridge: JsHostBridge = JsHostBridge::new().expect("bridge creation");
        let _runtime: &Runtime = bridge.runtime();
    }

    #[test]
    fn bridge_register_host_contract_success() {
        let mut bridge: JsHostBridge = JsHostBridge::new().expect("bridge creation");

        // Create a simple JS callable
        let persistent: PersistentFunction = bridge.context().with(|ctx| {
            let callable: Function<'_> = ctx
                .eval::<Function<'_>, _>("(function(fn_id, args, out) { return 0; })")
                .expect("eval function");
            Persistent::save(&ctx, callable)
        });

        // Register it
        let result: Result<(), BridgeError> =
            bridge.register_host_contract(1234, Box::new(persistent));
        assert!(result.is_ok());

        // Verify it's stored
        let contracts: std::sync::RwLockReadGuard<'_, HashMap<u64, PersistentFunction>> =
            bridge.contracts.read().expect("read lock");
        assert!(contracts.contains_key(&1234));
    }

    #[test]
    fn bridge_register_host_contract_duplicate_fails() {
        let mut bridge: JsHostBridge = JsHostBridge::new().expect("bridge creation");

        // Create two JS callables
        let persistent1: PersistentFunction = bridge.context().with(|ctx| {
            let callable: Function<'_> = ctx
                .eval::<Function<'_>, _>("(function(fn_id, args, out) { return 0; })")
                .expect("eval function");
            Persistent::save(&ctx, callable)
        });

        let persistent2: PersistentFunction = bridge.context().with(|ctx| {
            let callable: Function<'_> = ctx
                .eval::<Function<'_>, _>("(function(fn_id, args, out) { return 1; })")
                .expect("eval function");
            Persistent::save(&ctx, callable)
        });

        // Register first one
        let result1: Result<(), BridgeError> =
            bridge.register_host_contract(1234, Box::new(persistent1));
        assert!(result1.is_ok());

        // Try to register second one with same ID
        let result2: Result<(), BridgeError> =
            bridge.register_host_contract(1234, Box::new(persistent2));
        assert!(result2.is_err());
        let err: BridgeError = result2.expect_err("should fail");
        assert!(matches!(
            err,
            BridgeError::DuplicateContract { contract_id: 1234 }
        ));
    }

    #[test]
    fn bridge_register_host_contract_type_mismatch_fails() {
        let mut bridge: JsHostBridge = JsHostBridge::new().expect("bridge creation");

        // Try to register a non-PersistentFunction
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
        let bridge: JsHostBridge = JsHostBridge::new().expect("bridge creation");

        let result: AbiError =
            bridge.call_host_contract(9999, 0, std::ptr::null(), std::ptr::null_mut());
        assert_eq!(result.code, ABI_HOST_CONTRACT_NOT_FOUND);
    }

    #[test]
    fn bridge_call_host_contract_success() {
        let mut bridge: JsHostBridge = JsHostBridge::new().expect("bridge creation");

        // Create a simple JS callable that returns 0 (success)
        let persistent: PersistentFunction = bridge.context().with(|ctx| {
            let callable: Function<'_> = ctx
                .eval::<Function<'_>, _>("(function(fn_id, args, out) { return 0; })")
                .expect("eval function");
            Persistent::save(&ctx, callable)
        });

        // Register it
        bridge
            .register_host_contract(1234, Box::new(persistent))
            .expect("register");

        // Call it
        let result: AbiError =
            bridge.call_host_contract(1234, 5, std::ptr::null(), std::ptr::null_mut());
        assert!(abi_error_is_ok(&result));
    }

    #[test]
    fn bridge_call_host_contract_returns_error_code() {
        let mut bridge: JsHostBridge = JsHostBridge::new().expect("bridge creation");

        // Create a JS callable that returns a non-zero error code
        let persistent: PersistentFunction = bridge.context().with(|ctx| {
            let callable: Function<'_> = ctx
                .eval::<Function<'_>, _>("(function(fn_id, args, out) { return 42; })")
                .expect("eval function");
            Persistent::save(&ctx, callable)
        });

        // Register it
        bridge
            .register_host_contract(1234, Box::new(persistent))
            .expect("register");

        // Call it - should return error code 42
        let result: AbiError =
            bridge.call_host_contract(1234, 0, std::ptr::null(), std::ptr::null_mut());
        assert_eq!(result.code, 42);
    }

    #[test]
    fn bridge_call_host_contract_exception() {
        let mut bridge: JsHostBridge = JsHostBridge::new().expect("bridge creation");

        // Create a JS callable that throws an exception
        let persistent: PersistentFunction = bridge.context().with(|ctx| {
            let callable: Function<'_> = ctx
                .eval::<Function<'_>, _>(
                    "(function(fn_id, args, out) { throw new Error('test error'); })",
                )
                .expect("eval function");
            Persistent::save(&ctx, callable)
        });

        // Register it
        bridge
            .register_host_contract(1234, Box::new(persistent))
            .expect("register");

        // Call it - should return error
        let result: AbiError =
            bridge.call_host_contract(1234, 0, std::ptr::null(), std::ptr::null_mut());
        assert_eq!(result.code, ABI_HOST_CONTRACT_CALL_FAILED);
    }

    #[test]
    fn bridge_call_host_contract_with_fn_id() {
        let mut bridge: JsHostBridge = JsHostBridge::new().expect("bridge creation");

        // Create a JS callable that returns fn_id * 2 as error code
        let persistent: PersistentFunction = bridge.context().with(|ctx| {
            let callable: Function<'_> = ctx
                .eval::<Function<'_>, _>("(function(fn_id, args, out) { return fn_id * 2; })")
                .expect("eval function");
            Persistent::save(&ctx, callable)
        });

        // Register it
        bridge
            .register_host_contract(1234, Box::new(persistent))
            .expect("register");

        // Call with fn_id=5, expect error code 10
        let result: AbiError =
            bridge.call_host_contract(1234, 5, std::ptr::null(), std::ptr::null_mut());
        assert_eq!(result.code, 10);
    }
}

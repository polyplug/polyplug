//! PythonHostBridge — Bridge for Python hosts implementing host contracts.
//!
//! This module provides the bridge that allows Python hosts to implement
//! host contracts. The bridge stores Python callable objects and dispatches
//! calls through the Python interpreter with proper GIL handling.
//!
//! # Architecture
//!
//! When a Python host registers a host contract implementation:
//! 1. The Python callable is stored in a HashMap keyed by contract_id
//! 2. When a plugin calls a host contract function, the bridge:
//!    - Acquires the GIL
//!    - Looks up the Python callable
//!    - Invokes it with converted arguments
//!    - Converts the result back to ABI format
//!
//! # Thread Safety
//!
//! The bridge uses `RwLock` to protect the contracts HashMap because:
//! - Registration happens during initialization (write lock)
//! - Calls happen during plugin execution (read lock)
//! - Python's GIL provides additional synchronization for Python calls

use std::collections::HashMap;
use std::sync::RwLock;

use pyo3::Py;
use pyo3::Python;
use pyo3::types::PyAnyMethods;

use polyplug::host_bridge::BridgeError;
use polyplug::host_bridge::RuntimeLanguageBridge;
use polyplug_abi::AbiError;
use polyplug_abi::AbiErrorCode;
use polyplug_abi::RuntimeLanguage;
use polyplug_abi::StringView;

/// Bridge for Python hosts implementing host contracts.
///
/// This bridge stores Python callable objects and dispatches calls through
/// the Python interpreter. The bridge handles:
/// - GIL acquisition for all Python operations
/// - Python exception handling and conversion to AbiError
/// - Thread-safe storage of registered implementations
///
/// # Example
///
/// ```rust,ignore
/// use polyplug_python::bridge::PythonHostBridge;
/// use polyplug::host_bridge::RuntimeLanguageBridge;
///
/// let bridge = PythonHostBridge::new();
///
/// // Register a Python implementation
/// Python::with_gil(|py| {
///     let callable = py.eval(c"lambda x: x * 2", None, None).unwrap();
///     bridge.register_host_contract(1234, Box::new(callable.into()));
/// });
///
/// // Call through the bridge
/// let result = bridge.call_host_contract(1234, 0, args_ptr, out_ptr);
/// ```
pub struct PythonHostBridge {
    /// Registered host contract implementations.
    /// Key: contract_id (FNV-1a hash of "host_contract:name@major")
    /// Value: Python callable object (Py<PyAny>)
    contracts: RwLock<HashMap<u64, Py<pyo3::PyAny>>>,
}

impl PythonHostBridge {
    /// Create a new PythonHostBridge with no registered contracts.
    pub fn new() -> PythonHostBridge {
        PythonHostBridge {
            contracts: RwLock::new(HashMap::new()),
        }
    }

    /// Create a new PythonHostBridge with pre-allocated capacity.
    pub fn with_capacity(capacity: usize) -> PythonHostBridge {
        PythonHostBridge {
            contracts: RwLock::new(HashMap::with_capacity(capacity)),
        }
    }
}

impl Default for PythonHostBridge {
    fn default() -> PythonHostBridge {
        PythonHostBridge::new()
    }
}

impl RuntimeLanguageBridge for PythonHostBridge {
    /// Returns `RuntimeLanguage::Python` to identify this as a Python bridge.
    fn runtime_type(&self) -> RuntimeLanguage {
        RuntimeLanguage::Python
    }

    /// Register a Python callable as a host contract implementation.
    ///
    /// The `implementation` must be a `Box<Py<PyAny>>` containing a Python
    /// callable object. The bridge stores this callable for later dispatch.
    ///
    /// # Arguments
    ///
    /// - `contract_id`: The FNV-1a hash of `"host_contract:name@major"`
    /// - `implementation`: A boxed Python callable (`Py<PyAny>`)
    ///
    /// # Errors
    ///
    /// - `BridgeError::DuplicateContract`: Contract already registered
    /// - `BridgeError::TypeMismatch`: Implementation is not a `Py<PyAny>`
    fn register_host_contract(
        &mut self,
        contract_id: u64,
        implementation: Box<dyn core::any::Any>,
    ) -> Result<(), BridgeError> {
        // Attempt to downcast to Py<PyAny>
        let callable: Py<pyo3::PyAny> = implementation
            .downcast::<Py<pyo3::PyAny>>()
            .map_err(|_| BridgeError::TypeMismatch {
                contract_id,
                expected: "Py<PyAny>".to_owned(),
                got: "unknown type".to_owned(),
            })
            .map(|boxed| *boxed)?;

        // Acquire write lock and insert
        let mut contracts: std::sync::RwLockWriteGuard<'_, HashMap<u64, Py<pyo3::PyAny>>> = self
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

    /// Call a host contract function through Python dispatch.
    ///
    /// This method:
    /// 1. Acquires the GIL
    /// 2. Looks up the registered Python callable
    /// 3. Calls the function with converted arguments
    /// 4. Returns the result or an error
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
    fn call_host_contract(
        &self,
        contract_id: u64,
        fn_id: u32,
        args: *const (),
        out: *mut (),
    ) -> AbiError {
        // Acquire GIL and dispatch the call
        Python::attach(|py| {
            // Step 1: Look up the registered callable
            let contracts_guard: std::sync::RwLockReadGuard<'_, HashMap<u64, Py<pyo3::PyAny>>> =
                match self.contracts.read() {
                    Ok(guard) => guard,
                    Err(_) => {
                        return AbiError {
                            code: AbiErrorCode::HostContractCallFailed,
                            message: StringView::from_static(
                                b"failed to acquire read lock on contracts map",
                            ),
                        };
                    }
                };

            let callable: &Py<pyo3::PyAny> = match contracts_guard.get(&contract_id) {
                Some(c) => c,
                None => {
                    return AbiError {
                        code: AbiErrorCode::HostContractNotFound,
                        message: StringView::from_static(b"host contract not found"),
                    };
                }
            };

            // Step 2: Bind the callable to this Python context
            let bound_callable: pyo3::Bound<'_, pyo3::PyAny> = callable.bind(py).clone();

            // Step 3: Verify it's callable
            if !bound_callable.is_callable() {
                return AbiError {
                    code: AbiErrorCode::HostContractCallFailed,
                    message: StringView::from_static(b"registered object is not callable"),
                };
            }

            // Step 4: Call the function
            // For MVP, we pass fn_id as the first argument and args/out as opaque pointers
            // Full type marshaling will be implemented in future tasks
            let fn_id_arg: u32 = fn_id;
            let args_ptr: i64 = args as usize as i64;
            let out_ptr: i64 = out as usize as i64;

            let call_result: Result<pyo3::Bound<'_, pyo3::PyAny>, pyo3::PyErr> =
                bound_callable.call((fn_id_arg, args_ptr, out_ptr), None);

            match call_result {
                Ok(_result) => AbiError::ok(),
                Err(e) => {
                    // Print Python traceback for debugging
                    e.print(py);

                    // Return error with message
                    let message: String = format!("Python exception: {}", e);
                    // SAFETY: We leak the message string to create a 'static StringView.
                    // This is acceptable because:
                    // 1. Error messages are small and short-lived
                    // 2. The alternative would require host_alloc which we don't have here
                    // 3. This matches the pattern used in other loaders
                    let message_static: &'static str = Box::leak(message.into_boxed_str());
                    AbiError {
                        code: AbiErrorCode::HostContractCallFailed,
                        message: StringView {
                            ptr: message_static.as_ptr(),
                            len: message_static.len(),
                        },
                    }
                }
            }
        })
    }
}

// SAFETY: PythonHostBridge is Send because:
// - RwLock<HashMap<u64, Py<PyAny>>> is Send (RwLock is Send, HashMap is Send)
// - Py<PyAny> is Send when the GIL is not held (pyo3 guarantees this)
// - All operations that access Python objects acquire the GIL first
unsafe impl Send for PythonHostBridge {}

// SAFETY: PythonHostBridge is Sync because:
// - RwLock provides synchronization for the contracts HashMap
// - Python's GIL provides synchronization for Python object access
// - Concurrent reads are safe (read lock + GIL)
// - Concurrent writes are serialized (write lock + GIL)
unsafe impl Sync for PythonHostBridge {}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;

    #[test]
    fn bridge_new_creates_empty_bridge() {
        let bridge: PythonHostBridge = PythonHostBridge::new();
        let contracts: std::sync::RwLockReadGuard<'_, HashMap<u64, Py<pyo3::PyAny>>> =
            bridge.contracts.read().expect("read lock");
        assert!(contracts.is_empty());
    }

    #[test]
    fn bridge_default_creates_empty_bridge() {
        let bridge: PythonHostBridge = PythonHostBridge::default();
        let contracts: std::sync::RwLockReadGuard<'_, HashMap<u64, Py<pyo3::PyAny>>> =
            bridge.contracts.read().expect("read lock");
        assert!(contracts.is_empty());
    }

    #[test]
    fn bridge_with_capacity_creates_empty_bridge() {
        let bridge: PythonHostBridge = PythonHostBridge::with_capacity(10);
        let contracts: std::sync::RwLockReadGuard<'_, HashMap<u64, Py<pyo3::PyAny>>> =
            bridge.contracts.read().expect("read lock");
        assert!(contracts.is_empty());
    }

    #[test]
    fn bridge_runtime_type_returns_python() {
        let bridge: PythonHostBridge = PythonHostBridge::new();
        assert_eq!(bridge.runtime_type(), RuntimeLanguage::Python);
    }

    #[test]
    fn bridge_register_host_contract_success() {
        // Initialize Python interpreter
        crate::context::ensure_python_initialized(&crate::config::PythonConfig::default())
            .expect("Python init");

        let mut bridge: PythonHostBridge = PythonHostBridge::new();

        Python::attach(|py| {
            // Create a simple Python callable
            let callable: pyo3::Bound<'_, pyo3::PyAny> = py
                .eval(c"lambda fn_id, args, out: fn_id", None, None)
                .expect("eval lambda");
            let py_callable: Py<pyo3::PyAny> = callable.into();

            // Register it
            let result: Result<(), BridgeError> =
                bridge.register_host_contract(1234, Box::new(py_callable));
            assert!(result.is_ok());

            // Verify it's stored
            let contracts: std::sync::RwLockReadGuard<'_, HashMap<u64, Py<pyo3::PyAny>>> =
                bridge.contracts.read().expect("read lock");
            assert!(contracts.contains_key(&1234));
        });
    }

    #[test]
    fn bridge_register_host_contract_duplicate_fails() {
        // Initialize Python interpreter
        crate::context::ensure_python_initialized(&crate::config::PythonConfig::default())
            .expect("Python init");

        let mut bridge: PythonHostBridge = PythonHostBridge::new();

        Python::attach(|py| {
            // Create a simple Python callable
            let callable: pyo3::Bound<'_, pyo3::PyAny> = py
                .eval(c"lambda fn_id, args, out: fn_id", None, None)
                .expect("eval lambda");
            let py_callable: Py<pyo3::PyAny> = callable.into();

            // Register it twice (need to clone_ref for second registration)
            let result1: Result<(), BridgeError> =
                bridge.register_host_contract(1234, Box::new(py_callable.clone_ref(py)));
            assert!(result1.is_ok());

            let result2: Result<(), BridgeError> =
                bridge.register_host_contract(1234, Box::new(py_callable));
            assert!(result2.is_err());
            let err: BridgeError = result2.expect_err("should fail");
            assert!(matches!(
                err,
                BridgeError::DuplicateContract { contract_id: 1234 }
            ));
        });
    }

    #[test]
    fn bridge_register_host_contract_type_mismatch_fails() {
        let mut bridge: PythonHostBridge = PythonHostBridge::new();

        // Try to register a non-PyObject
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
        let bridge: PythonHostBridge = PythonHostBridge::new();

        let result: AbiError =
            bridge.call_host_contract(9999, 0, std::ptr::null(), std::ptr::null_mut());
        assert_eq!(result.code, AbiErrorCode::HostContractNotFound);
    }

    #[test]
    fn bridge_call_host_contract_success() {
        // Initialize Python interpreter
        crate::context::ensure_python_initialized(&crate::config::PythonConfig::default())
            .expect("Python init");

        let mut bridge: PythonHostBridge = PythonHostBridge::new();

        Python::attach(|py| {
            // Create a simple Python callable that returns fn_id
            let callable: pyo3::Bound<'_, pyo3::PyAny> = py
                .eval(c"lambda fn_id, args, out: fn_id", None, None)
                .expect("eval lambda");
            let py_callable: Py<pyo3::PyAny> = callable.into();

            // Register it
            bridge
                .register_host_contract(1234, Box::new(py_callable))
                .expect("register");
        });

        // Call it
        let result: AbiError =
            bridge.call_host_contract(1234, 5, std::ptr::null(), std::ptr::null_mut());
        assert!(result.is_ok());
    }

    #[test]
    fn bridge_call_host_contract_exception() {
        // Initialize Python interpreter
        crate::context::ensure_python_initialized(&crate::config::PythonConfig::default())
            .expect("Python init");

        let mut bridge: PythonHostBridge = PythonHostBridge::new();

        Python::attach(|py| {
            // Create a Python callable that raises an exception
            let callable: pyo3::Bound<'_, pyo3::PyAny> = py
                .eval(
                    c"lambda fn_id, args, out: (_ for _ in ()).throw(ValueError('test error'))",
                    None,
                    None,
                )
                .expect("eval lambda");
            let py_callable: Py<pyo3::PyAny> = callable.into();

            // Register it
            bridge
                .register_host_contract(1234, Box::new(py_callable))
                .expect("register");
        });

        // Call it - should return error
        let result: AbiError =
            bridge.call_host_contract(1234, 0, std::ptr::null(), std::ptr::null_mut());
        assert_eq!(result.code, AbiErrorCode::HostContractCallFailed);
    }

    #[test]
    fn bridge_call_host_contract_not_callable() {
        // Initialize Python interpreter
        crate::context::ensure_python_initialized(&crate::config::PythonConfig::default())
            .expect("Python init");

        let mut bridge: PythonHostBridge = PythonHostBridge::new();

        Python::attach(|py| {
            // Create a non-callable Python object
            let obj: pyo3::Bound<'_, pyo3::PyAny> = py.eval(c"42", None, None).expect("eval");
            let py_obj: Py<pyo3::PyAny> = obj.into();

            // Register it
            bridge
                .register_host_contract(1234, Box::new(py_obj))
                .expect("register");
        });

        // Call it - should return error
        let result: AbiError =
            bridge.call_host_contract(1234, 0, std::ptr::null(), std::ptr::null_mut());
        assert_eq!(result.code, AbiErrorCode::HostContractCallFailed);
    }
}

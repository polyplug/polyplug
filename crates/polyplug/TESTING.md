# Testing Polyplug

Polyplug uses a high-performance, lock-free architecture for plugin dispatch. To achieve this, several core components rely on `std::sync::OnceLock` for global state that is initialized once per process.

## Test Isolation Constraints

Because `OnceLock` can only be set once, subsequent attempts to initialize or reconfigure the global state within the same process will be silently ignored. This has significant implications for testing.

### 1. Global State Behavior

The following components use `OnceLock` and are persistent for the lifetime of the process:

*   **Global Registry**: Stores the mapping of contracts to plugin implementations.
*   **Extension Map**: Stores host-side extension vtables.
*   **Warning Callback**: The global handler for runtime warnings.

Once these are set (typically during the first `RuntimeBuilder::build()` call), they cannot be changed or reset.

### 2. Runtime::builder() Limitations

While you can create multiple `Runtime` instances using `Runtime::builder().build()`, only the **first** call to `build()` in a process successfully initializes the global state. 

Subsequent `Runtime` instances will:
*   Share the **original** global registry.
*   Ignore any new extensions registered in the builder.
*   Ignore any new warning callbacks.

This means that if Test A builds a runtime with Extension X, and Test B builds a runtime with Extension Y in the same process, Test B will still see Extension X (or no extension if Test A didn't register one) because the `OnceLock` was already initialized.

### 3. Test Isolation Requirements

To ensure fresh state and avoid cross-test contamination, tests that require different global configurations must run in **separate processes**.

#### Integration Tests

Cargo runs each file in the `tests/` directory as a separate test binary (and thus a separate process). This is the preferred way to write tests that need specific global state:

```rust
// tests/my_feature_test.rs
#[test]
fn test_with_specific_extension() {
    let rt = Runtime::builder()
        .extension(Box::new(MyExtension))
        .build()
        .unwrap();
    // ...
}
```

#### Unit Tests

Unit tests within a single module (e.g., in a `mod tests` block) run in the same process. If these tests depend on global state, they may interfere with each other. 

Strategies for unit tests:
*   Use `serial_test` crate to run tests sequentially if they share state.
*   Write idempotent setup functions that check if state is already initialized.
*   Prefer integration tests in `tests/` for state-heavy verification.

### 4. How to Write Isolated Tests

If you must test components that interact with the global registry without spawning new binaries, consider the following:

*   **FFI Facade**: Use the `testing` module (available with `feature = "testing"`) to interact with the internal FFI callbacks directly.
*   **Unique Contract IDs**: Use unique contract IDs or bundle names for different tests within the same file to avoid registry collisions.
*   **Separate Binaries**: For any test that verifies `RuntimeBuilder` configuration (extensions, callbacks, compatibility modes), always use a dedicated file in the `tests/` directory.

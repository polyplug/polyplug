# Testing Patterns

**Analysis Date:** 2026-04-02

## Test Framework

**Runner:**
- Built-in Rust test harness (`#[test]` attribute)
- Criterion for benchmarks (`crates/polyplug/benches/*.rs`)
- No explicit test runner configuration (uses Cargo defaults)

**Assertion Library:**
- Built-in `assert!`, `assert_eq!`, `assert_ne!` macros
- Custom panic messages with format strings: `assert!(result.is_ok(), "load_bundle failed: {e}");`

**Run Commands:**
```bash
cargo test                           # Run all tests in workspace
cargo test -p polyplug               # Run tests for specific crate
cargo test --test integration_load   # Run specific test binary
cargo test -- --test-threads=1       # Run tests serially (for thread-sensitive tests)
cargo bench -p polyplug --bench vtable_dispatch  # Run specific benchmark
cargo bench                          # Run all benchmarks
```

## Test File Organization

**Location:**
- Unit tests: Inline in source files under `#[cfg(test)] mod tests { }`
- Integration tests: Separate test files in `crates/*/tests/*.rs`
- Each test file is its own crate root (compiled as separate binary)
- Benchmarks: `crates/*/benches/*.rs` with `[[bench]]` in Cargo.toml

**Naming:**
- Integration tests: `integration_<feature>.rs` (e.g., `integration_load.rs`, `integration_panic.rs`)
- Stress tests: `stress_<feature>.rs` (e.g., `stress_error.rs`, `stress_memory.rs`)
- Smoke tests: `smoke.rs` (end-to-end validation)
- Edge case tests: `<feature>_edge_cases.rs` (e.g., `ffi_edge_cases.rs`)
- Benchmarks: `<feature>.rs` (e.g., `vtable_dispatch.rs`, `registry_resolve.rs`)

**Structure:**
```
crates/polyplug/
├── src/
│   ├── lib.rs           # Contains #[cfg(test)] mod tests { }
│   ├── runtime.rs       # Contains #[cfg(test)] mod tests { }
│   └── error.rs         # Contains #[cfg(test)] mod tests { }
├── tests/
│   ├── integration_load.rs       # Standalone test binary
│   ├── integration_panic.rs      # Standalone test binary
│   ├── stress_error.rs           # Standalone test binary
│   └── library_lifetime.rs       # Standalone test binary
└── benches/
    ├── vtable_dispatch.rs        # Criterion benchmark
    ├── registry_resolve.rs       # Criterion benchmark
    └── ffi_resolve.rs            # Criterion benchmark
```

## Test Structure

**Suite Organization:**
```rust
//! Integration test: load the test_plugin .so, verify ABI version, verify vtable registration.
//!
//! This test crate is the crate root for the `integration_load` test binary.

#![allow(clippy::expect_used)]

use polyplug_abi::ABI_OK;
use polyplug_abi::AbiError;
// ... more imports

// ─── Host functions for integration tests ─────────────────────────────────────

/// register_plugin callback that captures the registered vtable pointer for inspection.
unsafe extern "C" fn capture_register(...) -> AbiError { ... }

// ─── Thread-local state ──────────────────────────────────────────────────────
std::thread_local! {
    static CAPTURED_CONTRACT_ID: core::cell::RefCell<Option<u64>> = 
        const { core::cell::RefCell::new(None) };
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[test]
fn test_load_and_abi_version() {
    // Test implementation
}

#[test]
fn test_init_registers_vtable() {
    // Test implementation
}
```

**Patterns:**
- Setup: Thread-local state for capturing FFI callback results
- Teardown: `core::mem::forget(library)` to prevent dlclose
- Assertion: Direct `assert_eq!` with descriptive failure messages

## Mocking

**Framework:**
- No external mocking framework
- Manual stub implementations for FFI callbacks

**Patterns:**
```rust
/// No-op alloc callback.
unsafe extern "C" fn noop_alloc(
    _rt_ctx: *mut core::ffi::c_void,
    size: usize,
    align: usize,
) -> *mut u8 {
    polyplug_abi::ffi::polyplug_host_alloc(size, align)
}

/// No-op find_by_contract callback.
unsafe extern "C" fn noop_find_by_contract(
    _rt_ctx: *mut core::ffi::c_void,
    _contract_id: u64,
    _min_version: u32,
) -> GuestContractHandle {
    GuestContractHandle::null()
}
```

**What to Mock:**
- HostInterface callbacks for plugin registration tests
- BundleLoader trait implementations for runtime tests
- Registry operations for cross-plugin dispatch tests

**What NOT to Mock:**
- Actual FFI calls to loaded plugins (use real compiled fixtures)
- Memory allocation (use actual `polyplug_host_alloc`)
- ABI contract verification (test with real vtables)

## Fixtures and Factories

**Test Data:**
- Compiled plugin fixtures in `tests/fixtures/*/`
- Each fixture is a full Rust cdylib crate with `Cargo.toml` and `src/lib.rs`
- Fixture crates are workspace members

**Location:**
```
tests/fixtures/
├── test_plugin/          # Basic plugin for ABI tests
│   ├── Cargo.toml
│   └── src/lib.rs
├── error_plugin/         # Plugin for error propagation tests
│   ├── Cargo.toml
│   └── src/lib.rs
├── memory_plugin/        # Plugin for memory lifecycle tests
│   ├── Cargo.toml
│   └── src/lib.rs
├── reload_plugin_v1/     # Version 1 for hot reload tests
├── reload_plugin_v2/     # Version 2 for hot reload tests
└── error_plugin/         # Plugin that returns errors
```

**Build:**
- Fixtures compiled via `cargo build --release` as workspace members
- Build.rs scripts set environment variables for fixture paths:
  ```rust
  let test_plugin_so = env!("TEST_PLUGIN_SO");  // Set by build.rs
  ```

**Fixture Types:**
- `test_plugin`: Basic `add(a, b) -> u32` function
- `memory_plugin`: Buffer manipulation functions
- `error_plugin`: Error propagation and panic tests
- `panic_plugin`: Intentional panic for catch_unwind tests

## Coverage

**Requirements:**
- No explicit coverage target enforced
- Comprehensive test coverage via multiple test types

**Coverage Types:**
- Unit tests in `#[cfg(test)]` modules
- Integration tests in `tests/` directory
- Stress tests for concurrent/race conditions
- Edge case tests for boundary conditions
- Benchmark tests that also verify correctness

**View Coverage:**
```bash
cargo tarpaulin --workspace --out Html  # If tarpaulin installed
cargo llvm-cov --workspace              # If llvm-cov installed
```

## Test Types

**Unit Tests:**
- Inline in source files under `#[cfg(test)] mod tests`
- Test individual functions, error display, handle packing
- Example: `crates/polyplug/src/error.rs` has 30+ unit tests for error Display traits

**Integration Tests:**
- Standalone test binaries in `tests/` directories
- Load real compiled plugins via libloading
- Verify FFI boundary, ABI contracts, dispatch
- Example: `crates/polyplug/tests/integration_load.rs`

**E2E Tests:**
- Full codegen round-trip in `crates/polyplugc/tests/smoke.rs`
- Generates bindings, compiles plugin, loads, dispatches, verifies result
- Tests CLI tool: `polyplugc generate --bundle --lang --out`

**Stress Tests:**
- Concurrent operations, race conditions, repeated calls
- Example: `stress_concurrent_registry.rs`, `stress_quiescence_race.rs`
- Test thread-safety guarantees

**Hot Reload Tests:**
- `integration_hot_reload_notification.rs`
- `stress_hot_reload.rs`
- Tests runtime reload behavior with v1/v2 fixtures

## Common Patterns

**Async Testing:**
- Not applicable (synchronous design)
- Thread-based concurrent tests:
  ```rust
  use std::thread;
  use std::sync::Arc;
  use std::sync::atomic::{AtomicUsize, Ordering};

  let handles: Vec<thread::JoinHandle<()>> = (0..4)
      .map(|_| {
          thread::spawn(move || {
              // concurrent operations
          })
      })
      .collect();

  for h in handles {
      h.join().expect("thread should not panic");
  }
  ```

**Error Testing:**
```rust
#[test]
fn loader_error_init_failed_display() {
    let err: LoaderError = LoaderError::InitFailed {
        bundle: "init_bundle".to_owned(),
        error: "null pointer dereference".to_owned(),
    };
    let s: String = err.to_string();
    assert!(s.contains("init failed"), "got: {s}");
    assert!(s.contains("init_bundle"), "got: {s}");
    assert!(s.contains("null pointer dereference"), "got: {s}");
}
```

**FFI Testing:**
```rust
// Load cdylib
let library: libloading::Library = unsafe {
    libloading::Library::new(TEST_PLUGIN_SO).expect("failed to load plugin")
};

// Resolve symbol
let init_fn: libloading::Symbol<unsafe extern "C" fn(...) -> AbiError> = unsafe {
    library.get(b"polyplug_init\0").expect("symbol not found")
};

// Call and verify
let result: AbiError = unsafe { init_fn(...) };
assert_eq!(result.code, ABI_OK);

// Prevent dlclose
core::mem::forget(library);
```

**Panic Testing:**
```rust
#[test]
fn panic_during_init_is_caught() {
    let result = std::panic::catch_unwind(|| {
        let _rt: Runtime = Runtime::builder()
            .loader(PanicLoader)
            .build()
            .unwrap_or_else(|e| panic!("runtime build failed: {e}"));
    });
    if result.is_ok() {
        panic!("expected panic from PanicLoader");
    }
}
```

**Thread-Local State Capture:**
```rust
std::thread_local! {
    static CAPTURED_VTABLE: core::cell::Cell<*const GuestContractInterface> =
        const { core::cell::Cell::new(core::ptr::null()) };
}

unsafe extern "C" fn capture_vtable_callback(
    _rt_ctx: *mut c_void,
    _descriptor: *const PluginDescriptor,
    vtable: *const GuestContractInterface,
) -> AbiError {
    CAPTURED_VTABLE.with(|cell| cell.set(vtable));
    AbiError { code: ABI_OK, message: StringView::null() }
}

#[test]
fn test_vtable_capture() {
    // ... setup
    let vtable_ptr: *const GuestContractInterface = CAPTURED_VTABLE.with(|cell| cell.get());
    assert!(!vtable_ptr.is_null());
}
```

## Benchmark Patterns

**Criterion Structure:**
```rust
#![allow(clippy::expect_used)]

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

fn bench_dispatch_noop(c: &mut Criterion) {
    // Setup
    let _library: libloading::Library = load_and_init_plugin(TEST_PLUGIN_SO);
    let dispatch_fn = get_vtable_fn(0);

    let mut group = c.benchmark_group("dispatch");
    group.throughput(Throughput::Elements(1));

    group.bench_function(BenchmarkId::new("noop", "add(0,0)"), |b| {
        b.iter(|| {
            // SAFETY: args and out are valid
            let result: AbiError = unsafe {
                dispatch_fn(black_box(&args as *const _), black_box(&mut out as *mut _))
            };
            black_box(result);
        });
    });

    group.finish();
    core::mem::forget(_library);
}

criterion_group!(benches, bench_dispatch_noop, ...);
criterion_main!(benches);
```

**Benchmark Configuration:**
- `Cargo.toml` declares benchmarks:
  ```toml
  [[bench]]
  name = "vtable_dispatch"
  harness = false
  ```
- HTML reports enabled: `criterion = { version = "0.8", features = ["html_reports"] }`

---

*Testing analysis: 2026-04-02*
//! Benchmark for Python dispatch overhead.
//!
//! Measures the performance characteristics of the Python dispatch path:
//! 1. Python GIL acquisition overhead (via pyo3)
//! 2. Python function call overhead
//! 3. Native baseline for comparison
//!
//! NOTE: Python uses VM dispatch (pyo3 `vm.call`) like Lua and JS. The overhead
//! is primarily GIL acquisition plus the Python function call, which these
//! benchmarks isolate.

#![allow(clippy::expect_used)]

use core::hint::black_box;
use criterion::{Criterion, criterion_group, criterion_main};
use pyo3::Python;
use pyo3::types::PyAnyMethods;
use pyo3::types::PyDictMethods;
use pyo3::types::PyModule;

/// Benchmark Python GIL acquisition and function call overhead.
///
/// Measures the time to acquire the GIL and call a no-op Python function.
/// This simulates the dispatch path used by polyplug_python.
fn bench_python_dispatch(c: &mut Criterion) {
    // Initialize Python interpreter once.
    Python::initialize();

    let mut group = c.benchmark_group("python_dispatch");

    // Measure GIL acquisition + function call.
    group.bench_function("gil_acquire_and_call", |b| {
        b.iter(|| {
            Python::attach(|py| {
                // Use PyDict::new and py.run to define a simple no-op function.
                let code: &std::ffi::CStr = c"def noop_dispatch(args, out): return 0";
                let globals: pyo3::Bound<'_, pyo3::types::PyDict> = pyo3::types::PyDict::new(py);
                py.run(code, Some(&globals), None)
                    .expect("Failed to run code");

                let noop_fn: pyo3::Bound<'_, pyo3::PyAny> = globals
                    .get_item("noop_dispatch")
                    .expect("Failed to get noop_dispatch")
                    .expect("noop_dispatch not found");

                let args_i64: i64 = 0;
                let out_i64: i64 = 0;
                let _: Result<pyo3::Bound<'_, pyo3::PyAny>, pyo3::PyErr> =
                    noop_fn.call((args_i64, out_i64), None);
                black_box(())
            })
        })
    });

    // Measure 10 calls with single GIL acquisition.
    group.bench_function("gil_acquire_and_10_calls", |b| {
        b.iter(|| {
            Python::attach(|py| {
                let code: &std::ffi::CStr = c"def noop_dispatch(args, out): return 0";
                let globals: pyo3::Bound<'_, pyo3::types::PyDict> = pyo3::types::PyDict::new(py);
                py.run(code, Some(&globals), None)
                    .expect("Failed to run code");

                let noop_fn: pyo3::Bound<'_, pyo3::PyAny> = globals
                    .get_item("noop_dispatch")
                    .expect("Failed to get noop_dispatch")
                    .expect("noop_dispatch not found");

                let args_i64: i64 = 0;
                let out_i64: i64 = 0;
                for _ in 0..10 {
                    let _: Result<pyo3::Bound<'_, pyo3::PyAny>, pyo3::PyErr> =
                        noop_fn.call((args_i64, out_i64), None);
                }
                black_box(())
            })
        })
    });

    group.finish();
}

/// Benchmark GIL acquisition overhead alone.
///
/// GIL acquisition is the primary overhead in Python dispatch.
/// This benchmark isolates that cost.
fn bench_gil_acquisition(c: &mut Criterion) {
    // Initialize Python interpreter once.
    Python::initialize();

    let mut group = c.benchmark_group("gil_acquisition");

    group.bench_function("gil_acquire_only", |b| {
        b.iter(|| Python::attach(|_py| black_box(())))
    });

    group.bench_function("gil_acquire_with_module_import", |b| {
        b.iter(|| {
            Python::attach(|py| {
                let _: Result<pyo3::Bound<'_, PyModule>, pyo3::PyErr> = PyModule::import(py, "sys");
                black_box(())
            })
        })
    });

    group.finish();
}

/// Benchmark native function call baseline.
///
/// Provides a reference point for the minimum possible dispatch overhead.
fn bench_native_baseline(c: &mut Criterion) {
    let mut group = c.benchmark_group("native_baseline");

    fn native_add(a: i32, b: i32) -> i32 {
        a + b
    }

    group.bench_function("native_function_call", |b| {
        b.iter(|| black_box(native_add(black_box(1), black_box(2))))
    });

    type NativeFn = extern "C" fn(i32, i32) -> i32;

    extern "C" fn native_add_extern(a: i32, b: i32) -> i32 {
        a + b
    }

    let func_ptr: NativeFn = native_add_extern;

    group.bench_function("native_function_pointer_call", |b| {
        b.iter(|| black_box(func_ptr(black_box(1), black_box(2))))
    });

    group.finish();
}

/// Benchmark Python computation (non-trivial work).
///
/// Measures the overhead of a Python function that does actual work,
/// to compare against the no-op dispatch baseline.
fn bench_python_computation(c: &mut Criterion) {
    // Initialize Python interpreter once.
    Python::initialize();

    let mut group = c.benchmark_group("python_computation");

    group.bench_function("python_computation_100_iterations", |b| {
        b.iter(|| {
            Python::attach(|py| {
                let code: &std::ffi::CStr = c"def compute_sum(args, out):\n    total = 0\n    for i in range(100):\n        total += i\n    return total";
                let globals: pyo3::Bound<'_, pyo3::types::PyDict> =
                    pyo3::types::PyDict::new(py);
                py.run(code, Some(&globals), None).expect("Failed to run code");

                let compute_fn: pyo3::Bound<'_, pyo3::PyAny> = globals
                    .get_item("compute_sum")
                    .expect("Failed to get compute_sum")
                    .expect("compute_sum not found");

                let args_i64: i64 = 0;
                let out_i64: i64 = 0;
                let _: Result<pyo3::Bound<'_, pyo3::PyAny>, pyo3::PyErr> =
                    compute_fn.call((args_i64, out_i64), None);
                black_box(())
            })
        })
    });

    group.finish();
}

/// Benchmark cached Python function dispatch.
///
/// Measures the fast path where the Python function is cached
/// and reused across calls within a single GIL acquisition.
fn bench_cached_dispatch(c: &mut Criterion) {
    // Initialize Python interpreter once.
    Python::initialize();

    let mut group = c.benchmark_group("cached_dispatch");

    // Setup: Create function once (not measured).
    let cached_fn: pyo3::Py<pyo3::PyAny> = Python::attach(|py| {
        let code: &std::ffi::CStr = c"def noop_dispatch(args, out): return 0";
        let globals: pyo3::Bound<'_, pyo3::types::PyDict> = pyo3::types::PyDict::new(py);
        py.run(code, Some(&globals), None)
            .expect("Failed to run code");

        let noop_fn: pyo3::Bound<'_, pyo3::PyAny> = globals
            .get_item("noop_dispatch")
            .expect("Failed to get noop_dispatch")
            .expect("noop_dispatch not found");
        noop_fn.unbind()
    });

    // NOTE: function ids are loader-unique (cached_python_*) so they do NOT
    // collide with the Lua bench's cached_function_* and the JS bench's
    // cached_context_* — all three write to the shared `cached_dispatch`
    // criterion group, so identical ids would overwrite each other.
    group.bench_function("cached_python_single_call", |b| {
        b.iter(|| {
            Python::attach(|py| {
                let noop_fn = cached_fn.bind(py);
                let args_i64: i64 = 0;
                let out_i64: i64 = 0;
                let _: Result<pyo3::Bound<'_, pyo3::PyAny>, pyo3::PyErr> =
                    noop_fn.call((args_i64, out_i64), None);
                black_box(())
            })
        })
    });

    group.bench_function("cached_python_10_calls", |b| {
        b.iter(|| {
            Python::attach(|py| {
                let noop_fn = cached_fn.bind(py);
                let args_i64: i64 = 0;
                let out_i64: i64 = 0;
                for _ in 0..10 {
                    let _: Result<pyo3::Bound<'_, pyo3::PyAny>, pyo3::PyErr> =
                        noop_fn.call((args_i64, out_i64), None);
                }
                black_box(())
            })
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_python_dispatch,
    bench_gil_acquisition,
    bench_native_baseline,
    bench_python_computation,
    bench_cached_dispatch
);
criterion_main!(benches);

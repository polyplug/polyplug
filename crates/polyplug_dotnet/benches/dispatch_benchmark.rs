//! Benchmark for .NET dispatch overhead.
//!
//! Measures the performance characteristics of the .NET dispatch path:
//! 1. Native function pointer call overhead (via netcorehost)
//! 2. Native baseline for comparison
//!
//! NOTE: .NET uses native dispatch (function pointers via [UnmanagedCallersOnly]).
//! The overhead is minimal (~5-10 ns) since it's a direct function pointer call.
//!
//! IMPORTANT: This benchmark does NOT initialize the CLR because:
//! 1. CLR initialization is slow (~100+ ms) and happens once per process
//! 2. The dispatch overhead is just a function pointer call
//! 3. We measure the native function pointer call directly

#![allow(clippy::expect_used)]

use core::hint::black_box;
use criterion::{criterion_group, criterion_main, Criterion};

/// Benchmark native function pointer call overhead.
///
/// .NET plugins use [UnmanagedCallersOnly] which exposes a native function pointer.
/// The dispatch overhead is just a function pointer call (~5-10 ns).
fn bench_native_dispatch(c: &mut Criterion) {
    let mut group = c.benchmark_group("native_dispatch");

    // Simulate the .NET dispatch signature:
    // unsafe extern "system" fn(*mut c_void, *const HostVTable, *const PluginContext) -> u32
    type DotnetInitFn = unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *const core::ffi::c_void,
        *const core::ffi::c_void,
    ) -> u32;

    unsafe extern "system" fn mock_init(
        _rt_ctx: *mut core::ffi::c_void,
        _host_vtable: *const core::ffi::c_void,
        _ctx: *const core::ffi::c_void,
    ) -> u32 {
        0 // ABI_OK
    }

    let func_ptr: DotnetInitFn = mock_init;

    group.bench_function("native_function_pointer_call", |b| {
        b.iter(|| {
            // SAFETY: mock_init is a safe function, just returns 0.
            let result: u32 =
                unsafe { func_ptr(std::ptr::null_mut(), std::ptr::null(), std::ptr::null()) };
            black_box(result)
        })
    });

    // Measure 10 calls to amortize benchmark overhead.
    group.bench_function("native_function_pointer_10_calls", |b| {
        b.iter(|| {
            for _ in 0..10 {
                // SAFETY: mock_init is a safe function, just returns 0.
                let result: u32 =
                    unsafe { func_ptr(std::ptr::null_mut(), std::ptr::null(), std::ptr::null()) };
                black_box(result);
            }
            black_box(())
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

/// Benchmark the dispatch function signature used by .NET.
///
/// Measures the overhead of calling through a function pointer with
/// the exact signature used by polyplug_dotnet.
fn bench_dispatch_signature(c: &mut Criterion) {
    let mut group = c.benchmark_group("dispatch_signature");

    // Exact signature from polyplug_dotnet::context::InitFn
    type InitFn = unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *const core::ffi::c_void,
        *const core::ffi::c_void,
    ) -> u32;

    unsafe extern "system" fn noop_init(
        _rt_ctx: *mut core::ffi::c_void,
        _host_vtable: *const core::ffi::c_void,
        _ctx: *const core::ffi::c_void,
    ) -> u32 {
        0
    }

    let init_fn: InitFn = noop_init;

    // Null pointers (fastest case).
    group.bench_function("dispatch_with_null_pointers", |b| {
        b.iter(|| {
            // SAFETY: noop_init is safe to call with null pointers.
            let result: u32 =
                unsafe { init_fn(std::ptr::null_mut(), std::ptr::null(), std::ptr::null()) };
            black_box(result)
        })
    });

    // Stack-allocated context (realistic case).
    group.bench_function("dispatch_with_stack_context", |b| {
        b.iter(|| {
            let mut rt_ctx: u64 = 0;
            let host_vtable: u64 = 0;
            let ctx: u64 = 0;
            // SAFETY: noop_init is safe to call with any pointers.
            let result: u32 = unsafe {
                init_fn(
                    &mut rt_ctx as *mut u64 as *mut core::ffi::c_void,
                    &host_vtable as *const u64 as *const core::ffi::c_void,
                    &ctx as *const u64 as *const core::ffi::c_void,
                )
            };
            black_box(result)
        })
    });

    group.finish();
}

/// Benchmark computation through function pointer.
///
/// Measures the overhead of a function that does actual work,
/// to compare against the no-op dispatch baseline.
fn bench_computation_dispatch(c: &mut Criterion) {
    let mut group = c.benchmark_group("computation_dispatch");

    type ComputeFn = unsafe extern "system" fn(i64, i64) -> i64;

    unsafe extern "system" fn compute_sum(_args: i64, _out: i64) -> i64 {
        let mut sum: i64 = 0;
        for i in 0..100 {
            sum += i;
        }
        sum
    }

    let compute_fn: ComputeFn = compute_sum;

    group.bench_function("computation_100_iterations", |b| {
        b.iter(|| {
            // SAFETY: compute_sum is safe to call with any values.
            let result: i64 = unsafe { compute_fn(0, 0) };
            black_box(result)
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_native_dispatch,
    bench_native_baseline,
    bench_dispatch_signature,
    bench_computation_dispatch
);
criterion_main!(benches);

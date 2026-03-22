//! Benchmark for Lua dispatch overhead.
//!
//! Measures the performance characteristics of the Lua dispatch path:
//! 1. Lua VM function call overhead (via mlua/LuaJIT)
//! 2. Native baseline for comparison
//! 3. Full dispatch path with args/out pointers

#![allow(clippy::expect_used)]

use core::hint::black_box;
use criterion::{Criterion, criterion_group, criterion_main};
use mlua::Function;
use mlua::Lua;

/// Benchmark Lua VM dispatch overhead.
///
/// Measures the time to call a no-op Lua function through the mlua API.
/// This simulates the dispatch path used by polyplug_lua::loader::lua_dispatch.
fn bench_lua_dispatch(c: &mut Criterion) {
    // SAFETY: We trust the Lua scripts loaded in this benchmark.
    // Lua::unsafe_new() enables the FFI module required by LuaJIT.
    let lua: Lua = unsafe { Lua::unsafe_new() };

    // Define a simple no-op function that matches the polyplug ABI signature.
    let lua_code: &str = r#"
        function noop_dispatch(args, out)
            return 0
        end
    "#;

    lua.load(lua_code).exec().expect("Failed to load Lua code");

    let noop_fn: Function = lua
        .globals()
        .get::<Function>("noop_dispatch")
        .expect("Failed to get noop_dispatch function");

    let mut group = c.benchmark_group("lua_dispatch");

    // Measure single dispatch call.
    group.bench_function("vm_dispatch_single_call", |b| {
        b.iter(|| {
            // Pass args/out as i64 (pointer-width integers) matching the real dispatch.
            let args_i64: i64 = 0;
            let out_i64: i64 = 0;
            let _: Result<(), mlua::Error> = noop_fn.call::<()>((args_i64, out_i64));
            black_box(())
        })
    });

    // Measure 10 dispatch calls to amortize benchmark overhead.
    group.bench_function("vm_dispatch_10_calls", |b| {
        b.iter(|| {
            let args_i64: i64 = 0;
            let out_i64: i64 = 0;
            for _ in 0..10 {
                let _: Result<(), mlua::Error> = noop_fn.call::<()>((args_i64, out_i64));
            }
            black_box(())
        })
    });

    group.finish();
}

/// Benchmark Lua VM creation overhead.
///
/// Creating a new Lua VM is expensive (~100+ µs). This benchmark
/// quantifies that cost to justify caching VMs across dispatch calls.
fn bench_lua_vm_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("lua_vm_creation");

    group.bench_function("create_unsafe_vm", |b| {
        b.iter(|| {
            // SAFETY: Benchmark only, no untrusted code loaded.
            let lua: Lua = unsafe { Lua::unsafe_new() };
            black_box(lua)
        })
    });

    group.bench_function("create_vm_and_load_code", |b| {
        b.iter(|| {
            // SAFETY: Benchmark only, no untrusted code loaded.
            let lua: Lua = unsafe { Lua::unsafe_new() };
            let lua_code: &str = r#"
                function noop_dispatch(args, out)
                    return 0
                end
            "#;
            lua.load(lua_code).exec().expect("Failed to load Lua code");
            black_box(lua)
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

/// Benchmark Lua computation (non-trivial work).
///
/// Measures the overhead of a Lua function that does actual work,
/// to compare against the no-op dispatch baseline.
fn bench_lua_computation(c: &mut Criterion) {
    // SAFETY: Benchmark only, no untrusted code loaded.
    let lua: Lua = unsafe { Lua::unsafe_new() };

    let compute_code: &str = r#"
        function compute_sum(args, out)
            local sum = 0
            for i = 1, 100 do
                sum = sum + i
            end
            return sum
        end
    "#;

    lua.load(compute_code)
        .exec()
        .expect("Failed to load compute code");

    let compute_fn: Function = lua
        .globals()
        .get::<Function>("compute_sum")
        .expect("Failed to get compute_sum function");

    let mut group = c.benchmark_group("lua_computation");

    group.bench_function("lua_computation_100_iterations", |b| {
        b.iter(|| {
            let args_i64: i64 = 0;
            let out_i64: i64 = 0;
            let _: Result<i64, mlua::Error> = compute_fn.call::<i64>((args_i64, out_i64));
            black_box(())
        })
    });

    group.finish();
}

/// Benchmark cached Lua function dispatch.
///
/// Measures the fast path where the Lua VM and function are cached
/// and reused across calls (similar to the JS Persistent<Function> pattern).
fn bench_cached_dispatch(c: &mut Criterion) {
    // SAFETY: Benchmark only, no untrusted code loaded.
    let lua: Lua = unsafe { Lua::unsafe_new() };

    let lua_code: &str = r#"
        function noop_dispatch(args, out)
            return 0
        end
    "#;

    lua.load(lua_code).exec().expect("Failed to load Lua code");

    // Cache the function once (not measured).
    let cached_fn: Function = lua
        .globals()
        .get::<Function>("noop_dispatch")
        .expect("Failed to get noop_dispatch function");

    let mut group = c.benchmark_group("cached_dispatch");

    group.bench_function("cached_function_single_call", |b| {
        b.iter(|| {
            let args_i64: i64 = 0;
            let out_i64: i64 = 0;
            let _: Result<(), mlua::Error> = cached_fn.call::<()>((args_i64, out_i64));
            black_box(())
        })
    });

    group.bench_function("cached_function_10_calls", |b| {
        b.iter(|| {
            let args_i64: i64 = 0;
            let out_i64: i64 = 0;
            for _ in 0..10 {
                let _: Result<(), mlua::Error> = cached_fn.call::<()>((args_i64, out_i64));
            }
            black_box(())
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_lua_dispatch,
    bench_lua_vm_creation,
    bench_native_baseline,
    bench_lua_computation,
    bench_cached_dispatch
);
criterion_main!(benches);

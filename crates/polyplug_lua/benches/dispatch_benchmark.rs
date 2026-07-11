//! Benchmark for Lua dispatch overhead.
//!
//! Measures the performance characteristics of the Lua dispatch path:
//! 1. Lua VM function call overhead (via mlua/LuaJIT)
//! 2. Native baseline for comparison
//! 3. Full dispatch path with args/out pointers

#![allow(clippy::expect_used)]

use core::ffi::c_void;
use core::hint::black_box;
use core::ptr;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use criterion::{Criterion, criterion_group, criterion_main};
use mlua::Function;
use mlua::Lua;
use polyplug::runtime::Runtime;
use polyplug::runtime::RuntimeBuilder;
use polyplug_abi::StringView;
use polyplug_abi::runtime::Compatibility;
use polyplug_abi::runtime::RuntimeConfig;

use polyplug_lua::LuaLoader;
use polyplug_lua::ffi::PolyplugLuaLogBridge;
use polyplug_lua::ffi::polyplug_lua_log_trampoline;

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

/// Scalar log sink standing in for the LuaJIT-created callback.
///
/// Same signature the Lua host SDK installs:
/// `(user_data, level, scope_ptr, scope_len, msg_ptr, msg_len)`. It `black_box`es
/// its inputs so the optimizer cannot prove the trampoline's work is dead — but
/// does NOT cross into a real Lua VM, so this measures the *Rust-side* trampoline
/// cost (bridge read + StringView decomposition + the indirect callback call),
/// not the LuaJIT-callback + `ffi.string` cost (that full path is measured by the
/// `POLYPLUG_BENCH_ITERS` arm in `sdks/lua/host/tests/test_log_runtime.lua`,
/// ~255 ns/line locally).
unsafe extern "C" fn scalar_log_sink(
    user_data: *mut c_void,
    level: u32,
    scope_ptr: *const u8,
    scope_len: usize,
    msg_ptr: *const u8,
    msg_len: usize,
) {
    black_box((user_data, level, scope_ptr, scope_len, msg_ptr, msg_len));
}

/// Benchmark the Lua host custom-logger delivery trampoline (Rust side).
///
/// One `polyplug_lua_log_trampoline` call — the exact `RuntimeConfig::log`
/// signature (StringViews by value) — through a real `PolyplugLuaLogBridge` into
/// a scalar Rust callback. This is the cost the runtime pays per *delivered* log
/// line to bridge the by-value-StringView `RuntimeConfig::log` ABI down to the
/// scalar-only signature a LuaJIT FFI callback can implement. Disabled levels are
/// filtered inside the runtime before any of this runs, so this cost is paid only
/// for records that pass `log_max_level`.
fn bench_lua_log_trampoline(c: &mut Criterion) {
    let mut bridge: PolyplugLuaLogBridge = PolyplugLuaLogBridge {
        callback: Some(scalar_log_sink),
        user_data: ptr::null_mut(),
    };
    let bridge_ptr: *mut c_void = &mut bridge as *mut PolyplugLuaLogBridge as *mut c_void;
    let scope: StringView = StringView::from_static(b"loader.lua");
    let message: StringView = StringView::from_static(b"bundle dep 'x' has no bundle_id");

    let mut group = c.benchmark_group("lua_log");

    group.bench_function("trampoline_delivery", |b| {
        b.iter(|| {
            // SAFETY: bridge_ptr points to a live PolyplugLuaLogBridge on this
            // function's stack with a valid scalar callback; the StringViews point
            // at 'static byte literals, satisfying the trampoline's contract.
            unsafe {
                polyplug_lua_log_trampoline(
                    black_box(bridge_ptr),
                    black_box(2_u32),
                    black_box(scope),
                    black_box(message),
                );
            }
        })
    });

    group.finish();
}

/// A minimal valid Lua plugin script implementing `test.loader@1` with one
/// no-op function — the same shape the loader's reload integration test uses.
fn reload_plugin_script() -> &'static [u8] {
    br#"
local ffi = require("ffi")
local function make_noop(_host) return {} end
local function impl_noop(_instance, _args_ptr, _out_ptr)
end
function polyplug_init(_registrar_ptr, _ctx_ptr)
    return {
        ["test.loader"] = {
            contract_version = 1,
            plugin_name      = "lua-reload-bench",
            factory          = make_noop,
            functions        = { [0] = impl_noop },
        },
    }, { code = 0 }
end
"#
}

/// Write `content` to a temp bundle directory with a `manifest.toml` naming the
/// Lua loader. Returns the dir (kept alive) and the bundle dir path.
fn write_temp_lua_bundle(name: &str) -> (tempfile::TempDir, PathBuf) {
    let dir: tempfile::TempDir = tempfile::tempdir().expect("tempdir");
    fs::write(dir.path().join("bundle.lua"), reload_plugin_script()).expect("write bundle.lua");
    let bundle_id: u64 = polyplug_utils::bundle_id(name);
    let manifest: String = format!(
        "id = {}\nname = \"{}\"\nloader = \"lua\"\nfile = \"bundle.lua\"\n",
        bundle_id, name
    );
    fs::write(dir.path().join("manifest.toml"), manifest).expect("write manifest.toml");
    let bundle_dir: PathBuf = dir.path().to_path_buf();
    (dir, bundle_dir)
}

/// Benchmark the Lua loader hot-reload swap path.
///
/// `Runtime::reload_bundle` for a Lua bundle re-reads the on-disk entry file,
/// rebuilds (or reuses) the per-bundle Lua VM, re-runs `polyplug_init`, registers
/// the contract, and atomically swaps the live interface (retiring the old one).
/// This is the one-time cost a host pays to swap a Lua plugin's code WITHOUT
/// restarting — amortized over every subsequent dispatch (see benches/README.md
/// for the amortization curve). Hot-reload is gated on `hot_reload_enabled`.
fn bench_lua_reload(c: &mut Criterion) {
    let runtime: Arc<Runtime> = RuntimeBuilder::new()
        .config(RuntimeConfig {
            compatibility: Compatibility::Strict,
            hot_reload_enabled: true,
            ..Default::default()
        })
        .loader(LuaLoader::new())
        .build()
        .expect("runtime build must succeed");

    let (_dir, bundle_dir): (tempfile::TempDir, PathBuf) =
        write_temp_lua_bundle("lua_reload_bench");
    runtime
        .load_bundle(&bundle_dir)
        .expect("initial bundle load must succeed");

    let mut group = c.benchmark_group("lua_reload");

    group.bench_function("hot_reload_swap", |b| {
        b.iter(|| {
            runtime
                .reload_bundle(black_box(bundle_dir.as_path()))
                .expect("reload_bundle must succeed");
        })
    });

    group.finish();
    // Keep the temp dir alive for the whole bench (the bundle is read on reload).
    drop(_dir);
}

criterion_group!(
    benches,
    bench_lua_dispatch,
    bench_lua_vm_creation,
    bench_native_baseline,
    bench_lua_computation,
    bench_cached_dispatch,
    bench_lua_log_trampoline,
    bench_lua_reload
);
criterion_main!(benches);

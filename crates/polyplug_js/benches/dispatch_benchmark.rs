//! Benchmark for QuickJS dispatch overhead.
//!
//! Measures the performance characteristics of the JS dispatch path:
//! 1. Context creation per call (OLD approach - slow)
//! 2. Cached Context with Persistent<Function> (NEW approach - fast)
//! 3. Polyplug helper setup (readI32, writeI32)
//! 4. JS function execution
//! 5. Full round-trip time
//!
//! Key insight: Creating a new Context for each call adds ~107 µs overhead.
//! Caching the Context and using Persistent<Function> reduces this to ~14 µs.

#![allow(clippy::expect_used)]

use core::hint::black_box;
use std::path::PathBuf;
use std::sync::Arc;

use criterion::{Criterion, criterion_group, criterion_main};
use rquickjs::{Context, Function, Object, Persistent, Runtime};

// `rquickjs::Runtime` and `polyplug::runtime::Runtime` collide on the bare name,
// so the polyplug runtime types are referenced through fully-qualified paths in
// the reload arm below rather than imported.

fn bench_js_dispatch(c: &mut Criterion) {
    let runtime: Runtime = Runtime::new().expect("Failed to create runtime");

    let bundle: &str = r#"
        globalThis.TEST_VTABLE = {
            contractLo: 0xB0410D2B,
            contractHi: 0xCC4232FA,
            fnCount: 1,
            contractName: "test.add",
            functions: [{
                call: function(argsLo, argsHi, outLo, outHi) {
                    return 0;
                }
            }]
        };
    "#;

    let mut group = c.benchmark_group("js_dispatch");

    group.bench_function("full_dispatch_path", |b| {
        b.iter(|| {
            let ctx: Context = Context::full(&runtime).expect("Failed to create context");
            ctx.with(|ctx| {
                let polyplug: Object<'_> =
                    Object::new(ctx.clone()).expect("Failed to create object");

                let read_i32: Function<'_> =
                    Function::new(ctx.clone(), |lo: u32, hi: u32| -> i32 {
                        let ptr: *const i32 =
                            ((hi as u64) << 32 | lo as u64) as usize as *const i32;
                        if ptr.is_null() {
                            return 0;
                        }
                        // SAFETY: ptr is a valid pointer for benchmark purposes (null check above)
                        unsafe { *ptr }
                    })
                    .expect("Failed to create readI32");
                polyplug
                    .set("readI32", read_i32)
                    .expect("Failed to set readI32");

                let write_i32: Function<'_> =
                    Function::new(ctx.clone(), |lo: u32, hi: u32, val: i32| {
                        let ptr: *mut i32 = ((hi as u64) << 32 | lo as u64) as usize as *mut i32;
                        if ptr.is_null() {
                            return;
                        }
                        // SAFETY: ptr is a valid pointer for benchmark purposes (null check above)
                        unsafe {
                            *ptr = val;
                        }
                    })
                    .expect("Failed to create writeI32");
                polyplug
                    .set("writeI32", write_i32)
                    .expect("Failed to set writeI32");

                ctx.globals()
                    .set("polyplug", polyplug)
                    .expect("Failed to set polyplug");

                ctx.eval::<rquickjs::Value, _>(bundle)
                    .expect("Failed to eval bundle");

                let vtable: Object<'_> = ctx
                    .globals()
                    .get::<_, Object<'_>>("TEST_VTABLE")
                    .expect("Failed to get vtable");
                let funcs: rquickjs::Array<'_> = vtable
                    .get::<_, rquickjs::Array<'_>>("functions")
                    .expect("Failed to get functions array");
                let func_obj: Object<'_> = funcs
                    .get::<Object<'_>>(0)
                    .expect("Failed to get function object");
                let func: Function<'_> = func_obj
                    .get::<_, Function<'_>>("call")
                    .expect("Failed to get call function");

                func.call::<(u32, u32, u32, u32), ()>((0, 0, 0, 0))
                    .expect("Failed to call function");

                black_box(())
            })
        })
    });

    group.bench_function("context_creation_only", |b| {
        b.iter(|| {
            let ctx: Context = Context::full(&runtime).expect("Failed to create context");
            black_box(ctx)
        })
    });

    group.bench_function("context_and_eval", |b| {
        b.iter(|| {
            let ctx: Context = Context::full(&runtime).expect("Failed to create context");
            ctx.with(|ctx| {
                ctx.eval::<rquickjs::Value, _>(bundle)
                    .expect("Failed to eval");
                black_box(())
            })
        })
    });

    group.bench_function("context_and_polyplug_setup", |b| {
        b.iter(|| {
            let ctx: Context = Context::full(&runtime).expect("Failed to create context");
            ctx.with(|ctx| {
                let polyplug: Object<'_> =
                    Object::new(ctx.clone()).expect("Failed to create object");

                let read_i32: Function<'_> =
                    Function::new(ctx.clone(), |lo: u32, hi: u32| -> i32 {
                        let ptr: *const i32 =
                            ((hi as u64) << 32 | lo as u64) as usize as *const i32;
                        if ptr.is_null() {
                            return 0;
                        }
                        // SAFETY: ptr is a valid pointer for benchmark purposes
                        unsafe { *ptr }
                    })
                    .expect("Failed to create readI32");
                polyplug
                    .set("readI32", read_i32)
                    .expect("Failed to set readI32");

                let write_i32: Function<'_> =
                    Function::new(ctx.clone(), |lo: u32, hi: u32, _val: i32| {
                        let ptr: *mut i32 = ((hi as u64) << 32 | lo as u64) as usize as *mut i32;
                        if ptr.is_null() {}
                    })
                    .expect("Failed to create writeI32");
                polyplug
                    .set("writeI32", write_i32)
                    .expect("Failed to set writeI32");

                ctx.globals()
                    .set("polyplug", polyplug)
                    .expect("Failed to set polyplug");

                black_box(())
            })
        })
    });

    group.bench_function("full_dispatch_10_calls", |b| {
        b.iter(|| {
            let ctx: Context = Context::full(&runtime).expect("Failed to create context");
            ctx.with(|ctx| {
                let polyplug: Object<'_> =
                    Object::new(ctx.clone()).expect("Failed to create object");

                let read_i32: Function<'_> =
                    Function::new(ctx.clone(), |lo: u32, hi: u32| -> i32 {
                        let ptr: *const i32 =
                            ((hi as u64) << 32 | lo as u64) as usize as *const i32;
                        if ptr.is_null() {
                            return 0;
                        }
                        // SAFETY: ptr is a valid pointer for benchmark purposes
                        unsafe { *ptr }
                    })
                    .expect("Failed to create readI32");
                polyplug
                    .set("readI32", read_i32)
                    .expect("Failed to set readI32");

                let write_i32: Function<'_> =
                    Function::new(ctx.clone(), |lo: u32, hi: u32, val: i32| {
                        let ptr: *mut i32 = ((hi as u64) << 32 | lo as u64) as usize as *mut i32;
                        if ptr.is_null() {
                            return;
                        }
                        // SAFETY: ptr is a valid pointer for benchmark purposes
                        unsafe {
                            *ptr = val;
                        }
                    })
                    .expect("Failed to create writeI32");
                polyplug
                    .set("writeI32", write_i32)
                    .expect("Failed to set writeI32");

                ctx.globals()
                    .set("polyplug", polyplug)
                    .expect("Failed to set polyplug");

                ctx.eval::<rquickjs::Value, _>(bundle)
                    .expect("Failed to eval bundle");

                let vtable: Object<'_> = ctx
                    .globals()
                    .get::<_, Object<'_>>("TEST_VTABLE")
                    .expect("Failed to get vtable");
                let funcs: rquickjs::Array<'_> = vtable
                    .get::<_, rquickjs::Array<'_>>("functions")
                    .expect("Failed to get functions array");
                let func_obj: Object<'_> = funcs
                    .get::<Object<'_>>(0)
                    .expect("Failed to get function object");
                let func: Function<'_> = func_obj
                    .get::<_, Function<'_>>("call")
                    .expect("Failed to get call function");

                for _ in 0..10 {
                    func.call::<(u32, u32, u32, u32), ()>((0, 0, 0, 0))
                        .expect("Failed to call function");
                }

                black_box(())
            })
        })
    });

    group.finish();
}

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

fn bench_js_computation(c: &mut Criterion) {
    let runtime: Runtime = Runtime::new().expect("Failed to create runtime");

    let compute_bundle: &str = r#"
        globalThis.COMPUTE_VTABLE = {
            contractLo: 0x11111111,
            contractHi: 0x22222222,
            fnCount: 1,
            contractName: "test.compute",
            functions: [{
                call: function(argsLo, argsHi, outLo, outHi) {
                    let sum = 0;
                    for (let i = 0; i < 100; i++) {
                        sum += i;
                    }
                    return sum;
                }
            }]
        };
    "#;

    let mut group = c.benchmark_group("js_computation");

    group.bench_function("js_computation_100_iterations", |b| {
        b.iter(|| {
            let ctx: Context = Context::full(&runtime).expect("Failed to create context");
            ctx.with(|ctx| {
                let polyplug: Object<'_> =
                    Object::new(ctx.clone()).expect("Failed to create object");

                let read_i32: Function<'_> =
                    Function::new(ctx.clone(), |lo: u32, hi: u32| -> i32 {
                        let ptr: *const i32 =
                            ((hi as u64) << 32 | lo as u64) as usize as *const i32;
                        if ptr.is_null() {
                            return 0;
                        }
                        // SAFETY: ptr is a valid pointer for benchmark purposes
                        unsafe { *ptr }
                    })
                    .expect("Failed to create readI32");
                polyplug
                    .set("readI32", read_i32)
                    .expect("Failed to set readI32");

                let write_i32: Function<'_> =
                    Function::new(ctx.clone(), |lo: u32, hi: u32, val: i32| {
                        let ptr: *mut i32 = ((hi as u64) << 32 | lo as u64) as usize as *mut i32;
                        if ptr.is_null() {
                            return;
                        }
                        // SAFETY: ptr is a valid pointer for benchmark purposes
                        unsafe {
                            *ptr = val;
                        }
                    })
                    .expect("Failed to create writeI32");
                polyplug
                    .set("writeI32", write_i32)
                    .expect("Failed to set writeI32");

                ctx.globals()
                    .set("polyplug", polyplug)
                    .expect("Failed to set polyplug");

                ctx.eval::<rquickjs::Value, _>(compute_bundle)
                    .expect("Failed to eval bundle");

                let vtable: Object<'_> = ctx
                    .globals()
                    .get::<_, Object<'_>>("COMPUTE_VTABLE")
                    .expect("Failed to get vtable");
                let funcs: rquickjs::Array<'_> = vtable
                    .get::<_, rquickjs::Array<'_>>("functions")
                    .expect("Failed to get functions array");
                let func_obj: Object<'_> = funcs
                    .get::<Object<'_>>(0)
                    .expect("Failed to get function object");
                let func: Function<'_> = func_obj
                    .get::<_, Function<'_>>("call")
                    .expect("Failed to get call function");

                func.call::<(u32, u32, u32, u32), ()>((0, 0, 0, 0))
                    .expect("Failed to call function");

                black_box(())
            })
        })
    });

    group.finish();
}

/// Benchmark the NEW optimized approach: cached Context with Persistent<Function>.
///
/// This measures the fast path where:
/// 1. Context is created ONCE before the benchmark loop
/// 2. Function is stored in Persistent<Function> for reuse
/// 3. Only the dispatch call is measured (not Context creation)
fn bench_cached_dispatch(c: &mut Criterion) {
    let runtime: Runtime = Runtime::new().expect("Failed to create runtime");

    let bundle: &str = r#"
        globalThis.TEST_VTABLE = {
            contractLo: 0xB0410D2B,
            contractHi: 0xCC4232FA,
            fnCount: 1,
            contractName: "test.add",
            functions: [{
                call: function(argsLo, argsHi, outLo, outHi) {
                    return 0;
                }
            }]
        };
    "#;

    let mut group = c.benchmark_group("cached_dispatch");

    // NEW approach: Create Context ONCE, reuse for all calls
    group.bench_function("cached_context_single_call", |b| {
        // Setup: Create Context ONCE (not measured)
        let ctx: Context = Context::full(&runtime).expect("Failed to create context");

        // Setup: Get the function and store it in Persistent (not measured)
        let func_persistent: Persistent<Function<'static>> = ctx.with(|ctx| {
            let polyplug: Object<'_> = Object::new(ctx.clone()).expect("Failed to create object");

            let read_i32: Function<'_> = Function::new(ctx.clone(), |lo: u32, hi: u32| -> i32 {
                let ptr: *const i32 = ((hi as u64) << 32 | lo as u64) as usize as *const i32;
                if ptr.is_null() {
                    return 0;
                }
                // SAFETY: ptr is a valid pointer for benchmark purposes (null check above)
                unsafe { *ptr }
            })
            .expect("Failed to create readI32");
            polyplug
                .set("readI32", read_i32)
                .expect("Failed to set readI32");

            let write_i32: Function<'_> =
                Function::new(ctx.clone(), |lo: u32, hi: u32, val: i32| {
                    let ptr: *mut i32 = ((hi as u64) << 32 | lo as u64) as usize as *mut i32;
                    if ptr.is_null() {
                        return;
                    }
                    // SAFETY: ptr is a valid pointer for benchmark purposes (null check above)
                    unsafe {
                        *ptr = val;
                    }
                })
                .expect("Failed to create writeI32");
            polyplug
                .set("writeI32", write_i32)
                .expect("Failed to set writeI32");

            ctx.globals()
                .set("polyplug", polyplug)
                .expect("Failed to set polyplug");

            ctx.eval::<rquickjs::Value, _>(bundle)
                .expect("Failed to eval bundle");

            let vtable: Object<'_> = ctx
                .globals()
                .get::<_, Object<'_>>("TEST_VTABLE")
                .expect("Failed to get vtable");
            let funcs: rquickjs::Array<'_> = vtable
                .get::<_, rquickjs::Array<'_>>("functions")
                .expect("Failed to get functions array");
            let func_obj: Object<'_> = funcs
                .get::<Object<'_>>(0)
                .expect("Failed to get function object");
            let func: Function<'_> = func_obj
                .get::<_, Function<'_>>("call")
                .expect("Failed to get call function");

            // SAFETY: Persistent::save stores the function for reuse across scope boundaries
            Persistent::save(&ctx, func)
        });

        // Measure: Only the dispatch call (Context creation is NOT included)
        b.iter(|| {
            ctx.with(|ctx| {
                let func: Function<'_> = func_persistent
                    .clone()
                    .restore(&ctx)
                    .expect("Failed to restore function");
                func.call::<(u32, u32, u32, u32), ()>((0, 0, 0, 0))
                    .expect("Failed to call function");
            });
            black_box(())
        })
    });

    // NEW approach: 10 calls with cached Context
    group.bench_function("cached_context_10_calls", |b| {
        let ctx: Context = Context::full(&runtime).expect("Failed to create context");

        let func_persistent: Persistent<Function<'static>> = ctx.with(|ctx| {
            let polyplug: Object<'_> = Object::new(ctx.clone()).expect("Failed to create object");

            let read_i32: Function<'_> = Function::new(ctx.clone(), |lo: u32, hi: u32| -> i32 {
                let ptr: *const i32 = ((hi as u64) << 32 | lo as u64) as usize as *const i32;
                if ptr.is_null() {
                    return 0;
                }
                // SAFETY: ptr is a valid pointer for benchmark purposes
                unsafe { *ptr }
            })
            .expect("Failed to create readI32");
            polyplug
                .set("readI32", read_i32)
                .expect("Failed to set readI32");

            let write_i32: Function<'_> =
                Function::new(ctx.clone(), |lo: u32, hi: u32, val: i32| {
                    let ptr: *mut i32 = ((hi as u64) << 32 | lo as u64) as usize as *mut i32;
                    if ptr.is_null() {
                        return;
                    }
                    // SAFETY: ptr is a valid pointer for benchmark purposes
                    unsafe {
                        *ptr = val;
                    }
                })
                .expect("Failed to create writeI32");
            polyplug
                .set("writeI32", write_i32)
                .expect("Failed to set writeI32");

            ctx.globals()
                .set("polyplug", polyplug)
                .expect("Failed to set polyplug");

            ctx.eval::<rquickjs::Value, _>(bundle)
                .expect("Failed to eval bundle");

            let vtable: Object<'_> = ctx
                .globals()
                .get::<_, Object<'_>>("TEST_VTABLE")
                .expect("Failed to get vtable");
            let funcs: rquickjs::Array<'_> = vtable
                .get::<_, rquickjs::Array<'_>>("functions")
                .expect("Failed to get functions array");
            let func_obj: Object<'_> = funcs
                .get::<Object<'_>>(0)
                .expect("Failed to get function object");
            let func: Function<'_> = func_obj
                .get::<_, Function<'_>>("call")
                .expect("Failed to get call function");

            Persistent::save(&ctx, func)
        });

        b.iter(|| {
            ctx.with(|ctx| {
                let func: Function<'_> = func_persistent
                    .clone()
                    .restore(&ctx)
                    .expect("Failed to restore function");
                for _ in 0..10 {
                    func.call::<(u32, u32, u32, u32), ()>((0, 0, 0, 0))
                        .expect("Failed to call function");
                }
            });
            black_box(())
        })
    });

    group.finish();
}

/// Side-by-side comparison of OLD vs NEW approach.
///
/// This benchmark group directly compares:
/// - OLD: Create Context fresh for each call (slow - ~107 µs overhead)
/// - NEW: Reuse cached Context with Persistent<Function> (fast - ~14 µs)
fn bench_dispatch_comparison(c: &mut Criterion) {
    let runtime: Runtime = Runtime::new().expect("Failed to create runtime");

    let bundle: &str = r#"
        globalThis.TEST_VTABLE = {
            contractLo: 0xB0410D2B,
            contractHi: 0xCC4232FA,
            fnCount: 1,
            contractName: "test.add",
            functions: [{
                call: function(argsLo, argsHi, outLo, outHi) {
                    return 0;
                }
            }]
        };
    "#;

    let mut group = c.benchmark_group("dispatch_comparison");

    // OLD approach: Create Context for each call (measured in js_dispatch group)
    group.bench_function("old_fresh_context_per_call", |b| {
        b.iter(|| {
            let ctx: Context = Context::full(&runtime).expect("Failed to create context");
            ctx.with(|ctx| {
                let polyplug: Object<'_> =
                    Object::new(ctx.clone()).expect("Failed to create object");

                let read_i32: Function<'_> =
                    Function::new(ctx.clone(), |lo: u32, hi: u32| -> i32 {
                        let ptr: *const i32 =
                            ((hi as u64) << 32 | lo as u64) as usize as *const i32;
                        if ptr.is_null() {
                            return 0;
                        }
                        // SAFETY: ptr is a valid pointer for benchmark purposes
                        unsafe { *ptr }
                    })
                    .expect("Failed to create readI32");
                polyplug
                    .set("readI32", read_i32)
                    .expect("Failed to set readI32");

                let write_i32: Function<'_> =
                    Function::new(ctx.clone(), |lo: u32, hi: u32, val: i32| {
                        let ptr: *mut i32 = ((hi as u64) << 32 | lo as u64) as usize as *mut i32;
                        if ptr.is_null() {
                            return;
                        }
                        // SAFETY: ptr is a valid pointer for benchmark purposes
                        unsafe {
                            *ptr = val;
                        }
                    })
                    .expect("Failed to create writeI32");
                polyplug
                    .set("writeI32", write_i32)
                    .expect("Failed to set writeI32");

                ctx.globals()
                    .set("polyplug", polyplug)
                    .expect("Failed to set polyplug");

                ctx.eval::<rquickjs::Value, _>(bundle)
                    .expect("Failed to eval bundle");

                let vtable: Object<'_> = ctx
                    .globals()
                    .get::<_, Object<'_>>("TEST_VTABLE")
                    .expect("Failed to get vtable");
                let funcs: rquickjs::Array<'_> = vtable
                    .get::<_, rquickjs::Array<'_>>("functions")
                    .expect("Failed to get functions array");
                let func_obj: Object<'_> = funcs
                    .get::<Object<'_>>(0)
                    .expect("Failed to get function object");
                let func: Function<'_> = func_obj
                    .get::<_, Function<'_>>("call")
                    .expect("Failed to get call function");

                func.call::<(u32, u32, u32, u32), ()>((0, 0, 0, 0))
                    .expect("Failed to call function");

                black_box(())
            })
        })
    });

    // NEW approach: Cached Context with Persistent<Function>
    group.bench_function("new_cached_context_reuse", |b| {
        let ctx: Context = Context::full(&runtime).expect("Failed to create context");

        let func_persistent: Persistent<Function<'static>> = ctx.with(|ctx| {
            let polyplug: Object<'_> = Object::new(ctx.clone()).expect("Failed to create object");

            let read_i32: Function<'_> = Function::new(ctx.clone(), |lo: u32, hi: u32| -> i32 {
                let ptr: *const i32 = ((hi as u64) << 32 | lo as u64) as usize as *const i32;
                if ptr.is_null() {
                    return 0;
                }
                // SAFETY: ptr is a valid pointer for benchmark purposes
                unsafe { *ptr }
            })
            .expect("Failed to create readI32");
            polyplug
                .set("readI32", read_i32)
                .expect("Failed to set readI32");

            let write_i32: Function<'_> =
                Function::new(ctx.clone(), |lo: u32, hi: u32, val: i32| {
                    let ptr: *mut i32 = ((hi as u64) << 32 | lo as u64) as usize as *mut i32;
                    if ptr.is_null() {
                        return;
                    }
                    // SAFETY: ptr is a valid pointer for benchmark purposes
                    unsafe {
                        *ptr = val;
                    }
                })
                .expect("Failed to create writeI32");
            polyplug
                .set("writeI32", write_i32)
                .expect("Failed to set writeI32");

            ctx.globals()
                .set("polyplug", polyplug)
                .expect("Failed to set polyplug");

            ctx.eval::<rquickjs::Value, _>(bundle)
                .expect("Failed to eval bundle");

            let vtable: Object<'_> = ctx
                .globals()
                .get::<_, Object<'_>>("TEST_VTABLE")
                .expect("Failed to get vtable");
            let funcs: rquickjs::Array<'_> = vtable
                .get::<_, rquickjs::Array<'_>>("functions")
                .expect("Failed to get functions array");
            let func_obj: Object<'_> = funcs
                .get::<Object<'_>>(0)
                .expect("Failed to get function object");
            let func: Function<'_> = func_obj
                .get::<_, Function<'_>>("call")
                .expect("Failed to get call function");

            Persistent::save(&ctx, func)
        });

        b.iter(|| {
            ctx.with(|ctx| {
                let func: Function<'_> = func_persistent
                    .clone()
                    .restore(&ctx)
                    .expect("Failed to restore function");
                func.call::<(u32, u32, u32, u32), ()>((0, 0, 0, 0))
                    .expect("Failed to call function");
            });
            black_box(())
        })
    });

    group.finish();
}

/// Build a minimal QuickJS bundle that registers `test.reload.contract@1` with a
/// single no-op function via `polyplug.registerVtable` — the same registration
/// shape the loader's reload integration test uses.
fn make_reload_bundle_js(contract_id: u64, contract_name: &str) -> String {
    let contract_lo: u32 = contract_id as u32;
    let contract_hi: u32 = (contract_id >> 32) as u32;
    format!(
        r#"
function polyplug_init(host_lo, host_hi, ctx_lo, ctx_hi) {{
    var vtable = {{
        contractLo: {contract_lo},
        contractHi: {contract_hi},
        fnCount: 1,
        contractName: "{contract_name}",
        version: 0x00010000,
        functions: [ function(args, out) {{ return 0; }} ]
    }};
    polyplug.registerVtable(
        vtable.contractLo, vtable.contractHi, vtable,
        vtable.fnCount, vtable.contractName, vtable.version
    );
}}
"#
    )
}

/// Write a JS bundle + manifest naming the QuickJS loader to a temp dir. Returns
/// the dir (kept alive) and the bundle dir path.
fn write_temp_js_bundle(name: &str, content: &str) -> (tempfile::TempDir, PathBuf) {
    let dir: tempfile::TempDir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("bundle.js"), content).expect("write bundle.js");
    let bundle_id: u64 = polyplug_utils::bundle_id(name);
    let manifest: String = format!(
        "id = {}\nname = \"{}\"\nloader = \"js-quickjs\"\nfile = \"bundle.js\"\n",
        bundle_id, name
    );
    std::fs::write(dir.path().join("manifest.toml"), manifest).expect("write manifest.toml");
    let bundle_dir: PathBuf = dir.path().to_path_buf();
    (dir, bundle_dir)
}

/// Benchmark the QuickJS loader hot-reload swap path.
///
/// `Runtime::reload_bundle` for a JS bundle re-reads the on-disk source, rebuilds
/// the per-bundle QuickJS context, re-runs `polyplug_init`, registers the
/// contract, and atomically swaps the live interface (retiring the old one). This
/// is the one-time cost a host pays to swap a JS plugin's code WITHOUT restarting
/// — amortized over every subsequent dispatch (see benches/README.md for the
/// amortization curve). Hot-reload is gated on `hot_reload_enabled`.
fn bench_js_reload(c: &mut Criterion) {
    let contract_id: u64 = polyplug_utils::guest_contract_id("test.reload.contract", 1);
    let bundle: String = make_reload_bundle_js(contract_id, "test.reload.contract");
    let (_dir, bundle_dir): (tempfile::TempDir, PathBuf) =
        write_temp_js_bundle("test.reload.contract", &bundle);

    let runtime: Arc<polyplug::runtime::Runtime> = polyplug::runtime::RuntimeBuilder::new()
        .loader(polyplug_js::JsLoader::new(polyplug_js::JsConfig {}))
        .config(polyplug_abi::RuntimeConfig {
            compatibility: polyplug_abi::Compatibility::Strict,
            hot_reload_enabled: true,
            ..Default::default()
        })
        .build()
        .expect("runtime build must succeed");

    runtime
        .load_bundle(bundle_dir.as_path())
        .expect("initial bundle load must succeed");

    let mut group = c.benchmark_group("js_reload");

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
    bench_js_dispatch,
    bench_native_baseline,
    bench_js_computation,
    bench_cached_dispatch,
    bench_dispatch_comparison,
    bench_js_reload
);
criterion_main!(benches);

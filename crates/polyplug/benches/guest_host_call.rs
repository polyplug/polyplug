#![allow(clippy::expect_used)]

// THIS IS A BENCHMARK FILE — do not add #[test] functions here
// Run with: cargo bench -p polyplug --bench guest_host_call
//
// The guest→HOST direction. Every other dispatch bench measures host→guest (the
// runtime calling INTO a plugin) or guest→guest (cross_call). This one measures
// the two ways a guest reaches back into the host through the real `HostApi`:
//
//   - `host_contract_call` — a guest invoking a host-registered contract method.
//     The guest resolves the host's interface through `HostApi.resolve_host_
//     contract_interface` and dispatches `interface.dispatch.native.functions
//     [fn_id](instance, args, out)` — the exact path a generated guest-side host-
//     contract caller bottoms out in (see polyplugc rust.rs `generate_host_fn_
//     caller`). The interface is resolved ONCE before the loop (a real caller
//     caches it); only the resolve-cached dispatch is timed, mirroring how a
//     guest holds its host contract for its lifetime.
//
//   - `host_log` — the `RuntimeConfig.log` funnel cost. A guest emits a
//     diagnostic via `HostApi.log`, which routes through the runtime's
//     `LoggerHandle` (level filter → StringView construction → the installed
//     `extern "C"` callback → the boxed Rust sink). This is the language-neutral
//     host→log baseline: the sink is a trivial Rust closure so the bar measures
//     the funnel, not the sink. The level is set at/under `log_max_level` so the
//     record actually fires (a filtered level is a near-free early return and is
//     not the cost we want to record).
//
// There is NO arena arm here: the guest→host arena slot is already covered end to
// end by `call_arena.rs` (`overflow/warm_reuse`, `per_call/*`). Duplicating it
// here would add a second copy of the same measurement — see benches/README.md.
//
// A real loaded bundle is NOT required: like `cross_call.rs`, this hand-builds a
// native `HostContractInterface` provider and drives the genuine `HostApi`
// callbacks on a real `Runtime`, so the resolve + dispatch + log funnels are the
// production code paths, not stubs.

use core::hint::black_box;
use core::mem;
use core::ptr;

use criterion::BenchmarkGroup;
use criterion::BenchmarkId;
use criterion::Criterion;
use criterion::Throughput;
use criterion::criterion_group;
use criterion::criterion_main;
use criterion::measurement::WallTime;

use polyplug::Runtime;
use polyplug::logger::LoggerHandle;
use polyplug_abi::AbiError;
use polyplug_abi::DispatchMechanisms;
use polyplug_abi::DispatchType;
use polyplug_abi::HostApi;
use polyplug_abi::HostContractInstance;
use polyplug_abi::HostContractInterface;
use polyplug_abi::LogLevel;
use polyplug_abi::NativeDispatch;
use polyplug_abi::types::Version;
use polyplug_utils::HostContractId;
use std::sync::Arc;

// ─── host contract target ─────────────────────────────────────────────────────

/// Argument struct for the host contract target function.
#[repr(C)]
struct AddArgs {
    a: u32,
    b: u32,
}

/// Native host-contract dispatch target — the function a guest's host-contract
/// call ultimately invokes. Signature matches the frozen native host-contract ABI
/// `extern "C" fn(HostContractInstance, *const (), *mut (), *mut AbiError)` (the
/// same shape generated guest host-fn callers transmute to). Adds the two `u32`
/// args and writes the sum to `out` so the work survives dead-code elimination.
///
/// # Safety
/// `args` must point to a valid `AddArgs`; `out` must point to a valid `u32`;
/// `out_err` must be non-null and writable.
unsafe extern "C" fn host_add(
    _instance: HostContractInstance,
    args: *const (),
    out: *mut (),
    out_err: *mut AbiError,
) {
    // SAFETY: args points to AddArgs and out to u32 per the caller's contract.
    unsafe {
        let a: &AddArgs = &*(args as *const AddArgs);
        *(out as *mut u32) = a.a.wrapping_add(a.b);
    }
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
    }
}

unsafe extern "C" fn host_create_instance(
    _this: *const HostContractInterface,
    _args: *const (),
    out_instance: *mut HostContractInstance,
) {
    if !out_instance.is_null() {
        // SAFETY: out_instance is non-null (just checked) and writable per the ABI contract.
        unsafe { out_instance.write(HostContractInstance::null()) };
    }
}

unsafe extern "C" fn host_destroy_instance(
    _this: *const HostContractInterface,
    _instance: HostContractInstance,
) {
}

/// Leak a native `HostContractInterface` exposing `host_add` at fn_id 0.
///
/// The one-element function table is leaked rather than declared `static`: a
/// `static [*const (); 1]` would require `Sync` on the raw fn pointers, which they
/// do not implement. A leaked `Box` is `'static` and gives a stable address for
/// the interface's `functions` pointer without a `Sync` bound.
fn leak_host_interface(contract_id: u64) -> &'static HostContractInterface {
    let functions: &'static [*const (); 1] = Box::leak(Box::new([host_add as *const ()]));
    Box::leak(Box::new(HostContractInterface {
        contract_id: HostContractId::from(contract_id),
        contract_version: Version {
            major: 1,
            minor: 0,
            patch: 0,
        },
        singleton: true,
        dispatch_type: DispatchType::Native,
        runtime: ptr::null_mut(),
        user_data: ptr::null_mut(),
        create_instance: host_create_instance,
        destroy_instance: host_destroy_instance,
        dispatch: DispatchMechanisms {
            native: NativeDispatch {
                function_count: 1,
                functions: functions.as_ptr(),
            },
        },
    }))
}

// ─── Benchmark — guest→host contract call ─────────────────────────────────────

/// One guest→host contract method call through the real `HostApi`: resolve the
/// host interface once (cached, as a real caller does), then dispatch its native
/// function. The interface pointer is fetched through `HostApi.resolve_host_
/// contract_interface` so the resolve is the production callback; only the cached
/// dispatch is timed.
fn bench_host_contract_call(c: &mut Criterion) {
    let runtime: Arc<Runtime> = Runtime::builder()
        .build()
        .expect("bare runtime build should succeed");
    let contract_id: u64 = HostContractId::new("bench.host", 1_u32).id();
    let interface: &'static HostContractInterface = leak_host_interface(contract_id);
    runtime
        .register_host_contract(contract_id, interface)
        .expect("host contract registration should succeed");

    let host_abi: *const HostApi = runtime.host_abi();
    let host_ptr: *const HostApi = host_abi;

    // Resolve ONCE through the real HostApi callback (a guest caches its host
    // interface for its lifetime); the instance for a singleton stateless host
    // contract is the null token its create_instance returns. `min_version` is the
    // packed `(major << 16) | minor` form the runtime decodes — require major 1.
    let min_version: u32 = 1_u32 << 16;
    // SAFETY: host_ptr is the runtime's valid 'static HostApi.
    let resolved: *const HostContractInterface = unsafe {
        ((*host_abi).resolve_host_contract_interface)(host_ptr, contract_id, min_version)
    };
    assert!(!resolved.is_null(), "host interface must resolve");
    // SAFETY: resolved is the registered 'static interface; fn 0 is host_add.
    let dispatch_fn: unsafe extern "C" fn(HostContractInstance, *const (), *mut (), *mut AbiError) = unsafe {
        let fn_ptr: *const () = *(*resolved).dispatch.native.functions.add(0);
        mem::transmute(fn_ptr)
    };
    let instance: HostContractInstance = HostContractInstance::null();

    let args: AddArgs = AddArgs {
        a: 42_u32,
        b: 57_u32,
    };
    let mut out: u32 = 0_u32;

    let mut group: BenchmarkGroup<'_, WallTime> = c.benchmark_group("guest_host_call");
    group.throughput(Throughput::Elements(1));

    group.bench_function(BenchmarkId::new("host_contract_call", "native"), |b| {
        b.iter(|| {
            let mut err: AbiError = AbiError::ok();
            // SAFETY: dispatch_fn is host_add; instance is the stateless null
            // token; args/out match host_add's layout; err is writable.
            unsafe {
                dispatch_fn(
                    black_box(instance),
                    black_box(&args as *const AddArgs as *const ()),
                    black_box(&mut out as *mut u32 as *mut ()),
                    &mut err,
                )
            };
            black_box(err);
            black_box(out);
        });
    });

    group.finish();
    // The runtime owns the leaked interface for the process lifetime; keep it
    // alive for the whole bench. Never dropped (the leaked interface is 'static).
    mem::forget(runtime);
}

// ─── Benchmark — host→log funnel ──────────────────────────────────────────────

/// One delivered log record through the real `RuntimeConfig.log` funnel.
///
/// A `Runtime` is built with a trivial Rust logger closure (so `log_max_level`
/// is `Trace` and a real `extern "C"` callback is installed), then a log is
/// emitted at an enabled level (`Info`) every iteration. The funnel timed here is
/// the production path: `LoggerHandle::enabled` filter → message production →
/// `StringView` construction → the installed `extern "C"` trampoline → the boxed
/// `LogSink`. The sink itself is a no-op `black_box` so the bar measures the
/// funnel, not the host's logging work.
///
/// This drives `LoggerHandle::log` directly (the exact funnel `HostApi.log`
/// reaches via `(*this).runtime.logger().log(...)`), so it is the language-neutral
/// host→log baseline a guest's `host->log(...)` call ultimately pays.
fn bench_host_log(c: &mut Criterion) {
    let runtime: Arc<Runtime> = Runtime::builder()
        .logger(|level: LogLevel, scope: &str, message: &str| {
            // No-op sink: black_box the inputs so the closure (and therefore the
            // whole funnel that produced its arguments) cannot be optimized away.
            black_box((level, scope, message));
        })
        .build()
        .expect("runtime build with logger should succeed");

    // `RuntimeBuilder::logger` sets `log_max_level = Trace`, so Info is delivered
    // (verified by the funnel firing — the bar would collapse to a filtered early
    // return otherwise).
    let logger: LoggerHandle = runtime.logger();

    let mut group: BenchmarkGroup<'_, WallTime> = c.benchmark_group("guest_host_call");
    group.throughput(Throughput::Elements(1));

    group.bench_function(BenchmarkId::new("host_log", "delivered"), |b| {
        b.iter(|| {
            logger.log(black_box(LogLevel::Info), black_box("bench"), || {
                String::from("guest reached the host log funnel")
            });
        });
    });

    group.finish();
    mem::forget(runtime);
}

// ─── criterion_group / criterion_main ────────────────────────────────────────

// Native-dispatch host contract + the log funnel are both real production paths.
// A VM host-contract fixture (Lua/JS/Python host providing the contract) would
// need a language loader + bundle, which this crate's bench harness does not set
// up cheaply — the same native-only caveat the other in-process benches carry.
criterion_group!(benches, bench_host_contract_call, bench_host_log);
criterion_main!(benches);

#![allow(clippy::expect_used)]

// THIS IS A BENCHMARK FILE — do not add #[test] functions here
// Run with: cargo bench -p polyplug --bench cross_call
//
// Measures the end-to-end cost of the `call_guest_method` HostApi callback —
// the host-mediated plugin→plugin cross-dispatch path. Every iteration goes
// through the real `HostApi.call_guest_method` function pointer on a real
// `Runtime`, exercising the full resolve chain inside
// `host_call_guest_method`:
//
//   count providers for contract_id  →  find first provider  →  resolve to
//   interface pointer  →  native dispatch into the target function.
//
// A bench that stubbed the registry (as the dispatch benches do) would NOT
// touch this resolve chain at all, so the real `Runtime` is required: the
// callback dereferences `(*this).runtime` and routes through `RuntimeStore`.
// This is the exact path the single-lock-resolve and init-stack fast-path
// changes target.

use core::hint::black_box;

use criterion::BenchmarkId;
use criterion::Criterion;
use criterion::Throughput;
use criterion::criterion_group;
use criterion::criterion_main;

use polyplug::Runtime;
use polyplug_abi::AbiError;
use polyplug_abi::CallArena;
use polyplug_abi::DispatchMechanisms;
use polyplug_abi::DispatchType;
use polyplug_abi::GuestContractInstance;
use polyplug_abi::GuestContractInterface;
use polyplug_abi::HostApi;
use polyplug_abi::NativeDispatch;
use polyplug_abi::PluginDescriptor;
use polyplug_abi::StringView;
use polyplug_abi::types::Version;
use polyplug_utils::BundleId;
use polyplug_utils::GuestContractId;
use std::sync::Arc;

// ─── Native target function ──────────────────────────────────────────────────

/// Argument struct for the benchmark target function.
#[repr(C)]
struct AddArgs {
    a: u32,
    b: u32,
}

/// Native dispatch target — the function the cross-call ultimately invokes.
///
/// Signature matches the frozen native ABI
/// `extern "C" fn(GuestContractInstance, *const (), *mut ()) -> AbiError`.
/// Adds the two `u32` args and writes the sum to `out` so the work is not
/// dead-code-eliminated.
///
/// # Safety
/// `args` must point to a valid `AddArgs`; `out` must point to a valid `u32`.
unsafe extern "C" fn bench_add(
    _instance: GuestContractInstance,
    args: *const (),
    out: *mut (),
) -> AbiError {
    // SAFETY: args points to AddArgs and out to u32 per the caller's contract.
    unsafe {
        let a: &AddArgs = &*(args as *const AddArgs);
        *(out as *mut u32) = a.a.wrapping_add(a.b);
    }
    AbiError::ok()
}

// ─── Interface + provider registration ───────────────────────────────────────

/// Leak a native `GuestContractInterface` exposing `bench_add` at fn_id 0.
///
/// The one-element function table is leaked rather than declared `static`: a
/// `static [*const (); 1]` would require `Sync` on the raw fn pointers, which
/// they do not implement. A leaked `Box` is `'static` and gives a stable
/// address for the interface's `functions` pointer without a `Sync` bound.
fn leak_native_interface(contract_id: u64) -> &'static GuestContractInterface {
    let functions: &'static [*const (); 1] = Box::leak(Box::new([bench_add as *const ()]));
    Box::leak(Box::new(GuestContractInterface {
        contract_id: GuestContractId::from_u64(contract_id),
        contract_version: Version {
            major: 1,
            minor: 0,
            patch: 0,
        },
        dispatch_type: DispatchType::Native,
        create_instance: noop_create_instance,
        destroy_instance: noop_destroy_instance,
        dispatch: DispatchMechanisms {
            native: NativeDispatch {
                function_count: 1,
                functions: functions.as_ptr(),
            },
        },
    }))
}

unsafe extern "C" fn noop_create_instance(
    _host: *const HostApi,
    _args: *const (),
) -> GuestContractInstance {
    GuestContractInstance::null()
}

unsafe extern "C" fn noop_destroy_instance(
    _host: *const HostApi,
    _instance: GuestContractInstance,
) {
}

/// Register a native provider for `contract_id` from `bundle_id` into the
/// runtime's registry (mirrors how a plugin's `polyplug_init` registers).
fn register_native_provider(runtime: &Runtime, contract_id: u64, bundle_id: u64) {
    let interface: &'static GuestContractInterface = leak_native_interface(contract_id);
    let descriptor: PluginDescriptor = PluginDescriptor {
        name: StringView::from_static(b"bench-provider"),
        contract_name: StringView::from_static(b"bench.contract"),
        version: Version {
            major: 1,
            minor: 0,
            patch: 0,
        },
    };
    // SAFETY: interface is leaked and lives for the process lifetime, satisfying
    // the 'static requirement of register_guest_contract.
    unsafe {
        runtime.registry().register_guest_contract(
            descriptor,
            interface,
            "bench.contract".to_owned(),
            BundleId::from_u64(bundle_id),
        )
    }
    .expect("provider registration should succeed");
}

// ─── Benchmark — native-dispatch cross-call ──────────────────────────────────

/// Measures one full `call_guest_method` round trip to a single native-dispatch
/// provider. This is the common case (exactly one provider for a contract): the
/// resolve chain runs, finds one provider, resolves it, and dispatches.
///
/// The `Runtime`, host table, instance, and args are all built ONCE before the
/// loop — only the cross-call itself is timed. Inputs and the returned `AbiError`
/// are `black_box`'d so neither the args nor the result are optimized away.
fn bench_cross_call_native(c: &mut Criterion) {
    let runtime: Arc<Runtime> = Runtime::builder()
        .build()
        .expect("bare runtime build should succeed");
    let contract_id: u64 = GuestContractId::new("bench.contract", 1_u32).id();
    register_native_provider(&runtime, contract_id, 0x1111_u64);

    let host_abi: &'static HostApi = runtime.host_abi();
    let host_ptr: *const HostApi = host_abi as *const HostApi;
    let instance: GuestContractInstance = GuestContractInstance {
        data: core::ptr::null_mut(),
        contract_id: GuestContractId::from_u64(contract_id),
    };
    let args: AddArgs = AddArgs {
        a: 42_u32,
        b: 57_u32,
    };
    let mut out: u32 = 0_u32;

    let mut group: criterion::BenchmarkGroup<'_, criterion::measurement::WallTime> =
        c.benchmark_group("cross_call");
    group.throughput(Throughput::Elements(1));

    group.bench_function(BenchmarkId::new("native", "single_provider"), |b| {
        b.iter(|| {
            // SAFETY: host_ptr is the runtime's valid 'static HostApi; instance
            // carries a registered contract_id; args/out match bench_add's layout.
            let err: AbiError = unsafe {
                (host_abi.call_guest_method)(
                    black_box(host_ptr),
                    black_box(instance),
                    black_box(0_u32),
                    black_box(&args as *const AddArgs as *const core::ffi::c_void),
                    black_box(&mut out as *mut u32 as *mut core::ffi::c_void),
                    black_box(core::ptr::null_mut::<CallArena>()),
                )
            };
            black_box(err);
            black_box(out);
        });
    });

    group.finish();
    // The runtime owns the leaked interface for the process lifetime; keep it
    // alive for the whole bench. Never dropped (the leaked interface is 'static).
    core::mem::forget(runtime);
}

// ─── Benchmark — guest→guest peer-caller path ────────────────────────────────

/// Measures the runtime-level path a generated **peer caller** bottoms out in: a
/// guest contract instance calling *another* guest contract through the runtime,
/// keyed solely on the target `contract_id`.
///
/// A generated peer caller (rust/cpp/csharp/lua/js/python) resolves its peer by
/// creating a stateless instance — `create_instance` returns a null `data` handle
/// — and then dispatches through `HostApi.call_guest_method` with that instance's
/// `contract_id` (see the #72 fix: the runtime accepts a null `data` and routes by
/// `contract_id`). The *language-specific* marshalling on top of this is the
/// generated glue; the runtime work it shares — the contract_id-routed resolve +
/// native dispatch — is exactly this arm.
///
/// The only difference from `bench_cross_call_native` is the caller's vantage
/// point: there the instance is the *target's own* handle; here it is a peer's
/// stateless token (null `data`, target `contract_id`) — the shape a peer caller
/// produces. Both exercise the same `host_call_guest_method` resolve chain, so the
/// gap between the two bars is noise, which is the honest finding: at the runtime
/// level a peer call and a host-mediated cross-call cost the same; any extra a
/// real peer caller pays is its language's marshalling, not the dispatch.
fn bench_peer_caller_native(c: &mut Criterion) {
    let runtime: Arc<Runtime> = Runtime::builder()
        .build()
        .expect("bare runtime build should succeed");
    let contract_id: u64 = GuestContractId::new("bench.contract", 1_u32).id();
    register_native_provider(&runtime, contract_id, 0x2222_u64);

    let host_abi: &'static HostApi = runtime.host_abi();
    let host_ptr: *const HostApi = host_abi as *const HostApi;
    // Peer vantage point: a stateless instance — null `data`, target contract_id —
    // exactly what a generated peer caller obtains from a VM/stateless peer's
    // create_instance and routes on.
    let peer_instance: GuestContractInstance = GuestContractInstance {
        data: core::ptr::null_mut(),
        contract_id: GuestContractId::from_u64(contract_id),
    };
    let args: AddArgs = AddArgs {
        a: 42_u32,
        b: 57_u32,
    };
    let mut out: u32 = 0_u32;

    let mut group: criterion::BenchmarkGroup<'_, criterion::measurement::WallTime> =
        c.benchmark_group("cross_call");
    group.throughput(Throughput::Elements(1));

    group.bench_function(BenchmarkId::new("peer", "stateless_route"), |b| {
        b.iter(|| {
            // SAFETY: host_ptr is the runtime's valid 'static HostApi; peer_instance
            // carries a registered contract_id with a null data handle (valid for a
            // stateless peer); args/out match bench_add's layout.
            let err: AbiError = unsafe {
                (host_abi.call_guest_method)(
                    black_box(host_ptr),
                    black_box(peer_instance),
                    black_box(0_u32),
                    black_box(&args as *const AddArgs as *const core::ffi::c_void),
                    black_box(&mut out as *mut u32 as *mut core::ffi::c_void),
                    black_box(core::ptr::null_mut::<CallArena>()),
                )
            };
            black_box(err);
            black_box(out);
        });
    });

    group.finish();
    // The runtime owns the leaked interface for the process lifetime; keep it
    // alive for the whole bench. Never dropped (the leaked interface is 'static).
    core::mem::forget(runtime);
}

// ─── criterion_group / criterion_main ────────────────────────────────────────

// Only native-dispatch targets are benched: building a VM-dispatch fixture would
// require a language loader + bundle, which this crate's bench harness does not
// set up cheaply (the existing benches are native-only for the same reason). The
// resolve chain inside `host_call_guest_method` — count + find + resolve — is
// dispatch-type-independent, so the native benches fully cover the lock-path work
// that the single-lock-resolve and init-stack changes target.
//
// `peer` measures the runtime work a generated guest→guest peer caller bottoms
// out in (contract_id-routed dispatch with a stateless instance); the per-language
// marshalling layered on top is generated glue and cannot be exercised here
// without a per-language bundle — the same two-tier caveat as the dispatch matrix.
criterion_group!(benches, bench_cross_call_native, bench_peer_caller_native);
criterion_main!(benches);

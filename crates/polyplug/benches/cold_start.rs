#![allow(clippy::expect_used)]

// THIS IS A BENCHMARK FILE — do not add #[test] functions here
// Run with: cargo bench -p polyplug --bench cold_start
//
// ─── What this measures ──────────────────────────────────────────────────────
//
// The *first* dispatch into a just-registered contract (everything cache-cold:
// the registry slot was just inserted, the interface pointer has never been
// chased, the dispatch path has never run on this data) versus the *warm*
// steady-state dispatch a long-lived host pays once it has resolved the handle
// and called it many times.
//
//   - `cold/first_dispatch` — a fresh `Runtime` with a single freshly-registered
//     native provider is built in untimed `iter_batched` setup; the TIMED body
//     does the first ever `find_guest_contract` + `resolve_guest_contract` +
//     native dispatch on that runtime. Each measurement is a genuine first-touch:
//     a cold registry HashMap probe, a cold interface-pointer chase, and a
//     cold-icache run of the dispatch code.
//   - `warm/find_resolve_dispatch` — the SAME find + resolve + dispatch, but on a
//     single long-lived runtime hammered in a tight loop, so every line of the
//     path is hot in cache. The steady-state cost when the host re-resolves.
//   - `warm/cached_dispatch` — the documented cache-the-handle hot path: resolve
//     ONCE before the loop, then dispatch straight through the cached interface
//     pointer with no registry lookup at all (what `counter_inc/polyplug` does).
//
// HOW TO READ IT. The gap `cold/first_dispatch` − `warm/find_resolve_dispatch`
// is the cold-cache tax a host pays roughly once per contract on its very first
// call; `warm/cached_dispatch` shows where steady state lands once the host
// follows the resolve-once pattern. The argument is a trivial `add(42, 57)`, so
// the bars isolate the dispatch path, not any per-byte work.
//
// The provider is a synthetic in-process native interface registered directly
// into the runtime's `RuntimeStore` — the SAME shape `contention.rs` uses, with
// the canonical 3-arg native dispatch ABI
// (`extern "C" fn(GuestContractInstance, *const (), *mut ()) -> AbiError`) the
// runtime expects of generated plugins. It needs no on-disk bundle or loader, so
// the only thing that varies between the cold and warm arms is cache warmth.

use core::hint::black_box;
use std::sync::Arc;

use criterion::BatchSize;
use criterion::BenchmarkId;
use criterion::Criterion;
use criterion::Throughput;
use criterion::criterion_group;
use criterion::criterion_main;

use polyplug::Runtime;
use polyplug_abi::AbiError;
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

/// Argument struct for the benchmark target function.
#[repr(C)]
struct AddArgs {
    a: u32,
    b: u32,
}

/// Native dispatch target — adds two `u32` args, writes the sum to `out` so the
/// work is not dead-code-eliminated. Canonical 3-arg native ABI.
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

/// Leak a native `GuestContractInterface` exposing `bench_add` at fn_id 0.
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

/// Build a bare runtime and register one synthetic native provider into it.
fn runtime_with_provider(contract_id: u64) -> Arc<Runtime> {
    let runtime: Arc<Runtime> = Runtime::builder()
        .build()
        .expect("bare runtime build should succeed");
    let interface: &'static GuestContractInterface = leak_native_interface(contract_id);
    let descriptor: PluginDescriptor = PluginDescriptor {
        name: StringView::from_static(b"cold-start-provider"),
        contract_name: StringView::from_static(b"coldstart.contract"),
        version: Version {
            major: 1,
            minor: 0,
            patch: 0,
        },
    };
    // SAFETY: interface is leaked ('static), valid for the runtime lifetime.
    unsafe {
        runtime.registry().register_guest_contract(
            descriptor,
            interface,
            "coldstart.contract".to_owned(),
            BundleId::from_u64(0x3333_u64),
        )
    }
    .expect("provider registration should succeed");
    runtime
}

/// One find + resolve + native dispatch of fn 0 on `runtime`. Returns the
/// dispatched sum so the work survives dead-code elimination.
fn find_resolve_dispatch(runtime: &Runtime, contract_id: u64) -> u32 {
    let handle = runtime
        .find_guest_contract(contract_id, 0)
        .expect("find_guest_contract must succeed");
    let interface_ptr: *const GuestContractInterface = runtime
        .resolve_guest_contract(handle)
        .expect("resolve_guest_contract must succeed");
    // SAFETY: interface_ptr is a valid 'static pointer returned by resolve.
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };
    dispatch_fn0(interface)
}

/// Native dispatch of fn 0 (`bench_add`) through an already-resolved interface.
fn dispatch_fn0(interface: &GuestContractInterface) -> u32 {
    // SAFETY: the interface is native with fn 0 present (registered above).
    let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions };
    // SAFETY: transmute to the canonical 3-arg native dispatch signature.
    let dispatch_fn: unsafe extern "C" fn(GuestContractInstance, *const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    let instance: GuestContractInstance = GuestContractInstance {
        data: core::ptr::null_mut(),
        contract_id: GuestContractId::from_u64(interface.contract_id.id()),
    };
    let args: AddArgs = AddArgs {
        a: 42_u32,
        b: 57_u32,
    };
    let mut out: u32 = 0_u32;
    // SAFETY: instance carries the contract_id; args/out match bench_add.
    let err: AbiError = unsafe {
        dispatch_fn(
            instance,
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };
    black_box(err);
    out
}

fn bench_cold_start(c: &mut Criterion) {
    let contract_id: u64 = GuestContractId::new("coldstart.contract", 1).id();

    let mut group: criterion::BenchmarkGroup<'_, criterion::measurement::WallTime> =
        c.benchmark_group("cold_start");
    group.throughput(Throughput::Elements(1));

    // ── cold: first dispatch into a just-registered contract ──────────────────
    //
    // A fresh runtime with the provider registered is built in untimed setup; the
    // timed body is the FIRST find + resolve + dispatch on it — every line of the
    // path cold in cache. The runtime is returned so criterion drops it untimed.
    group.bench_function(BenchmarkId::new("cold", "first_dispatch"), |b| {
        b.iter_batched(
            || runtime_with_provider(contract_id),
            |runtime: Arc<Runtime>| {
                let sum: u32 = find_resolve_dispatch(&runtime, black_box(contract_id));
                black_box(sum);
                runtime
            },
            BatchSize::SmallInput,
        );
    });

    // ── warm: same find + resolve + dispatch, hot in cache ────────────────────
    let warm_runtime: Arc<Runtime> = runtime_with_provider(contract_id);

    group.bench_function(BenchmarkId::new("warm", "find_resolve_dispatch"), |b| {
        b.iter(|| {
            let sum: u32 = find_resolve_dispatch(&warm_runtime, black_box(contract_id));
            black_box(sum);
        });
    });

    // ── warm: cached interface pointer (resolve once, dispatch many) ──────────
    let cached_handle = warm_runtime
        .find_guest_contract(contract_id, 0)
        .expect("find for cached bench");
    let cached_iface_ptr: *const GuestContractInterface = warm_runtime
        .resolve_guest_contract(cached_handle)
        .expect("resolve for cached bench");
    // SAFETY: cached_iface_ptr is a valid 'static interface pointer.
    let cached_iface: &GuestContractInterface = unsafe { &*cached_iface_ptr };

    group.bench_function(BenchmarkId::new("warm", "cached_dispatch"), |b| {
        b.iter(|| {
            let sum: u32 = dispatch_fn0(black_box(cached_iface));
            black_box(sum);
        });
    });

    group.finish();
    // Keep the warm runtime alive for the whole bench (its leaked interface backs
    // the cached pointer). Never dropped.
    core::mem::forget(warm_runtime);
}

criterion_group!(benches, bench_cold_start);
criterion_main!(benches);

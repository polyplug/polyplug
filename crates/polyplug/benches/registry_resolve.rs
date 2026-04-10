#![allow(clippy::expect_used)]

// THIS IS A BENCHMARK FILE — do not add #[test] functions here
// Run with: cargo bench -p polyplug --bench registry_resolve
//
// Benchmark: Registry::resolve hot path
// Measures: Time for handle validation + interface pointer return

use core::hint::black_box;

use criterion::BenchmarkId;
use criterion::Criterion;
use criterion::Throughput;
use criterion::criterion_group;
use criterion::criterion_main;

use polyplug::registry::RuntimeStore;
use polyplug_abi::DispatchType;
use polyplug_abi::GuestContractInterface;
use polyplug_abi::GuestContractInstance;
use polyplug_abi::HostInterface;
use polyplug_abi::NativeDispatch;
use polyplug_abi::PluginDescriptor;
use polyplug_abi::DispatchMechanisms;
use polyplug_abi::GuestContractHandle;
use polyplug_abi::StringView;
use polyplug_utils::GuestContractId;
use polyplug_utils::BundleId;

// ─── Instance lifecycle stubs for benchmarks ────────────────────────────────────

/// Stub create_instance for benchmarks - returns null instance.
unsafe extern "C" fn bench_create_instance(
    _host: *const HostInterface,
    _args: *const (),
) -> GuestContractInstance {
    GuestContractInstance::null()
}

/// Stub destroy_instance for benchmarks - no cleanup needed.
unsafe extern "C" fn bench_destroy_instance(
    _host: *const HostInterface,
    _instance: GuestContractInstance,
) {
}

// ─── Mock interface for benchmarking ────────────────────────────────────────────────

static BENCH_INTERFACE: GuestContractInterface = GuestContractInterface {
    contract_id: GuestContractId::from_u64(0x0000_0000_0000_0001_u64),
    contract_version: polyplug_abi::Version { major: 1, minor: 0, patch: 0 },
    dispatch_type: DispatchType::Native,
    create_instance: bench_create_instance,
    destroy_instance: bench_destroy_instance,
    dispatch: DispatchMechanisms {
        native: NativeDispatch {
            function_count: 0,
            functions: core::ptr::null(),
        },
    },
};

fn make_descriptor(name: &'static str, contract_name: &'static str) -> PluginDescriptor {
    PluginDescriptor {
        name: StringView::from_static(name.as_bytes()),
        contract_name: StringView::from_static(contract_name.as_bytes()),
        version: polyplug_abi::Version { major: 1, minor: 0, patch: 0 },
    }
}

/// Create an interface for dynamic benchmarks
fn make_interface(id: u64) -> GuestContractInterface {
    GuestContractInterface {
        contract_id: GuestContractId::from_u64(id),
        contract_version: polyplug_abi::Version { major: 1, minor: 0, patch: 0 },
        dispatch_type: DispatchType::Native,
        create_instance: bench_create_instance,
        destroy_instance: bench_destroy_instance,
        dispatch: DispatchMechanisms {
            native: NativeDispatch {
                function_count: 0,
                functions: core::ptr::null(),
            },
        },
    }
}

// ─── Benchmark: resolve with single slot ─────────────────────────────────────

fn bench_registry_resolve_single(c: &mut Criterion) {
    let registry: RuntimeStore = RuntimeStore::new();
    let descriptor: PluginDescriptor = make_descriptor("bench_plugin", "bench.contract");

    // SAFETY: BENCH_INTERFACE is 'static, pointer is valid for Registry lifetime.
    let handle: GuestContractHandle = unsafe {
        registry
            .register_guest_contract(descriptor, &BENCH_INTERFACE, "bench.contract".to_owned(), BundleId::from_u64(0u64))
            .expect("registration should succeed")
    };

    let mut group: criterion::BenchmarkGroup<'_, criterion::measurement::WallTime> =
        c.benchmark_group("registry");
    group.throughput(Throughput::Elements(1));

    group.bench_function(BenchmarkId::new("resolve", "single_slot"), |b| {
        b.iter(|| {
            let result: Result<*const GuestContractInterface, _> =
                registry.resolve_guest_contract(black_box(handle));
            let _ = black_box(result);
        });
    });

    group.finish();
}

// ─── Benchmark: resolve with multiple slots ──────────────────────────────────

fn bench_registry_resolve_multiple_slots(c: &mut Criterion) {
    let registry: RuntimeStore = RuntimeStore::new();

    // Use leaked Box to get 'static interfaces
    let interfaces: Vec<Box<GuestContractInterface>> = (0..100_u64)
        .map(|i| Box::new(make_interface(0x1000_0000_0000_0000_u64 + i)))
        .collect();

    // Leak the interfaces to make them 'static
    let interface_refs: Vec<&'static GuestContractInterface> =
        interfaces.into_iter().map(|b| &*Box::leak(b)).collect();

    for (i, interface) in interface_refs.iter().enumerate() {
        let i_u64: u64 = i as u64;
        let descriptor: PluginDescriptor = PluginDescriptor {
            name: StringView::from_static(b"plugin"),
            contract_name: StringView::from_static(b"contract"),
            version: polyplug_abi::Version { major: 1, minor: 0, patch: 0 },
        };

        // SAFETY: interface is 'static (leaked), pointer is valid for Registry lifetime.
        unsafe {
            registry
                .register_guest_contract(descriptor, *interface, format!("contract.{}", i_u64), BundleId::from_u64(i_u64))
                .expect("registration should succeed");
        }
    }

    // Get handle for slot 50 (middle of the array)
    let handle: GuestContractHandle = registry
        .find(GuestContractId::from_u64(0x1000_0000_0000_0032_u64), 0)
        .expect("find should succeed");

    let mut group: criterion::BenchmarkGroup<'_, criterion::measurement::WallTime> =
        c.benchmark_group("registry");
    group.throughput(Throughput::Elements(1));

    group.bench_function(BenchmarkId::new("resolve", "100_slots"), |b| {
        b.iter(|| {
            let result: Result<*const GuestContractInterface, _> =
                registry.resolve_guest_contract(black_box(handle));
            let _ = black_box(result);
        });
    });

    group.finish();
}

// ─── Benchmark: resolve with stale handle ────────────────────────────────────

fn bench_registry_resolve_stale(c: &mut Criterion) {
    let registry: RuntimeStore = RuntimeStore::new();
    let descriptor: PluginDescriptor = make_descriptor("bench_plugin", "bench.contract");

    // SAFETY: BENCH_INTERFACE is 'static, pointer is valid for Registry lifetime.
    let _handle: GuestContractHandle = unsafe {
        registry
            .register_guest_contract(descriptor, &BENCH_INTERFACE, "bench.contract".to_owned(), BundleId::from_u64(0u64))
            .expect("registration should succeed")
    };

    // Create a stale handle - use an index that doesn't exist
    let stale_handle: GuestContractHandle = GuestContractHandle { index: u32::MAX - 1 };

    let mut group: criterion::BenchmarkGroup<'_, criterion::measurement::WallTime> =
        c.benchmark_group("registry");
    group.throughput(Throughput::Elements(1));

    group.bench_function(BenchmarkId::new("resolve", "stale_handle"), |b| {
        b.iter(|| {
            let result: Result<*const GuestContractInterface, _> =
                registry.resolve_guest_contract(black_box(stale_handle));
            let _ = black_box(result);
        });
    });

    group.finish();
}

// ─── criterion_group / criterion_main ────────────────────────────────────────

criterion_group!(
    benches,
    bench_registry_resolve_single,
    bench_registry_resolve_multiple_slots,
    bench_registry_resolve_stale,
);
criterion_main!(benches);
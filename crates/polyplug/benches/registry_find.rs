#![allow(clippy::expect_used)]

// THIS IS A BENCHMARK FILE — do not add #[test] functions here
// Run with: cargo bench -p polyplug --bench registry_find
//
// Benchmark: Registry::find_by_contract hot path
// Measures: Time for contract lookup with various slot counts

use core::hint::black_box;

use criterion::BenchmarkId;
use criterion::Criterion;
use criterion::Throughput;
use criterion::criterion_group;
use criterion::criterion_main;

use polyplug::plugin_registry::PluginRegistry;
use polyplug_abi::DispatchType;
use polyplug_abi::GuestContractInterface;
use polyplug_abi::HostInterface;
use polyplug_abi::NativeDispatch;
use polyplug_abi::PluginDescriptor;
use polyplug_abi::DispatchMechanisms;
use polyplug_abi::PluginHandle;
use polyplug_abi::GuestContractInstance;
use polyplug_abi::StringView;
use polyplug_utils::GuestContractId;
use polyplug_utils::BundleId;

// ─── Mock vtable for benchmarking ────────────────────────────────────────────

static BENCH_VTABLE: GuestContractInterface = GuestContractInterface {
    contract_id: GuestContractId::from_raw(0x0000_0000_0000_0001_u64),
    contract_version: polyplug_abi::Version { major: 1, minor: 0, patch: 0 },
    dispatch_type: DispatchType::Native,
    create_instance: |_| GuestContractInstance::null(),
    destroy_instance: |_, _| {},
    dispatch: DispatchMechanisms {
        native: NativeDispatch {
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

// ─── Benchmark: find_by_contract with single slot ────────────────────────────

fn bench_registry_find_by_contract_single(c: &mut Criterion) {
    let registry: PluginRegistry = PluginRegistry::new();
    let descriptor: PluginDescriptor = make_descriptor("bench_plugin", "bench.contract");

    // SAFETY: BENCH_VTABLE is 'static, pointer is valid for Registry lifetime.
    let _handle: PluginHandle = unsafe {
        registry
            .register(descriptor, &BENCH_VTABLE, "bench.contract".to_owned(), BundleId::from_u64(0u64))
            .expect("registration should succeed")
    };

    let contract_id: u64 = BENCH_VTABLE.contract_id.id();

    let mut group: criterion::BenchmarkGroup<'_, criterion::measurement::WallTime> =
        c.benchmark_group("registry");
    group.throughput(Throughput::Elements(1));

    group.bench_function(BenchmarkId::new("find_by_contract", "single_slot"), |b| {
        b.iter(|| {
            let result: Result<PluginHandle, _> =
                registry.find(black_box(GuestContractId::from_raw(contract_id)), black_box(0u32));
            let _ = black_box(result);
        });
    });

    group.finish();
}

// ─── Benchmark: find_by_contract with multiple slots (same contract) ─────────

fn bench_registry_find_by_contract_multi_impl(c: &mut Criterion) {
    let registry: PluginRegistry = PluginRegistry::new();

    // Use leaked Box to get 'static vtables
    let vtables: Vec<Box<GuestContractInterface>> = (0..10_usize)
        .map(|i| {
            Box::new(GuestContractInterface {
                contract_id: GuestContractId::from_raw(0xAAAA_BBBB_CCCC_DDDD_u64),
                contract_version: polyplug_abi::Version { major: 1, minor: 0, patch: 0 },
                dispatch_type: DispatchType::Native,
                create_instance: |_| GuestContractInstance::null(),
                destroy_instance: |_, _| {},
                dispatch: DispatchMechanisms {
                    native: NativeDispatch {
                        functions: core::ptr::null(),
                    },
                },
            })
        })
        .collect();

    let vtable_refs: Vec<&'static GuestContractInterface> =
        vtables.into_iter().map(|b| &*Box::leak(b)).collect();

    for (i, vtable) in vtable_refs.iter().enumerate() {
        let descriptor: PluginDescriptor = PluginDescriptor {
            name: StringView::from_static(b"multi_plugin"),
            contract_name: StringView::from_static(b"multi.contract"),
            version: polyplug_abi::Version { major: 1, minor: 0, patch: 0 },
        };

        // SAFETY: vtable is 'static (leaked), pointer is valid for Registry lifetime.
        unsafe {
            registry
                .register(descriptor, *vtable, "multi.contract".to_owned(), BundleId::from_u64(i as u64))
                .expect("registration should succeed");
        }
    }

    let contract_id: u64 = 0xAAAA_BBBB_CCCC_DDDD_u64;

    let mut group: criterion::BenchmarkGroup<'_, criterion::measurement::WallTime> =
        c.benchmark_group("registry");
    group.throughput(Throughput::Elements(1));

    group.bench_function(
        BenchmarkId::new("find_by_contract", "10_impls_same_contract"),
        |b| {
            b.iter(|| {
                let result: Result<PluginHandle, _> =
                    registry.find(black_box(GuestContractId::from_raw(contract_id)), black_box(0u32));
                let _ = black_box(result);
            });
        },
    );

    group.finish();
}

// ─── Benchmark: find_by_contract with many different contracts ──────────────

fn bench_registry_find_by_contract_many_contracts(c: &mut Criterion) {
    let registry: PluginRegistry = PluginRegistry::new();

    // Use leaked Box to get 'static vtables
    let vtables: Vec<Box<GuestContractInterface>> = (0..100_u64)
        .map(|i| {
            Box::new(GuestContractInterface {
                contract_id: GuestContractId::from_raw(0x2000_0000_0000_0000_u64 + i),
                contract_version: polyplug_abi::Version { major: 1, minor: 0, patch: 0 },
                dispatch_type: DispatchType::Native,
                create_instance: |_| GuestContractInstance::null(),
                destroy_instance: |_, _| {},
                dispatch: DispatchMechanisms {
                    native: NativeDispatch {
                        functions: core::ptr::null(),
                    },
                },
            })
        })
        .collect();

    let vtable_refs: Vec<&'static GuestContractInterface> =
        vtables.into_iter().map(|b| &*Box::leak(b)).collect();

    for (i, vtable) in vtable_refs.iter().enumerate() {
        let i_u64: u64 = i as u64;
        let descriptor: PluginDescriptor = PluginDescriptor {
            name: StringView::from_static(b"plugin"),
            contract_name: StringView::from_static(b"contract"),
            version: polyplug_abi::Version { major: 1, minor: 0, patch: 0 },
        };

        // SAFETY: vtable is 'static (leaked), pointer is valid for Registry lifetime.
        unsafe {
            registry
                .register(descriptor, *vtable, format!("contract.{}", i_u64), BundleId::from_u64(i_u64))
                .expect("registration should succeed");
        }
    }

    // Look up contract at index 50 (middle of the HashMap)
    let target_contract_id: u64 = 0x2000_0000_0000_0032_u64;

    let mut group: criterion::BenchmarkGroup<'_, criterion::measurement::WallTime> =
        c.benchmark_group("registry");
    group.throughput(Throughput::Elements(1));

    group.bench_function(
        BenchmarkId::new("find_by_contract", "100_different_contracts"),
        |b| {
            b.iter(|| {
                let result: Result<PluginHandle, _> =
                    registry.find(black_box(GuestContractId::from_raw(target_contract_id)), black_box(0u32));
                let _ = black_box(result);
            });
        },
    );

    group.finish();
}

// ─── Benchmark: find_by_contract not found ───────────────────────────────────

fn bench_registry_find_by_contract_not_found(c: &mut Criterion) {
    let registry: PluginRegistry = PluginRegistry::new();
    let descriptor: PluginDescriptor = make_descriptor("bench_plugin", "bench.contract");

    // SAFETY: BENCH_VTABLE is 'static, pointer is valid for Registry lifetime.
    let _handle: PluginHandle = unsafe {
        registry
            .register(descriptor, &BENCH_VTABLE, "bench.contract".to_owned(), BundleId::from_u64(0u64))
            .expect("registration should succeed")
    };

    let nonexistent_contract_id: u64 = 0xDEAD_BEEF_CAFE_0000_u64;

    let mut group: criterion::BenchmarkGroup<'_, criterion::measurement::WallTime> =
        c.benchmark_group("registry");
    group.throughput(Throughput::Elements(1));

    group.bench_function(BenchmarkId::new("find_by_contract", "not_found"), |b| {
        b.iter(|| {
            let result: Result<PluginHandle, _> =
                registry.find(black_box(GuestContractId::from_raw(nonexistent_contract_id)), black_box(0u32));
            let _ = black_box(result);
        });
    });

    group.finish();
}

// ─── criterion_group / criterion_main ────────────────────────────────────────

criterion_group!(
    benches,
    bench_registry_find_by_contract_single,
    bench_registry_find_by_contract_multi_impl,
    bench_registry_find_by_contract_many_contracts,
    bench_registry_find_by_contract_not_found,
);
criterion_main!(benches);
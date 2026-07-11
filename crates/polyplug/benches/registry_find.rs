#![allow(clippy::expect_used)]

// THIS IS A BENCHMARK FILE — do not add #[test] functions here
// Run with: cargo bench -p polyplug --bench registry_find
//
// Benchmark: Registry::find_guest_contract hot path
// Measures: Time for contract lookup with various slot counts

use core::ffi::c_void;
use core::hint::black_box;
use core::ptr;

use criterion::BenchmarkGroup;
use criterion::BenchmarkId;
use criterion::Criterion;
use criterion::Throughput;
use criterion::criterion_group;
use criterion::criterion_main;
use criterion::measurement::WallTime;

use polyplug::runtime_store::RuntimeStore;
use polyplug_abi::DispatchMechanisms;
use polyplug_abi::DispatchType;
use polyplug_abi::GuestContractHandle;
use polyplug_abi::GuestContractInstance;
use polyplug_abi::GuestContractInterface;
use polyplug_abi::HostApi;
use polyplug_abi::NativeDispatch;
use polyplug_abi::PluginDescriptor;
use polyplug_abi::StringView;
use polyplug_abi::dispatch::VmLoaderData;
use polyplug_abi::types::Version;
use polyplug_utils::BundleId;
use polyplug_utils::GuestContractId;

// ─── Instance lifecycle stubs for benchmarks ────────────────────────────────────

/// Stub create_instance for benchmarks - writes null instance to out_instance.
unsafe extern "C" fn bench_create_instance(
    _adapter_context: *mut c_void,
    _loader_data: VmLoaderData,
    _host: *const HostApi,
    _args: *const (),
    out_instance: *mut GuestContractInstance,
) {
    if !out_instance.is_null() {
        // SAFETY: out_instance is non-null (just checked) and writable per the ABI contract.
        unsafe { out_instance.write(GuestContractInstance::null()) };
    }
}

/// Stub destroy_instance for benchmarks - no cleanup needed.
unsafe extern "C" fn bench_destroy_instance(
    _adapter_context: *mut c_void,
    _loader_data: VmLoaderData,
    _host: *const HostApi,
    _instance: GuestContractInstance,
) {
}

// ─── Mock interface for benchmarking ────────────────────────────────────────────────

static BENCH_INTERFACE: GuestContractInterface = GuestContractInterface {
    contract_id: GuestContractId::from_u64(0x0000_0000_0000_0001_u64),
    contract_version: Version {
        major: 1,
        minor: 0,
        patch: 0,
    },
    dispatch_type: DispatchType::Native,
    adapter_context: ptr::null_mut(),
    create_instance: bench_create_instance,
    destroy_instance: bench_destroy_instance,
    dispatch: DispatchMechanisms {
        native: NativeDispatch {
            function_count: 0,
            functions: ptr::null(),
        },
    },
};

fn make_descriptor(name: &'static str, contract_name: &'static str) -> PluginDescriptor {
    PluginDescriptor {
        name: StringView::from_static(name.as_bytes()),
        contract_name: StringView::from_static(contract_name.as_bytes()),
        version: Version {
            major: 1,
            minor: 0,
            patch: 0,
        },
    }
}

/// Create an interface for dynamic benchmarks
fn make_interface(id: u64) -> GuestContractInterface {
    GuestContractInterface {
        contract_id: GuestContractId::from_u64(id),
        contract_version: Version {
            major: 1,
            minor: 0,
            patch: 0,
        },
        dispatch_type: DispatchType::Native,
        adapter_context: ptr::null_mut(),
        create_instance: bench_create_instance,
        destroy_instance: bench_destroy_instance,
        dispatch: DispatchMechanisms {
            native: NativeDispatch {
                function_count: 0,
                functions: ptr::null(),
            },
        },
    }
}

// ─── Benchmark: find_guest_contract with single slot ────────────────────────────

fn bench_registry_find_by_contract_single(c: &mut Criterion) {
    let registry: RuntimeStore = RuntimeStore::new();
    let descriptor: PluginDescriptor = make_descriptor("bench_plugin", "bench.contract");

    // SAFETY: BENCH_INTERFACE is 'static, pointer is valid for Registry lifetime.
    let _handle: GuestContractHandle = unsafe {
        registry
            .register_guest_contract(
                descriptor,
                &BENCH_INTERFACE,
                "bench.contract".to_owned(),
                BundleId::from_u64(0u64),
            )
            .expect("registration should succeed")
    };

    let contract_id: u64 = BENCH_INTERFACE.contract_id.id();

    let mut group: BenchmarkGroup<'_, WallTime> = c.benchmark_group("registry");
    group.throughput(Throughput::Elements(1));

    group.bench_function(
        BenchmarkId::new("find_guest_contract", "single_slot"),
        |b| {
            b.iter(|| {
                let result: Result<GuestContractHandle, _> = registry.find(
                    black_box(GuestContractId::from_u64(contract_id)),
                    black_box(0u32),
                );
                let _ = black_box(result);
            });
        },
    );

    group.finish();
}

// ─── Benchmark: find_guest_contract with multiple slots (same contract) ─────────

fn bench_registry_find_by_contract_multi_impl(c: &mut Criterion) {
    let registry: RuntimeStore = RuntimeStore::new();

    // Use leaked Box to get 'static interfaces
    let interfaces: Vec<Box<GuestContractInterface>> = (0..10_usize)
        .map(|_| Box::new(make_interface(0xAAAA_BBBB_CCCC_DDDD_u64)))
        .collect();

    let interface_refs: Vec<&'static GuestContractInterface> =
        interfaces.into_iter().map(|b| &*Box::leak(b)).collect();

    for (i, interface) in interface_refs.iter().enumerate() {
        let descriptor: PluginDescriptor = PluginDescriptor {
            name: StringView::from_static(b"multi_plugin"),
            contract_name: StringView::from_static(b"multi.contract"),
            version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
        };

        // SAFETY: interface is 'static (leaked), pointer is valid for Registry lifetime.
        unsafe {
            registry
                .register_guest_contract(
                    descriptor,
                    *interface,
                    "multi.contract".to_owned(),
                    BundleId::from_u64(i as u64),
                )
                .expect("registration should succeed");
        }
    }

    let contract_id: u64 = 0xAAAA_BBBB_CCCC_DDDD_u64;

    let mut group: BenchmarkGroup<'_, WallTime> = c.benchmark_group("registry");
    group.throughput(Throughput::Elements(1));

    group.bench_function(
        BenchmarkId::new("find_guest_contract", "10_impls_same_contract"),
        |b| {
            b.iter(|| {
                let result: Result<GuestContractHandle, _> = registry.find(
                    black_box(GuestContractId::from_u64(contract_id)),
                    black_box(0u32),
                );
                let _ = black_box(result);
            });
        },
    );

    group.finish();
}

// ─── Benchmark: find_guest_contract with many different contracts ──────────────

fn bench_registry_find_by_contract_many_contracts(c: &mut Criterion) {
    let registry: RuntimeStore = RuntimeStore::new();

    // Use leaked Box to get 'static interfaces
    let interfaces: Vec<Box<GuestContractInterface>> = (0..100_u64)
        .map(|i| Box::new(make_interface(0x2000_0000_0000_0000_u64 + i)))
        .collect();

    let interface_refs: Vec<&'static GuestContractInterface> =
        interfaces.into_iter().map(|b| &*Box::leak(b)).collect();

    for (i, interface) in interface_refs.iter().enumerate() {
        let i_u64: u64 = i as u64;
        let descriptor: PluginDescriptor = PluginDescriptor {
            name: StringView::from_static(b"plugin"),
            contract_name: StringView::from_static(b"contract"),
            version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
        };

        // SAFETY: interface is 'static (leaked), pointer is valid for Registry lifetime.
        unsafe {
            registry
                .register_guest_contract(
                    descriptor,
                    *interface,
                    format!("contract.{}", i_u64),
                    BundleId::from_u64(i_u64),
                )
                .expect("registration should succeed");
        }
    }

    // Look up contract at index 50 (middle of the HashMap)
    let target_contract_id: u64 = 0x2000_0000_0000_0032_u64;

    let mut group: BenchmarkGroup<'_, WallTime> = c.benchmark_group("registry");
    group.throughput(Throughput::Elements(1));

    group.bench_function(
        BenchmarkId::new("find_guest_contract", "100_different_contracts"),
        |b| {
            b.iter(|| {
                let result: Result<GuestContractHandle, _> = registry.find(
                    black_box(GuestContractId::from_u64(target_contract_id)),
                    black_box(0u32),
                );
                let _ = black_box(result);
            });
        },
    );

    group.finish();
}

// ─── Benchmark: find_guest_contract not found ───────────────────────────────────

fn bench_registry_find_by_contract_not_found(c: &mut Criterion) {
    let registry: RuntimeStore = RuntimeStore::new();
    let descriptor: PluginDescriptor = make_descriptor("bench_plugin", "bench.contract");

    // SAFETY: BENCH_INTERFACE is 'static, pointer is valid for Registry lifetime.
    let _handle: GuestContractHandle = unsafe {
        registry
            .register_guest_contract(
                descriptor,
                &BENCH_INTERFACE,
                "bench.contract".to_owned(),
                BundleId::from_u64(0u64),
            )
            .expect("registration should succeed")
    };

    let nonexistent_contract_id: u64 = 0xDEAD_BEEF_CAFE_0000_u64;

    let mut group: BenchmarkGroup<'_, WallTime> = c.benchmark_group("registry");
    group.throughput(Throughput::Elements(1));

    group.bench_function(BenchmarkId::new("find_guest_contract", "not_found"), |b| {
        b.iter(|| {
            let result: Result<GuestContractHandle, _> = registry.find(
                black_box(GuestContractId::from_u64(nonexistent_contract_id)),
                black_box(0u32),
            );
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

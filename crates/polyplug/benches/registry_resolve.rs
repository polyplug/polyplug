#![allow(clippy::expect_used)]

// THIS IS A BENCHMARK FILE — do not add #[test] functions here
// Run with: cargo bench -p polyplug --bench registry_resolve
//
// Benchmark: Registry::resolve_guard hot path
// Measures: Time for handle validation + vtable guard creation

use core::hint::black_box;

use criterion::BenchmarkId;
use criterion::Criterion;
use criterion::Throughput;
use criterion::criterion_group;
use criterion::criterion_main;

use polyplug::registry::Registry;
use polyplug_abi::DispatchType;
use polyplug_abi::NativeDispatch;
use polyplug_abi::PluginDescriptor;
use polyplug_abi::PluginDispatch;
use polyplug_abi::PluginHandle;
use polyplug_abi::PluginInterface;
use polyplug_abi::StringView;

// ─── Mock vtable for benchmarking ────────────────────────────────────────────

static BENCH_VTABLE: PluginInterface = PluginInterface {
    rt_ctx: core::ptr::null(),
    contract_id: 0x0000_0000_0000_0001_u64,
    contract_version: (1 << 16),
    function_count: 0,
    dispatch_type: DispatchType::Native,
    dispatch: PluginDispatch {
        native: NativeDispatch {
            functions: core::ptr::null(),
        },
    },
};

fn make_descriptor(name: &'static str, contract_name: &'static str) -> PluginDescriptor {
    PluginDescriptor {
        name: StringView::from_static(name.as_bytes()),
        contract_name: StringView::from_static(contract_name.as_bytes()),
        version_major: 1,
        version_minor: 0,
        version_patch: 0,
    }
}

// ─── Benchmark: resolve_guard with single slot ───────────────────────────────

fn bench_registry_resolve_guard_single(c: &mut Criterion) {
    let registry: Registry = Registry::new();
    let descriptor: PluginDescriptor = make_descriptor("bench_plugin", "bench.contract");

    // SAFETY: BENCH_VTABLE is 'static, pointer is valid for Registry lifetime.
    let handle: PluginHandle = unsafe {
        registry
            .register(descriptor, &BENCH_VTABLE, "bench.contract".to_owned(), 0u64)
            .expect("registration should succeed")
    };

    let mut group: criterion::BenchmarkGroup<'_, criterion::measurement::WallTime> =
        c.benchmark_group("registry");
    group.throughput(Throughput::Elements(1));

    group.bench_function(BenchmarkId::new("resolve_guard", "single_slot"), |b| {
        b.iter(|| {
            let result: Result<polyplug::registry::PluginGuard, _> =
                registry.resolve_guard(black_box(handle));
            let _ = black_box(result);
        });
    });

    group.finish();
}

// ─── Benchmark: resolve_guard with multiple slots ─────────────────────────────

fn bench_registry_resolve_guard_multiple_slots(c: &mut Criterion) {
    let registry: Registry = Registry::new();

    // Use leaked Box to get 'static vtables
    let vtables: Vec<Box<PluginInterface>> = (0..100_u64)
        .map(|i| {
            Box::new(PluginInterface {
                rt_ctx: core::ptr::null(),
                contract_id: 0x1000_0000_0000_0000_u64 + i,
                contract_version: (1 << 16),
                function_count: 0,
                dispatch_type: DispatchType::Native,
                dispatch: PluginDispatch {
                    native: NativeDispatch {
                        functions: core::ptr::null(),
                    },
                },
            })
        })
        .collect();

    // Leak the vtables to make them 'static
    let vtable_refs: Vec<&'static PluginInterface> =
        vtables.into_iter().map(|b| &*Box::leak(b)).collect();

    for (i, vtable) in vtable_refs.iter().enumerate() {
        let i_u64: u64 = i as u64;
        let descriptor: PluginDescriptor = PluginDescriptor {
            name: StringView::from_static(b"plugin"),
            contract_name: StringView::from_static(b"contract"),
            version_major: 1,
            version_minor: 0,
            version_patch: 0,
        };

        // SAFETY: vtable is 'static (leaked), pointer is valid for Registry lifetime.
        unsafe {
            registry
                .register(descriptor, *vtable, format!("contract.{}", i_u64), i_u64)
                .expect("registration should succeed");
        }
    }

    // Get handle for slot 50 (middle of the array)
    let handle: PluginHandle = registry
        .find_by_contract(0x1000_0000_0000_0032_u64, 0)
        .expect("find should succeed");

    let mut group: criterion::BenchmarkGroup<'_, criterion::measurement::WallTime> =
        c.benchmark_group("registry");
    group.throughput(Throughput::Elements(1));

    group.bench_function(BenchmarkId::new("resolve_guard", "100_slots"), |b| {
        b.iter(|| {
            let result: Result<polyplug::registry::PluginGuard, _> =
                registry.resolve_guard(black_box(handle));
            let _ = black_box(result);
        });
    });

    group.finish();
}

// ─── Benchmark: resolve_guard with stale handle ──────────────────────────────

fn bench_registry_resolve_guard_stale(c: &mut Criterion) {
    let registry: Registry = Registry::new();
    let descriptor: PluginDescriptor = make_descriptor("bench_plugin", "bench.contract");

    // SAFETY: BENCH_VTABLE is 'static, pointer is valid for Registry lifetime.
    let handle: PluginHandle = unsafe {
        registry
            .register(descriptor, &BENCH_VTABLE, "bench.contract".to_owned(), 0u64)
            .expect("registration should succeed")
    };

    let stale_handle: PluginHandle = PluginHandle {
        index: handle.index,
        generation: handle.generation.wrapping_add(1),
    };

    let mut group: criterion::BenchmarkGroup<'_, criterion::measurement::WallTime> =
        c.benchmark_group("registry");
    group.throughput(Throughput::Elements(1));

    group.bench_function(BenchmarkId::new("resolve_guard", "stale_handle"), |b| {
        b.iter(|| {
            let result: Result<polyplug::registry::PluginGuard, _> =
                registry.resolve_guard(black_box(stale_handle));
            let _ = black_box(result);
        });
    });

    group.finish();
}

// ─── criterion_group / criterion_main ────────────────────────────────────────

criterion_group!(
    benches,
    bench_registry_resolve_guard_single,
    bench_registry_resolve_guard_multiple_slots,
    bench_registry_resolve_guard_stale,
);
criterion_main!(benches);

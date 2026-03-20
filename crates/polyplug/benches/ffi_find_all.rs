#![allow(clippy::expect_used)]

// THIS IS A BENCHMARK FILE — do not add #[test] functions here
// Run with: cargo bench -p polyplug --bench ffi_find_all
//
// Benchmark: polyplug_runtime_find_all_by_contract FFI path
// Measures: Time for various output buffer sizes (1, 10, 100)

use core::hint::black_box;

use criterion::BenchmarkId;
use criterion::Criterion;
use criterion::Throughput;
use criterion::criterion_group;
use criterion::criterion_main;

use polyplug::ffi::OpaqueRuntime;

// ─── Plugin paths from build.rs ──────────────────────────────────────────────

const TEST_PLUGIN_DIR: &str = env!("TEST_PLUGIN_DIR");

// ─── FFI function declarations ───────────────────────────────────────────────

#[allow(improper_ctypes)]
unsafe extern "C" {
    fn polyplug_runtime_create() -> *mut OpaqueRuntime;
    fn polyplug_runtime_destroy(rt: *mut OpaqueRuntime);
    fn polyplug_runtime_load_bundle(
        rt: *mut OpaqueRuntime,
        path: *const u8,
        path_len: usize,
    ) -> u32;
    fn polyplug_runtime_find_all_by_contract(
        rt: *const OpaqueRuntime,
        contract_id: u64,
        min_version: u32,
        out: *mut u64,
        out_cap: usize,
    ) -> usize;
}

// ─── Setup helper ────────────────────────────────────────────────────────────

fn setup_runtime_with_plugins() -> (*mut OpaqueRuntime, u64) {
    // SAFETY: polyplug_runtime_create has no pointer preconditions.
    let rt: *mut OpaqueRuntime = unsafe { polyplug_runtime_create() };
    assert!(!rt.is_null(), "polyplug_runtime_create returned null");

    let path_bytes: &[u8] = TEST_PLUGIN_DIR.as_bytes();
    // SAFETY: rt is non-null valid OpaqueRuntime; path_bytes is valid for path_len bytes.
    let load_result: u32 =
        unsafe { polyplug_runtime_load_bundle(rt, path_bytes.as_ptr(), path_bytes.len()) };
    assert_eq!(load_result, 0, "polyplug_runtime_load_bundle failed");

    // test.add contract with major version 1
    let contract_id: u64 = polyplug_abi::contract_id("test.add", 1);
    (rt, contract_id)
}

// ─── Benchmark: find_all_by_contract with various buffer sizes ───────────────

fn bench_ffi_find_all_by_contract(c: &mut Criterion) {
    let (rt, contract_id): (*mut OpaqueRuntime, u64) = setup_runtime_with_plugins();

    let mut group: criterion::BenchmarkGroup<'_, criterion::measurement::WallTime> =
        c.benchmark_group("ffi");
    group.throughput(Throughput::Elements(1));

    for buffer_size in [1_usize, 10_usize, 100_usize] {
        let mut out_buffer: Vec<u64> = vec![0u64; buffer_size];

        group.bench_function(
            BenchmarkId::new("find_all_by_contract", format!("cap_{}", buffer_size)),
            |b| {
                b.iter(|| {
                    // SAFETY: rt is non-null valid OpaqueRuntime; out_buffer is valid for buffer_size u64 elements.
                    let count: usize = unsafe {
                        polyplug_runtime_find_all_by_contract(
                            black_box(rt),
                            black_box(contract_id),
                            black_box(0u32),
                            black_box(out_buffer.as_mut_ptr()),
                            black_box(buffer_size),
                        )
                    };
                    black_box(count);
                });
            },
        );
    }

    group.finish();

    // SAFETY: rt was returned by polyplug_runtime_create and is non-null.
    unsafe { polyplug_runtime_destroy(rt) };
}

// ─── Benchmark: find_all_by_contract with empty result ───────────────────────

fn bench_ffi_find_all_empty_result(c: &mut Criterion) {
    let (rt, _contract_id): (*mut OpaqueRuntime, u64) = setup_runtime_with_plugins();

    let nonexistent_contract_id: u64 = 0xDEAD_BEEF_CAFE_0000_u64;

    let mut group: criterion::BenchmarkGroup<'_, criterion::measurement::WallTime> =
        c.benchmark_group("ffi");
    group.throughput(Throughput::Elements(1));

    let mut out_buffer: Vec<u64> = vec![0u64; 10];

    group.bench_function(
        BenchmarkId::new("find_all_by_contract", "empty_result"),
        |b| {
            b.iter(|| {
                // SAFETY: rt is non-null valid OpaqueRuntime; out_buffer is valid for 10 u64 elements.
                let count: usize = unsafe {
                    polyplug_runtime_find_all_by_contract(
                        black_box(rt),
                        black_box(nonexistent_contract_id),
                        black_box(0u32),
                        black_box(out_buffer.as_mut_ptr()),
                        black_box(10_usize),
                    )
                };
                black_box(count);
            });
        },
    );

    group.finish();

    // SAFETY: rt was returned by polyplug_runtime_create and is non-null.
    unsafe { polyplug_runtime_destroy(rt) };
}

// ─── criterion_group / criterion_main ────────────────────────────────────────

criterion_group!(
    benches,
    bench_ffi_find_all_by_contract,
    bench_ffi_find_all_empty_result
);
criterion_main!(benches);

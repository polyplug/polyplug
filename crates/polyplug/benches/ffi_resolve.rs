#![allow(clippy::expect_used)]

// THIS IS A BENCHMARK FILE — do not add #[test] functions here
// Run with: cargo bench -p polyplug --bench ffi_resolve
//
// Benchmark: polyplug_runtime_resolve_guest_contract FFI path
// Measures: Time from FFI call to interface pointer return (direct, no allocation)

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
    fn polyplug_runtime_find_guest_contract(
        rt: *const OpaqueRuntime,
        contract_id: u64,
        min_version: u32,
    ) -> u64;
    // New FFI: returns interface pointer directly, no allocation
    fn polyplug_runtime_resolve_guest_contract(
        rt: *const OpaqueRuntime,
        packed_handle: u64,
    ) -> *const ();
}

// ─── Setup helper ────────────────────────────────────────────────────────────

fn setup_runtime_with_plugin() -> (*mut OpaqueRuntime, u64, u64) {
    // SAFETY: polyplug_runtime_create has no pointer preconditions.
    let rt: *mut OpaqueRuntime = unsafe { polyplug_runtime_create() };
    assert!(!rt.is_null(), "polyplug_runtime_create returned null");

    let path_bytes: &[u8] = TEST_PLUGIN_DIR.as_bytes();
    // SAFETY: rt is non-null valid OpaqueRuntime; path_bytes is valid for path_len bytes.
    let load_result: u32 =
        unsafe { polyplug_runtime_load_bundle(rt, path_bytes.as_ptr(), path_bytes.len()) };
    assert_eq!(load_result, 0, "polyplug_runtime_load_bundle failed");

    // test.add contract with major version 1
    let contract_id: u64 = polyplug_utils::guest_contract_id("test.add", 1);

    // SAFETY: rt is non-null valid OpaqueRuntime.
    let packed_handle: u64 = unsafe { polyplug_runtime_find_guest_contract(rt, contract_id, 0) };
    assert_ne!(packed_handle, u64::MAX, "plugin not found in registry");

    (rt, contract_id, packed_handle)
}

// ─── Benchmark: resolve_plugin FFI path (no allocation) ───────────────────────

fn bench_ffi_resolve_plugin(c: &mut Criterion) {
    let (rt, _contract_id, packed_handle): (*mut OpaqueRuntime, u64, u64) =
        setup_runtime_with_plugin();

    let mut group: criterion::BenchmarkGroup<'_, criterion::measurement::WallTime> =
        c.benchmark_group("ffi");
    group.throughput(Throughput::Elements(1));

    group.bench_function(
        BenchmarkId::new("resolve_plugin", "direct_interface"),
        |b| {
            b.iter(|| {
                // SAFETY: rt is non-null valid OpaqueRuntime; packed_handle is a valid handle.
                // New FFI returns interface pointer directly - no allocation, no release needed.
                let interface_ptr: *const () = unsafe {
                    polyplug_runtime_resolve_guest_contract(black_box(rt), black_box(packed_handle))
                };
                black_box(interface_ptr);
            });
        },
    );

    group.finish();

    // SAFETY: rt was returned by polyplug_runtime_create and is non-null.
    unsafe { polyplug_runtime_destroy(rt) };
}

// ─── Benchmark: resolve_plugin with null handle (early return path) ───────────

fn bench_ffi_resolve_null_handle(c: &mut Criterion) {
    let (rt, _contract_id, _packed_handle): (*mut OpaqueRuntime, u64, u64) =
        setup_runtime_with_plugin();

    let mut group: criterion::BenchmarkGroup<'_, criterion::measurement::WallTime> =
        c.benchmark_group("ffi");
    group.throughput(Throughput::Elements(1));

    group.bench_function(BenchmarkId::new("resolve_plugin", "null_handle"), |b| {
        b.iter(|| {
            // SAFETY: rt is non-null valid OpaqueRuntime; u64::MAX is null handle sentinel.
            let interface_ptr: *const () = unsafe {
                polyplug_runtime_resolve_guest_contract(black_box(rt), black_box(u64::MAX))
            };
            black_box(interface_ptr);
        });
    });

    group.finish();

    // SAFETY: rt was returned by polyplug_runtime_create and is non-null.
    unsafe { polyplug_runtime_destroy(rt) };
}

// ─── criterion_group / criterion_main ────────────────────────────────────────

criterion_group!(
    benches,
    bench_ffi_resolve_plugin,
    bench_ffi_resolve_null_handle
);
criterion_main!(benches);

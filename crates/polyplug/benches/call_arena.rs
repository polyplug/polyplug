#![allow(clippy::expect_used)]

// THIS IS A BENCHMARK FILE — do not add #[test] functions here
// Run with: cargo bench -p polyplug --bench call_arena
//
// CallArena microbench — the per-call bump allocator handed to VM dispatch.
//
// The retain-and-rewind work (#49) changed three arena paths but benched none of
// them: (a) warm alloc within the primary block, (b) reset/rewind cost, and
// (c) the overflow path — where reset RETAINS host-allocated blocks instead of
// freeing them, so the SECOND call that overflows reuses the retained block at
// near-zero cost instead of paying a host `alloc` again. That reuse number is
// the whole justification for retain-and-rewind; this bench measures it directly
// next to its first-call (block malloc'd) counterpart.
//
// HOW TO READ THE RESULT.
//   - `primary/alloc` is a few-instruction bump (pointer align + add); it is the
//     floor and should sit in the sub-nanosecond / low-ns range.
//   - `reset/primary_only` is just rewinding `cur` to `base` (no overflow chain).
//   - `overflow/cold_first_block` PAYS a host allocation each iteration (the
//     arena is reset AND its chain freed every iter), so it measures the
//     malloc-backed first-overflow cost.
//   - `overflow/warm_reuse` resets WITHOUT freeing (retain-and-rewind), so every
//     iteration after the first reuses the same retained block. The gap between
//     `cold_first_block` and `warm_reuse` is exactly what retain-and-rewind buys:
//     warm reuse should be close to the primary-bump floor, cold should be an
//     order of magnitude (or more) above it — a malloc per call.
//   - `per_call/<size>` is the realistic shape: reset + a handful of mixed-size
//     allocs, at a small (64 B, primary-resident) and a large (64 KiB, overflow)
//     payload, so you can see the two regimes side by side.
//
// The arena is constructed ONCE per benchmark function and kept alive across all
// iterations; its `Drop` frees every retained overflow block at teardown, so the
// bench itself does not leak (verified by the arena's own `drop_frees_all_blocks`
// unit test in polyplug_abi).

use core::hint::black_box;

use criterion::BenchmarkId;
use criterion::Criterion;
use criterion::Throughput;
use criterion::criterion_group;
use criterion::criterion_main;

use polyplug_abi::AbiError;
use polyplug_abi::Array;
use polyplug_abi::CallArena;
use polyplug_abi::DependencyInfo;
use polyplug_abi::GuestContractHandle;
use polyplug_abi::GuestContractInterface;
use polyplug_abi::HostApi;
use polyplug_abi::HostContractInstance;
use polyplug_abi::HostContractInterface;
use polyplug_abi::PluginDescriptor;
use polyplug_abi::ffi::polyplug_host_alloc;
use polyplug_abi::ffi::polyplug_host_free;
use polyplug_utils::BundleId;

// ─── Minimal HostApi backing the arena's overflow alloc/free ──────────────────
//
// CallArena only ever calls `alloc` and `free`. The remaining fields are
// populated with no-op stubs so the struct is a fully valid HostApi (no zeroed
// function pointers) — this mirrors the `test_host()` used by CallArena's own
// unit tests.

unsafe extern "C" fn stub_register_guest(
    _this: *const HostApi,
    _descriptor: *const PluginDescriptor,
    _interface: *const GuestContractInterface,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
    }
}

unsafe extern "C" fn stub_find(_this: *const HostApi, _id: u64, _ver: u32) -> GuestContractHandle {
    GuestContractHandle::null()
}

unsafe extern "C" fn stub_find_all(
    _this: *const HostApi,
    _id: u64,
    _ver: u32,
) -> Array<GuestContractHandle> {
    Array::empty()
}

unsafe extern "C" fn stub_resolve_guest(
    _this: *const HostApi,
    _handle: GuestContractHandle,
) -> *const GuestContractInterface {
    core::ptr::null()
}

unsafe extern "C" fn stub_get_host_contract(
    _this: *const HostApi,
    _id: u64,
    _ver: u32,
) -> HostContractInstance {
    HostContractInstance::null()
}

unsafe extern "C" fn stub_resolve_host_interface(
    _this: *const HostApi,
    _id: u64,
    _ver: u32,
) -> *const HostContractInterface {
    core::ptr::null()
}

unsafe extern "C" fn stub_list_bundles(_this: *const HostApi) -> Array<BundleId> {
    Array::empty()
}

unsafe extern "C" fn stub_get_deps(_this: *const HostApi) -> Array<DependencyInfo> {
    Array::empty()
}

unsafe extern "C" fn stub_load(
    _this: *const HostApi,
    _p: *const u8,
    _l: usize,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
    }
}

unsafe extern "C" fn stub_register_host(
    _this: *const HostApi,
    _interface: *const HostContractInterface,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
    }
}

unsafe extern "C" fn stub_register_loader(
    _this: *const HostApi,
    _loader: *mut core::ffi::c_void,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
    }
}

unsafe extern "C" fn stub_get_last_error(
    _this: *const HostApi,
    _buf: *mut u8,
    _len: usize,
) -> usize {
    0
}

unsafe extern "C" fn stub_get_len(_this: *const HostApi) -> usize {
    0
}

unsafe extern "C" fn stub_unload(
    _this: *const HostApi,
    _bundle_id: BundleId,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
    }
}

/// Alloc wrapper delegating to the host allocator (used for overflow blocks).
///
/// # Safety
/// Delegates to polyplug_host_alloc which is safe for any size/align.
unsafe extern "C" fn arena_alloc(_this: *const HostApi, size: usize, align: usize) -> *mut u8 {
    polyplug_host_alloc(size, align)
}

/// Free wrapper delegating to the host allocator (frees overflow blocks).
///
/// # Safety
/// `ptr` must have been allocated by `arena_alloc` with the same `size`/`align`.
unsafe extern "C" fn arena_free(_this: *const HostApi, ptr: *mut u8, size: usize, align: usize) {
    // SAFETY: ptr was allocated by arena_alloc (polyplug_host_alloc).
    unsafe { polyplug_host_free(ptr, size, align) };
}

/// Build a `HostApi` whose `alloc`/`free` route to the host allocator. Every
/// other field is a valid no-op stub so the table is well-formed.
fn arena_host_api() -> HostApi {
    HostApi {
        runtime: core::ptr::null_mut(),
        register_guest_contract: stub_register_guest,
        alloc: arena_alloc,
        free: arena_free,
        find_guest_contract: stub_find,
        find_all_guest_contracts: stub_find_all,
        resolve_guest_contract: stub_resolve_guest,
        get_host_contract: stub_get_host_contract,
        resolve_host_contract_interface: stub_resolve_host_interface,
        list_bundles: stub_list_bundles,
        get_dependencies: stub_get_deps,
        load_bundle: stub_load,
        reload_bundle: stub_load,
        register_host_contract: stub_register_host,
        register_loader: stub_register_loader,
        get_last_error: stub_get_last_error,
        get_error_len: stub_get_len,
        unload_bundle: stub_unload,
        log: stub_host_log,
        create_guest_instance: stub_create_guest_instance,
        destroy_guest_instance: stub_destroy_guest_instance,
        revision_counter: stub_revision_counter,
        reserved: core::ptr::null(),
    }
}

// Primary-region size for the arena: matches the generated host caller's inline
// buffer order of magnitude (a few KiB), large enough that the warm-alloc and
// per-call/64B benches never overflow.
const PRIMARY_BYTES: usize = 4096;

// ─── Benchmark 1 — warm alloc within the primary block ────────────────────────

/// Single 64-byte bump-alloc from the primary region. Resets each iteration so
/// `cur` never advances past the buffer — measures the pure align-and-bump cost
/// with no overflow and no host round trip.
fn bench_primary_alloc(c: &mut Criterion) {
    let host: HostApi = arena_host_api();
    let mut buf: Vec<u8> = vec![0_u8; PRIMARY_BYTES];
    let mut arena: CallArena = CallArena::new(&mut buf, &host as *const HostApi);

    let mut group: criterion::BenchmarkGroup<'_, criterion::measurement::WallTime> =
        c.benchmark_group("call_arena");
    group.throughput(Throughput::Elements(1));

    group.bench_function(BenchmarkId::new("primary", "alloc_64"), |b| {
        b.iter(|| {
            arena.reset();
            let p: *mut u8 = black_box(arena.alloc(black_box(64), black_box(8)));
            black_box(p);
        });
    });

    group.finish();
    // arena drops here at end of function — frees any retained block (none here).
}

// ─── Benchmark 2 — reset / rewind cost ────────────────────────────────────────

/// Cost of `reset()` alone with no overflow chain — just rewinding `cur` to
/// `base`. Pairs with the overflow reset implicit in benches 3/4 to show reset
/// is near-free on the common (primary-only) path.
fn bench_reset_primary_only(c: &mut Criterion) {
    let host: HostApi = arena_host_api();
    let mut buf: Vec<u8> = vec![0_u8; PRIMARY_BYTES];
    let mut arena: CallArena = CallArena::new(&mut buf, &host as *const HostApi);
    // Advance the cursor once so reset has something to rewind.
    let _ = arena.alloc(64, 8);

    let mut group: criterion::BenchmarkGroup<'_, criterion::measurement::WallTime> =
        c.benchmark_group("call_arena");
    group.throughput(Throughput::Elements(1));

    group.bench_function(BenchmarkId::new("reset", "primary_only"), |b| {
        b.iter(|| {
            arena.reset();
            black_box(&arena.cur);
        });
    });

    group.finish();
}

// ─── Benchmark 3 — overflow: cold first-block vs warm retained reuse ──────────

/// THE retain-and-rewind comparison. Both arms allocate a payload that does not
/// fit the primary region, forcing the overflow path:
///   - `cold_first_block`: each iteration FREES the overflow chain (via a fresh
///     arena teardown) before allocating, so it pays a host `malloc` every time —
///     the cost of the very first overflowing call.
///   - `warm_reuse`: each iteration `reset()`s WITHOUT freeing, so the retained
///     block is reused — no host allocation after the first. This is the number
///     that justifies retain-and-rewind.
fn bench_overflow_reuse(c: &mut Criterion) {
    let host: HostApi = arena_host_api();
    let host_ptr: *const HostApi = &host as *const HostApi;

    // Overflow payload: larger than the primary region so it always spills.
    const OVERFLOW_ALLOC: usize = PRIMARY_BYTES + 1024;

    let mut group: criterion::BenchmarkGroup<'_, criterion::measurement::WallTime> =
        c.benchmark_group("call_arena");
    group.throughput(Throughput::Elements(1));

    // COLD: build a fresh arena per iteration so its Drop frees the block, then
    // the alloc pays a brand-new host malloc. `iter_batched` would amortize the
    // drop into setup; we want the malloc+free in the measured body, so we
    // construct, alloc, and drop inside the timed closure.
    group.bench_function(BenchmarkId::new("overflow", "cold_first_block"), |b| {
        b.iter(|| {
            let mut buf: [u8; 16] = [0_u8; 16];
            let mut arena: CallArena = CallArena::new(&mut buf, host_ptr);
            let p: *mut u8 = black_box(arena.alloc(black_box(OVERFLOW_ALLOC), black_box(8)));
            black_box(p);
            // arena dropped here → frees the just-allocated overflow block.
        });
    });

    // WARM: one arena, kept across all iterations. After the first overflow the
    // block is retained; every reset rewinds it and the next alloc reuses it with
    // no host call. The arena's Drop frees the single retained block at teardown.
    {
        let mut buf: [u8; 16] = [0_u8; 16];
        let mut arena: CallArena = CallArena::new(&mut buf, host_ptr);
        // Prime the retained block once, outside the timed loop.
        let _ = arena.alloc(OVERFLOW_ALLOC, 8);

        group.bench_function(BenchmarkId::new("overflow", "warm_reuse"), |b| {
            b.iter(|| {
                arena.reset();
                let p: *mut u8 = black_box(arena.alloc(black_box(OVERFLOW_ALLOC), black_box(8)));
                black_box(p);
            });
        });
        // arena dropped here → frees the single retained overflow block.
    }

    group.finish();
}

// ─── Benchmark 4 — realistic per-call pattern at two payload sizes ────────────

/// A realistic generated-caller pattern: `reset()` then a handful of mixed-size
/// allocs (a small header + the payload + a small trailer), at two payload
/// sizes — 64 B (fully primary-resident) and 64 KiB (forces the overflow path).
/// The arena is reused across iterations so the 64 KiB arm exercises warm
/// retained-block reuse, exactly as a real per-call loop would.
fn bench_per_call(c: &mut Criterion) {
    let host: HostApi = arena_host_api();
    let host_ptr: *const HostApi = &host as *const HostApi;

    // Primary region sized so 64 B stays resident but 64 KiB always overflows.
    let mut buf: Vec<u8> = vec![0_u8; PRIMARY_BYTES];
    let mut arena: CallArena = CallArena::new(&mut buf, host_ptr);

    let payload_sizes: [usize; 2] = [64, 65536];

    let mut group: criterion::BenchmarkGroup<'_, criterion::measurement::WallTime> =
        c.benchmark_group("call_arena");
    group.throughput(Throughput::Elements(1));

    for &payload in &payload_sizes {
        // Prime any retained overflow block for the large payload once before
        // timing so the warm-reuse path is what we measure (matching real calls).
        arena.reset();
        let _ = arena.alloc(16, 8);
        let _ = arena.alloc(payload, 8);
        let _ = arena.alloc(32, 8);

        group.bench_with_input(BenchmarkId::new("per_call", payload), &payload, |b, &p| {
            b.iter(|| {
                arena.reset();
                // header (16 B), payload (p), trailer (32 B) — the shape a
                // generated caller writes for one returned value plus framing.
                let h: *mut u8 = black_box(arena.alloc(black_box(16), black_box(8)));
                let body: *mut u8 = black_box(arena.alloc(black_box(p), black_box(8)));
                let t: *mut u8 = black_box(arena.alloc(black_box(32), black_box(8)));
                black_box((h, body, t));
            });
        });
    }

    group.finish();
    // arena drops here → frees the single retained 64 KiB overflow block.
}

// ─── criterion_group / criterion_main ────────────────────────────────────────

criterion_group!(
    benches,
    bench_primary_alloc,
    bench_reset_primary_only,
    bench_overflow_reuse,
    bench_per_call,
);
criterion_main!(benches);

/// `HostApi.log` stub for test hosts — drops the record.
unsafe extern "C" fn stub_host_log(
    _this: *const polyplug_abi::HostApi,
    _level: u32,
    _scope: polyplug_abi::StringView,
    _message: polyplug_abi::StringView,
) {
}

unsafe extern "C" fn stub_create_guest_instance(
    _this: *const polyplug_abi::HostApi,
    _interface: *const polyplug_abi::GuestContractInterface,
    _args: *const core::ffi::c_void,
    out_instance: *mut polyplug_abi::GuestContractInstance,
) {
    if !out_instance.is_null() {
        // SAFETY: out_instance is non-null (just checked) and writable per the ABI contract.
        unsafe { out_instance.write(polyplug_abi::GuestContractInstance::null()) };
    }
}

unsafe extern "C" fn stub_destroy_guest_instance(
    _this: *const polyplug_abi::HostApi,
    _interface: *const polyplug_abi::GuestContractInterface,
    _instance: polyplug_abi::GuestContractInstance,
) {
}

unsafe extern "C" fn stub_revision_counter(_this: *const polyplug_abi::HostApi) -> *const u64 {
    core::ptr::null()
}

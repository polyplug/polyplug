#![allow(clippy::expect_used)]

// THIS IS A BENCHMARK FILE — do not add #[test] functions here
// Run with: cargo bench -p polyplug --bench revision_check
//
// Benchmark: the per-dispatch overhead of the self-revalidating caller's
// staleness check — the ONLY cost the auto-cache feature adds to the hot path.
//
// The generated host/peer callers cache the resolved interface and, before each
// dispatch, poll the runtime revision counter through a cached pointer with one
// acquire load and compare it to the value cached at resolve time. When it is
// unchanged (the steady state between reloads) they dispatch directly through the
// cached interface; only a change triggers a re-resolve. Crucially this is a
// direct pointer poll — NOT a function call into the runtime per dispatch.
//
// This bench isolates that fast path: it dispatches a real native function with
// and without the staleness check, so the delta between the two is exactly the
// per-call cost the feature adds. The expectation (and the honest claim) is that
// one acquire load of a read-mostly word plus an integer compare is lost in the
// noise of the dispatch it guards.

use core::hint::black_box;
use core::ptr;
use core::sync::atomic::AtomicU64;
use core::sync::atomic::Ordering;

use criterion::Criterion;
use criterion::criterion_group;
use criterion::criterion_main;

use polyplug_abi::AbiError;
use polyplug_abi::GuestContractInstance;

#[repr(C)]
struct AddArgs {
    a: u32,
    b: u32,
}

/// A real native dispatch target with the canonical out-param ABI signature:
/// adds the two `u32` args and writes the sum through `out`.
unsafe extern "C" fn bench_add(
    _instance: GuestContractInstance,
    args: *const (),
    out: *mut (),
    _err: *mut AbiError,
) {
    // SAFETY: args points to a valid AddArgs and out to a valid u32 (the bench supplies both).
    unsafe {
        let a: &AddArgs = &*(args as *const AddArgs);
        *(out as *mut u32) = a.a.wrapping_add(a.b);
    }
}

// FFI fn-pointer signature alias (the single CLAUDE.md-sanctioned `type` form):
// the native dispatch calling convention, defined once for the bench.
type DispatchFn = unsafe extern "C" fn(GuestContractInstance, *const (), *mut (), *mut AbiError);

fn dispatch_once(dispatch_fn: DispatchFn) -> u32 {
    let args: AddArgs = AddArgs { a: 2, b: 3 };
    let mut out: u32 = 0;
    let mut err: AbiError = AbiError::ok();
    // SAFETY: args/out match the ABI; bench_add only reads args and writes out.
    unsafe {
        dispatch_fn(
            GuestContractInstance::null(),
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
            &mut err as *mut AbiError,
        );
    }
    out
}

/// Acquire-load the revision counter through the cached pointer — exactly the
/// read the generated caller performs before each dispatch.
fn read_revision(revision_ptr: *const u64) -> u64 {
    // SAFETY: revision_ptr points to a live AtomicU64 owned by the bench for its full run.
    unsafe { (*(revision_ptr as *const AtomicU64)).load(Ordering::Acquire) }
}

fn bench_revision_check(c: &mut Criterion) {
    let dispatch_fn: DispatchFn = bench_add;

    // The runtime revision counter the generated caller polls. Read-mostly: the
    // bench never mutates it, mirroring the steady state between reloads where the
    // cache line stays Shared in every reader's L1.
    let revision: AtomicU64 = AtomicU64::new(7);
    let revision_ptr: *const u64 = ptr::addr_of!(revision).cast::<u64>();
    let cached_revision: u64 = read_revision(revision_ptr);

    let mut group = c.benchmark_group("revision_check");

    // Floor: dispatch only — what a non-revalidating caller does.
    group.bench_function("dispatch_only", |b| {
        b.iter(|| black_box(dispatch_once(black_box(dispatch_fn))));
    });

    // Feature hot path: the staleness check (one acquire load + compare, branch
    // not taken) followed by the same dispatch. The delta vs `dispatch_only` is
    // the entire per-call overhead the auto-cache feature adds.
    group.bench_function("staleness_check_then_dispatch", |b| {
        b.iter(|| {
            if read_revision(black_box(revision_ptr)) != black_box(cached_revision) {
                // Re-resolve branch — never taken in steady state. Kept as a black-boxed
                // no-op so the bench measures the common (unchanged) path.
                black_box(0_u32);
            }
            black_box(dispatch_once(black_box(dispatch_fn)))
        });
    });

    group.finish();
}

criterion_group!(benches, bench_revision_check);
criterion_main!(benches);

#![allow(clippy::expect_used)]

// THIS IS A BENCHMARK FILE — do not add #[test] functions here
// Run with: cargo bench -p polyplug --bench contention
//
// Multi-threaded registry-dispatch throughput.
//
// The registry hot paths — `find_guest_contract` and `resolve_guest_contract` —
// are `RwLock`-guarded reads on a single shared `RuntimeStore`. Every other
// bench in this crate is single-threaded, so none of them would notice a
// hot-path lock regression (a read lock silently becoming a write lock, or a new
// `Mutex` landing on the resolve path). This bench is the regression sentinel for
// exactly that.
//
// METHODOLOGY — why `iter_custom` + a barrier-started thread pool.
// Criterion times the closure body on one thread; it has no notion of "N
// threads did work in parallel". To measure aggregate throughput we:
//   1. Spawn N worker threads ONCE per benchmark function (outside the timed
//      region) that park on a channel waiting for an iteration count.
//   2. For each criterion measurement, send each worker its share of the
//      iteration budget, release them simultaneously through a `Barrier`, and
//      time the wall-clock span from "all released" to "all finished".
//   3. Report `Throughput::Elements(N_THREADS)` per iteration so criterion's
//      throughput line reads as calls/sec aggregated across all threads.
// The `Barrier` start ensures we time concurrent execution, not the cost of
// spawning threads. The thread pool is reused across all measurements of a
// given thread count so per-measurement overhead is just the channel hand-off.
//
// HOW TO READ THE RESULT.
// The headline is the SHAPE of the `threads/1 → threads/8` curve, not any one
// number. A lock-free read path should scale close to linearly: aggregate
// throughput at 8 threads should approach 8× the 1-thread figure on an 8-core
// box (memory bandwidth and turbo clock decay keep it below a perfect 8×).
// If aggregate throughput *flattens* or *collapses* as threads rise, a writer
// lock or contended mutex has crept onto the resolve path — that is the
// finding this bench exists to surface. Per-thread throughput (aggregate / N)
// falling toward zero is the same signal viewed per-worker.
//
// THE `contention/cached/*` GROUP.
// The bench resolves the interface pointer ONCE before the timed loop (the
// documented cache-the-handle pattern that `counter_inc/polyplug` uses) and then
// dispatches each iteration straight through the cached pointer with ZERO
// registry-lock traffic. With no shared reader counter on the hot path, the
// `cached` curve should scale up close to linearly across thread counts — a
// flattening or collapse signals a lock regression on the dispatch path.

use core::ffi::c_void;
use core::hint::black_box;
use core::time::Duration;
use std::sync::Arc;
use std::sync::Barrier;
use std::sync::mpsc;
use std::thread;
use std::time::Instant;

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
use polyplug_abi::dispatch::VmLoaderData;
use polyplug_abi::types::Version;
use polyplug_utils::BundleId;
use polyplug_utils::GuestContractId;

use core::mem;
use core::ptr;
use criterion::BenchmarkGroup;
use criterion::measurement::WallTime;

// ─── Native target + provider ─────────────────────────────────────────────────

/// Argument struct for the benchmark target function.
#[repr(C)]
struct AddArgs {
    a: u32,
    b: u32,
}

/// Native dispatch target — adds two `u32` args, writes the sum to `out` so the
/// work is not dead-code-eliminated.
///
/// # Safety
/// `args` must point to a valid `AddArgs`; `out` must point to a valid `u32`.
unsafe extern "C" fn bench_add(
    _adapter_context: *mut c_void,
    _instance: GuestContractInstance,
    args: *const (),
    out: *mut (),
    out_err: *mut AbiError,
) {
    // SAFETY: args points to AddArgs and out to u32 per the caller's contract.
    unsafe {
        let a: &AddArgs = &*(args as *const AddArgs);
        *(out as *mut u32) = a.a.wrapping_add(a.b);
    }
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
    }
}

unsafe extern "C" fn noop_create_instance(
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

unsafe extern "C" fn noop_destroy_instance(
    _adapter_context: *mut c_void,
    _loader_data: VmLoaderData,
    _host: *const HostApi,
    _instance: GuestContractInstance,
) {
}

/// Leak a native `GuestContractInterface` exposing `bench_add` at fn_id 0.
///
/// The one-element function table is leaked (not `static`) because raw fn
/// pointers do not implement `Sync`; a leaked `Box` is `'static` with a stable
/// address and no `Sync` bound.
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
        adapter_context: ptr::null_mut(),
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

/// Register a native provider for `contract_id` into the runtime's registry.
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

// ─── Thread-pool worker plumbing ──────────────────────────────────────────────

/// A `'static` interface pointer resolved ONCE before the timed loop, shared
/// read-only across the cached-dispatch workers. The interface is leaked, so the
/// pointer is `'static` and never moves; concurrent native dispatch through it
/// touches no shared mutable state.
#[derive(Clone, Copy)]
struct SharedInterface(*const GuestContractInterface);

// SAFETY: the interface is leaked ('static) and immutable; native dispatch
// through it reads only the function table and writes to caller-owned out slots,
// so concurrent read-only sharing across worker threads is sound.
unsafe impl Send for SharedInterface {}
// SAFETY: see Send — read-only concurrent access to an immutable 'static interface.
unsafe impl Sync for SharedInterface {}

/// One unit of CACHED dispatch work: dispatch fn 0 straight through an
/// already-resolved interface pointer, with NO registry lookup and NO registry
/// lock. This is the documented cache-the-handle hot path — the interface
/// pointer is resolved once before the timed loop and stays valid for as long as
/// the bundle stays loaded. Returns the sum so the work survives DCE.
///
/// # Safety
/// `interface` must point to a valid `'static` `GuestContractInterface` whose
/// native dispatch table has at least one function (fn 0 = `bench_add`).
#[inline]
unsafe fn cached_dispatch(interface: *const GuestContractInterface, contract_id: u64) -> u32 {
    // SAFETY: interface is the runtime's 'static interface pointer for the bench.
    let iface: &GuestContractInterface = unsafe { &*interface };
    // SAFETY: dispatch_type is Native (registered as such); fn 0 is in range.
    let fn_ptr: *const () = unsafe { *iface.dispatch.native.functions };
    // SAFETY: transmute to the native dispatch signature bench_add was built with.
    let dispatch_fn: unsafe extern "C" fn(
        *mut c_void,
        GuestContractInstance,
        *const (),
        *mut (),
        *mut AbiError,
    ) = {
        // SAFETY: the pointer comes from the generated dispatch table and is cast to that function's exact ABI.
        unsafe { mem::transmute(fn_ptr) }
    };

    let instance: GuestContractInstance = GuestContractInstance {
        data: ptr::null_mut(),
        contract_id: GuestContractId::from_u64(contract_id),
    };
    let args: AddArgs = AddArgs {
        a: 42_u32,
        b: 57_u32,
    };
    let mut out: u32 = 0_u32;
    let mut err: AbiError = AbiError::ok();
    // SAFETY: instance carries the registered contract_id; args/out match bench_add.
    unsafe {
        dispatch_fn(
            iface.adapter_context,
            instance,
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
            &mut err,
        )
    };
    black_box(err);
    out
}

/// Message sent to a parked worker: how many iterations to run, plus the shared
/// start barrier so all workers begin the timed region together.
struct WorkBatch {
    iters: u64,
    barrier: Arc<Barrier>,
}

/// A reusable pool of worker threads. Spawned once per benchmark function,
/// outside the timed region; each `run(iters)` releases all workers through a
/// fresh barrier and returns the wall-clock span of the concurrent run.
struct WorkerPool {
    senders: Vec<mpsc::Sender<WorkBatch>>,
    done: mpsc::Receiver<()>,
    n_threads: usize,
    handles: Vec<thread::JoinHandle<()>>,
}

impl WorkerPool {
    /// Spawn `n_threads` workers, each parked on its own channel and pinned to
    /// the shared cached interface + contract.
    fn new(n_threads: usize, cached: SharedInterface, contract_id: u64) -> WorkerPool {
        let mut senders: Vec<mpsc::Sender<WorkBatch>> = Vec::with_capacity(n_threads);
        let mut handles: Vec<thread::JoinHandle<()>> = Vec::with_capacity(n_threads);
        let (done_tx, done_rx): (mpsc::Sender<()>, mpsc::Receiver<()>) = mpsc::channel();

        for _ in 0..n_threads {
            let (work_tx, work_rx): (mpsc::Sender<WorkBatch>, mpsc::Receiver<WorkBatch>) =
                mpsc::channel();
            let done_tx_worker: mpsc::Sender<()> = done_tx.clone();
            let cached_for_worker: SharedInterface = cached;

            let handle: thread::JoinHandle<()> = thread::spawn(move || {
                // Re-bind the wrapper inside the closure so the Send capture is
                // `SharedInterface` (which is Send via its unsafe impl), not a
                // disjoint raw pointer field — Rust 2021 closures capture fields directly.
                let worker_cached: SharedInterface = cached_for_worker;
                // Park until a batch arrives; an Err means the pool was dropped.
                while let Ok(batch) = work_rx.recv() {
                    // Synchronize the start so all workers run concurrently.
                    batch.barrier.wait();
                    let mut acc: u32 = 0;
                    for _ in 0..batch.iters {
                        // SAFETY: interface is the runtime's 'static interface pointer
                        // resolved before the loop; native dispatch through it touches
                        // no shared mutable state across threads.
                        acc = acc
                            .wrapping_add(unsafe { cached_dispatch(worker_cached.0, contract_id) });
                    }
                    black_box(acc);
                    // Signal completion; ignore send error if the pool is tearing down.
                    let _ = done_tx_worker.send(());
                }
            });
            senders.push(work_tx);
            handles.push(handle);
        }

        WorkerPool {
            senders,
            done: done_rx,
            n_threads,
            handles,
        }
    }

    /// Run `total_iters` of work spread across all workers and return the
    /// wall-clock duration of the concurrent region (start barrier release →
    /// last worker done). Iterations are split as evenly as possible.
    fn run(&self, total_iters: u64) -> Duration {
        let n: u64 = self.n_threads as u64;
        let base: u64 = total_iters / n;
        let extra: u64 = total_iters % n;

        // Barrier of N workers + this dispatcher thread, so the dispatcher can
        // start the clock at the exact moment the workers are released.
        let barrier: Arc<Barrier> = Arc::new(Barrier::new(self.n_threads + 1));

        for (i, sender) in self.senders.iter().enumerate() {
            let iters: u64 = base + if (i as u64) < extra { 1 } else { 0 };
            sender
                .send(WorkBatch {
                    iters,
                    barrier: Arc::clone(&barrier),
                })
                .expect("worker channel must accept a batch");
        }

        // Release all workers, then time until each reports completion.
        barrier.wait();
        let start: Instant = Instant::now();
        for _ in 0..self.n_threads {
            self.done
                .recv()
                .expect("every worker must report completion");
        }
        start.elapsed()
    }
}

impl Drop for WorkerPool {
    fn drop(&mut self) {
        // Dropping the senders closes each worker's recv loop; join to avoid
        // leaking threads across benchmark functions.
        self.senders.clear();
        for handle in self.handles.drain(..) {
            let _ = handle.join();
        }
    }
}

// ─── Benchmark — scaling across thread counts ─────────────────────────────────

/// Builds one shared runtime, registers a single native provider, then measures
/// aggregate dispatch throughput at 1, 2, 4, and 8 threads through the cached
/// pre-resolved interface pointer. Touching no registry lock, the curve should
/// scale up close to linearly — a flattening signals a lock regression.
fn bench_contention(c: &mut Criterion) {
    let runtime: Arc<Runtime> = Runtime::builder()
        .build()
        .expect("bare runtime build should succeed");
    let contract_id: u64 = GuestContractId::new("bench.contract", 1_u32).id();
    register_native_provider(&runtime, contract_id, 0x2222_u64);

    let host_abi: *const HostApi = runtime.host_abi();

    // Resolve the interface pointer ONCE (the cache-the-handle pattern). This is
    // the pointer the cached workers dispatch through with no further lookup.
    let host_ptr: *const HostApi = host_abi;
    // SAFETY: host_ptr is the runtime's 'static HostApi; contract_id is registered.
    let handle = unsafe { ((*host_abi).find_guest_contract)(host_ptr, contract_id, 0) };
    // SAFETY: handle was just returned for a registered contract.
    let interface_ptr: *const GuestContractInterface =
        unsafe { ((*host_abi).resolve_guest_contract)(host_ptr, handle) };
    assert!(
        !interface_ptr.is_null(),
        "cached interface must resolve for the contention bench"
    );
    let cached: SharedInterface = SharedInterface(interface_ptr);

    let thread_counts: [usize; 4] = [1, 2, 4, 8];

    let mut group: BenchmarkGroup<'_, WallTime> = c.benchmark_group("contention");
    // Threaded measurements are heavier than the single-shot benches; trim the
    // sample count so a full run stays in the low-seconds range per thread count.
    group.sample_size(30);

    for &n_threads in &thread_counts {
        // Aggregate throughput: report one "element" per thread per criterion
        // iteration, so the throughput line reads as total calls/sec across all
        // workers. Per-thread throughput is this figure divided by n_threads.
        group.throughput(Throughput::Elements(n_threads as u64));

        let pool: WorkerPool = WorkerPool::new(n_threads, cached, contract_id);

        group.bench_with_input(
            BenchmarkId::new("cached", n_threads),
            &n_threads,
            |b, &n_threads| {
                b.iter_custom(|criterion_iters: u64| {
                    // Each criterion "iteration" = one round of n_threads dispatches.
                    // Total dispatch work = criterion_iters * n_threads, split across
                    // the pool so every thread runs `criterion_iters` calls.
                    let total: u64 = criterion_iters * n_threads as u64;
                    pool.run(black_box(total))
                });
            },
        );

        // Drop the pool (join its threads) before building the next pool so
        // thread counts never overlap on the cores.
        drop(pool);
    }

    group.finish();
    // The runtime owns the leaked interface for the process lifetime; keep it
    // alive for the whole bench. Never dropped (the leaked interface is 'static).
    mem::forget(runtime);
}

// ─── criterion_group / criterion_main ────────────────────────────────────────

criterion_group!(benches, bench_contention);
criterion_main!(benches);

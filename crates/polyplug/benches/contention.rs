#![allow(clippy::expect_used)]

// THIS IS A BENCHMARK FILE — do not add #[test] functions here
// Run with: cargo bench -p polyplug --bench contention
//
// Multi-threaded registry-dispatch throughput.
//
// The registry hot paths — `find_guest_contract`, `resolve_guest_contract`, and
// the resolve chain inside `host_call_guest_method`
// (`resolve_single_provider` / `find_all`) — are all `RwLock`-guarded reads on a
// single shared `RuntimeStore`. Every other bench in this crate is
// single-threaded, so none of them would notice a hot-path lock regression
// (a read lock silently becoming a write lock, or a new `Mutex` landing on the
// resolve path). This bench is the regression sentinel for exactly that.
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
use polyplug_abi::CallArena;
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

// ─── Native target + provider (mirrors cross_call.rs) ─────────────────────────

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

/// A `*const HostApi` is not `Send`/`Sync` by default, but the runtime's
/// `HostApi` is `'static` and its registry hot paths are internally synchronized
/// (`RwLock`), so the pointer is safe to share read-only across worker threads
/// for the lifetime of this bench. This newtype carries that guarantee.
#[derive(Clone, Copy)]
struct SharedHost(*const HostApi);

// SAFETY: the pointed-to HostApi is the runtime's 'static table; every field a
// worker touches (find_guest_contract / resolve_guest_contract / call_guest_method)
// routes through RwLock-guarded RuntimeStore reads, so concurrent read access
// across threads is sound. The runtime is kept alive for the whole bench.
unsafe impl Send for SharedHost {}
// SAFETY: see Send — read-only concurrent access to an internally synchronized table.
unsafe impl Sync for SharedHost {}

/// One unit of dispatch work: find the contract handle, resolve it to an
/// interface pointer, then dispatch fn 0 through the runtime — the full
/// per-call hot path a host pays when it does NOT cache the handle. Returns the
/// dispatched sum so the work survives dead-code elimination.
///
/// # Safety
/// `host` must point to the runtime's valid `'static` `HostApi`; `contract_id`
/// must name a registered native provider.
#[inline]
unsafe fn resolve_and_dispatch(host: *const HostApi, contract_id: u64) -> u32 {
    // SAFETY: host is the runtime's valid 'static HostApi for the bench lifetime.
    let api: &HostApi = unsafe { &*host };

    // find → resolve: the RwLock-guarded registry read path.
    // SAFETY: find/resolve are valid extern-C fns backed by the runtime store.
    let handle = unsafe { (api.find_guest_contract)(host, contract_id, 0) };

    let instance: GuestContractInstance = GuestContractInstance {
        data: core::ptr::null_mut(),
        contract_id: GuestContractId::from_u64(contract_id),
    };
    let args: AddArgs = AddArgs {
        a: 42_u32,
        b: 57_u32,
    };
    let mut out: u32 = 0_u32;

    // dispatch through the host-mediated cross-call (count + resolve + native call).
    // Using call_guest_method (not a cached interface pointer) keeps every
    // iteration inside the resolve chain, which is the locked region under test.
    // SAFETY: instance carries the registered contract_id; args/out match bench_add.
    let err: AbiError = unsafe {
        (api.call_guest_method)(
            host,
            instance,
            0_u32,
            &args as *const AddArgs as *const core::ffi::c_void,
            &mut out as *mut u32 as *mut core::ffi::c_void,
            core::ptr::null_mut::<CallArena>(),
        )
    };
    black_box(handle);
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
    /// the shared runtime + contract.
    fn new(n_threads: usize, host: SharedHost, contract_id: u64) -> WorkerPool {
        let mut senders: Vec<mpsc::Sender<WorkBatch>> = Vec::with_capacity(n_threads);
        let mut handles: Vec<thread::JoinHandle<()>> = Vec::with_capacity(n_threads);
        let (done_tx, done_rx): (mpsc::Sender<()>, mpsc::Receiver<()>) = mpsc::channel();

        for _ in 0..n_threads {
            let (work_tx, work_rx): (mpsc::Sender<WorkBatch>, mpsc::Receiver<WorkBatch>) =
                mpsc::channel();
            let done_tx_worker: mpsc::Sender<()> = done_tx.clone();
            let host_for_worker: SharedHost = host;

            let handle: thread::JoinHandle<()> = thread::spawn(move || {
                // Re-bind the whole wrapper inside the closure so the Send capture
                // is `SharedHost` (which is Send), not the disjoint `.0` raw pointer
                // field (which is not) — Rust 2021 closures capture fields directly.
                let worker_host: SharedHost = host_for_worker;
                // Park until a batch arrives; an Err means the pool was dropped.
                while let Ok(batch) = work_rx.recv() {
                    // Synchronize the start so all workers run concurrently.
                    batch.barrier.wait();
                    let mut acc: u32 = 0;
                    for _ in 0..batch.iters {
                        // SAFETY: worker_host wraps the runtime's 'static HostApi
                        // and contract_id names a registered provider (both fixed for
                        // the pool lifetime). Concurrent reads are RwLock-synchronized.
                        acc = acc.wrapping_add(unsafe {
                            resolve_and_dispatch(worker_host.0, contract_id)
                        });
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
/// aggregate resolve-and-dispatch throughput at 1, 2, 4, and 8 threads. A clean
/// read path scales near-linearly; a flattening curve flags a lock regression.
fn bench_contention(c: &mut Criterion) {
    let runtime: Arc<Runtime> = Runtime::builder()
        .build()
        .expect("bare runtime build should succeed");
    let contract_id: u64 = GuestContractId::new("bench.contract", 1_u32).id();
    register_native_provider(&runtime, contract_id, 0x2222_u64);

    let host_abi: &'static HostApi = runtime.host_abi();
    let shared: SharedHost = SharedHost(host_abi as *const HostApi);

    let thread_counts: [usize; 4] = [1, 2, 4, 8];

    let mut group: criterion::BenchmarkGroup<'_, criterion::measurement::WallTime> =
        c.benchmark_group("contention");
    // Threaded measurements are heavier than the single-shot benches; trim the
    // sample count so a full run stays in the low-seconds range per thread count.
    group.sample_size(30);

    for &n_threads in &thread_counts {
        // Aggregate throughput: report one "element" per thread per criterion
        // iteration, so the throughput line reads as total calls/sec across all
        // workers. Per-thread throughput is this figure divided by n_threads.
        group.throughput(Throughput::Elements(n_threads as u64));

        let pool: WorkerPool = WorkerPool::new(n_threads, shared, contract_id);

        group.bench_with_input(
            BenchmarkId::new("threads", n_threads),
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

        // Drop the pool (join its threads) before building the next, larger pool
        // so thread counts never overlap on the cores.
        drop(pool);
    }

    group.finish();
    // The runtime owns the leaked interface for the process lifetime; keep it
    // alive for the whole bench. Never dropped (the leaked interface is 'static).
    core::mem::forget(runtime);
}

// ─── criterion_group / criterion_main ────────────────────────────────────────

criterion_group!(benches, bench_contention);
criterion_main!(benches);

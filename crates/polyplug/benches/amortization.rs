#![allow(clippy::expect_used)]

// THIS IS A BENCHMARK FILE — do not add #[test] functions here
// Run with: cargo bench -p polyplug --bench amortization
//
// ─── What this measures ──────────────────────────────────────────────────────
//
// The *one-time* costs a host pays around the steady-state dispatch hot path,
// and where they amortize:
//
//   1. load   — `Runtime::load_bundle`: dlopen the cdylib, check the ABI
//               version, run `polyplug_init`, register the contract. Paid once
//               per bundle, at startup.
//   2. resolve — `find_guest_contract` + `resolve_guest_contract`: handle lookup
//               + interface-pointer return. Paid once per contract; the
//               recommended pattern is to cache the resolved pointer and reuse
//               it (see `counter_inc`), so this is *not* a per-call cost.
//   3. reload  — `Runtime::reload_bundle` (native-only hot-reload): dlopen the
//               new dylib, re-run `polyplug_init`, atomically swap the interface,
//               and epoch-reclaim the old library. A capability static linking
//               cannot offer at all.
//
// Honesty notes:
//   * These are one-time costs. A critic rightly points out they are irrelevant
//     to steady-state throughput. True — the value here is the *amortization
//     curve* (load cost / N calls → ~0 as N grows) and the existence of cheap
//     hot-reload, not the raw nanoseconds. The README works the curve.
//   * `load`/`reload` re-`dlopen` the *same* file every iteration. The first
//     dlopen in the process pays the cold page-in; subsequent ones are
//     refcounted by the loader and warm. So these numbers are the *warm* load
//     cost, not a cold first-ever start. A real "reload after the file changed
//     on disk" would also pay the cold mmap once.
//   * This bench cannot depend on `polyplug_native` — that crate depends on
//     `polyplug`, so a dev-dependency would be a cycle. It reuses the
//     `TestNativeLoader` the integration tests already use (dlopen + ABI check +
//     `polyplug_init` + epoch-deferred reclaim on unload), wired in through the
//     shared test module below.

use core::hint::black_box;
use std::path::Path;
use std::sync::Arc;

use criterion::BatchSize;
use criterion::Criterion;
use criterion::criterion_group;
use criterion::criterion_main;

use polyplug::Runtime;
use polyplug_abi::GuestContractHandle;
use polyplug_abi::GuestContractInterface;
use polyplug_abi::runtime::RuntimeConfig;
use polyplug_utils::GuestContractId;

use criterion::BenchmarkGroup;
use criterion::measurement::WallTime;

// The integration tests' native loader. `polyplug` is loader-agnostic, so a
// native bundle needs a registered loader; this mirrors `NativeLoader` using
// only `polyplug`'s public surface (see the module's own doc comment).
#[path = "../tests/common/mod.rs"]
mod common;

use common::TestNativeLoader;

/// Directory containing `manifest.toml` + the native test_plugin cdylib.
const TEST_PLUGIN_DIR: &str = env!("TEST_PLUGIN_DIR");
/// Reload fixture v1 (`reload.test` contract, version fn returns 100).
const RELOAD_PLUGIN_V1_DIR: &str = env!("RELOAD_PLUGIN_V1_DIR");
/// Reload fixture v2 (same contract, version fn returns 200).
const RELOAD_PLUGIN_V2_DIR: &str = env!("RELOAD_PLUGIN_V2_DIR");

/// Build a fresh runtime with a native loader registered.
fn fresh_runtime() -> Arc<Runtime> {
    Runtime::builder()
        .loader(TestNativeLoader::new())
        .build()
        .expect("runtime build must succeed")
}

/// Build a fresh runtime with hot-reload enabled and a native loader registered.
fn fresh_hot_reload_runtime() -> Arc<Runtime> {
    Runtime::builder()
        .config(RuntimeConfig {
            hot_reload_enabled: true,
            ..RuntimeConfig::default()
        })
        .loader(TestNativeLoader::new())
        .build()
        .expect("hot-reload runtime build must succeed")
}

fn bench_amortization(c: &mut Criterion) {
    let mut group: BenchmarkGroup<'_, WallTime> = c.benchmark_group("amortization");

    // ── load: one-time cost of bringing a bundle online ───────────────────────
    //
    // A fresh runtime per iteration (built in untimed setup) so each measured
    // call is a real load into an empty registry, not a no-op re-load. The
    // runtime is returned from the routine so criterion drops it untimed.
    group.bench_function("load_bundle", |b| {
        b.iter_batched(
            fresh_runtime,
            |runtime: Arc<Runtime>| {
                runtime
                    .load_bundle(black_box(Path::new(TEST_PLUGIN_DIR)))
                    .expect("load_bundle must succeed");
                runtime
            },
            BatchSize::SmallInput,
        );
    });

    // ── resolve: find + resolve on a pre-loaded runtime (the cached-handle path)
    let resolved_runtime: Arc<Runtime> = fresh_runtime();
    resolved_runtime
        .load_bundle(Path::new(TEST_PLUGIN_DIR))
        .expect("preload for resolve bench");
    let contract_id: u64 = GuestContractId::new("test.add", 1).id();

    group.bench_function("find_and_resolve", |b| {
        b.iter(|| {
            let handle: GuestContractHandle = resolved_runtime
                .find_guest_contract(black_box(contract_id), 0)
                .expect("find_guest_contract must succeed");
            let interface: *const GuestContractInterface = resolved_runtime
                .resolve_guest_contract(handle)
                .expect("resolve_guest_contract must succeed");
            black_box(interface)
        });
    });

    // ── reload: native hot-reload swap (a capability static linking lacks) ─────
    //
    // A runtime preloaded with v1; each iteration reloads v2 over it (dlopen +
    // init + atomic interface swap + epoch-reclaim the old library). Real work every
    // time — reload is never idempotent.
    let reload_runtime: Arc<Runtime> = fresh_hot_reload_runtime();
    reload_runtime
        .load_bundle(Path::new(RELOAD_PLUGIN_V1_DIR))
        .expect("preload v1 for reload bench");

    group.bench_function("hot_reload_swap", |b| {
        b.iter(|| {
            reload_runtime
                .reload_bundle(black_box(Path::new(RELOAD_PLUGIN_V2_DIR)))
                .expect("reload_bundle must succeed");
        });
    });

    group.finish();
}

criterion_group!(benches, bench_amortization);
criterion_main!(benches);

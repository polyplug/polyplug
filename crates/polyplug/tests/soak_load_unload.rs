#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

//! Load → dispatch → unload SOAK harness (task #35, Bench Phase 4).
//!
//! ─── What this proves ────────────────────────────────────────────────────────
//!
//! polyplug **truly unloads** bundles: unloading or hot-reloading a bundle hands the
//! superseded interface + library/VM to crossbeam-epoch, which frees them once no
//! reader is still pinned in the prior epoch (.NET goes via the collectible-ALC reclaim
//! path, #68). RSS therefore returns to a flat baseline rather than climbing within a
//! long-lived runtime. This soak additionally builds a fresh `Runtime` each cycle and
//! drops it fully, so a non-flat RSS line is an unambiguous leak signal that also covers
//! the runtime lifecycle itself.
//!
//! So this soak measures the **reclaim/teardown** path, which is the honest
//! leak test: every cycle builds a FRESH `Runtime`, loads a native bundle,
//! dispatches its contract a few times, unloads it, then DROPS the whole runtime
//! (which drops the loader, which `dlclose`s every library it held). Because each
//! cycle is fully torn down, process RSS must return to a **flat steady-state**
//! after warmup. If RSS grew unbounded under full-teardown cycling, that would be
//! a REAL leak in core — and this harness would surface it.
//!
//! ─── Two outputs ─────────────────────────────────────────────────────────────
//!
//! 1. churn throughput — completed load→dispatch→unload→drop cycles per second.
//! 2. RSS-over-time — process resident set size (KiB), sampled every K cycles,
//!    written as a `<cycle> <rss_kib>` series for `scripts/gen_bench_charts.py`
//!    to render as a chart.
//!
//! ─── How it is gated ─────────────────────────────────────────────────────────
//!
//! This is NOT a criterion microbench — criterion measures call latency, not
//! memory over time, and it would re-run the body an unbounded number of times of
//! its own choosing. The repo's convention for long opt-in runs is an env-gated
//! loop (`POLYPLUG_BENCH_ITERS`, see `examples/hosts/roundtrip_bench.sh`). This
//! harness follows the same convention with `POLYPLUG_SOAK_ITERS`:
//!
//! * UNSET → `DEFAULT_SOAK_ITERS` (tiny) so a normal `cargo test` is a fast
//!   no-op-sized smoke run that still exercises the full path.
//! * SET → that many cycles (set it to thousands to observe steady-state).
//!
//! Sampling stride is `POLYPLUG_SOAK_SAMPLE_EVERY` (default `DEFAULT_SAMPLE_EVERY`).
//! When `POLYPLUG_SOAK_OUT` is set, the RSS series is also written there for the
//! chart generator; otherwise it is only printed.
//!
//! Run a real soak:
//!
//! ```bash
//! cargo build --release -p polyplug --tests
//! POLYPLUG_SOAK_ITERS=20000 POLYPLUG_SOAK_SAMPLE_EVERY=500 \
//!   POLYPLUG_SOAK_OUT=$PWD/target/soak/soak_rss.txt \
//!   cargo test --release -p polyplug --test soak_load_unload -- --nocapture --exact soak_load_unload_churn
//! ```

use core::ffi::c_void;
use core::hint::black_box;
use core::mem::transmute;
use core::time::Duration;
use std::env::var;
use std::fs::{create_dir_all, read_to_string, write};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use polyplug::Runtime;
use polyplug_abi::{
    AbiError, AbiErrorCode, DispatchType, GuestContractHandle, GuestContractInstance,
    GuestContractInterface,
};
use polyplug_utils::{BundleId, GuestContractId};

#[path = "common/mod.rs"]
mod common;

use common::TestNativeLoader;

/// Directory containing `manifest.toml` + the native test_plugin cdylib.
const TEST_PLUGIN_DIR: &str = env!("TEST_PLUGIN_DIR");

/// The bundle name declared in `test_plugin_dir/manifest.toml`.
const TEST_BUNDLE_NAME: &str = "test_plugin";

/// Cycles run when `POLYPLUG_SOAK_ITERS` is unset — small enough that a normal
/// `cargo test` run pays only a handful of milliseconds, while still exercising
/// the full load→dispatch→unload→drop path so the harness itself stays tested.
const DEFAULT_SOAK_ITERS: u64 = 8;

/// Default RSS sampling stride (sample every N cycles) when
/// `POLYPLUG_SOAK_SAMPLE_EVERY` is unset.
const DEFAULT_SAMPLE_EVERY: u64 = 1;

/// Number of dispatch calls per cycle. A few calls per load are enough to
/// exercise the resolve + native-dispatch path without dominating the churn
/// (the point of the soak is load/unload turnover, not dispatch throughput).
const DISPATCHES_PER_CYCLE: u32 = 4;

/// Native dispatch signature for the frozen out-param ABI (`GuestContractInstance`,
/// `*const args`, `*mut out`, `*mut AbiError` out-param; returns void). Matches the
/// integration-dispatch tests.
type NativeDispatchFn =
    unsafe extern "C" fn(*mut c_void, GuestContractInstance, *const (), *mut (), *mut AbiError);

/// `test.add` argument struct (matches `libtest_plugin`'s `add` contract).
#[repr(C)]
struct AddArgs {
    a: u32,
    b: u32,
}

/// Read the current process resident set size in KiB from `/proc/self/status`.
///
/// The `VmRSS:` line reports resident memory directly in KiB, so no page-size
/// conversion (and no `libc` dependency) is needed. Returns `None` on non-Linux
/// or if the proc file is unreadable, so the soak still runs (churn-only) where
/// RSS sampling is unavailable.
fn current_rss_kib() -> Option<u64> {
    let status: String = read_to_string("/proc/self/status").ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            // Format: `VmRSS:\t   5380 kB`
            return rest.split_whitespace().next()?.parse::<u64>().ok();
        }
    }
    None
}

/// Parse a positive-integer environment variable, falling back to `default`.
fn env_u64(name: &str, default: u64) -> u64 {
    match var(name) {
        Ok(raw) => raw.trim().parse::<u64>().unwrap_or(default).max(1),
        Err(_) => default,
    }
}

/// Resolve `test.add` on `runtime` and return its native dispatch fn pointer.
fn resolve_add_dispatch(runtime: &Runtime) -> (NativeDispatchFn, *mut c_void) {
    let contract_id: u64 = GuestContractId::new("test.add", 1).id();
    let handle: GuestContractHandle = runtime
        .find_guest_contract(contract_id, 0)
        .expect("test.add must be registered after load");
    let interface_ptr: *const GuestContractInterface = runtime
        .resolve_guest_contract(handle)
        .expect("handle must resolve");

    // SAFETY: interface_ptr is non-null and points to the registered native
    // interface, kept alive by the loaded library for this cycle's runtime.
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };
    assert_eq!(
        interface.dispatch_type,
        DispatchType::Native,
        "test_plugin is a native bundle"
    );

    // SAFETY: the registered `add` function pointer occupies slot 0 of the
    // native function table.
    let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(0) };
    // SAFETY: `add` was registered with the frozen native dispatch signature.
    (
        unsafe { transmute::<*const (), NativeDispatchFn>(fn_ptr) },
        interface.adapter_context,
    )
}

/// One full churn cycle: fresh runtime, load, dispatch a few times, unload,
/// then drop the runtime (which drops the loader → `dlclose` every library).
fn run_one_cycle() {
    let runtime: Arc<Runtime> = Runtime::builder()
        .loader(TestNativeLoader::new())
        .build()
        .expect("runtime build must succeed");

    runtime
        .load_bundle(Path::new(TEST_PLUGIN_DIR))
        .expect("load_bundle must succeed");

    let (dispatch_fn, adapter_context): (NativeDispatchFn, *mut c_void) =
        resolve_add_dispatch(&runtime);
    for i in 0..DISPATCHES_PER_CYCLE {
        let args: AddArgs = AddArgs { a: i, b: 1 };
        let mut out: u32 = u32::MAX;
        let mut rc: AbiError = AbiError::ok();
        // SAFETY: args/out are valid and correctly typed; null stateless instance,
        // exactly as the native dispatch ABI expects for a stateless contract.
        unsafe {
            dispatch_fn(
                adapter_context,
                GuestContractInstance::null(),
                &args as *const AddArgs as *const (),
                &mut out as *mut u32 as *mut (),
                &mut rc as *mut AbiError,
            )
        };
        assert_eq!(rc.code, AbiErrorCode::Ok as u32, "dispatch must succeed");
        assert_eq!(out, i.wrapping_add(1), "add(i, 1) must equal i + 1");
        black_box(out);
    }

    runtime
        .unload_bundle(BundleId::new(TEST_BUNDLE_NAME))
        .expect("unload_bundle must succeed");

    // Drop the runtime fully: this drops the TestNativeLoader, whose `Library`
    // handles `dlclose` on drop, reclaiming the dylib mapping. After this point
    // nothing from this cycle remains mapped — which is exactly what keeps RSS
    // flat across cycles and is the leak signal the soak watches.
    drop(runtime);
}

#[test]
fn soak_load_unload_churn() {
    let iters: u64 = env_u64("POLYPLUG_SOAK_ITERS", DEFAULT_SOAK_ITERS);
    let sample_every: u64 = env_u64("POLYPLUG_SOAK_SAMPLE_EVERY", DEFAULT_SAMPLE_EVERY);
    let out_path: Option<String> = var("POLYPLUG_SOAK_OUT").ok();

    // (cycle_index, rss_kib) samples — populated only when RSS is readable.
    let mut series: Vec<(u64, u64)> = Vec::new();
    if let Some(rss) = current_rss_kib() {
        series.push((0, rss));
    }

    let start: Instant = Instant::now();
    for cycle in 1..=iters {
        run_one_cycle();
        if cycle % sample_every == 0 {
            if let Some(rss) = current_rss_kib() {
                series.push((cycle, rss));
            }
        }
    }
    let elapsed: Duration = start.elapsed();

    let cycles_per_sec: f64 = iters as f64 / elapsed.as_secs_f64();

    println!("[soak] cycles={iters} elapsed={elapsed:?} churn={cycles_per_sec:.0} cycles/sec");
    if let Some(rss) = current_rss_kib() {
        println!("[soak] final RSS = {rss} KiB");
    }

    // Print the RSS series so a `--nocapture` run is self-documenting, and emit a
    // steady-state verdict so the operator does not have to eyeball it. The
    // verdict compares the post-warmup tail against the warmup baseline.
    if !series.is_empty() {
        let first: (u64, u64) = series[0];
        let mid: (u64, u64) = series[series.len() / 2];
        let last: (u64, u64) = series[series.len() - 1];
        println!(
            "[soak] RSS series (KiB): first=({},{}) mid=({},{}) last=({},{})  samples={}",
            first.0,
            first.1,
            mid.0,
            mid.1,
            last.0,
            last.1,
            series.len()
        );

        // Steady-state heuristic: compare the last-quartile mean to the
        // second-quartile mean (skip the first quartile as warmup/allocator
        // arena growth). A leak under full teardown shows monotonic growth, so
        // the tail mean would keep rising above the mid mean by a wide margin.
        let q: usize = (series.len() / 4).max(1);
        let mid_slice: &[(u64, u64)] = &series[q..(2 * q).min(series.len())];
        let tail_slice: &[(u64, u64)] = &series[series.len().saturating_sub(q)..];
        let mean = |s: &[(u64, u64)]| -> f64 {
            if s.is_empty() {
                0.0
            } else {
                s.iter().map(|(_, r)| *r as f64).sum::<f64>() / s.len() as f64
            }
        };
        let mid_mean: f64 = mean(mid_slice);
        let tail_mean: f64 = mean(tail_slice);
        let growth_pct: f64 = if mid_mean > 0.0 {
            (tail_mean - mid_mean) / mid_mean * 100.0
        } else {
            0.0
        };
        println!(
            "[soak] steady-state: mid_mean={mid_mean:.0} KiB tail_mean={tail_mean:.0} KiB drift={growth_pct:+.1}%"
        );
    }

    if let Some(path) = out_path {
        if let Some(parent) = Path::new(&path).parent() {
            let _ = create_dir_all(parent);
        }
        let mut body: String =
            String::from("# cycle rss_kib — polyplug load/unload soak (RSS over time)\n");
        for (cycle, rss) in &series {
            body.push_str(&format!("{cycle} {rss}\n"));
        }
        write(&path, body).expect("write soak RSS series");
        println!("[soak] wrote RSS series to {path}");
    }
}

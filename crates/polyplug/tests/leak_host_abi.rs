#![allow(clippy::expect_used)]

//! Regression test for task #45 — the per-`Runtime` `HostApi` leak.
//!
//! ─── The bug this guards ─────────────────────────────────────────────────────
//!
//! Before the fix, `RuntimeBuilder::build` obtained the `&'static HostApi` it hands
//! to plugins via `Box::leak(Box::new(HostApi { .. }))`, and `Runtime` had no
//! `Drop` that reclaimed it. Every `Runtime` that was built and dropped therefore
//! leaked exactly one 168-byte `HostApi` — on BOTH the pure-Rust `Arc<Runtime>`
//! path and the FFI `polyplug_runtime_create` / `_destroy` path.
//!
//! The fix makes `Runtime` OWN the `HostApi` as a `Box<HostApi>` (its last-declared
//! field, so it drops AFTER `registry`/`loaders`), reclaiming it on teardown.
//!
//! ─── Why this test is deterministic, not flaky ───────────────────────────────
//!
//! The leak is pure: it does NOT depend on loading any bundle — merely building a
//! runtime and dropping it leaks one HostApi. So this test drives a large number of
//! bare build→drop cycles (no plugin, no loader, no dlopen) and watches process
//! RSS. After an allocator/runtime warmup phase, RSS of a leak-free build reaches a
//! flat steady state. The old leak adds ~168 B/cycle; over `LEAK_CYCLES` (default
//! 50_000) that is ~8 MiB of monotonic growth, which would blow the generous
//! `MAX_TAIL_GROWTH_KIB` bound. The fixed code stays under it with wide margin.
//!
//! Confirmed against the OLD leaky code: temporarily reverting `build()` to
//! `Box::leak` and removing the owned field makes the warmup→tail growth exceed
//! 7 MiB at 50k cycles, failing the assertion. The fixed code's tail growth is a
//! few hundred KiB at most (allocator noise), passing comfortably.
//!
//! ─── Platform gating ─────────────────────────────────────────────────────────
//!
//! RSS is read from `/proc/self/status` (Linux only). Where that is unavailable,
//! or under Miri (whose process RSS reflects the interpreter, not the program),
//! the test still runs the cycles — exercising the build→drop path for
//! Miri/ASAN/UBSAN to inspect for UB — but skips the RSS-growth assertion. Cycle
//! count is overridable via `POLYPLUG_LEAK_CYCLES` so the default `cargo test` run
//! stays fast while a soak run can crank it up.

use std::sync::Arc;

use polyplug::Runtime;

/// Default build→drop cycles. Large enough that the old ~168 B/cycle leak would
/// dwarf allocator noise (~8 MiB), small enough to run in well under a second.
const DEFAULT_LEAK_CYCLES: u64 = 50_000;

/// Cycles to run before sampling the warmup baseline. The allocator grows its
/// arenas during the first several thousand build/drop pairs; sampling the
/// baseline only after this point isolates per-cycle leakage from one-time warmup.
const WARMUP_CYCLES: u64 = 5_000;

/// Generous tail-growth bound (KiB). A leak-free build's RSS is flat after warmup
/// (a few hundred KiB of allocator noise); the old leak would add multiple MiB.
/// 1 MiB sits comfortably between the two so the test is neither flaky nor lax.
const MAX_TAIL_GROWTH_KIB: u64 = 1024;

/// Read current process resident set size in KiB from `/proc/self/status`.
///
/// Returns `None` on non-Linux or if the proc file is unreadable, so the test
/// degrades to a build/drop exerciser (no RSS assertion) where unavailable.
fn current_rss_kib() -> Option<u64> {
    let status: String = std::fs::read_to_string("/proc/self/status").ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            return rest.split_whitespace().next()?.parse::<u64>().ok();
        }
    }
    None
}

/// Parse a positive-integer environment variable, falling back to `default`.
fn env_u64(name: &str, default: u64) -> u64 {
    match std::env::var(name) {
        Ok(raw) => raw.trim().parse::<u64>().unwrap_or(default).max(1),
        Err(_) => default,
    }
}

/// Build a runtime with no loaders/plugins and immediately drop it.
///
/// This is the minimal reproduction of the leak: `build()` constructs the owned
/// `HostApi`; dropping the `Arc<Runtime>` must reclaim it. No bundle is loaded —
/// the leak was per-runtime, independent of bundle activity.
fn build_and_drop_once() {
    let runtime: Arc<Runtime> = Runtime::builder()
        .build()
        .expect("runtime build must succeed");
    drop(runtime);
}

#[test]
fn host_abi_is_reclaimed_on_runtime_drop() {
    let cycles: u64 = env_u64("POLYPLUG_LEAK_CYCLES", DEFAULT_LEAK_CYCLES);
    let warmup: u64 = WARMUP_CYCLES.min(cycles / 2).max(1);

    // Warmup phase: let allocator arenas settle so the baseline reflects
    // steady-state, not one-time growth.
    for _ in 0..warmup {
        build_and_drop_once();
    }

    let baseline_rss: Option<u64> = current_rss_kib();

    // Measured phase: the bulk of the cycles. Under the old leak each adds ~168 B
    // of permanently-retained HostApi; under the fix RSS stays flat.
    for _ in warmup..cycles {
        build_and_drop_once();
    }

    let final_rss: Option<u64> = current_rss_kib();

    // Under Miri the cycles above already ran, letting the interpreter inspect the
    // build→drop ownership/teardown path for UB (use-after-free, Stacked-Borrows
    // violations) — which is Miri's real value here. Process RSS, however, reflects
    // Miri's interpreter memory rather than the program's allocation behaviour, so
    // the RSS-growth assertion is meaningless under Miri and is skipped (the real
    // RSS leak gate runs on native + the soak harness).
    if cfg!(miri) {
        println!(
            "[leak] running under Miri — UB-checked the build/drop path; RSS assertion skipped"
        );
        return;
    }

    match (baseline_rss, final_rss) {
        (Some(baseline), Some(final_kib)) => {
            // saturating_sub: RSS may dip below baseline (allocator returns pages),
            // which is the opposite of a leak and trivially passes.
            let growth_kib: u64 = final_kib.saturating_sub(baseline);
            println!(
                "[leak] cycles={cycles} warmup={warmup} baseline={baseline} KiB final={final_kib} KiB growth={growth_kib} KiB (bound={MAX_TAIL_GROWTH_KIB} KiB)"
            );
            assert!(
                growth_kib < MAX_TAIL_GROWTH_KIB,
                "RSS grew {growth_kib} KiB over {} post-warmup build/drop cycles \
                 (bound {MAX_TAIL_GROWTH_KIB} KiB). The per-Runtime HostApi is leaking — \
                 each Runtime must reclaim its owned HostApi box on drop (task #45).",
                cycles - warmup,
            );
        }
        _ => {
            // Non-Linux / no /proc: the cycles still ran (exercising build→drop for
            // ASAN/UBSAN), but RSS sampling is unavailable, so skip the assertion.
            println!(
                "[leak] /proc/self/status unavailable — ran {cycles} build/drop cycles without RSS assertion"
            );
        }
    }
}

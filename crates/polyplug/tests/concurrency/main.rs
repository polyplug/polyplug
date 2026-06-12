#![allow(clippy::expect_used)]

//! # polyplug concurrency & parallelism test suite
//!
//! A single, organized home for every test that exercises polyplug under
//! concurrent or parallel load. The suite exists because the historical
//! concurrent-reload flake (a stale pre-reload slot snapshot racing
//! `apply_reload_swap`) passed 25/25 in isolation and only failed under
//! full-suite parallel load — proving that scattered, non-deterministic stress
//! tests are insufficient on their own. This suite therefore pairs
//! **deterministic interleaving probes** (where a callback bracket or a counter
//! makes the corrupting interleave observable) with **high-iteration stress
//! tests** (run under the nightly TSAN/ASAN/Miri harness, which catches the FFI
//! races loom cannot model because it cannot intercept `dlopen`/crossbeam).
//!
//! The lock-free core it guards: registry reads pin a `crossbeam-epoch` guard
//! and serve from an immutable published `ReadView`; reload/unload republish that
//! view atomically and hand the superseded interface `Arc` + library/VM to
//! epoch-deferred reclamation, freed only once no reader is still pinned in the
//! prior epoch. The standalone protocol model behind this guarantee is
//! loom-checked in the `loom_epoch_model` crate; this suite verifies the real
//! `RuntimeStore` / `Runtime` paths that implement it.
//!
//! ## Surfaces covered (one module per concurrent surface)
//!
//! | Module          | Surface |
//! |-----------------|---------|
//! | [`dispatch`]    | Concurrent dispatch (`resolve` + call through function pointers) while a reload/unload swaps the slot in flight — resolved pointers stay valid under an epoch pin or fail cleanly, never UAF; no torn interface reads. |
//! | [`reload`]      | Concurrent reload of the same bundle — post-quiesce resolvability invariant; reload critical sections are mutually exclusive (deterministic probe); rapid alternating reload cycles; per-cycle reload callbacks. |
//! | [`registry`]    | Concurrent `register_guest_contract` / `register_host_contract` / `register_loader` from many threads (`DuplicateProvider` correctness under races); concurrent `find` / `find_all` / `resolve` / `get_dependencies` during load (init-stack counter correctness). |
//! | [`load_unload`] | Concurrent load + unload of the same and different bundles — retire/reclaim races, exactly-once `destroy_instance`, no use-after-free on retired interfaces. |
//! | [`multi_runtime`] | Multi-runtime parallelism — N runtimes built, used, and destroyed across threads simultaneously, asserting full Rule-12 isolation (no cross-runtime state bleed) and no leak (bounded RSS). |
//! | [`logger`]      | Concurrent `host->log` delivery from many threads into one `LoggerHandle` funnel — no lost or torn records, no deadlock. |
//!
//! Run the whole suite:    `cargo test -p polyplug --test concurrency`
//! Under ThreadSanitizer:  see `.github/workflows/nightly.yml` (the `concurrency`
//! target is the TSAN/ASAN job's race oracle).

#[path = "../common/mod.rs"]
mod common;

#[macro_use]
mod fixtures;

mod dispatch;
mod load_unload;
mod logger;
mod multi_runtime;
mod registry;
mod reload;

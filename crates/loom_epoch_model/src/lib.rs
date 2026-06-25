#![cfg(loom)]
//! Loom model of polyplug's epoch publish/reclaim protocol.
//!
//! # What this verifies
//!
//! `RuntimeStore` serves reads lock-free: a reader pins a `crossbeam_epoch`
//! guard, atomically loads the published `ReadView` pointer, and dereferences it
//! without taking a lock (see `runtime_store.rs::find_guest_contract` and
//! friends). Writers republish a fresh view and `defer_destroy` the superseded
//! one; loaders `schedule_reclaim` the superseded dylib/VM through the same
//! epoch domain. The whole design rests on ONE guarantee:
//!
//! > A value handed to deferred reclamation is freed only after every guard
//! > pinned at defer time has been dropped.
//!
//! This crate models exactly that protocol — published `AtomicPtr` + a guard /
//! defer / reclaim collector — and exhaustively checks, with loom, that:
//!
//! 1. A reader that holds its guard across the dereference (`reader_pinned`)
//!    never observes a reclaimed slot, under every interleaving.
//! 2. A reader that drops its guard before the dereference (`reader_unpinned`)
//!    — the exact use-after-free fixed in commit 949d10ec ("pin the epoch across
//!    resolve→deref") — DOES race reclamation, and loom finds it.
//!
//! Test 2 is what makes Test 1 meaningful: it proves the checker has teeth and
//! that pinning across the dereference is necessary, not incidental.
//!
//! # Scope / honesty
//!
//! This models the safety *contract* that `crossbeam-epoch` provides and that
//! polyplug depends on. It does NOT re-verify `crossbeam-epoch`'s *implementation*
//! of that contract (its 3-epoch advance/pin/defer machinery) — crossbeam
//! loom-checks that itself, upstream. crossbeam-epoch cannot be driven by loom
//! from a downstream crate without rebuilding the whole crossbeam stack under its
//! internal, undocumented `--cfg crossbeam_loom`, so re-deriving its internals
//! here would be both redundant and brittle.
//!
//! # Running
//!
//! The crate is empty unless built with `--cfg loom`, so a normal workspace
//! build never compiles loom in. Run the model with:
//!
//! ```text
//! RUSTFLAGS="--cfg loom" cargo test -p loom_epoch_model --release
//! ```
//!
//! The model is a test harness, so its entire body lives in the `tests` module:
//! under `--cfg loom` without `--test` the library is empty (no dead-code noise),
//! and the test build wires every piece together.
#![allow(clippy::expect_used)] // model/test code: lock poisoning is surfaced via expect

#[cfg(test)]
mod tests {
    use core::ptr;

    use loom::sync::Arc;
    use loom::sync::Mutex;
    use loom::sync::MutexGuard;
    use loom::sync::atomic::AtomicBool;
    use loom::sync::atomic::AtomicPtr;
    use loom::sync::atomic::Ordering;
    use loom::thread;
    use std::panic::{self, PanicHookInfo};
    use std::thread::Result as ThreadResult;

    /// One published snapshot in the model (the stand-in for a `ReadView`).
    ///
    /// `alive` represents whether the slot's backing storage has been
    /// epoch-reclaimed: `true` = still valid, `false` = logically freed. A reader
    /// asserts `alive` after dereferencing the raw published pointer — the model's
    /// stand-in for "a lock-free reader must never touch reclaimed memory".
    struct Slot {
        alive: AtomicBool,
    }

    /// Epoch-collector bookkeeping.
    ///
    /// Models the single guarantee `crossbeam-epoch` provides: a slot handed to
    /// `defer_free` is reclaimed only once every guard pinned at defer time has
    /// been dropped.
    struct Collector {
        /// Monotonic guard-id source.
        next_guard: u64,
        /// Ids of guards currently pinned.
        active: Vec<u64>,
        /// Slots awaiting reclamation, each paired with the guard set that was
        /// active when it was deferred. The slot is reclaimed once none of those
        /// guards remain active.
        deferred: Vec<(*const Slot, Vec<u64>)>,
    }

    /// Shared world: the published pointer, the collector, and the backing storage.
    struct World {
        /// The lock-free-readable published snapshot pointer (the `published`
        /// Atomic).
        published: AtomicPtr<Slot>,
        /// Epoch bookkeeping, behind a lock — off the read path, exactly as
        /// crossbeam's internals are.
        collector: Mutex<Collector>,
        /// Backing storage for every slot, inline in the `Arc`-held `World` so it
        /// keeps a stable address for the whole model run. Index 0 is the initial
        /// ("old") published view; index 1 is the reload's replacement ("new")
        /// view. Memory is never actually freed — "reclamation" flips `Slot::alive`
        /// instead — so a stale raw pointer is never a *real* dangling pointer,
        /// which would make the model itself UB and mask the property under test.
        slots: [Slot; 2],
    }

    // SAFETY: `World`'s only non-`Send`/`Sync` member is the raw `*const Slot`
    // stored in `Collector::deferred`. Every such pointer aliases an element of the
    // inline `World::slots` array, which lives for the entire `loom::model` run, and
    // is only ever used to load/store the slot's `AtomicBool` (a `Sync` type). No
    // thread frees or non-atomically mutates the pointee, so sharing `&World`
    // across threads is sound.
    unsafe impl Send for World {}
    // SAFETY: see the `Send` impl above — the same retention and atomic-only-access
    // argument makes shared `&World` access sound.
    unsafe impl Sync for World {}

    impl World {
        /// Build a fresh world with `slots[0]` published.
        ///
        /// The published pointer is stored only AFTER the `World` is inside the
        /// `Arc`, so it points into the stable heap allocation — not at a local
        /// about to be moved.
        fn new() -> Arc<World> {
            let world: Arc<World> = Arc::new(World {
                published: AtomicPtr::new(ptr::null_mut()),
                collector: Mutex::new(Collector {
                    next_guard: 0,
                    active: Vec::new(),
                    deferred: Vec::new(),
                }),
                slots: [
                    Slot {
                        alive: AtomicBool::new(true),
                    },
                    Slot {
                        alive: AtomicBool::new(true),
                    },
                ],
            });
            let old_ptr: *mut Slot = world.slot_ptr(0);
            world.published.store(old_ptr, Ordering::Release);
            world
        }

        /// Raw pointer to the backing slot at `idx` (stable for the model run).
        fn slot_ptr(&self, idx: usize) -> *mut Slot {
            &self.slots[idx] as *const Slot as *mut Slot
        }

        /// Pin the epoch: register an active guard and return its id.
        fn pin(&self) -> u64 {
            let mut collector: MutexGuard<'_, Collector> =
                self.collector.lock().expect("collector poisoned");
            let id: u64 = collector.next_guard;
            collector.next_guard += 1;
            collector.active.push(id);
            id
        }

        /// Unpin guard `id`, then reclaim anything whose defer-time guard set
        /// drained.
        fn unpin(&self, id: u64) {
            let mut collector: MutexGuard<'_, Collector> =
                self.collector.lock().expect("collector poisoned");
            collector.active.retain(|&g| g != id);
            World::reclaim(&mut collector);
        }

        /// Publish `new_ptr` and defer-free the superseded pointer (the `publish` +
        /// `defer_destroy` pair).
        fn publish_swap(&self, new_ptr: *mut Slot) {
            let old_ptr: *mut Slot = self.published.swap(new_ptr, Ordering::AcqRel);
            let mut collector: MutexGuard<'_, Collector> =
                self.collector.lock().expect("collector poisoned");
            let snapshot: Vec<u64> = collector.active.clone();
            collector.deferred.push((old_ptr as *const Slot, snapshot));
            World::reclaim(&mut collector);
        }

        /// Reclaim every deferred slot whose defer-time guard set no longer
        /// intersects the active set. Reclamation = flip `alive` to `false`
        /// (logical free).
        fn reclaim(collector: &mut Collector) {
            let mut i: usize = 0;
            while i < collector.deferred.len() {
                let still_pinned: bool = collector.deferred[i]
                    .1
                    .iter()
                    .any(|g| collector.active.contains(g));
                if still_pinned {
                    i += 1;
                } else {
                    let (ptr, _guards): (*const Slot, Vec<u64>) = collector.deferred.remove(i);
                    // SAFETY: `ptr` aliases a `Box<Slot>` in `World::slots`, retained
                    // for the whole model run, so the dereference targets live memory.
                    // Flipping `alive` is the model's stand-in for freeing storage.
                    unsafe { (*ptr).alive.store(false, Ordering::Release) };
                }
            }
        }
    }

    /// Pinned reader: pin → load → deref+check → unpin.
    ///
    /// The guard spans the dereference, so a concurrent `publish_swap` cannot
    /// reclaim the slot before it is read. No interleaving may observe a reclaimed
    /// slot.
    fn reader_pinned(world: &World) {
        let id: u64 = world.pin();
        let ptr: *mut Slot = world.published.load(Ordering::Acquire);
        // SAFETY: `ptr` is a retained boxed `Slot` (see `World::slots`).
        let alive: bool = unsafe { (*ptr).alive.load(Ordering::Acquire) };
        assert!(alive, "pinned reader observed a reclaimed slot");
        world.unpin(id);
    }

    /// Unpinned reader: the exact 949d10ec bug — the guard is dropped BEFORE the
    /// dereference. A concurrent `publish_swap` may then reclaim the slot between
    /// the load and the deref, so some interleaving observes a reclaimed slot.
    fn reader_unpinned(world: &World) {
        let id: u64 = world.pin();
        let ptr: *mut Slot = world.published.load(Ordering::Acquire);
        world.unpin(id); // BUG: unpin before the dereference
        // SAFETY: `ptr` targets retained memory in the model; `alive` reports
        // whether that slot has been logically reclaimed.
        let alive: bool = unsafe { (*ptr).alive.load(Ordering::Acquire) };
        assert!(alive, "unpinned reader observed a reclaimed slot");
    }

    /// One concurrent execution: a reader races a writer that swaps the published
    /// pointer and defers the old one. `reader_unpins_early` selects the buggy
    /// (unpinned) reader.
    fn scenario(reader_unpins_early: bool) {
        let world: Arc<World> = World::new();
        let w_reader: Arc<World> = Arc::clone(&world);
        let w_writer: Arc<World> = Arc::clone(&world);

        let reader: thread::JoinHandle<()> = thread::spawn(move || {
            if reader_unpins_early {
                reader_unpinned(&w_reader);
            } else {
                reader_pinned(&w_reader);
            }
        });
        let writer: thread::JoinHandle<()> = thread::spawn(move || {
            let new_ptr: *mut Slot = w_writer.slot_ptr(1);
            w_writer.publish_swap(new_ptr);
        });

        // A panicked reader (assertion failure) surfaces here and propagates into
        // `loom::model`, which reports the failing interleaving.
        reader.join().expect("reader thread panicked");
        writer.join().expect("writer thread panicked");
    }

    /// The pinned protocol (the shipped `RuntimeStore` read path) is UAF-free:
    /// loom exhausts every interleaving without observing a reclaimed slot.
    #[test]
    fn pinned_reader_never_observes_reclaim() {
        loom::model(|| scenario(false));
    }

    /// The unpinned protocol is unsound: loom MUST discover an interleaving in
    /// which the reader dereferences a reclaimed slot. We assert that loom *fails*
    /// this model — proving both that the checker has teeth and that pinning across
    /// the dereference (the 949d10ec fix) is necessary, not incidental.
    #[test]
    fn unpinned_reader_race_is_detected_by_loom() {
        let prev: Box<dyn Fn(&PanicHookInfo<'_>) + Sync + Send> = panic::take_hook();
        panic::set_hook(Box::new(|_| {})); // keep the expected failure quiet
        let result: ThreadResult<()> = panic::catch_unwind(|| loom::model(|| scenario(true)));
        panic::set_hook(prev);
        assert!(
            result.is_err(),
            "loom did NOT detect the unpinned-deref use-after-free — the model lost its teeth"
        );
    }
}

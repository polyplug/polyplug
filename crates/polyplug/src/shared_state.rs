//! Process-global shared state for once-per-process external-runtime constraints.
//!
//! Some guest runtimes (notably CPython) can only be initialized once per OS
//! process, and they expose a single process-global namespace (`sys.modules`)
//! that every [`crate::runtime::Runtime`] in the process shares. That is an
//! external-runtime limitation, not runtime state — see CLAUDE.md Rule 12
//! "Known Limitations". This module owns the single process-global datum that
//! captures it, so the individual language loaders no longer carry their own
//! statics.
//!
//! [`SharedState`] is mutated **only** through `Runtime` methods (e.g.
//! [`crate::runtime::Runtime::with_python_load`]); loaders read the values the
//! `Runtime` vends to them. Keeping the runtime the sole mutator preserves the
//! "all mutation flows through an instance" discipline even for the one datum
//! that is unavoidably process-global.

use std::sync::Mutex;
use std::sync::MutexGuard;

use polyplug_abi::SupportedLanguage;

/// The single process-global record of once-per-process external-runtime state.
///
/// Kept deliberately small and extensible: later waves fold the .NET CLR
/// bootstrap flag in here too, replacing that loader's own static.
pub struct SharedState {
    /// Languages whose once-per-process interpreter/runtime init has already
    /// run in this process. A language is recorded only after its init closure
    /// returns `Ok`, so a failed init is retried on the next load. A `Vec` is
    /// used (not a `HashSet`) because `Vec::new()` is `const` — required to
    /// initialize the `static` below without a lazy wrapper — and the set is
    /// bounded by the small fixed number of `SupportedLanguage` variants, so
    /// linear membership is trivially cheap.
    initialized: Vec<SupportedLanguage>,
    /// Monotonic uniqueness ticket. Vended (and incremented) on every guarded
    /// load so two `Runtime` instances loading the same-named bundle never
    /// compute colliding process-global keys (e.g. Python `sys.modules`
    /// re-key prefixes).
    isolation_nonce: u64,
}

impl SharedState {
    /// Whether `lang`'s once-per-process init has already completed.
    pub(crate) fn is_initialized(&self, lang: SupportedLanguage) -> bool {
        self.initialized.contains(&lang)
    }

    /// Record that `lang`'s once-per-process init has completed. Idempotent: a
    /// language already present is not duplicated.
    pub(crate) fn mark_initialized(&mut self, lang: SupportedLanguage) {
        if !self.initialized.contains(&lang) {
            self.initialized.push(lang);
        }
    }

    /// Vend the next uniqueness ticket and advance the counter.
    pub(crate) fn next_nonce(&mut self) -> u64 {
        let nonce: u64 = self.isolation_nonce;
        self.isolation_nonce += 1;
        nonce
    }
}

/// The one process-global `SharedState`. Accessed exclusively through
/// [`lock_shared_state`]; mutated only by `Runtime` methods.
static SHARED_STATE: Mutex<SharedState> = Mutex::new(SharedState {
    initialized: Vec::new(),
    isolation_nonce: 0,
});

/// Acquire the process-global `SharedState`, recovering from a poisoned lock.
///
/// A panic while the guard was held leaves the list/counter in a valid (if
/// possibly stale) state — neither is invariant-coupled — so recovering the
/// inner value is sound and lets loads proceed rather than poisoning every
/// future load.
pub(crate) fn lock_shared_state() -> MutexGuard<'static, SharedState> {
    SHARED_STATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

# Reload & Unload Limitations by Loader

> **Status: reference / revisit-later.** Captures *why* Python and .NET bundles
> are not hot-reloadable, so the .NET case can be reassessed once a
> collectible-ALC swap strategy is designed. Not a current-work item.

## What hot-reload requires

The reload path (`crates/polyplug/src/reload.rs`) needs two things from a loader:

1. A **per-bundle, independently reclaimable code unit** that can be dropped and
   replaced without affecting any other bundle.
2. A **synchronous, atomic old→new interface swap**, after which the superseded
   interface (and its backing code unit) is reclaimed via crossbeam-epoch
   deferral — the old code stays mapped until no reader is still pinned in the
   prior epoch.

A loader that cannot provide both cannot hot-reload safely. Such a loader reports
`supports_hot_reload() == false`, and the runtime returns
`RuntimeError::HotReloadDisabled` without calling its `reload()`.

## Per-loader status

| Loader        | Per-bundle reclaimable unit      | Unload            | Reload |
|---------------|----------------------------------|-------------------|--------|
| native        | per-bundle dylib (`dlclose`)     | yes               | yes    |
| lua           | per-bundle Lua state             | yes               | yes    |
| js (QuickJS)  | per-bundle QuickJS context       | yes               | yes    |
| python        | — (one shared interpreter)       | partial (purge)   | no     |
| .NET          | per-bundle collectible ALC       | yes (async)       | no     |

The three reloadable loaders each own a per-bundle unit they can drop and
recreate; that is exactly what reload's drop-old + swap + epoch-reclaim cycle
needs.

## Python — fundamental limitation

CPython initializes **once per process**; the interpreter and `sys.modules` are a
**single shared resource across all bundles and all runtimes**. There is no
per-bundle VM to drop:

- Re-importing a bundle mutates the one shared interpreter, leaving stale module
  objects that other code may still hold.
- CPython has no clean per-module teardown or reclaim.

So there is no safe unit to swap, and reload cannot be made safe. Unload is only a
**best-effort purge** of the bundle's re-keyed `sys.modules` entries (the
module-isolation nonce mechanism); the interpreter itself never goes away.

## .NET — design-mismatch, not impossibility

The CLR also initializes once per process, but each bundle loads into its **own
collectible `AssemblyLoadContext`** keyed by `(runtime_id, bundle_id)` via the
managed byte-load bridge. As a result:

- **Unload genuinely works** — the ALC is GC-reclaimed once all references clear
  (proven by WeakReference-after-GC in the dotnet loader tests).
- **Reload does not** — ALC unload is **cooperative and asynchronous**: managed
  memory reclaims only after the next GC, once all managed *and* native frames
  into the ALC have cleared. That cannot provide the **synchronous, atomic**
  old→new swap the epoch-based reload path requires — the old ALC is not
  guaranteed gone at the swap point.

So .NET reload is disabled by **design-mismatch**, not impossibility.

## Revisit checklist (later)

- [ ] **.NET:** prototype reload as unload-old-ALC + load-new-ALC + swap, with the
      superseded ALC reclaimed lazily (epoch-style) rather than synchronously, so
      the async ALC teardown stops being a blocker.
- [ ] **Python:** no safe path today. Would require per-bundle sub-interpreters
      (PEP 684 per-interpreter GIL) to get an independently reclaimable unit —
      track CPython capability; not actionable now.

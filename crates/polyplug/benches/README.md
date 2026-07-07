# polyplug benchmarks

**These benchmarks are for local use only.** They are *not* run in CI — they
load native fixture plugins, embed VMs, and are sensitive to machine and
scheduler noise, so a shared CI runner produces numbers you can't trust. Run
them on your own hardware, on a quiet machine, and compare *ratios* rather than
absolute nanoseconds.

```bash
# All polyplug core benches
cargo bench -p polyplug

# A single bench
cargo bench -p polyplug --bench counter_inc

# Quick pass (shorter warm-up / measurement)
cargo bench -p polyplug --bench counter_inc -- --warm-up-time 1 --measurement-time 3
```

Criterion writes results to `target/criterion/`. To compare two runs locally
(e.g. before/after a change), the helper `scripts/check_bench_regression.py` walks
that directory and flags any benchmark that regressed beyond a threshold:

```bash
python3 scripts/check_bench_regression.py target/criterion --threshold 1.5
```

To turn a run into the committed SVG charts embedded in `docs/PERFORMANCE.md`
(dependency-free — no matplotlib, no web service), run:

```bash
python3 scripts/gen_bench_charts.py target/criterion docs/assets/benches
```

Both are also wrapped as `just` recipes — `just bench-check` and
`just bench-charts` — so you rarely call them by hand.

> Prerequisite: the benches `dlopen` the fixture plugins. Build them once with
> `bash tests/fixtures/build_all.sh` before running (the root-level cdylibs are
> not committed).

---

## Read this first: the benchmarks are deliberately *unfair to us*

Every benchmark here measures **fixed per-call overhead with the cheapest
possible payload** — an integer increment, a pointer lookup, a handle
validation. That is the **worst case for polyplug**, on purpose.

Why stack the deck against ourselves? Because a benchmark that does real work
per call (hash a buffer, parse a document, transform a record) *hides* the
boundary cost: a fixed ~1 ns sitting next to hundreds or thousands of ns of
useful work rounds to zero. By stripping the payload down to almost nothing, we
isolate the overhead and force it to show up. **If polyplug looks good when the
payload is `x + 1`, it is invisible on any real workload.** So when a comparison
below looks "not fair" — that unfairness runs in the *reader's* favor, not ours.

The numbers quoted below are illustrative (one developer machine). Re-run
locally for your own hardware. Treat the **ordering and the gaps**, not the
absolute ns, as the result.

---

## The benchmarks

### `counter_inc` — the headline "count to 1,000,000" comparison

Runs the identical loop `for _ in 0..1_000_000 { counter = inc(counter) }`
through four mechanisms. Each arm changes exactly one thing, so the per-call
delta is the cost of that one thing:

| Arm | Mechanism | ~ns/call | What it isolates |
|---|---|---|---|
| `native/inline_never` | direct Rust call, `#[inline(never)]` | ~1.1 | the floor — no ABI boundary at all |
| `ffi/by_value` | raw `dlsym` `extern "C" inc(u32)->u32` | ~1.8 | hand-rolled unsafe FFI |
| `native/abi_marshalled` | ptr-in / ptr-out convention, **static** | ~2.1 | polyplug's calling convention, *no* dynamic lib |
| `polyplug/dispatch` | resolved contract dispatch over a loaded `.so` | ~2.3 | the full product |

**Why the stress test is "not fair" — and why we built it anyway.** Arm 1, the
baseline you asked for, is a plain function the compiler may not inline. It is
*genuinely* cheaper than any of the others because it has **no ABI boundary**:
the argument stays in a register, there is no dynamic library, no indirect call,
no marshalling. Comparing it to a plugin call is apples-to-oranges and we do not
claim to match it. We keep it as the **floor reference** — the speed of light
for "call a function and come back."

The honest, like-for-like comparison is **arm 4 vs arm 3**: polyplug's *safe*
dispatch versus the *raw, unsafe FFI a user would otherwise hand-write* to load
a plugin at runtime. Both load the **same** `libtest_plugin.so`; the only
difference is polyplug's safety machinery (type-checked registration, lifecycle,
hot-reload, epoch-reclaimed unload). That gap is **~0.5 ns** — about one L1 cache hit.

And arm 2 explains *where* that 0.5 ns goes: it pays ~2.1 ns with **no dynamic
library at all**, just the pointer-in / pointer-out convention. So most of
polyplug's cost over by-value FFI is the *calling convention* (a struct by
pointer + a result through an out-pointer), not the dispatch or the `.so`
boundary — crossing that boundary adds only ~0.2 ns on top.

> Mechanics: arms 3 and 4 reach the same compiled object two ways. The fixture
> exports a non-ABI `polyplug_bench_inc` symbol (resolved by `dlsym`, arm 3)
> alongside the registered `add` contract (dispatched, arm 4); both compute
> `x + 1`. The contract is resolved **once** before the loop, which is how a
> real host uses it — see `contract_dispatch` below for the re-resolve case.

### `payload_scaling` — where the overhead disappears (the honest one)

The companion to `counter_inc`. `counter_inc` uses the cheapest payload to
*expose* the fixed per-call overhead; this one shows **how little that overhead
matters once the call does real work**. It runs the *same* unit of work — write
N bytes, one at a time — two ways across a sweep of N:

- `native_direct` — an `#[inline(never)]` copy of the byte-write loop, statically
  linked (the work with the cheapest call).
- `polyplug_dispatch` — the **identical** loop, but living in a dynamically
  loaded plugin (`memory_plugin` fn 0) reached through resolved dispatch.

Because both arms run the same per-byte loop, the gap between them at any N is
*only* the dispatch overhead. Measured locally:

| N (bytes) | `native_direct` | `polyplug_dispatch` | overhead | overhead % |
|---|---|---|---|---|
| 0 | ~2.7 ns | ~2.9 ns | ~0.25 ns | ~9% |
| 16 | ~3.4 ns | ~3.7 ns | ~0.27 ns | ~8% |
| 256 | ~5.6 ns | ~5.9 ns | ~0.33 ns | ~6% |
| 1024 | ~9.9 ns | ~10.4 ns | ~0.49 ns | ~5% |
| 4096 | ~37 ns | ~33 ns | below noise | ~0% |
| 16384 | ~125 ns | ~121 ns | below noise | ~0% |

The `overhead` column is roughly **constant** (~0.25–0.5 ns — the fixed dispatch
cost), so the `overhead %` column collapses toward zero as the payload grows. By
a few KB the two arms are statistically indistinguishable (the dispatch arm even
measures *faster* sometimes — that's run-to-run noise on identical work, which is
the point: the fixed cost is now smaller than the measurement jitter). **On any
call that does meaningful work, the safety boundary is free.** Caveat: a
byte-write loop is a light per-byte workload; a heavier one (crypto, parsing)
only makes the overhead % *smaller*.

### `counter_inc` cross-language dispatch matrix (native rows) + the VM benches

The "matrix" answers: *how much does dispatch cost in each guest language?* It is
deliberately split into two tiers, because the two are **not** measured the same
way and lumping them into one table would be dishonest.

> Charted versions of both boundary directions — **guest dispatch** (runtime →
> plugin) and **host call overhead** (host → runtime) — live in
> `docs/PERFORMANCE.md` (`docs/assets/benches/cross_lang_{guest,host}.svg`).
> The **guest** chart reads every bar live from criterion (native from
> `counter_inc`, each VM from its loader's warm dispatch bench; regenerated by
> `just bench-charts`), so run the loader crates too. The **host** chart is
> measured by a live sweep of the example hosts — `just bench-hostcall` times
> one `find_guest_contract` call through the runtime in every host language
> (each needs its language runtime installed).

**Native tier — fully comparable.** `counter_inc` arms 4 and 5 dispatch the
*identical* `test.add` contract through the *identical* native dispatch path; the
only difference is the plugin's source language. These numbers are apples-to-apples:

| Plugin language | `counter_inc` arm | ~ns/call | ~throughput |
|---|---|---|---|
| Rust (cdylib) | `polyplug/dispatch` | ~2.3 | ~430 M/s |
| C++ (cdylib) | `polyplug/dispatch_cpp` | ~2.5 | ~400 M/s |

The ~0.2 ns spread is compiler codegen + run-to-run noise, not a polyplug
property: **native dispatch does not care what language wrote the plugin.** Any
language that compiles to a native cdylib (Rust, C++, C, Zig, …) lands here.

**VM tier — measured per loader, *not* directly comparable to the native rows.**
Lua, JavaScript (QuickJS), Python, and .NET dispatch *into an embedded
interpreter*, so the dominant cost is the VM's own call/GIL/marshalling overhead,
not polyplug's. Each loader crate ships its own `dispatch_benchmark.rs` that
isolates that VM's call cost. Run them locally:

```bash
cargo bench -p polyplug      --bench counter_inc        # rust + cpp native (arms 4, 5)
cargo bench -p polyplug_lua    --bench dispatch_benchmark  # Lua (mlua)
cargo bench -p polyplug_js     --bench dispatch_benchmark  # JavaScript (QuickJS)
cargo bench -p polyplug_python --bench dispatch_benchmark  # Python (pyo3 / GIL)
cargo bench -p polyplug_dotnet --bench dispatch_benchmark  # .NET (CLR)
```

> **Why no single combined table with VM numbers?** The VM benches each measure
> *the interpreter*, and they each measure it slightly differently (e.g. the
> Python one times GIL acquisition + a Python function call). A row reading
> "Python: 300 ns" is mostly CPython, not polyplug — quoting it next to the
> ~2.3–2.5 ns native rows would invite a false "polyplug is 100× slower in Python"
> read. The honest statement is: **native dispatch is ~2.3–2.5 ns regardless of
> plugin language; VM-hosted languages additionally pay their interpreter's
> per-call cost, which is a property of that VM, and which you pay no matter how
> you'd embed it.** Keep the two tiers separate.

### `amortization` — one-time load / resolve / hot-reload costs

`counter_inc` and `payload_scaling` measure the *steady-state* per-call hot path.
This one measures the costs *around* it — the things you pay once, not per call —
so you can see where they amortize:

| Stage | What it measures | ~cost (local) |
|---|---|---|
| `load_bundle` | dlopen + ABI check + `polyplug_init` + register | ~13 µs (once per bundle) |
| `find_and_resolve` | handle lookup + interface-pointer return | ~22 ns (once per contract, *cached* in real use) |
| `hot_reload_swap` | dlopen new dylib + init + atomic swap + retire old | ~17 µs (once per reload) |

**The amortization curve.** `load_bundle` is a fixed ~13 µs. Spread over *N*
dispatch calls (each ~2.5 ns), its per-call contribution is `13 µs / N`:

| N calls after load | load cost / call | as % of a ~2.5 ns dispatch |
|---|---|---|
| 1 | ~13 µs | — (startup) |
| ~5,200 | ~2.5 ns | ~100% (break-even: load ≈ one dispatch) |
| 100,000 | ~0.13 ns | ~5% |
| 1,000,000 | ~0.013 ns | ~0.5% |

So past a few thousand calls the load cost is below the per-call dispatch cost,
and by a million calls it is noise. **A long-lived plugin pays its load cost
once and then runs at native dispatch speed forever.**

`find_and_resolve` (~22 ns) is the per-call cost *only if you re-resolve every
call* — which nobody should: resolve once, cache the interface pointer (that is
exactly what `counter_inc`/`polyplug` does), and it drops out of the hot path
entirely. The pessimal "re-resolve every call" path is measured separately in
`contract_dispatch::cross_plugin`.

`hot_reload_swap` (~17 µs, native bundles only) is the price of swapping a
plugin's code *without restarting the host* — a capability static linking
cannot offer at all. Its value is the capability, not the nanoseconds.

> Honesty note: `load`/`reload` re-`dlopen` the *same* file each iteration, so the
> first dlopen pays the cold page-in and the rest are warm/refcounted. These are
> *warm* load costs; a real "reload after the file changed on disk" pays the cold
> mmap once on top.

#### VM-loader reload arms (Lua + QuickJS)

Native (`amortization::hot_reload_swap`, ~17 µs) is not the only reloadable tier:
the **Lua** and **QuickJS** loaders also support hot-reload, and each ships a
reload arm in its own loader-crate bench (run them locally):

```bash
cargo bench -p polyplug_lua --bench dispatch_benchmark lua_reload   # lua_reload/hot_reload_swap
cargo bench -p polyplug_js  --bench dispatch_benchmark js_reload    # js_reload/hot_reload_swap
```

Each builds a `Runtime` with its loader registered and `hot_reload_enabled`,
loads a path-backed bundle, then times `reload_bundle` — re-reading the on-disk
source, rebuilding/re-running the per-bundle VM through `polyplug_init`,
re-registering the contract, and atomically swapping the live interface (retiring
the old one). Measured locally:

| Loader | `hot_reload_swap` ~cost (local) |
|---|---|
| native (cdylib) | ~17 µs (`amortization::hot_reload_swap`) |
| Lua (LuaJIT) | ~107 µs |
| QuickJS | ~158 µs |

The VM reloads cost more than native because the swap rebuilds/re-evaluates the
interpreter's per-bundle state, not just an `mmap` + symbol lookup. As with every
one-time cost, the value is the **amortization curve**: a Lua reload at ~107 µs
spread over *N* subsequent dispatches contributes `107 µs / N` per call — past a
few thousand calls it is below the per-call dispatch cost, and the **capability**
(swap a plugin's code without restarting the host) is the point, not the µs.

> Honesty notes: (1) like the native arm these re-read the *same* file each
> iteration, so they are *warm* reload costs (the first reload's cold page-in is
> amortized away across the loop). (2) If a reload happens while the host still
> holds a live stateful instance of the bundle, the runtime logs its own `live
> guest instance … Proceeding with reload anyway` warning (the same path the
> integration tests exercise); the reload still succeeds and the measured number
> is the swap cost.

### `contract_dispatch` — dispatch overhead by argument shape

Calls a registered contract function directly through its resolved interface
pointer, with different argument shapes:

- `noop` — `add(0, 0)`: raw dispatch with trivial args.
- `buffer_arg` — fills a pre-allocated 4096-byte `Buffer` (allocated **once**,
  outside the loop, so only dispatch is measured).
- `struct_arg_and_return` — `add(42, 57)` with a real result, to defeat
  dead-code elimination of the plugin's computation.
- `cross_plugin` — **the pessimal path**: `find_guest_contract` +
  `resolve_guest_contract` + dispatch on *every* call. Nobody re-resolves inside
  a tight loop, so this is a deliberate worst case showing the registry-lookup
  cost you avoid by caching the handle (which `counter_inc/polyplug` does).

The same bench file also produces the **`marshalling`** group (chart:
`docs/assets/benches/marshalling.svg`): the cost of *returning* data two ways,
across payload sizes `{16, 256, 4096, 16384}`:

- `marshalling/borrowed/N` — the plugin returns a `StringView` that aliases the
  caller's bytes (zero-copy). Flat at ~1.8 ns regardless of `N`.
- `marshalling/owned/N` — the plugin host-allocates `N` bytes and `memcpy`s the
  input in. Scales with `N` (~11 ns at 16 B → ~158 ns at 16 KB).

This is the measured cost behind borrowed-view returns (`&str` / `string_view` /
`ReadOnlySpan` / `memoryview`) versus owned native `String` / `bytes`, and why
the call arena exists for VM guests (`docs/PERFORMANCE.md`).

### `revision_check` — the self-revalidating caller's per-dispatch overhead

The generated host and peer callers cache the resolved interface and, before each
dispatch, poll the runtime revision counter through a cached pointer — one acquire
load of a read-mostly word plus an integer compare — re-resolving only when it
changed (a hot-reload or unload). This is a direct pointer poll, **not** a function
call into the runtime per dispatch. This bench isolates that fast path by
dispatching the same real native function with and without the check, so the delta
is exactly the cost the auto-cache feature adds:

- `dispatch_only` — bare native dispatch (the floor): **~2.1 ns**.
- `staleness_check_then_dispatch` — the acquire load + compare (branch not taken),
  then the same dispatch: **~2.6 ns**.

The delta is **~0.5 ns** — one atomic load of a Shared cache line. On the real
cross-language dispatches (50 ns native host → 13 µs for a JS host, per the matrix)
it is proportionally invisible, which is the point: the safety the feature buys
(a cached interface pointer can never dangle after a reload/unload) costs one
predicted branch, not a per-call round-trip into the runtime.

### `cold_start` — first dispatch (cache-cold) vs warm dispatch

`counter_inc` measures the steady-state hot path; this one measures the **first**
dispatch into a just-registered contract, when everything is cache-cold (the
registry slot was just inserted, the interface pointer has never been chased, the
dispatch code has never run on this data). Three arms, all over the same trivial
`add(42, 57)` so they isolate the dispatch path, not the work:

- `cold/first_dispatch` — a fresh runtime with one freshly-registered native
  provider is built in **untimed** `iter_batched` setup; the timed body is the
  first `find_guest_contract` + `resolve_guest_contract` + native dispatch on it.
- `warm/find_resolve_dispatch` — the same find + resolve + dispatch on a
  long-lived runtime hammered in a tight loop, so every line is hot in cache.
- `warm/cached_dispatch` — resolve **once** before the loop, then dispatch
  through the cached interface pointer (what `counter_inc/polyplug` does).

Measured locally (chart: `docs/assets/benches/cold_start.svg`):

| Arm | ~cost (local) | what it isolates |
|---|---|---|
| `cold/first_dispatch` | ~143 ns | cold HashMap probe + cold interface chase + cold-icache dispatch |
| `warm/find_resolve_dispatch` | ~27 ns | the same path, hot in cache (the pessimal re-resolve steady state) |
| `warm/cached_dispatch` | ~1.8 ns | the floor — resolve once, dispatch many |

The cold tax (~143 ns − ~27 ns) is paid roughly **once per contract** on its very
first call, then amortizes away as the registry slot, interface pointer, and
dispatch code stay hot. The provider is a synthetic in-process native interface
(the same shape `contention.rs` uses, canonical 3-arg native ABI), so the only
thing that varies between cold and warm is cache warmth — no on-disk bundle or
loader is involved. (Honesty note: the cold registry holds a single contract, so
the cold figure is the first-touch cost of a *small* registry; a host that loaded
the contract among hundreds pays the same flat resolve — see the `ffi/*/registry_*`
sweep below — plus its own cold-cache page-ins.)

### `ffi_resolve` — `HostApi.resolve_guest_contract`

Time from the FFI call to the returned interface pointer. Pure handle →
pointer, no allocation. This is the per-call cost a host pays if it resolves
once and caches (the recommended pattern).

The `resolve_plugin/registry_{10,100,1000}` arms register that many distinct
contracts and resolve a middle handle, to show **resolve does not scale with
registry size** — it is a generation-checked slot index, not a scan. Measured
locally it is **flat across three orders of magnitude**:

| Registered contracts | `resolve` ~cost (local) |
|---|---|
| 10 | ~9.7 ns |
| 100 | ~9.8 ns |
| 1000 | ~9.6 ns |

### `ffi_find_all` — `HostApi.find_all_guest_contracts`

Time to count, allocate, and populate an `Array<GuestContractHandle>`. Unlike
the others this one **does allocate** (the result array), so it is the natural
home for watching host-allocator cost — its "unfairness" is the opposite
direction: it includes an allocation a single-contract lookup wouldn't.

The `find_all_by_contract/registry_{10,100,1000}` arms register that many
distinct contracts and `find_all` a single-match target, to show find_all's
per-call cost is dominated by the by-id HashMap probe + the single-match
collect, **not** the total registry size. Measured locally it is **flat**:

| Registered contracts | `find_all` (single match) ~cost (local) |
|---|---|
| 10 | ~47 ns |
| 100 | ~48 ns |
| 1000 | ~47 ns |

### `registry_resolve` — `Registry::resolve` hot path

Handle validation (generation check) + interface pointer return, below the FFI
layer. Pairs with `ffi_resolve` to separate the registry cost from the FFI
trampoline cost.

### `registry_find` — `Registry::find_guest_contract` hot path

Contract lookup across **various slot counts**, so you can see how lookup scales
as a host loads more contracts.

### `contention` — multi-threaded dispatch throughput (the scaling sentinel)

Every other bench here is single-threaded, so none of them would notice if a
registry hot path stopped scaling — a read lock quietly becoming a write lock, or
a new `Mutex` landing on the resolve chain. This one runs **N threads (1, 2, 4,
8)** all hammering the *same* `Runtime`: each iteration does the full uncached
hot path — `find_guest_contract` (a registry read-lock) + the full count + resolve
chain (more read-locks) + the native dispatch.
The provider is registered **once**; only the per-call resolve-and-dispatch is
timed.

**Methodology (criterion + threads is awkward).** Criterion times a closure on
one thread and has no notion of "N threads ran in parallel," so this bench uses
`iter_custom` with a reused, barrier-started thread pool: workers are spawned
once *outside* the timed region and park on a channel; each measurement hands
every worker its share of the iteration budget, releases them simultaneously
through a `Barrier`, and times the wall-clock span until the last worker
finishes. `Throughput::Elements(N)` is reported per criterion iteration so the
throughput line reads as **aggregate calls/sec across all threads**
(per-thread = aggregate / N). Sample count is trimmed (`sample_size(30)`) so a
full run stays in the low seconds per thread count.

It runs **two groups** so the contrast is explicit:
`contention/uncached/*` re-runs the full resolve chain every call (the pessimal
sentinel), and `contention/cached/*` resolves the interface pointer **once**
before the loop and dispatches straight through it (the documented cache-the-handle
pattern `counter_inc/polyplug` uses) with **zero** registry-lock traffic.

**How to read it.** The signal is the **shape of the 1→8 curve**, not any single
number. A clean read-only path should scale up: aggregate throughput at 8
threads approaching some multiple of the 1-thread figure. If aggregate
throughput **flattens or collapses** as threads rise, contention has crept onto
the resolve path — that is the regression this bench exists to catch.

> **Honest finding (one developer machine, measured this run).** The two groups
> tell the whole story. The `uncached` curve does **not** scale up — aggregate
> throughput *falls* from ~21 M/s at 1 thread to ~8 M/s at 8. The `cached` curve
> scales **near-linearly** — ~185 M/s at 1 thread to ~1.40 G/s at 8 (~7.5×):
>
> | Threads | uncached time/round | uncached aggregate | cached time/round | cached aggregate |
> |---|---|---|---|---|
> | 1 | ~47 ns | ~21 M/s | ~5.4 ns | ~185 M/s |
> | 2 | ~167 ns | ~12.0 M/s | ~5.6 ns | ~359 M/s |
> | 4 | ~433 ns | ~9.2 M/s | ~5.7 ns | ~706 M/s |
> | 8 | ~985 ns | ~8.1 M/s | ~5.7 ns | ~1.40 G/s |
>
> The `uncached` decay is **not** a write-lock bug (the whole hot path —
> `find_guest_contract`, `count`, `resolve_single_provider`,
> `resolve_guest_contract` — takes `read()` locks, verified in
> `runtime_store.rs`). It is `std::sync::RwLock`'s shared reader counter: every
> `read()` acquire/release is an atomic RMW on one cache line, and an uncached
> dispatch takes **several** acquire/release cycles per call (find, then count +
> resolve). Eight cores bouncing that line serialize on
> it, so more threads buy *less* aggregate throughput. The `cached` group is the
> proof of the mitigation: with the handle resolved once, the hot path touches
> **no** registry lock, the shared reader counter never moves, and per-round time
> stays flat (~5.4 → ~5.7 ns) while aggregate throughput scales with cores. Treat
> the `uncached` table as a baseline: a future change that pushed a *write* onto
> that path would turn the gentle decay into a cliff.

### `call_arena` — the per-call bump allocator (`CallArena`)

`CallArena` is the host-owned bump allocator handed to a VM dispatch call so a
guest can write variable-size returns without a `host->alloc` round trip per
value. The retain-and-rewind work (#49) changed three of its paths but benched
none of them; this microbench covers each:

- `primary/alloc_64` — a warm 64-byte bump from the primary block (align + add),
  resetting each iteration so it never overflows. The floor (~2.7 ns locally).
- `reset/primary_only` — `reset()` with no overflow chain: just rewinding `cur`
  to `base` (~0.45 ns locally — effectively free).
- `overflow/cold_first_block` — an alloc that spills past the primary region,
  with the overflow block **freed every iteration** (fresh arena, dropped in the
  timed body) so each call pays a host `malloc`. ~34 ns locally.
- `overflow/warm_reuse` — the **same** overflowing alloc, but the arena is reused
  and `reset()` **retains** the block (retain-and-rewind), so every iteration
  after the first reuses it with no host call. ~3.4 ns locally.
- `per_call/{64,65536}` — a realistic per-call shape: `reset()` + a header
  (16 B) + payload + trailer (32 B), at a primary-resident size (64 B) and an
  overflow size (64 KiB). Both land near ~7.6–7.8 ns because the 64 KiB arm hits
  the *warm retained block*, not a fresh malloc.

**How to read it.** The headline is `overflow/cold_first_block` **vs**
`overflow/warm_reuse` — the ~10× gap (~34 ns → ~3.4 ns locally) is exactly what
retain-and-rewind buys: after the first call that overflows, every later call
reuses the retained block instead of mallocing again. That `per_call/65536` sits
right next to `per_call/64` (rather than 10× higher) is the same win in the
realistic pattern. The arena is constructed once per benchmark function and kept
alive across iterations; its `Drop` frees every retained overflow block at
teardown, so the bench does not leak (the `drop_frees_all_blocks` unit test in
`polyplug_abi` proves the teardown path). Charted as
`docs/assets/benches/call_arena.svg`.

### `cross_call` — REMOVED (historical note)

> This benchmark **no longer exists** — the `cross_call.rs` file was deleted along
> with the `HostApi.call_guest_method` field it measured. This note is kept only to
> record where the ~38.5 ns historical baseline in `docs/PERFORMANCE.md` came from.
> Do not try to run `--bench cross_call`; the peer/direct-dispatch path is now
> measured by `revision_check`.

It measured the end-to-end cost of the former `HostApi.call_guest_method` callback —
the plugin→plugin cross-dispatch path through the real `Runtime`, exercising the
full resolve chain inside `host_call_guest_method` (count providers → find first
→ resolve → native dispatch). `call_guest_method` has since been **removed from
the ABI**; generated peer callers now dispatch directly through the cached interface
(~2.45 ns, see `revision_check`). The figures below are the historical baseline the
direct-dispatch path replaced.

Two arms:

- `native/single_provider` — the common case: an instance carrying the target
  contract's own handle, one cross-call to the single registered provider (~38.5 ns
  locally, measured 2026-06-19).
- `peer/stateless_route` — the former **dynamic** `call_guest_method` route: a
  *stateless* instance (null `data`, target `contract_id`) dispatched through the
  host-mediated path, routed solely by `contract_id`. It lands at ~38.5 ns —
  **identical** to `native/single_provider`, because both shared the same resolve
  chain; the gap is noise. This arm measured only the uncached dynamic capability
  (~16× slower than the direct cached path that replaced it). The per-language
  **generated** marshalling layered on top (QuickJS/CPython/CLR…) is glue that
  cannot be exercised in a pure in-process bench without a per-language bundle —
  the same two-tier caveat as the dispatch matrix.

### `guest_host_call` — the guest → host direction

The reverse of every other dispatch bench: a guest reaching back into the host.
Drives the real `HostApi` callbacks on a real `Runtime` (a hand-built native
`HostContractInterface`, no bundle required). Two arms:

- `host_contract_call/native` — a guest resolves a host-registered contract
  interface **once** through the real `HostApi.resolve_host_contract_interface`
  (a real caller caches it for its lifetime), then dispatches its native function.
  Only the cached dispatch is timed — the native floor (~1.8 ns locally, one
  indirect call). This is the path a generated guest-side host-contract caller
  bottoms out in (polyplugc `generate_host_fn_caller`).
- `host_log/delivered` — one **delivered** log record through the
  `RuntimeConfig.log` funnel: `LoggerHandle::enabled` filter → message build →
  `StringView` construction → the installed `extern "C"` callback → boxed Rust
  sink (a no-op `black_box`, so the bar measures the funnel, not the sink). ~6.9 ns
  locally, paid **only** for records that pass `log_max_level`. This is the
  language-neutral host→log baseline a guest's `host->log(...)` call pays.

There is **no arena arm** here: the guest→host arena slot is already covered end
to end by `call_arena` (`overflow/warm_reuse`, `per_call/*`) — duplicating it
would be a second copy of the same measurement.

A VM host-contract fixture (a Lua/JS/Python host *providing* the contract) would
need a language loader + bundle, which this crate's bench harness does not set up
cheaply — the same native-only caveat the other in-process benches carry.

---

### Lua custom-logger delivery path (criterion arm + the full-VM SDK test)

The Lua loader bench (`cargo bench -p polyplug_lua --bench dispatch_benchmark`)
carries the `lua_log/trampoline_delivery` arm: one `polyplug_lua_log_trampoline`
call (the exact `RuntimeConfig.log` signature, StringViews by value) → a
`PolyplugLuaLogBridge` read → a scalar Rust callback. This is the **Rust-side**
trampoline cost only (bridge read + StringView decomposition + indirect call):
**~2.5 ns** locally. It does *not* cross into a LuaJIT VM, so it isolates the
bridging cost from the VM-callback cost.

The **full** Lua path — including the LuaJIT-callback transition and two
`ffi.string` copies into a user Lua function — is measured separately by the
opt-in (`POLYPLUG_BENCH_ITERS`-gated) arm in
`sdks/lua/host/tests/test_log_runtime.lua`:
one `polyplug_lua_log_trampoline` call → `PolyplugLuaLogBridge`
read → LuaJIT scalar callback → two `ffi.string` copies → user Lua function.

- **~255 ns per delivered log line** locally (`LOGPATH_NS=254–261`,
  2M iterations, release build) — the trampoline itself (~2.5 ns) is a rounding
  error; the cost is the VM crossing + the `ffi.string` copies.
- This cost is paid **only for delivered records**: levels above
  `log_max_level` are filtered inside the runtime before any formatting work,
  so disabled levels stay zero-cost, and dispatch hot paths never touch the
  logger at all.

Run it:

```bash
cargo build --release -p polyplug -p polyplug_lua
POLYPLUG_BENCH_ITERS=2000000 \
POLYPLUG_LIB=$PWD/target/release/libpolyplug.so \
POLYPLUG_LUA_LIB=$PWD/target/release/libpolyplug_lua.so \
luajit sdks/lua/host/tests/test_log_runtime.lua
```

### Python guest dispatch — the corrected `gil_acquire_and_call` arm

The Python loader bench (`cargo bench -p polyplug_python --bench
dispatch_benchmark`) `python_dispatch/gil_acquire_and_call` arm used to
**re-define its no-op Python function from source (`py.run`) inside `b.iter()`
every iteration**, so it timed Python *source compilation* (~12-14 µs), not GIL
acquire + dispatch — the origin of the inflated "GIL costs ~13 µs" myth. The arm
now compiles the function exactly **once** before the loop (caching a
`Py<PyAny>`, like `cached_python_single_call` already did) and measures only
`Python::attach` + `call`: **~56 ns**, almost identical to the ~60 ns cached fast
path, because an uncontended GIL re-attach is nearly free. See
`docs/PERFORMANCE.md` (Python guest dispatch) for the full reconciliation.

---

### `soak_load_unload` — load/unload churn + RSS over time (the leak detector)

Unlike every criterion bench above, this is **not** a latency microbench — it is a
memory soak, and criterion is the wrong tool for "RSS over time." It lives as an
**env-gated `#[test]`** (`tests/soak_load_unload.rs`), following the same opt-in
convention as `examples/hosts/roundtrip_bench.sh`'s `POLYPLUG_BENCH_ITERS`: with
the env var unset it runs a tiny built-in cycle count so a normal `cargo test`
stays fast and green; set `POLYPLUG_SOAK_ITERS` high to run a real soak.

Each cycle does a **full teardown**: build a *fresh* `Runtime` (native loader),
load `test_plugin`, dispatch `test.add` a few times, unload the bundle, then
**drop the whole runtime**. It samples process RSS (`/proc/self/status` `VmRSS`)
every `POLYPLUG_SOAK_SAMPLE_EVERY` cycles and reports two things: **churn
throughput** (cycles/sec) and the **RSS-over-time series**.

**Why full teardown is the whole point.** polyplug **truly unloads** bundles:
unload and reload hand the superseded interface + library/VM to crossbeam-epoch,
which frees them once no reader is still pinned in the prior epoch. This soak builds
a fresh runtime each cycle and **drops it fully** so that a non-flat RSS line is an
unambiguous leak signal — covering both per-bundle reclamation and the runtime
lifecycle itself (the `HostApi` table leak this soak caught).

```bash
cargo build --release -p polyplug --tests
POLYPLUG_SOAK_ITERS=100000 POLYPLUG_SOAK_SAMPLE_EVERY=2000 \
  POLYPLUG_SOAK_OUT=$PWD/target/soak/soak_rss.txt \
  cargo test --release -p polyplug --test soak_load_unload -- \
  --nocapture --exact soak_load_unload_churn
python3 scripts/gen_bench_charts.py --soak target/soak/soak_rss.txt \
  target/criterion docs/assets/benches   # → soak_rss.svg
```

> **Honest finding (measured this run, one developer machine).** Churn is
> ~17,500–18,600 full load→dispatch→unload→drop cycles/sec, and RSS stays **flat**:
> ~3.0 MiB after warm-up, holding steady across 100,000 cycles (a single tiny step
> is allocator noise, not slope). That flat line is the result of a fix. An earlier
> version of this soak *did* climb linearly (~3.1 → ~20.6 MiB over 100,000 cycles);
> a build-and-drop-only bisection isolated it to a **real core leak in the `Runtime`
> lifecycle** — while a pure `dlopen`+`dlclose` loop with no runtime stayed flat, so
> it was neither the loader nor libc. Root cause: `RuntimeBuilder::build` `Box::leak`ed
> the `&'static HostApi` the FFI needs (the 184-byte `HostApi` struct, matching the
> observed per-cycle growth) with no owner to reclaim it. **Fixed** (commit
> `7a9d96fa`): the `Runtime` now *owns* its `HostApi` as a `Box<HostApi>` final field
> that drops at teardown after the loaders `dlclose`, and the regression is locked by
> `crates/polyplug/tests/leak_host_abi.rs`. The chart
> (`docs/assets/benches/soak_rss.svg`) now draws flat. See `docs/PERFORMANCE.md`
> (load/unload churn soak) for the full write-up.

## Future benchmark ideas (documented, not yet built)

These are worth building, but each has a caveat that keeps it from being a clean
"polyplug wins" headline — recorded here so they
aren't lost. **Priority: benches for what we currently ship come first.**

> **Already built** (kept off this list, documented in their own sections above):
> `payload_scaling` (overhead vs work), the **cross-language dispatch matrix**
> (`counter_inc` native rows + per-loader VM benches — every bar now read live
> from criterion), **one-time cost amortization** (`amortization` — load /
> resolve / hot-reload), **dispatch by argument shape** (`contract_dispatch`),
> and **return marshalling** (`contract_dispatch::marshalling` — borrowed view vs
> owned copy). Charts: `docs/assets/benches/{hero,plugin_call_cost,dispatch_by_shape,
> payload_scaling,marshalling,native_round_trip,amortization,
> call_arena,cold_start}.svg` — `plugin_call_cost.svg` is the merged all-language
> dispatch chart (native overhead tier + VM tier, log scale)
> (criterion-sourced, `just bench-charts`) plus the live-sweep pair
> `cross_lang_host.svg` (`just bench-hostcall`) and `cross_lang_matrix.svg`
> (`just bench-roundtrip`), and the env-gated soak chart `soak_rss.svg`
> (`soak_load_unload`, `--soak` data file — see the soak section above).

> The **runtime-level** guest→host and peer-caller paths are now built too:
> `guest_host_call` (host-contract call + host→log funnel) and the direct cached
> peer dispatch measured by `revision_check`. What remains future is only the
> *per-language generated marshalling on top* of those runtime entry points (the
> table row below), which needs per-language bundles.

| Idea | What it would show | The caveat ("it can be argued against") |
|---|---|---|
| **per-language guest→host / peer-caller marshalling** | The generated-caller overhead *on top of* the runtime entry points `guest_host_call` / `revision_check` (direct peer dispatch) already measure — what a plugin pays in its own language's glue (QuickJS/CPython/CLR…) to call back into the host or a peer contract. | Needs per-language **generated** caller fixtures, so any number conflates the language runtime with polyplug's marshalling. The native rows would be honest; the VM rows mostly measure the interpreter — same two-tier caveat as the dispatch matrix. |

If you build any of these, keep them **local-only** (this folder), keep the
payload-isolation discipline above, and state the caveat next to the number so
the data stays honest.

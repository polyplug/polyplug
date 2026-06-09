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
(e.g. before/after a change), the helper `ci/check_bench_regression.py` walks
that directory and flags any benchmark that regressed beyond a threshold:

```bash
python3 ci/check_bench_regression.py target/criterion --threshold 1.5
```

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
hot-reload, retire-not-drop). That gap is **~0.5 ns** — about one L1 cache hit.

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

### `ffi_resolve` — `HostApi.resolve_guest_contract`

Time from the FFI call to the returned interface pointer. Pure handle →
pointer, no allocation. This is the per-call cost a host pays if it resolves
once and caches (the recommended pattern).

### `ffi_find_all` — `HostApi.find_all_guest_contracts`

Time to count, allocate, and populate an `Array<GuestContractHandle>`. Unlike
the others this one **does allocate** (the result array), so it is the natural
home for watching host-allocator cost — its "unfairness" is the opposite
direction: it includes an allocation a single-contract lookup wouldn't.

### `registry_resolve` — `Registry::resolve` hot path

Handle validation (generation check) + interface pointer return, below the FFI
layer. Pairs with `ffi_resolve` to separate the registry cost from the FFI
trampoline cost.

### `registry_find` — `Registry::find_guest_contract` hot path

Contract lookup across **various slot counts**, so you can see how lookup scales
as a host loads more contracts.

---

## Future benchmark ideas (documented, not yet built)

These are worth building, but each has a caveat that keeps it from being a clean
"polyplug wins" headline — recorded here and in `ROADMAP.md` (Lane C) so they
aren't lost. **Priority: benches for what we currently ship come first.**

| Idea | What it would show | The caveat ("it can be argued against") |
|---|---|---|
| **Payload-scaling** | Overhead as a *fraction of useful work* (inc → hash 1 KB → transform 4 KB → parse a doc); the fraction → ~0% as payload grows. The single most honest real-world view. | The chosen payloads are arbitrary; a critic can always pick a payload small enough to make overhead look large. Mitigate by sweeping a range and plotting, not quoting one point. |
| **Cross-language dispatch matrix** | One table, same contract, all 6 languages (native vs VM dispatch) so a user can price their language choice. | VM numbers depend heavily on the embedded VM version and JIT warm-up; not a polyplug property, so it measures the *language* as much as us. Label clearly. |
| **vs sandboxed alternatives** | Call overhead vs a WASM boundary (wasmtime) and vs subprocess/IPC, to quantify what "trusted, same-process, native speed" buys. | Apples-to-oranges on *safety guarantees* — WASM/IPC give isolation we don't. It's a speed-vs-isolation trade, not a strict win; frame it as "here's the cost of isolation you may not need." |
| **One-time cost amortization** | Load-bundle + resolve-contract latency and where it amortizes over N calls; hot-reload swap latency (a feature static linking / WASM can't cheaply match). | These are one-time costs; a critic notes they're irrelevant to steady-state throughput. True — the value is in the amortization curve and the hot-reload capability, not the raw number. |

If you build any of these, keep them **local-only** (this folder), keep the
payload-isolation discipline above, and state the caveat next to the number so
the data stays honest.

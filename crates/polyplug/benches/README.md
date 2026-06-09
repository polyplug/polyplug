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

To turn a run into the committed SVG charts embedded in `docs/PERFORMANCE.md`
(dependency-free — no matplotlib, no web service), run:

```bash
python3 ci/gen_bench_charts.py target/criterion docs/assets/benches
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

**Native tier — fully comparable.** `counter_inc` arms 4 and 5 dispatch the
*identical* `test.add` contract through the *identical* native dispatch path; the
only difference is the plugin's source language. These numbers are apples-to-apples:

| Plugin language | `counter_inc` arm | ~ns/call | ~throughput |
|---|---|---|---|
| Rust (cdylib) | `polyplug/dispatch` | ~2.5 | ~395 M/s |
| C++ (cdylib) | `polyplug/dispatch_cpp` | ~2.7 | ~370 M/s |

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
> ~2.5 ns native rows would invite a false "polyplug is 100× slower in Python"
> read. The honest statement is: **native dispatch is ~2.5 ns regardless of
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
plugin's code *without restarting the host* — a capability static linking and the
WASM/subprocess alternatives cannot match cheaply. Its value is the capability,
not the nanoseconds.

> Honesty note: `load`/`reload` re-`dlopen` the *same* file each iteration, so the
> first dlopen pays the cold page-in and the rest are warm/refcounted. These are
> *warm* load costs; a real "reload after the file changed on disk" pays the cold
> mmap once on top.

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

> **Already built** (kept off this list, documented in their own sections above):
> `payload_scaling` (overhead vs work), the **cross-language dispatch matrix**
> (`counter_inc` native rows + per-loader VM benches), and **one-time cost
> amortization** (`amortization` — load / resolve / hot-reload).

| Idea | What it would show | The caveat ("it can be argued against") |
|---|---|---|
| **vs sandboxed alternatives** | Call overhead vs a WASM boundary (wasmtime) and vs subprocess/IPC, to quantify what "trusted, same-process, native speed" buys. | Apples-to-oranges on *safety guarantees* — WASM/IPC give isolation we don't. It's a speed-vs-isolation trade, not a strict win; frame it as "here's the cost of isolation you may not need." Also pulls in a heavy `wasmtime` dev-dependency. |

If you build any of these, keep them **local-only** (this folder), keep the
payload-isolation discipline above, and state the caveat next to the number so
the data stays honest.

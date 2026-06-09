# Profiling polyplug

A practical reference for profiling the runtime locally. Like the benchmarks,
profiling is a **local** activity — nothing here runs in CI. The goal of this
document is that anyone can produce a flamegraph of any hot path with one
command and know how to read it.

> **TL;DR**
> ```bash
> just bench                       # collect criterion numbers first (optional)
> just flamegraph counter_inc      # flamegraph the dispatch hot path → flamegraph-counter_inc.svg
> just flamegraph amortization     # flamegraph load / resolve / reload
> ```

---

## What to profile, and what to expect

polyplug has a handful of distinct paths, and they have very different
profiles. Knowing the shape *before* you look keeps you from chasing the wrong
thing — the dispatch hot path is already near the metal, so the area worth
optimizing lives elsewhere.

| Path | Bench to profile | Expected flamegraph shape | Optimization headroom |
|---|---|---|---|
| **Native dispatch** (the hot path) | `counter_inc`, `contract_dispatch` | A *thin tower*: ~3 instructions (load args ptr, indirect call, write out ptr). Almost all time is the `black_box` loop itself. | **Near zero.** ~2.3 ns/call, within an L1 hit of raw FFI. Don't optimize this without a workload proving it matters. |
| **Marshalling / payload** | `payload_scaling` | The byte-fill loop dominates; the dispatch frame is a sliver on top. | Low — the boundary cost is fixed and already vanishes under real work. |
| **Load / resolve / reload** | `amortization` | Wide: `dlopen`, symbol resolution, `polyplug_init`, manifest parse, registry insert. **This is where the area is** (~14 µs load). | **Highest.** One-time, but the biggest single cost in the system. |
| **VM dispatch** | `dispatch_benchmark` in `polyplug_lua` / `_js` / `_python` / `_dotnet` | Dominated by the embedded VM (GIL acquire, context restore, JS call). polyplug's own frames are tiny. | Medium — but most of it is the VM, not us. Measure before assuming we can move it. |
| **Cross-call (plugin → plugin)** | `contract_dispatch::cross_plugin` | `find_guest_contract` + `resolve_guest_contract` + dispatch, per call. | Low–medium — registry lookup is a `HashMap` hit (~20 ns); only matters if a caller re-resolves in a loop (which it shouldn't). |

**The honest framing:** if you flamegraph `counter_inc` expecting to find fat to
trim in dispatch, you will be disappointed — that is the point of the benchmark.
Spend profiling effort on **load**, **VM dispatch**, and **marshalling of large
payloads**, in that order.

---

## Setup

### Option A — `cargo-flamegraph` (the `just flamegraph` recipe)

The default. Uses Linux `perf` under the hood and emits an interactive SVG.

```bash
cargo install flamegraph        # installs the `cargo flamegraph` subcommand
```

`perf` must be available and permitted to sample:

```bash
# Check current setting (2 = unprivileged perf blocked, the common default)
cat /proc/sys/kernel/perf_event_paranoid

# Allow unprivileged sampling for this session (resets on reboot)
sudo sysctl kernel.perf_event_paranoid=1
# (use -1 if 1 is not permissive enough; never leave a multi-user box at -1)
```

Then:

```bash
just flamegraph counter_inc                  # → flamegraph-counter_inc.svg
just flamegraph amortization                 # → flamegraph-amortization.svg
just flamegraph dispatch_benchmark polyplug_lua   # a VM loader bench
```

Output SVGs (`flamegraph-*.svg`) and `perf.data` are gitignored — they are
machine-specific and not committed.

### Option B — `samply` (no root, browser UI)

```bash
cargo install samply
cargo bench -p polyplug --bench counter_inc --no-run   # build the bench binary
samply record target/release/deps/counter_inc-*  --bench
```

`samply` also uses `perf_event_open`; the same `perf_event_paranoid` note applies.

### Option C — `valgrind --tool=callgrind` (deterministic, slow, no perf)

Best when `perf` is unavailable (containers, restricted hosts). Counts
instructions deterministically instead of sampling time, so it is immune to
scheduler noise — but runs ~20–50× slower, so use a short measurement.

```bash
cargo bench -p polyplug --bench amortization --no-run
valgrind --tool=callgrind --callgrind-out-file=callgrind.out \
    target/release/deps/amortization-* --bench --warm-up-time 0.1 --measurement-time 0.5
callgrind_annotate callgrind.out | head -50
# graph it: gprof2dot -f callgrind callgrind.out | dot -Tsvg -o callgrind.svg
```

---

## Reading a flamegraph

- **Width = total time** spent in a frame and its children (the wider, the more
  expensive). **Height = call depth.** Colors are arbitrary.
- Look for **wide plateaus** low in the stack — those are where time accumulates.
  A tall thin spire is deep but cheap.
- For a **dispatch** flamegraph, nearly all width is the criterion measurement
  loop; the plugin function is a thin sliver. That is the correct, healthy shape.
- For a **load** flamegraph, expect wide frames under `dlopen`/`Library::new`
  and `polyplug_init`. That is the headroom.
- Re-run 2–3× — sampling jitters frame widths a little between runs. Trust a
  shape that reproduces, not a single sample.

---

## Before you optimize anything

1. **Have a workload that proves it matters.** The dispatch path is already
   ~2.3 ns; "making it faster" in a microbenchmark with no real consumer is
   polishing a number nobody pays.
2. **Re-run the relevant bench before and after** (`just bench` →
   `just bench-check`) so a change that helps one path doesn't quietly regress
   another. `just bench-charts` refreshes the committed SVGs once you're done.
3. **Keep the safety guarantees.** A faster hot path that drops a generation
   check or the retire-not-drop guarantee is not faster — it's broken. See
   `TRUST_MODEL.md`.

---

## See also

- [`crates/polyplug/benches/README.md`](../crates/polyplug/benches/README.md) — what each benchmark measures
- [`PERFORMANCE.md`](./PERFORMANCE.md) — measured numbers and the architecture behind them
- [`TRUST_MODEL.md`](../TRUST_MODEL.md) — the guarantees any optimization must preserve

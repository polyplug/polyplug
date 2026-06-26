# Call Arena

A `CallArena` is a small per-call **bump allocator** the host hands to a VM
dispatch call. The guest writes its variable-size return values (strings,
arrays) into the arena's region instead of calling `host->alloc` once per value.
This is the mechanism that gives *VM* plugins (JS / Lua / Python) the same flat,
zero-allocation return cost a native plugin gets by borrowing its own memory. See
[../glossary.md](glossary.md) for the one-line definition of arena and the
related terms.

## Bump region and overflow chain

The arena is a 40-byte `#[repr(C)]` struct: a primary `[base, end)` bump region
(an inline buffer owned by the caller) plus a fallback chain of host-allocated
overflow blocks for returns larger than the primary region.

The host caller **resets** the arena at the start of each call (a single pointer
rewind, freeing any overflow blocks in one pass). So after a warmup phase the
common case — a small string return — is served entirely from the bump region
with **zero host allocations**.

## Allocator round trips

Without the arena, every dispatch call that returns a string does one
`host->alloc` + one `host->free` per value. The arena turns the steady-state
return path into a pointer increment, removing the allocator round trip from hot
dispatch loops. For a 10,000-iteration echo loop the integration tests assert the
host allocator is hit **zero** times after warmup.

This is what gives *VM* plugins the same flat, zero-allocation return cost shown
in [Returning data: borrow vs copy](PERFORMANCE.md#returning-data-borrow-vs-copy):
native plugins borrow their own memory directly, and the arena lets a JS or Lua
plugin write its return into a caller-owned region instead of allocating per value.

## Lifetime rule

> A view returned from an arena-backed call is valid **until the next
> arena-backed call on the same caller.**

The caller resets the arena at the start of each call, which invalidates the
previous call's arena allocations. Guests **never free** arena memory — the
arena owns it and reclaims it on reset. If you need a return value to outlive the
next call, copy it out, or use the explicit `alloc`/`free` path instead of the
arena helper.

## Dispatch paths

| Path | Uses arena? | How |
|---|---|---|
| JS (QuickJS) guest returns | Yes | the loader threads a per-call `arena_ptr` + a `bridge` into dispatch; the generated wrapper calls `bridge.arenaAlloc(size, arena_ptr)` (no `globalThis` — Rule 12) |
| Lua (LuaJIT) guest returns | Yes | the loader threads `(arena_ptr, arena_alloc)` as the final two dispatch args; the generated handler calls `alloc_string_arena(arena_alloc, arena_ptr, s)` in the guest SDK |
| Rust host callers | Yes | per-caller `CallArena` field threaded into VM dispatch when a return needs it |
| Native Rust / C++ / C# guest returns | N/A | returns are already **borrowed zero-allocation views** into guest-owned memory — nothing to allocate, so no arena is needed |
| Python (CPython) guest returns | Yes | Python guests register **`DispatchType::VirtualMachine`** (`polyplug_python` loader); the loader threads `(arena_ptr, arena_alloc)` as the final two dispatch args (no module-injected bridge — Rule 12) and the generated callable writes returns into the per-call arena |

## Null-arena fallback

The VM dispatch ABI signature is always
`call(loader_data, instance, fn_id, args, out, arena)`. Passing a **null arena**
means "no arena": the threaded arena allocator (js `bridge.arenaAlloc` / lua &
python `arena_alloc`) falls back to per-value `host->alloc`. Host callers that cannot hold a per-caller arena
(e.g. the Lua/Python guest-side host-contract callers) pass null and remain
correct — just not zero-allocation. Every loader passes the arena slot, so the
signature is uniform across all languages.

## Measured costs (retain-and-rewind)

![per-call return buffer: the retain-and-rewind win](assets/benches/call_arena.svg)

The `call_arena` microbench (`cargo bench -p polyplug --bench call_arena`) reads
every bar live from criterion. Measured locally:

| Path | ~cost (local) | what it shows |
|---|---|---|
| `reset/primary_only` | ~0.45 ns | a `reset()` with no overflow is just a cursor rewind — effectively free |
| `primary/alloc_64` | ~2.7 ns | a warm bump inside the primary region (align + add) |
| `overflow/warm_reuse` | ~3.4 ns | an overflowing alloc that **reuses the retained block** — no host call |
| `per_call/64`, `per_call/65536` | ~7.6 ns, ~7.8 ns | a realistic header + payload + trailer at primary and overflow sizes |
| `overflow/cold_first_block` | ~34 ns | the **first** overflow — pays a host `malloc` |

The headline is `overflow/cold_first_block` (~34 ns) **vs** `overflow/warm_reuse`
(~3.4 ns): the ~10× gap is exactly what **retain-and-rewind** buys — after the
first call that overflows, `reset()` retains the host-allocated block and every
later call reuses it instead of mallocing again. That `per_call/65536` (64 KiB
payload) sits right next to `per_call/64` (rather than 10× higher) is the same
win in the realistic per-call shape.

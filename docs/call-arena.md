# Call Arena

A `CallArena` is a small per-call **bump allocator** the host owns and uses to
carry a guest's *variable-size return values* (strings, buffers, arrays) back
across the boundary. It exists to answer one hard question: **when a plugin
returns a list of things whose size the host can't predict, who allocates that
memory and who frees it — without the plugin and host fighting over ownership?**

See [glossary.md](glossary.md) for the one-line definition and related terms.

## Why it exists — the problem it removes

Take a real call: the host asks a plugin to enumerate running processes, and the
plugin returns **2 processes, each with a `path` and a `name` string**. The size
is unknown up front — you don't know the count or the string lengths until the
plugin computes them. That "variable-size return" is the *only* situation the
arena exists for.

**The classic C-plugin ABI (what you'd hand-roll without polyplug):**

```text
host  : enum_processes(&out_ptr, &out_len)
plugin: malloc(2 * sizeof(Process))           // the array
        strdup("/demo/game.exe"), strdup(...)  // proc[0].path, proc[0].name
        strdup(...), strdup(...)               // proc[1].path, proc[1].name
        *out_ptr = array; *out_len = 2
host  : copy each Process out to owned memory (cstr -> owned String)
host  : free(proc[0].path); free(proc[0].name);
        free(proc[1].path); free(proc[1].name);
        free(array)                            // 5 separate frees
```

That is **1 array alloc + 4 string allocs + 5 frees**, and — the dangerous part —
the *plugin* allocates while the *host* frees, through a `free` function pointer
the plugin supplies. If the allocators don't match, or the plugin hands back a
string literal, or the host frees the wrong pointer, the result is undefined
behaviour. It works, but it is fragile, and it is the single most common place a
plugin author gets memory wrong.

**With the arena:** the host owns one pre-allocated buffer (a scratchpad with a
single "next free byte" cursor) and reuses it for every call. The guest's return
is written into that host-owned region, and cleanup is one cursor rewind:

```text
host  : arena.reset()                 // rewind cursor to start — O(1), frees nothing per value
guest : produce 2 Process elements into the arena  (see "How the guest reaches
        the arena" below): bump the array, bump each string, set each StringView
        to point INTO the arena, write Array{ items, len: 2 } to `out`
host  : read the Array — copy it out to owned Vec<Process> (same as before)
host  : (next call) arena.reset()     // rewind — reclaims everything at once
```

The plugin never calls `malloc`/`free` for the return, and the host never frees
per value. Allocation is "add to a cursor"; cleanup is "move the cursor back to
the start" — **one O(1) reset, not five frees.** All the strings and the array
live in one contiguous, host-owned block. The whole allocate-here / free-there
ownership hazard is gone by construction.

## When it applies

The arena is engaged **only for variable-size returns** — strings
(`StringView`), buffers (`Buffer`), and arrays (`Array<T>`), including a struct
that embeds any of those. Fixed-size returns never touch it: a `read()` that
returns a `bool`, or a `query()` that returns a single fixed `MemoryRegion`, is
written straight into the `out` pointer. So in a memory-access plugin,
`read`/`write`/`query`/`alloc` do **not** use the arena, while
`enum_processes` / `enum_modules` / `enum_memory` / `get_option_definitions` do.

## How the guest reaches the arena (native vs VM)

The two dispatch families reach the host-owned return memory differently — this
is the one detail the worked example above abstracts over:

- **VM guests (Lua / JS / Python):** a VM can't hand back host-compatible heap
  memory, so the loader threads the caller's arena into the dispatch call and the
  generated guest glue **bump-allocates the return directly into the arena.** This
  is the arena's primary reason to exist.
- **Native guests (Rust / C / C++):** the guest allocates its return through
  `host->alloc` (the `HostApi` allocator) — e.g. the Rust SDK's
  `HostContext::alloc_string` — or, when it can, returns a zero-copy **borrowed
  view** into memory it already owns. Either way the *host* controls the lifetime;
  the guest never runs a `free_memory_fn` dance.

Both families obey the same lifetime rule below, so callers treat every
variable-size return identically regardless of the plugin's language.

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
| Native Rust / C++ / C# guest returns | No (host allocator instead) | the guest returns either a zero-copy **borrowed view** into memory it already owns, or memory it obtains from `host->alloc` (e.g. `HostContext::alloc_string`) — the host controls the lifetime, so no per-caller arena is threaded into native dispatch |
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

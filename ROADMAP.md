# polyplug Roadmap

Three goals, in order of dependency. Each is an independently executable
orchestrated session. The ABI freezes at v1.0 and the project is still pre-1.0,
so these goals may touch `HostApi` — but only with explicit owner approval,
never unilaterally (see CLAUDE.md Rule 7).

---

## Goal 1 — `generate` output is immediately compilable + `validate --bundle-dir`

There is **no `pack` command**. `polyplugc` does two things: `generate` and
`validate`. Users build the generated glue with their own toolchains and
assemble the bundle directory themselves; `validate --bundle-dir` then checks
the assembled result against the runtime loader's own rules.

The goal has two halves:

1. **`generate` output compiles with zero hand edits.**
   `polyplugc generate --bundle bundle.toml --lang <lang> --out <dir>` emits the
   guest-side contract glue (type stubs, dispatch shims, `guest/` modules) **and**
   a ship-ready `manifest.toml` with the precomputed `bundle_id`. Dropping that
   glue into a minimal project (a `Cargo.toml` cdylib + `src/lib.rs` that includes
   the generated `guest/mod.rs`, or the language equivalent) and building it must
   succeed without editing any generated file.

2. **`validate --bundle-dir <dir>` catches assembly mistakes.**
   After the user compiles and assembles `dist/<name>/` (manifest + entry
   artifact), `validate --bundle-dir` drives the runtime loader's own manifest
   machinery (`polyplug::loader::parse_manifest` + `ManifestData::validate`) so the
   CLI accepts exactly what the runtime would. It verifies: the manifest parses;
   `id == fnv1a_64(name)` (tamper check); the per-platform `[file]` entry resolves
   and the artifact exists in the dir; the artifact extension matches the declared
   `runtime` (native → `.so`/`.dylib`/`.dll`, lua → `.lua`, python → `.py`,
   js-quickjs → `.js`, dotnet → `.dll`); and `version` parses.

Covered languages: `rust`, `cpp`, `csharp`, `python`, `lua`, `js-quickjs`.

### e2e bar

- **Compile proof (Rust reference):** `crates/polyplugc/tests/generate_e2e.rs`
  runs `generate --lang rust`, the test writes the project shell, then `cargo
  build` produces a cdylib — proving generated output needs zero hand edits.
  Remaining work: equivalent `cpp`/`csharp`/`python`/`lua`/`js` build/load steps.
- **Assembly proof:** the same file asserts `validate --bundle-dir` accepts a
  correct bundle (exit 0, prints `OK: <dir>`), rejects a missing entry artifact,
  and rejects a tampered `id`.

Code truth: `crates/polyplugc/src/` — `validate.rs`, `generators/` (one per
language), the `generate` command path; `crates/polyplug/src/loader/manifest.rs`
is the single manifest parser shared by the runtime and the CLI.

---

## Goal 2 — Extension System ✅ Done

The extension system has shipped. `get_extension(extension_id: u32) → *const ()` is the
17th function pointer on `HostApi` (offset 136; struct is 144 bytes). The host
registers extension pointers by ID (no versioning, no contract machinery); plugins call
`host->get_extension(id)` and cast the result to the expected struct if non-null. It is
designed for optional host capabilities: tracing, debug hooks, custom metrics, etc.

Delivered:

- `polyplug_abi`: `get_extension` fn pointer on `HostApi`.
- `polyplug_utils`: `fnv1a_32(name: &[u8]) → u32` alongside `fnv1a_64`.
- `polyplug` crate: `Runtime.extensions` map, `Runtime::register_extension`, and
  `host_get_extension` wired to the `get_extension` field.
- All 6 SDK `HostApi` ABI definitions and the 6 generators carry the field.
- `sdk_validator` checks for the field.

Generic mechanism only — no built-in extensions. Lifetime contract: extension pointers are
registered once at startup and valid for the runtime's entire lifetime; plugins read and
cast, never free, following the "host owns ABI-crossing memory" model.

---

## Goal 3 — Call Arena for Dispatch

Replace per-allocation `host->alloc` / `host->free` in dispatch calls with
a per-call bump allocator (arena). Zero API change for plugin authors —
entirely hidden inside generated code.

### Why

Current model: every dispatch call that returns a string or array does one
`host->alloc` (malloc) + one `host->free` per value. With complex return
types this means multiple round-trips through the allocator per call.

Arena model: one bump allocator per call. Allocation is `pos += size` (a
pointer increment). Free is a single pointer reset at call end. For calls
with multiple string/array outputs, this is an order-of-magnitude improvement
in allocation overhead.

### Design

- A `CallArena` is a small bump allocator created at the start of each
  dispatch call. For small calls it lives on the stack; for large ones it
  falls back to a single heap allocation.
- The arena pointer flows through the generated dispatch shim — it is a
  hidden implementation detail, not a new API surface.
- Plugin authors are completely unaware: in Python they `return "hello"`, in
  Lua they `return str`, in JS they `return str` — the generated marshal
  code allocates from the arena. No alloc or free function is ever visible
  in plugin code.
- After the call returns, the host frees the entire arena in one operation.

### Languages that benefit most

VM-dispatched managed languages (Lua, JS via QuickJS) where the guest builds a
fresh return value (string/array) on every call and the host must reclaim it.
Those are the only languages where the arena replaces a real per-value
`host->alloc` round trip.

**Native Rust / C++ / C# are EXCLUDED — and this is a finding, not an omission.**
Their guest functions return values as **borrowed `StringView`/`Array` views into
guest-owned memory that is already valid for the call** (e.g. a `'static` string,
a field on the instance, or a buffer the host passes in). The return marshal is a
pointer write — it performs **no allocation at all**, so there is nothing for an
arena to replace. Threading an arena into native dispatch would also require
changing the frozen native dispatch ABI signature (`fn(instance, args, out)`),
which has no arena parameter. Native callers therefore pass a null arena.

### Status (per-language)

| Language | Guest dispatch | Return allocation today | Arena-routed? |
|---|---|---|---|
| JS (QuickJS) | VM | `arenaAlloc` → `CallArena`, falls back to `host->alloc` | **Yes** (`allocStringArena`) |
| Lua (LuaJIT) | VM | `_polyplug_arena_alloc` → `CallArena`, falls back to `host->alloc` | **Yes** (`alloc_string_arena`) |
| Rust (host caller) | Native | borrowed view, zero-alloc; real per-caller arena when a return needs one (`fn_needs_arena`) | N/A for returns (already zero-alloc) |
| C++ (host caller) | Native/VM | borrowed view, zero-alloc | N/A for returns (already zero-alloc) |
| C# | Native | borrowed view, zero-alloc | N/A for returns (already zero-alloc) |
| Python | **Native** (ctypes CFUNCTYPE) | borrowed view / `host->alloc`; **no arena in the native ABI signature** | **No** — same exclusion as native Rust/C++ |

Lifetime rule for arena-backed returns: a view returned from an arena-backed call
is valid **until the next arena-backed call on the same caller** (the caller resets
its arena at the start of each call). Guests never free arena allocations.

The host (every loader) always passes the arena slot in the canonical 6-argument
VM dispatch signature `call(loader_data, instance, fn_id, args, out, arena)`; a
**null arena** means "no arena", and the guest bridge falls back to per-value
`host->alloc` — so all paths remain correct whether or not an arena is supplied.

### Scope

- `polyplug_abi`: introduce `CallArena` type; update dispatch shim signature
  to thread the arena through (generated code only, no ABI-visible change to
  plugin authors).
- All 6 generators in `crates/polyplug_codegen/src/languages/` — update
  marshal/unmarshal code to allocate from the arena.
- All 6 language SDK guest helpers (`sdks/`) — update `polyplug_guest.*`
  for each language.
- Tests: verify arena memory is valid during a call and released after;
  verify zero explicit alloc/free calls visible in generated plugin code.

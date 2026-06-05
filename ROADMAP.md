# polyplug Roadmap

Three goals, in order of dependency. Each is an independently executable
orchestrated session. The ABI freezes at v1.0 and the project is still pre-1.0,
so these goals may touch `HostApi` — but only with explicit owner approval,
never unilaterally (see CLAUDE.md Rule 7).

---

## Goal 1 — Complete `polyplugc pack`

`polyplugc pack --bundle api.toml --lang <lang> --out ./my-plugin/` must
produce a fully wired, immediately compilable plugin project in one command.
The user's only job is filling in business logic.

A complete output contains three things:

1. **Generated contract glue** — same output as `polyplugc generate` for that
   language (guest-side type stubs, dispatch shims, `manifest.toml`).
2. **Project files** — `Cargo.toml` / `CMakeLists.txt` / `.csproj` /
   `pyproject.toml` / etc., with real paths and no TODO placeholders.
3. **`manifest.toml`** — bundle manifest ready to ship with the plugin.

Covered languages: `rust`, `cpp`, `csharp`, `python`, `lua`, `js-quickjs`.

Code truth: `crates/polyplugc/src/` — `pack.rs`, `generators/` (one per
language), the existing `generate` command path as the reference for glue
generation and manifest emission.

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

All managed languages (Python, Lua, JS) where plugin authors expect to work
with normal strings and collections. Native languages (Rust, C++, C#)
also benefit from the reduced allocator pressure.

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

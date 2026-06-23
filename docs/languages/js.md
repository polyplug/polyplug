# JavaScript / TypeScript — polyplug

JavaScript and TypeScript are first-class citizens in polyplug on both sides of
the boundary: a JS/TS application can act as a **host** that loads and calls
plugins written in any language, and a developer can write a plugin **guest** in
TypeScript that the embedded QuickJS loader executes.

The two roles use different runtimes by design:

- **JS host** — runs on Deno, Node.js, or Bun. The SDK detects the runtime at
  startup and selects the appropriate FFI backend (Deno's native FFI, koffi on
  Node, or `bun:ffi` on Bun). A single package works across all three.
- **JS guest** — runs inside the embedded [QuickJS](https://bellard.org/quickjs/)
  interpreter managed by the `polyplug_js` loader (`loader = "js-quickjs"`). The
  plugin is a single self-contained flat `bundle.js` produced by
  [rolldown](https://rolldown.rs/); no `node_modules`, no imports at runtime.

---

## Install

### CLI — `polyplugc`

`polyplugc` turns a contract `.toml` into typed glue code. Install it once; the
non-`cargo` paths ship a prebuilt binary and need no Rust toolchain:

```bash
npm install -g @polyplug/cli          # Node (also: bunx @polyplug/cli)
deno install -A npm:@polyplug/cli     # Deno
cargo install polyplugc               # from source
curl -fsSL https://polyplug.github.io/install.sh | bash  # prebuilt binary
```

### Host SDK

```bash
# npm / Node / Bun
npm install @polyplug/host \
            @polyplug/loaders-native \
            @polyplug/loaders-js \
            @polyplug/loaders-lua \
            @polyplug/loaders-python \
            @polyplug/loaders-dotnet

# Deno — jsr mirrors (same package, JSR import map)
deno add jsr:@polyplug/host \
          jsr:@polyplug/loaders-native \
          jsr:@polyplug/loaders-js \
          jsr:@polyplug/loaders-lua \
          jsr:@polyplug/loaders-python \
          jsr:@polyplug/loaders-dotnet
```

Register only the loaders for the guest languages your application needs to
support. `@polyplug/loaders-native` covers Rust/C/C++ bundles and is almost
always included.

### Guest SDK

```bash
# npm / Bun
npm install @polyplug/guest

# Deno
deno add jsr:@polyplug/guest
```

---

## Guides

- **[JS — Host (app)](js-host.md)** — embed the runtime, register loaders, load
  bundles, and call plugin contracts from a Deno, Node, or Bun application.
- **[JS — Guest (plugin)](js-guest.md)** — write a TypeScript plugin that runs
  inside the QuickJS loader and can be loaded by any polyplug host.

---

## Examples

Working code lives in the repo:

- `examples/hosts/js/` — Deno host that loads all six guest languages and runs
  the full pipeline.
- `examples/guests/js/` — five TypeScript plugins (`decoder`, `encoder`,
  `transformer`, `reporter`, `validator`) each bundled to a single `bundle.js`.

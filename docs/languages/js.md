# JavaScript / QuickJS — polyplug

JavaScript works as both a host and a guest. As a host it runs on Deno, Node.js,
or Bun from one package. As a guest it runs inside the
embedded [QuickJS](https://bellard.org/quickjs/) interpreter (`loader =
"js-quickjs"`) as a single self-contained flat `bundle.js`. For measured
overhead, see [Performance](../PERFORMANCE.md).

## Install

**CLI** — generates host callers and guest glue from an `api.toml` contract:

```bash
npm install -g @polyplug/cli          # npm / Bun (also: bunx @polyplug/cli)
deno install -gA -n polyplugc npm:@polyplug/cli   # Deno (CLI is npm-only)
```

**Host SDK** — add the runtime and a loader per guest language you support:

```bash
# npm / Bun
npm install @polyplug/host \
            @polyplug/loaders-native \   # native (.so / .dylib / .dll) bundles
            @polyplug/loaders-js \       # JavaScript (QuickJS) bundles
            @polyplug/loaders-lua \
            @polyplug/loaders-python \
            @polyplug/loaders-dotnet

# Deno (jsr mirrors — same packages)
deno add jsr:@polyplug/host \
          jsr:@polyplug/loaders-native \
          jsr:@polyplug/loaders-js
```

`@polyplug/loaders-native` covers Rust/C/C++ bundles and is almost always
included.

**Guest SDK** — add to your plugin project, plus rolldown to bundle:

```bash
# npm / Bun
npm install @polyplug/guest
npm install --save-dev rolldown

# Deno
deno add jsr:@polyplug/guest
```

## Guides

- **[JS — Host (app)](js-host.md)** — embed the runtime on Deno/Node/Bun, load
  plugins of any language, call contracts.
- **[JS — Guest (plugin)](js-guest.md)** — write a TypeScript plugin, generate
  glue, bundle to a flat `bundle.js`, assemble and validate the bundle.

New to polyplug? Start with the [Quick Start](../QUICKSTART.md).

## Examples

- Host: `examples/hosts/js/` (`host.js`) — Deno host that registers all five
  loaders and runs the full five-stage pipeline.
- Guests: `examples/guests/js/` — five TypeScript plugins (`decoder`,
  `transformer`, `encoder`, `reporter`, `validator`).

Generated code lives under `examples/hosts/js/generated/` (host callers) and
`examples/guests/js/<plugin>/generated/` (guest glue).

## Internal plugin profile

External plugins use the standard bundle command. An application can instead
generate one internal profile with
`polyplugc generate --bundle bundle.toml --internal --lang js-quickjs --out ./generated`.
It supplies ordinary JavaScript/TypeScript factories to generated guest provider
bindings and receives generated host caller bindings from the committed handles;
registration, calls, and unload then follow the same pipeline as an external
plugin.

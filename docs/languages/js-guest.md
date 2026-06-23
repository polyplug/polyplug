# JavaScript — Guest (plugin)

A polyplug JS guest is a TypeScript/JavaScript plugin that runs inside the
embedded **QuickJS** interpreter, managed by the `polyplug_js` loader
(`loader = "js-quickjs"`). The plugin is built into a single self-contained flat
`bundle.js` using [rolldown](https://rolldown.rs/); no `node_modules` or dynamic
imports exist at runtime.

JS/QuickJS bundles support **hot-reload**: the loader re-reads `bundle.js` from
disk and swaps the live implementation when the host calls `reloadBundle`.

See [JS — overview](js.md) for install instructions and the CLI install.

---

## Step 1 — Install the guest SDK

```bash
# npm / Bun
npm install @polyplug/guest

# Deno
deno add jsr:@polyplug/guest
```

Also install rolldown as a dev dependency:

```bash
npm install --save-dev rolldown
```

---

## Step 2 — Get the contract definition (`api.toml`)

Obtain the `api.toml` from the host application developer. It defines every
contract your plugin may implement and every host service it may call.

---

## Step 3 — Write `bundle.toml`

```toml
[bundle]
name    = "my_decoder"
version = "1.0.0"
api     = "../api.toml"
loader  = "js-quickjs"
file    = "bundle.js"

[[plugin]]
name       = "decoder"
implements = ["pipeline.Decoder@1.0"]
```

`implements` references contracts as `name@major_version`. Multiple `[[plugin]]`
sections register multiple contracts from one bundle.

---

## Step 4 — Generate guest glue

```bash
polyplugc generate --bundle bundle.toml --lang js-quickjs --out generated/
```

This produces into `generated/guest/`:

```
generated/guest/
├── contracts.ts         # DECODER_INTERFACE, DECODER_DESCRIPTOR, setDecoderFactory
├── host_contracts.ts    # typed callers for host-provided services (e.g. HostLoggerContract)
├── init.ts              # polyplug_init — the ABI entry point
├── interface.ts         # dispatch wrappers
├── index.ts             # re-exports
└── types.ts             # shared enum/struct types
generated/
└── manifest.toml        # precomputed bundle_id (never edit by hand)
```

The generated `manifest.toml` carries the precomputed `bundle_id`
(`fnv1a_64(name)`). Never hand-write or edit it.

---

## Step 5 — Implement the contract

Create your entry-point source file (e.g. `index.ts`). Import the generated
factory setter and provide your implementation:

```ts
// index.ts
import { setDecoderFactory } from "./generated/guest/contracts";
import { polyplug_init }     from "./generated/guest/init";
import { toStr }             from "@polyplug/guest";

// The loader calls this factory once per instance, threading the bridge and
// host pointer explicitly. Capture bridge in the returned object so methods
// can reach host services through it.
setDecoderFactory((bridge, hostLo, hostHi) => ({
  fn0: (input) => {
    const s = toStr(bridge, input);
    return `DECODED:${s.replace(/,/g, "|")}`;
  },
}));

// Re-export so rolldown promotes polyplug_init to globalThis (required).
export { polyplug_init };
```

The bridge object provides memory accessors, host-contract callers, and the
arena allocator. Never store it in a module-level variable; the loader threads
it per-call.

To call a host service (e.g. `host.logger`):

```ts
import { HostLoggerContract } from "./generated/guest/host_contracts";

setDecoderFactory((bridge, hostLo, hostHi) => {
  const logger = HostLoggerContract.fromHost(bridge, { lo: hostLo, hi: hostHi });
  return {
    fn0: (input) => {
      logger?.Log("decoding…");
      const s = toStr(bridge, input);
      return `DECODED:${s.replace(/,/g, "|")}`;
    },
  };
});
```

---

## Step 6 — Bundle to a single flat file

```bash
rolldown index.ts --format iife --platform neutral --file bundle.js
```

This bundles your source, the generated glue, and any pure-logic npm packages
into one self-contained `bundle.js`. The IIFE format promotes `polyplug_init` to
`globalThis`, which is how the QuickJS loader finds the plugin entry point.

No `node_modules` directory is needed at runtime; all dependencies are inlined.

---

## Step 7 — Assemble the bundle directory

```
dist/my_decoder/
├── manifest.toml   # copy from generated/manifest.toml
└── bundle.js       # rolldown output
```

Copy `generated/manifest.toml` (never the hand-written `bundle.toml`) into the
bundle directory alongside `bundle.js`.

---

## Step 8 — Validate

```bash
polyplugc validate --bundle-dir dist/my_decoder/
```

The validator runs the same checks the loader performs at runtime:
- `manifest.toml` parses correctly
- `bundle_id` matches `fnv1a_64(name)`
- `bundle.js` is present and has a `.js` extension
- contract references resolve against `api.toml`

Fix any errors before shipping the bundle.

---

## Step 9 — Sign (optional)

```bash
polyplugc sign --bundle-dir dist/my_decoder/ --key my_private_key.pem
```

See `docs/TRUST_MODEL.md` for the full signing and verification workflow.

---

## Example plugins

Five complete TypeScript plugins live in `examples/guests/js/`:

| Plugin      | Contract             |
|-------------|----------------------|
| decoder     | `pipeline.Decoder`   |
| transformer | `data.Transformer`   |
| encoder     | `pipeline.Encoder`   |
| reporter    | `data.Reporter`      |
| validator   | `pipeline.Validator` |

Each follows the same structure: a `bundle.toml`, an `index.ts` (or equivalent
entry file), a `generated/` directory produced by `polyplugc`, and a `bundle.js`
assembled by `rolldown`.

# JavaScript — Guest (plugin)

Write a polyplug plugin in TypeScript: generate the ABI glue, bundle it to a
single flat `bundle.js`, and assemble a bundle any polyplug host can load. The
plugin runs inside the embedded **QuickJS** interpreter (`loader =
"js-quickjs"`). New to polyplug? Start with the [Quick Start](../QUICKSTART.md).

See also: [JS overview](js.md) · [JS — Host (app)](js-host.md) ·
[glossary](../glossary.md)

## 1. Install

Install the CLI, the guest SDK, and rolldown to bundle:

```bash
# npm / Bun
npm install -g @polyplug/cli
npm install @polyplug/guest
npm install --save-dev rolldown

# Deno
deno install -gA -n polyplugc npm:@polyplug/cli   # CLI is npm-only, not JSR
deno add jsr:@polyplug/guest
```

## 2. Write the bundle manifest

`bundle.toml` declares the bundle name, target loader, the flat JS file, and
which contracts this bundle implements. The `api` field points at the shared
`api.toml` contract (see `examples/api.toml`).

```toml
# bundle.toml
[bundle]
name    = "my_decoder"
version = "1.0.0"
api     = "../api.toml"      # path to api.toml, relative to this file
loader  = "js-quickjs"
file    = "bundle.js"        # the flat file rolldown produces

[[plugin]]
name       = "decoder"
implements = ["pipeline.Decoder@1.0"]
```

`implements` names each contract as `<namespace>.<Name>@<major_version>`. Add one
`[[plugin]]` section per plugin in the bundle. One `polyplug_init` call returns one
registration for every declared plugin contract. The runtime validates and publishes
the complete registration set atomically; a rejected registration never exposes a
partial bundle. Logical unload removes every contract's QuickJS VM state after
in-flight runtime calls quiesce. To declare a runtime dependency on another
contract, add a `[[dependency]]` section:

```toml
[[dependency]]
kind        = "contract"
contract    = "pipeline.Validator"
min_version = "1.0"
```

## 3. Generate the guest glue

```bash
polyplugc generate --bundle bundle.toml --lang js-quickjs --out generated
```

This writes the contract glue (`contracts.ts`), host-contract callers
(`host_contracts.ts`), `polyplug_init` (`init.ts`), dispatch wrappers
(`interface.ts`), and a `manifest.toml` under `generated/`. Re-run
whenever `bundle.toml` or `api.toml` changes; never edit generated files. For
the emitted symbol names — interface/descriptor consts, the factory setter, and
the positional `fn{N}` methods — see [Generated names](../generated-names.md).

## 4. Implement the plugin

Create your entry-point source file, import the generated factory setter, and
return an instance object whose methods are **positional** (`fn0`, `fn1`, … in
the contract's declared method order). Full source:
`examples/guests/js/decoder`.

```ts
// index.ts
import { setDecoderFactory } from "./generated/guest/contracts";
import { polyplug_init }     from "./generated/guest/init";
import { toStr }             from "@polyplug/guest";

// Capture `bridge` in the returned object so methods can reach host services.
setDecoderFactory((bridge, hostLo, hostHi) => ({
  fn0: (input) => {                       // fn0 == decode (first declared method)
    const s = toStr(bridge, input);
    return `DECODED:${s.replace(/,/g, "|")}`;
  },
}));

// Re-export so the bundler promotes polyplug_init to the entry point (required).
export { polyplug_init };
```

- Store `bridge` in the returned instance — never a module global.
- `toStr` reads a `StringView` as a string.
- Methods are positional: `fn0` is the first declared method, `fn1` the second,
  and so on. For the descriptor's contract-id fields, see
  [Generated names](../generated-names.md).

To call a host contract (such as a logging service) from your plugin, use the
typed caller in the generated `host_contracts.ts`:

```ts
import { HostLoggerContract } from "./generated/guest/host_contracts";

setDecoderFactory((bridge, hostLo, hostHi) => {
  const logger = HostLoggerContract.fromHost(bridge, { lo: hostLo, hi: hostHi });
  return {
    fn0: (input) => {
      logger?.log("decoding…");
      const s = toStr(bridge, input);
      return `DECODED:${s.replace(/,/g, "|")}`;
    },
  };
});
```

## 5. Bundle to a single flat file

QuickJS has no module loader, so bundle your source, the generated glue, and any
pure-logic npm packages into one self-contained `bundle.js`:

```bash
rolldown index.ts --format iife --platform neutral --file bundle.js
```

No `node_modules` directory is needed at runtime — every dependency is inlined.
The re-exported `polyplug_init` becomes the entry point the QuickJS loader calls.

## 6. Assemble the bundle

Copy the bundled `bundle.js` next to the generated `manifest.toml`:

```
dist/my_decoder/
├── manifest.toml       # from generated/manifest.toml — never the hand-written bundle.toml
└── bundle.js           # rolldown output
```

## 7. Validate the bundle

```bash
polyplugc validate --bundle-dir dist/my_decoder
```

This checks the manifest is consistent, the declared `bundle.js` is present, and
the bundle conforms to the ABI rules.

## 8. Sign the bundle (optional)

If the target host enforces a signature policy, sign the bundle:

```bash
polyplugc keygen --out keys/           # generate keypair once; keep signing.key secret
polyplugc sign --bundle-dir dist/my_decoder --key keys/signing.key
polyplugc verify --bundle-dir dist/my_decoder
```

`sign` validates the bundle, then writes a detached `bundle.sig`. See the
[Trust Model](../TRUST_MODEL.md).

## Full reference

Reference plugins:

| Plugin | Path | Contract |
|---|---|---|
| decoder | `examples/guests/js/decoder/` | `pipeline.Decoder` |
| transformer | `examples/guests/js/transformer/` | `data.Transformer` (declares a dependency) |
| encoder | `examples/guests/js/encoder/` | `pipeline.Encoder` |
| reporter | `examples/guests/js/reporter/` | `data.Reporter` (calls a host contract) |
| validator | `examples/guests/js/validator/` | `pipeline.Validator` |

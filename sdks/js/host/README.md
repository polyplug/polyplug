# @polyplug/host

polyplug host runtime for JavaScript/TypeScript: load plugin bundles at runtime
and call guest contracts through the frozen C ABI.

> **Runtime requirement: Deno.** The host loads the native runtime through
> Deno's FFI (`Deno.dlopen`) and reads `Deno.build` / `Deno.env`. It installs and
> imports under Node.js but throws at runtime there. A Node FFI backend is
> planned. Until then, use Deno.

Depends on [`@polyplug/abi`](https://www.npmjs.com/package/@polyplug/abi).

## Bundle signing & key pinning

`runtimeNew(lib, { config })` configures signature enforcement:

- `config.signaturePolicy` — `SignaturePolicy.Off` (default), `WarnOnly`, or
  `Required`. Controls whether each bundle's `bundle.sig` is verified.
- `config.trustedKeys` — an array of Ed25519 verifying keys, each **32 raw
  bytes** (a `Uint8Array` or `ArrayBuffer`). This is the key-pinning allowlist:

  - **Empty / unset (default)** — Trust-On-First-Use. With a non-`Off` policy
    the runtime verifies each bundle's embedded signature for integrity, but
    does not pin any particular signing key.
  - **Non-empty** — key pinning. After signature verification the runtime
    additionally requires the bundle's embedded verifying key to be in the
    allowlist; a bundle re-signed with an attacker key is rejected.

  Only public (verifying) keys are pinned — the private signing key stays
  offline.

```js
import { openPolyplug, runtimeNew, SignaturePolicy } from "@polyplug/host";

const lib = openPolyplug("/path/to/libpolyplug.so");
const runtime = runtimeNew(lib, {
  config: {
    signaturePolicy: SignaturePolicy.Required,
    trustedKeys: [key1, key2], // each a 32-byte Uint8Array
  },
});
```

The runtime copies `trusted_keys` during `runtimeNew` (`polyplug_runtime_create`),
so the host SDK only holds the packed key buffer across that call and lets it go
once create returns.

## In-process bundles

Generated JavaScript host bindings create an `InProcessBundle` containing one
complete canonical registration table and a rooted resident. Register it
synchronously with `Runtime.registerInProcessBundle(bundle)`. The runtime takes
sole ownership of the resident only after core accepts the entire bundle; the
resident keeps generated callback handles, implementation factories, objects,
and table storage reachable for the registered lifetime.
Successful registration transfers that resident exactly once; a rejected
registration leaves the bundle reusable.
Generated callback adapters forward their runtime-local opaque context on every
lifecycle and dispatch call. A thrown JavaScript callback exception is reported
to the ABI caller as `AbiErrorCode.Panic`.

Pass the JavaScript loader's bridge library explicitly when creating each
adapter. The bridge expands lifecycle and VM ABI records in Rust, so JavaScript
callbacks use only pointers and scalar values across Deno, Node, and Bun FFI.

```js
import { bridgeLibrary } from "@polyplug/loaders/js";
import { buildInProcessGuestContract } from "@polyplug/host";

const adapter = buildInProcessGuestContract(spec, bridgeLibrary());
```

`unloadBundle(bundleId)` performs logical unload. If core reports that active
calls, instances, or leases still prevent unload, the resident remains rooted
and the bundle remains usable. On successful unload, the runtime releases the
resident, after which a newly constructed generated bundle may be registered
under the same bundle name.

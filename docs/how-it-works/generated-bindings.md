# Generated bindings

`polyplugc` converts a validated contract definition into language-specific code. The
generated code has two public roles and a private ABI layer. Applications use the
public roles; generated code keeps ABI structures and conversions behind them.

## Generation profiles

The current CLI has three relevant forms. Choose one target language from `rust`,
`cpp`, `csharp`, `python`, `lua`, or `js-quickjs`.

```text
# Generated host caller bindings from an API definition
polyplugc generate --api api.toml --lang rust --out generated/host

# Ordinary generated guest output for an external bundle
polyplugc generate --bundle bundle.toml --lang rust --out generated/guest

# Matching bindings for one internal plugin bundle
polyplugc generate --bundle bundle.toml --internal --lang rust --out generated
```

`--internal` requires `--bundle`. It is an explicit profile for one internal plugin
bundle, not a change to either default profile. The internal profile namespaces its
files by bundle identity, for example
`internal/<bundle-name>-<bundle-id>/`, so profiles for different bundles do not
collide in one output directory.

## Generated guest provider bindings

Generated guest provider bindings are the provider-author surface for an internal
plugin. They provide typed contract definitions, generated provider aggregates, a
registration façade, and private ABI glue. The author implements the generated typed
contract and supplies that implementation to the generated registration call.

For Rust, the internal profile exposes generated domain traits and provider aggregate
types beneath `guest`, then a generated `guest::init::register` function. Its
`Registration` result contains the canonical `bundle_id` and named generated host
callers for the providers in that bundle. Other supported languages provide the same
role in their language-appropriate generated surface.

The generated registration façade performs the one registration operation that matters
to an author:

```text
construct typed implementation(s)
    -> construct generated provider aggregate
    -> generated register(runtime, providers)
    -> Registration { bundle ID, named typed caller(s) }
```

The providers input is consumed for a registration attempt. A failed attempt does not
leave partially published providers and requires a fresh generated aggregate for a
retry. On success, the runtime owns the backing aggregate until lifecycle cleanup.

### What remains private

The provider author does not write or manage manifest bytes, `ManifestData`, provider
descriptors, contract-interface records, host ABI pointers, adapter contexts, return
arenas, ABI marshaling, or prepared-transaction controls. Generated bindings use those
mechanisms to expose the ordinary typed implementation through the canonical runtime
pipeline.

The application likewise does not build a private registry entry or manually assemble
a caller from a string lookup. The generated result already contains the exact callers
built from the handles committed by the registration transaction.

## Generated host caller bindings

Generated host caller bindings are typed application-facing callers. They resolve the
contract records published by the runtime and provide ordinary language-level calls.
Under the hood, a caller follows the common sequence:

```text
resolve contract handle
    -> create one caller-owned instance
    -> marshal and dispatch each typed call through the selected guest interface
    -> convert each result
    -> reset or caller teardown destroys the instance
```

That sequence is the same after external and internal acquisition. A caller observes
contract identity, version compatibility, and normal lifecycle validity; it does not
receive a different type or dispatch route based on acquisition origin.

Variable-size ABI views have different backing owners by dispatch kind. A native
provider keeps them in a `ReturnArena` owned by its guest instance; the provider
resets that arena before its next variable-size return, and instance teardown releases it.
A VM bridge writes them into the `CallArena` when its generated caller supplies one.
That caller resets the `CallArena` at the start of its next arena-using call and
releases it at teardown. A VM caller that cannot retain a `CallArena` uses the
bridge's host-allocation fallback instead, not a caller-borrowed arena view. Therefore
an exposed borrowed view lasts until the relevant arena reset, not merely until the
producing method returns. A binding that converts the view to an owning language value
copies it before that boundary.

## Why an internal plugin emits both roles

An application that only consumes external plugins needs generated host caller bindings
and can load a configured bundle through `Runtime`. It has no internal implementation
to expose, so it does not need generated guest provider bindings.

An application that supplies an internal plugin has two responsibilities in the same
process: it provides the typed implementation and it consumes its typed contract.
The internal profile therefore emits both matching roles. The generated guest provider
bindings adapt the implementation into registration; the generated host caller bindings
let the application use the exact canonical records that were committed. This is still
one runtime and one registry path, not a separate application model.

## External-only proof

An external-only application can retain the default host workflow:

```rust,ignore
use polyplug::runtime::Runtime;
use polyplug_native::{NativeConfig, NativeLoader};
use std::path::Path;

let runtime = Runtime::builder()
    .loader(NativeLoader::new(NativeConfig {}))
    .build()?;
runtime.load_bundle(Path::new("plugins/platform"))?;
// Use the generated host caller bindings for the contract.
```

This example runs no internal-profile generation and invokes no generated provider
registration. The external loader acquires the bundle, after which the generated host
caller bindings use the same registry and instance lifecycle described above.

See [Acquisition](acquisition.md) for loader ownership and [Application
integration](application-integration.md) for end-to-end examples.

## This section

- [Overview](overview.md)
- [Generated bindings](generated-bindings.md)
- [Acquisition](acquisition.md)
- [Registration](registration.md)
- [Registry and calls](registry-and-calls.md)
- [Lifecycle](lifecycle.md)
- [Application integration](application-integration.md)

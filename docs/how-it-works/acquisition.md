# Acquisition

Acquisition is where an application chooses an external plugin or an internal plugin.
It is the only part of the workflow that has different inputs. Both routes hand a
prepared bundle to the same registration transaction; neither creates a separate
registry or caller model.

## Application inputs

The application supplies one of these inputs to its configured `Runtime`:

| Plugin kind | Application supplies | Polyplug acquires |
|---|---|---|
| External plugin | A bundle directory path, or a `BundleSource` with its parsed `ManifestData` | The manifest, loader-selected code, and loader resources. |
| Internal plugin | Typed implementation objects through the generated guest provider bindings | The generated manifest, descriptors, interfaces, and backing provider aggregate. |

An application controls which plugins are enabled and where external bundles are
found. It does not turn either input into registry records itself. The runtime returns
or makes available canonical bundle and contract information after registration, and
the application uses generated host caller bindings for typed work.

## External plugin acquisition

For a path-backed external bundle, `Runtime::load_bundle` reads the companion manifest,
selects the loader named by that manifest, and dispatches the loader. The application
must configure the runtime with loaders appropriate to the bundle types it intends to
load. The loader path performs external-only work before prepared registration,
including artifact access and loader-specific checks.

```rust,ignore
use std::path::Path;

// `runtime` has the loaders required by this bundle type.
runtime.load_bundle(Path::new("plugins/platform"))?;
```

`Runtime::load_bundle_from_source(manifest, source)` is available when the application
has a non-path `BundleSource`; it accepts the already-parsed `ManifestData` because
such a source has no bundle directory to scan. Programmatic path loading retains its
explicit load-order behavior: it loads the named bundle and does not add a separate
graph prevalidation pass.

Before publication, external acquisition validates the complete declared provider set
and the relevant version, function, and dependency requirements. A failed load
publishes no contract, metadata, or dependency record. The loader releases the
resources it acquired for that failed attempt.

## Internal plugin acquisition

An internal plugin starts as an ordinary typed implementation in application code. The
application first opts into the matching profile for its bundle:

```text
polyplugc generate --bundle bundle.toml --internal --lang rust --out generated
```

The generated guest provider bindings accept the implementation through generated
domain traits and provider aggregate types. Their generated registration façade stages
the corresponding descriptors and interfaces with `Runtime`; the runtime uses the
same prepared registration, validation, publication, and rollback path as external
acquisition.

The implementation author supplies typed behavior only. Generated bindings keep the
ABI-facing manifest data, descriptors, interfaces, callback state, marshaling, and
staging mechanics private. After successful registration, the returned generated
registration result contains the canonical bundle ID and the exact named typed callers
for the committed providers.

An internal plugin has no external artifact to inspect. That difference belongs solely
to acquisition; it does not relax validation of its declared provider set, versions,
functions, or dependencies.

## Ownership at the boundary

Each acquisition route owns only the backing resources it needs:

- An external loader owns its artifact access and loader resources until canonical
  registration succeeds; the runtime then retains the resources required to keep the
  published interfaces valid.
- Generated guest provider bindings transfer their provider aggregate to the runtime
  only on a successful internal registration. The runtime retains it until lifecycle
  cleanup.
- A failed acquisition releases the resources not transferred to the runtime exactly
  once. No partial registry entry becomes observable.

This is source-specific ownership, not a source-specific plugin model. Once the
transaction publishes, registry lookup, generated host callers, typed dispatch,
instance handling, and unload operate on the canonical bundle and contract records.

## What follows acquisition

The next step is [Registration](registration.md): Polyplug stages every provider,
checks the exact set, and either publishes all canonical records together or rolls the
attempt back. From there, [Registry and calls](registry-and-calls.md) describes how the
generated host caller bindings use those records without observing acquisition origin.

## This section

- [Overview](overview.md)
- [Generated bindings](generated-bindings.md)
- [Acquisition](acquisition.md)
- [Registration](registration.md)
- [Registry and calls](registry-and-calls.md)
- [Lifecycle](lifecycle.md)
- [Application integration](application-integration.md)

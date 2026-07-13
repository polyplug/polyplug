# Registry and calls: source-neutral typed dispatch

The registry begins after the prepared-bundle transaction has published a
complete bundle. It records contracts by identity and bundle, not by how their
code was acquired. A generated host caller binding therefore works the same
whether the provider arrived through an external loader or generated guest
provider bindings for an internal plugin.

Read [Registration](registration.md) for the validation and atomic publication
that create these records, and [Lifecycle](lifecycle.md) for invalidation,
reload, and unload.

## Published registry state

The write-side registry maintains these related records as one coherent state:

| Record | Purpose |
|---|---|
| Contract slots | A slot holds one provider entry, its immutable `GuestContractInterface`, and its generation. |
| Contract index | Maps a guest-contract ID to the live slot indices that provide it; multiple providers are supported. |
| Bundle data | Maps a `BundleId` to its descriptor and the slots owned by that bundle. |
| Bundle-name index | Maps a bundle name to loaded bundle IDs. |
| Declared dependency set | Maps a bundle to the guest contracts it is permitted to resolve. |

A mutation rebuilds and atomically publishes an immutable `ReadView`. Lock-free
contract discovery pins an epoch, reads that view with acquire ordering, and
uses the contract index. Bundle-specific discovery uses the bundle record to
select a particular provider. The published view retains its interfaces while a
reader is pinned, so replacing a view cannot free an interface that an active
reader is examining.

A `GuestContractHandle` contains a slot index and that slot's generation.
Resolving a handle succeeds only when the slot is still occupied and has the
same generation. Invalidating a slot increments its generation, making every
older handle fail as stale instead of referring to a reused slot.

## Generated host caller bindings

Generated host caller bindings provide the typed application-facing path:

1. A generated discovery helper chooses a provider by contract and minimum
   major version, or by both bundle and contract when the application requires
   a named bundle.
2. It resolves the returned handle to the immutable guest interface and builds
   the typed caller around that interface.
3. Before dispatch, the caller compares the registry revision it last observed
   with the current acquire-synchronized revision. If the revision changed, it
   resolves again; otherwise it uses its cached interface.
4. The binding marshals typed arguments, invokes the interface's native or VM
   dispatch mechanism, and unmarshals the typed result.

The revision is advanced only after the new read view has been published. A
caller that observes a new revision therefore re-resolves against the matching
view rather than caching a superseded interface. Unchanged revisions keep the
ordinary dispatch path free of registry locks and repeated lookup.

The interface itself chooses dispatch, not the acquisition origin:

- A native interface exposes the generated function-pointer table directly.
- A virtual-machine interface routes through the loader dispatch function with
  its loader data and call-arena arguments.

There is no caller branch for internal versus external plugins. Both paths
supply the same descriptor, contract interface, handle, and revision behavior.
The registry intentionally exposes no acquisition-origin field to generated
host caller bindings.

## Instances and typed calls

A typed caller creates a guest instance through the runtime's host-mediated
`create_guest_instance` operation, dispatches methods with that instance, and
destroys it through the corresponding host-mediated operation. The runtime
pins the epoch across creation and destruction, passes the interface's opaque
adapter context and VM loader data when applicable, and attributes stateful
instances to the owning bundle. A null instance payload represents a stateless
instance and does not add live-instance ownership.

This mediation is important at lifecycle boundaries: construction and teardown
cannot run against an interface reclaimed by a concurrent unload. Generated
caller wrappers own the typed instance state and must release it before unload;
see [Lifecycle](lifecycle.md#drain-before-unload).

## Revisions, generations, and reload

Every published load, successful reload swap, and unload changes the registry
revision. The revision tells callers to revalidate their cached interface; the
slot generation makes an individual stale handle unresolvable.

During a supported reload, freshly registered replacement providers are kept
pending and out of the contract index. The reload swap reconciles them with the
previous bundle in one publish, so readers do not see two simultaneously live
providers from the reloading bundle. A surviving provider keeps its slot and
generation while receiving the replacement interface; a removed provider is
vacated and its generation advances. Failed reloads retain the old published
interfaces.

Capability to reload belongs to the loader and its backing resource, not to the
caller type. See [Lifecycle](lifecycle.md#reload-capability) and the
[reload capability matrix](../RELOAD_LIMITATIONS.md) for the supported
loader-specific behavior.

For the complete pipeline, start with [How polyplug works](overview.md).

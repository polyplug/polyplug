# Lifecycle: ownership, reload, and unload

A registered plugin remains usable until the runtime invalidates its bundle.
The ownership, caller, and unload rules keep registry visibility, caller state,
instances, and acquisition resources in a single ordered pipeline. They apply equally
after external acquisition and after registration through generated guest provider
bindings; reload is the loader-backed external-only capability described below.

[Registration](registration.md) describes ownership before publication, while
[Registry and calls](registry-and-calls.md) explains the handles and callers
that must be drained before invalidation.

## Ownership after registration

| Resource | Owner while the bundle is live | Release boundary |
|---|---|---|
| Manifest, descriptor, dependency records, contract slots, and immutable interfaces | Runtime registry | Bundle invalidation followed by epoch-safe reclamation |
| External plugin resource, such as a native library or per-bundle VM | The selected loader | Its reclaim hook after registry invalidation |
| Internal plugin provider aggregate and language roots | Runtime, keyed by the committed bundle ID | Canonical bundle cleanup after invalidation |
| Generated host caller bindings and their typed instances | Application | Caller reset or teardown; application quiescence before unload |
| Native guest-instance `ReturnArena` | Native guest provider | The provider's next variable-size return reset, or guest-instance teardown |
| VM caller `CallArena`, when supplied | Generated host caller bindings | The next arena-using call's reset, or caller teardown/drop |

The runtime copies an interface but leaves its `adapter_context` opaque. The
language-specific adapter owns the referenced roots and interface backing for
as long as the registry can dispatch through that interface. Core owns the
registry's copy and never assumes a universal language-object representation.

For a native dispatch, an ABI view returned by a native provider borrows that guest
instance's `ReturnArena`; it stays valid until the provider resets the arena for its
next variable-size return or tears down the instance. When a VM caller supplies a
`CallArena`, the bridge writes the view there; it stays valid until that caller's next
arena-using reset or teardown. A VM caller without a retained `CallArena` uses the
bridge's host-allocation fallback instead. Neither borrowed arena view expires merely
because the producing method returned. Bindings that expose an owning language value
copy it before the corresponding arena boundary.

This ownership rule also governs failures. An acquisition source retains its
resource until a prepared transaction commits. If loading or validation fails,
the source releases its own resource and the unpublished staged registrations
are discarded. A generated internal plugin registration owns the provider
aggregate for the registration attempt; only a successful commit transfers it
to the runtime. Thus failure releases the attempt once, while a retry provides
a fresh aggregate.

## Loaded registry state versus application policy

The runtime's state is binary at the bundle level: a committed bundle is loaded
and its registered guest contracts are eligible for lookup; after a successful
unload, the bundle and registrations are invalidated. Snapshot
`bundle_descriptors` and `registered_contract_descriptors` report this live
runtime state, not application intent.

Application code owns a separate per-plugin policy. A typical application may
retain a loaded bundle while `enabled == false`, refrain from making calls, and
later set its own flag before explicitly calling a contract-defined
`initialize` operation. If the contract has an `uninitialize` operation, that
is likewise application behavior. Polyplug does not add an enabled/disabled
state to contracts and does not call those operations as a side effect of load,
reload, or unload.

Changing the application flag is not sufficient for unload. It merely stops
the application's planned work; the application must still quiesce every
caller and destroy every instance before it asks the runtime to invalidate the
bundle.

## Drain before unload

Before requesting unload, the application must stop initiating new calls for
the bundle, drop or quiesce generated host caller bindings, and destroy every
stateful guest instance it owns. The runtime emits its `Unloading` callback
before invalidation so the host can perform that coordination.

Runtime-mediated instance construction and destruction hold an epoch pin for
the entire callback. This keeps the interface and loader resource alive while
those operations run. Every committed internal bundle is marked privately in
`Runtime` by its bundle ID, regardless of the provider language or which layer
owns its resident state. If stateful instances remain when an internal bundle is
unloaded, `Runtime` returns `InternalPluginInUse` and leaves that bundle live.

After the application stops new calls, destroys or resets its callers, and destroys
every stateful guest instance, it can retry the internal unload successfully,
subject to the normal dependency checks. The refusal is a guard, not a teardown
mechanism: the host still owns quiescence and instance teardown. External bundle
unload paths may instead warn and proceed with live instances, so they cannot rely
on the internal live-instance guard.

Direct raw-interface dispatch is the trusted fast path. An application must
not keep using a cached raw interface pointer concurrently with, or after,
unload. The caller revision check prevents ordinary generated host caller
bindings from retaining a stale cached interface, but it cannot make a raw
pointer safe when the application bypasses that binding.

## Direct and cascade unload

`unload_bundle` is conservative. It finds the target's exported contracts and
refuses the operation when another loaded bundle declared a dependency on any
of them. This protects direct peer dispatch; the error identifies the dependent
bundles.

`unload_bundle_cascade` is the explicit alternative. It follows those declared
dependencies, unloads dependents before their provider, and uses a visited set
to break cycles. Both direct and cascade requests reach the same per-bundle
cleanup boundary. There is not a separate cascade cleanup model.

For each bundle that reaches that boundary, the runtime performs this order:

1. Capture the loader identity and notify the host that unloading is beginning.
2. The host uses the synchronous `Unloading` callback to quiesce callers and destroy
   guest instances before invalidation. For an internal bundle, the
   `InternalPluginInUse` refusal while stateful instances remain is a guard for that
   host responsibility; external unload paths may warn and proceed instead.
3. Invalidate the bundle's slots: remove its entries from bundle and contract
   indices, advance the affected slot generations, and publish a new read view.
4. Remove the stored manifest/reload recipe and reset the bundle's instance
   accounting.
5. Invoke the selected loader's reclaim hook once, if there is one.
6. Release the runtime-owned internal provider aggregate once.

The common boundary makes cleanup exact for an invalidated bundle: a direct
unload and a cascade unload cannot each run an independent loader teardown or
root release path. Once invalidated, no new lookup can resolve the bundle; an
older handle fails the generation check. Readers pinned before the new view was
published retain the old interface until their epoch ends, after which the
registry can reclaim it safely.

Loader reclamation is resource-specific. Native loaders release their dynamic
library; Lua and JavaScript loaders release a per-bundle VM; Python clears the
bundle's isolated module entries; and the .NET loader releases its collectible
assembly-load context when its references and frames have cleared. The precise
resource is an acquisition concern, not a second plugin model.

## Reload capability

`reload_bundle` is loader-backed and external-only: it requires a loader that can
replace an external bundle's backing resource and provide a synchronous atomic
interface swap. To replace an internal plugin, the application quiesces its callers
and instances, unloads the registered bundle, and performs fresh generated
registration with the replacement implementation; it does not call `reload_bundle`.

The runtime serializes external reload writers, keeps replacement registrations pending
during loader initialization, and publishes the resulting swap as one registry
revision. The old published interface remains available when preparation or swap fails.

Current capability is loader-specific: native, Lua, and JavaScript loaders
support reload; Python and .NET have unload behavior but do not provide the
required synchronous reload guarantee. Consult the maintained
[reload capability matrix](../RELOAD_LIMITATIONS.md) before promising reload
for an application deployment.

A successful reload changes the registry revision, so generated host caller
bindings re-resolve before their next call. Existing stateful instances belong
to the replaced lifecycle and should be drained and recreated; reload does not
make old instance state portable to the new provider.

For registration's publication boundary, return to
[Registration](registration.md); for resolution after a reload or unload, see
[Registry and calls](registry-and-calls.md). The overall flow is in
[How polyplug works](overview.md).

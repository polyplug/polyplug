# Registration: one prepared-bundle transaction

Registration is the point at which an acquired plugin becomes part of the
runtime. The acquisition mechanism has finished before this point: an external
plugin's loader and an internal plugin's generated guest provider bindings both
submit the same data to the same transaction. The transaction is based on the
existing `ManifestData`, not on a second manifest or registry format.

For the acquisition boundary, see [Acquisition](acquisition.md). For the
published result and its use by typed callers, see
[Registry and calls](registry-and-calls.md).

## Prepare, then stage

A registration attempt begins with `ManifestData`:

1. The runtime validates its canonical metadata and derives the `BundleId` from
   the manifest name. A bundle that is already registered is rejected before
   staging begins.
2. The runtime opens a prepared-bundle transaction for that manifest and its
   runtime language. The staging area is scoped to the registration thread, so
   nested initialization does not mix providers between bundles.
3. The loader or generated guest provider bindings register each provider
   through the existing guest-contract callback. Every registration supplies a
   `PluginDescriptor` and a `GuestContractInterface`.
4. The runtime copies the descriptor data and the immutable interface into the
   prepared bundle. The opaque `adapter_context` is retained and forwarded to
   the interface callbacks; core does not interpret a language implementation
   object.

At this stage there is no bundle descriptor, dependency record, contract slot,
or lookup result visible to readers. Staging therefore lets a plugin register
several providers while its initialization is still fallible.

## Validate the complete bundle

Commit takes the completed staged bundle and validates it before changing the
published registry:

- **Provider set:** the registered providers must match `ManifestData.provides`
  exactly. A provider absent from the manifest, a declared provider that was
  not registered, duplicate providers, and contract-ID/name collisions are
  rejected.
- **Provider versions:** a `name@major` declaration must agree with the
  provider's major version; a full semantic-version declaration must agree
  exactly. A version suffix carried by the provider registration is checked
  against the interface version as well.
- **Function counts:** for native interfaces, declared `function_count` values
  are compared with the staged interface exports. Strict compatibility rejects
  a missing or mismatched count; relaxed compatibility reports it, and the
  explicit permissive mode skips the comparison.
- **Dependencies:** the manifest's declared guest-contract dependencies become
  the bundle's dependency set at publication. Guest resolution is restricted to
  declared dependencies, and those declarations later determine whether a
  provider can be unloaded. Bundle dependency metadata is also retained with
  the descriptor.

The checks use the staged copies, so a rejected provider never appears briefly
in discovery. This is also why registration accepts descriptors and interfaces
rather than separately publishing each callback.

## Publish atomically

After validation, the runtime constructs one `BundleDescriptor` from the
manifest: bundle ID, name, parsed version, language, bundle path, and declared
bundle dependencies. Under the registry write lock it validates every staged
contract against existing registrations, records the descriptor and dependency
sets, allocates all contract slots, and builds the handles.

Only then does it publish one new immutable read view and advance the registry
revision. Readers consequently observe either the previous registry or the
complete bundle; they cannot observe a descriptor without its providers, a
provider without its dependencies, or only the first provider of a
multi-provider bundle.

The commit result contains the **exact committed handles in staging order**:
one `GuestContractHandle` per staged provider, in the order in which the
registration callback supplied them. Generated guest provider bindings use
those exact handles to construct their named generated host caller bindings;
they do not repeat an ambiguous contract lookup to guess which provider was
committed.

## Failure and rollback ownership

Before publication, staged descriptors and interfaces are owned by the
transaction. A callback error, validation error, or failed commit discards the
prepared bundle, leaving no registry metadata, dependency edge, or provider
slot to roll back publicly.

The acquisition side owns its backing resource until publication succeeds. An
external loader that fails is asked to release the resource it acquired. A
generated internal plugin registration owns the supplied provider aggregate for
its attempt: it is retained by the runtime only after the commit succeeds and
is released on the canonical unload path. A failed attempt consumes and
releases that aggregate once; retrying means supplying a fresh aggregate rather
than reusing a partially registered one.

The next stages are [Registry and calls](registry-and-calls.md) and
[Lifecycle](lifecycle.md). The end-to-end map is in
[How polyplug works](overview.md).

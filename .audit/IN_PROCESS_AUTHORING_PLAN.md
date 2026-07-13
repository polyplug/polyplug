# Unified plugin acquisition and registration plan

**Status:** owner-review draft. No implementation is authorized by this document.

## 1. Goal

A plugin is a plugin after registration. Generated host callers, the runtime
transaction, and the core registry must not know or branch on whether its implementation
is an internal plugin supplied by the application or an external plugin acquired from
an external bundle.

The only origin-specific work is acquiring the implementation and keeping its backing
resources alive. Both acquisition routes must feed the same existing bundle data,
contract interfaces, validation, publication, lookup, dispatch, and lifecycle pipeline.

## 2. Architectural law

Origin ends at the acquisition adapter.

```text
external plugin -> loader -> generated guest provider bindings -----┐
                                                                    ├-> one canonical registration transaction
internal plugin -> generated guest provider bindings ---------------┘
                                                                            │
                                                                            v
                                                                    one core registry
                                                                            │
                                                                            v
                                                              generated host caller bindings
```

The core consumes existing data only:

- `ManifestData`;
- `PluginDescriptor`;
- `GuestContractInterface`;
- `BundleDescriptor` derived from the manifest;
- existing dependency, version, function-count, and contract IDs.

The redesign must not add:

- an internal-plugin-only bundle schema;
- an internal-plugin registration envelope;
- parallel contract or bundle records;
- a second registry path;
- origin-specific caller types;
- origin-specific lookup, dispatch, enable, disable, or unload semantics.

`internal plugin` and `external plugin` describe acquisition only. They are not plugin
identity and never affect how a registered contract is called. The legacy terms
`in-process`, `linked`, `embedded`, and `built-in` are removed from active APIs,
generated names, examples, and product documentation during the final cutover.

### Generated-code terminology

Use these exact terms in code, generated APIs, tests, and documentation:

- **generated guest provider bindings** — generated types and private ABI glue that
  expose an implementation as existing `PluginDescriptor + GuestContractInterface`
  records. External plugins compile/package these bindings with the plugin. Internal
  plugins compile/include them in the host application.
- **generated host caller bindings** — generated typed callers used by the application
  to resolve and call a registered contract.

An application that supplies an internal plugin includes both roles:

```text
ordinary internal implementation
    -> generated guest provider bindings
    -> polyplug registry
    -> generated host caller bindings
    -> application subsystem
```

The host already receives generated host caller bindings today. The redesign does not
invent host generation; it lets the same application also include generated guest
provider bindings for its internal plugins without exposing ABI plumbing.

## 3. What already works and must remain canonical

The external path already provides the correct transaction:

1. acquire and validate canonical `ManifestData`;
2. acquire the implementation through a `BundleLoader`;
3. begin the existing prepared-bundle stage;
4. let the implementation emit existing `PluginDescriptor + GuestContractInterface`
   pairs through `HostApi.register_guest_contract`;
5. validate the complete provider set, function counts, versions, and dependencies;
6. publish the complete `BundleDescriptor` and contract slots atomically;
7. resolve with the existing registry APIs;
8. create, dispatch, destroy, reload, and unload through existing handles and interfaces.

The source becomes irrelevant at step 4. `RuntimeStore` already stores canonical bundle
metadata and contract slots without an internal/external origin field. Existing
`GuestContractHandle` generation/revision checks already make callers source-neutral.

The current internal-plugin path uses the same ABI records but duplicates transaction
orchestration with legacy `begin_in_process_bundle`, `commit_in_process_bundle`,
`abort_in_process_bundle`, `register_in_process_bundle`, and per-SDK bundle containers.
That duplication is the problem to remove.

## 4. Target layers

### 4.1 Acquisition adapters

Acquisition answers only two questions:

1. How do we obtain callable implementations?
2. What must remain alive until the registered bundle has been drained and unloaded?

#### External acquisition adapter

The existing external adapter continues to:

- parse/read the external manifest and artifact;
- enforce path, signature, and loader policies;
- select the existing native/VM/.NET loader;
- load the library, module, VM, or assembly;
- invoke its existing initialization entrypoint;
- reclaim its loader-owned resources after successful logical unload;
- implement reload where that loader already supports it.

External public loading APIs and bundle formats remain compatible.

#### Internal plugin acquisition adapter

Generated guest provider bindings:

- receive an ordinary implementation constructor/factory;
- create the existing descriptors and interfaces privately;
- invoke the same canonical registration transaction;
- keep only the minimum language-specific values alive;
- release those values after the same logical unload succeeds.

There is no internal-plugin artifact, filesystem manifest lookup, loader scan,
reflection, global factory table, static initializer, or runtime compilation.

### 4.2 One canonical registration transaction

Extract the common transaction currently embedded in the external and internal plugin
paths into one private runtime operation. Conceptually it accepts:

```text
canonical ManifestData
SupportedLanguage
synchronous registration producer using HostApi.register_guest_contract
```

It performs exactly once:

1. manifest validation;
2. bundle-ID derivation and duplicate rejection;
3. `RuntimeStore::begin_prepared_bundle`;
4. synchronous producer invocation;
5. `take_prepared_bundle`;
6. exact provider-set validation;
7. function-count and compatibility validation;
8. the same dependency declaration enforcement performed by the existing explicit-load
   path;
9. `RuntimeStore::register_prepared_bundle`;
10. manifest/reload-recipe retention where applicable;
11. discard and source-adapter cleanup on failure.

Registration input ownership is identical across languages. A new internal-plugin
registration attempt consumes its supplied constructor/provider aggregate immediately.
Success transfers the generated backing roots to the runtime/SDK owner. Producer
failure, commit failure, panic, or exception releases every untransferred root exactly
once. Retry always supplies a freshly constructed input; the same failed aggregate is
never reusable. Legacy APIs keep their current language-specific retry behavior until
they are removed together in Phase E.

The external plugin adapter performs external-only signature/path/loader checks before
entering this operation. The internal plugin adapter has no artifact checks because it
has no artifact. That difference stays outside the canonical transaction.

Dependency graph pre-validation remains a caller/load-mode concern exactly as it is
today. Programmatic `Runtime::load_bundle` intentionally permits explicit load order
and must not begin rejecting a bundle merely because another declared provider has not
yet been loaded. Extracting the transaction cannot move discovery-time graph
pre-validation into explicit registration.

The transaction must not accept a new public bundle registration record. It operates on
the existing manifest, descriptor, interface, and registry types.

### 4.3 Core registry

`RuntimeStore` remains unchanged in meaning:

- one prepared staging model;
- one atomic publication path;
- one bundle index;
- one contract index;
- one revision/generation model;
- one dependency graph;
- one invalidation path.

No origin field is added to registry records. No source-specific registry API is added.

### 4.4 Source ownership and cleanup

There is no universal public `Resident`, factory, callback, or state model.

Each acquisition adapter owns only what its language/source actually requires:

- native external loader: loaded library handle;
- Lua/JavaScript external loader: per-bundle VM/runtime objects;
- Python external loader: isolated module keys and Python roots;
- .NET external loader: collectible assembly context and managed roots;
- Rust internal plugin adapter: the ordinary constructor and implementation instances;
- C++ internal plugin adapter: ordinary implementation objects;
- C# internal plugin adapter: ordinary implementations and delegates that must be
  GC-rooted;
- Python/Lua/JavaScript internal plugin adapters: ordinary implementation objects and
  callable roots required by their runtimes.

Generated static ABI thunks are not described as owned callbacks when no runtime object
must be retained. Implementation fields are ordinary application state, not polyplug
state.

The lifecycle rule is uniform:

```text
acquire resources
    -> register atomically
    -> keep backing resources alive while interfaces can be reached
    -> invalidate and drain through existing runtime lifecycle
    -> release resources after successful unload
```

The registry never inspects these resources. Runtime orchestration may invoke one
source-neutral cleanup/lifecycle hook, but must not branch on an origin enum. Existing
loader cleanup and internal-plugin language cleanup remain adapter responsibilities
behind that single lifecycle boundary.

Direct unload and cascade unload must invoke this same lifecycle boundary for every
invalidated bundle ID. The current Rust cascade path must not bypass internal-plugin
backing cleanup or its live-instance safety rule. Add dependent-internal-plugin cascade
tests proving exact-once cleanup and no release while instances remain reachable.
Non-Rust SDKs currently expose direct unload only; any future cascade exposure must
return or notify every recursively invalidated bundle ID before SDK-owned roots can be
released.

### 4.5 Generation input and optional profile

Generation reuses the existing `ValidatedIr` / `ResolvedBundle`, provider declarations,
and canonical manifest renderer. There is no internal-plugin-only schema or composition
model.

Internal plugin bindings are explicitly opt-in. Preserve the existing entrypoints:

- `polyplugc generate --api ...` remains the external-only host caller generation path;
- `polyplugc generate --bundle ...` remains the external-plugin guest generation path;
- `polyplugc generate --bundle <bundle.toml> --internal` is the new conceptual form
  that generates both guest provider bindings and matching host caller bindings for
  one internal plugin bundle.

The exact flag spelling is finalized through CLI tests. One command handles one bundle;
multiple internal plugins use separate commands. Output is namespaced by validated
bundle identity, for example
`internal/<bundle_name>/{host,guest,...}`, so two bundles cannot overwrite each other.
Generation rejects duplicate output paths before writing. Two-bundle compile tests in
all six languages must prove collision-free imports, including bundles referencing
different API TOMLs.

Internal mode parses the same `[bundle]` identity/version/API reference, `[[plugin]]`
providers, and dependencies as external mode. External-only acquisition fields
`loader` and `file` remain required and fully validated for external generation but are
optional and ignored for internal generation. Internal generation must not synthesize a
fake artifact path. Split parser validation into canonical bundle metadata validation
and external acquisition validation while continuing to produce the existing
`ResolvedBundle` and canonical runtime metadata.

The existing public `GenerateConfig` and `generate(GenerateConfig)` remain unchanged so
current struct literals keep compiling. Add a separate internal-generation function and
configuration type, following the existing specialized-generation pattern, rather than
adding a field to `GenerateConfig`.

Before Phase E, all current default output remains unchanged, including legacy
internal-plugin helper files that some generators currently emit unconditionally. At
the atomic cutover, those legacy files are removed from default host/external output
and replaced by the opt-in internal profile. Golden tests classify canonical
host/external files separately from legacy internal files: canonical files remain
byte-identical, while legacy files disappear only at the scheduled cutover.

## 5. Author experience

For all six maintained languages, the provider-author workflow is:

```text
API TOML
    -> generated typed contract
    -> ordinary implementation
    -> one generated registration call
```

This workflow exists only when the internal generation profile was explicitly selected
for that bundle. External-only applications keep the current host-caller workflow and
receive no new internal registration surface after the Phase E cutover.

Illustrative Rust shape:

```rust
struct Platform {
    memory: MemoryBackend,
}

impl generated::PlatformPlugin for Platform {
    // ordinary typed methods
}

let plugin = generated::register(&runtime, || Platform::new(memory))?;
```

Equivalent generated façades exist for C++, C#, Python, Lua, and JavaScript.

The author never handles:

- `ManifestData` or manifest bytes;
- `PluginDescriptor`;
- `GuestContractInterface`;
- `HostApi` pointers;
- `adapter_context`;
- return arenas or ABI conversion helpers;
- staging begin/commit/abort;
- library bridges;
- resident containers;
- manual `find` and caller construction.

The generated call returns the normal typed plugin caller/handle plus the canonical
bundle ID needed for lifecycle operations. This is a language convenience result, not a
new core or ABI registration record.

For multiple generated providers, the input and result are generated named aggregates,
never string maps or unlabelled four-plus-value tuples.

## 6. Application experience after acquisition

The application passes external plugin bundle locations and internal plugin
implementations to polyplug. Polyplug performs acquisition, canonical registration,
discovery, typed caller construction, dispatch, and lifecycle coordination. Both
routes produce the same application-level loaded-plugin information:

- canonical bundle ID and name;
- canonical plugin descriptors;
- generated typed caller(s);
- source capabilities used only for diagnostics/policy, such as whether reload is
  supported and an optional display path.

After insertion, application behavior is origin-blind:

```text
acquire plugin by any adapter
    -> add canonical plugin records to PluginManager
    -> select/enable by contract and application policy
    -> pass generated typed caller to the subsystem
    -> call it normally
    -> disable/unload by canonical bundle ID
```

The application must not maintain separate internal/external enable or dispatch paths.
A diagnostic origin label may be displayed, and reload UI may consult a `reloadable`
capability, but those values never select a different caller or registry operation.

## 7. CheatGear final shape

CheatGear becomes the first real consumer of the unified abstraction.

### Keep

- authored `SDK/cheatgear.toml`;
- existing external plugin discovery/loading behavior;
- real Linux, Windows, and mock platform domain implementations;
- `PluginManager`, generated callers, and `PlatformManager` roles;
- canonical bundle/plugin identity and lifecycle behavior.

### Remove

- `impl_in_process_platform_guest!`;
- `ReturnArena` and handwritten ABI conversions;
- `InProcessPlatformFactory` and `InProcessPlatformRegistration`;
- Linux/Windows/mock ABI wrapper types;
- manual manifest/IR mutation and the uncommitted
  `crates/plugin_runtime/build/in_process_bundle.rs` experiment;
- manual register -> find -> generated caller construction;
- separate internal-plugin insertion/enable logic where it changes normal plugin behavior;
- hardcoded source-specific bundle/plugin lookup.

### Final flow

```text
PluginManager creates one runtime
    ├─ external plugin loader acquires configured bundle
    └─ generated guest provider bindings register selected internal platform plugin
                 │
                 v
        same canonical registered bundle/plugin records
                 │
                 v
        one PluginManager insertion and enable path
                 │
                 v
        generated platform.Plugin caller
                 │
                 v
        PlatformManager
```

Linux, Windows, and mock selection occurs only while constructing the internal platform
plugin implementation. Once registered, the selected implementation is an ordinary
plugin.

## 8. Reload and unload

### Lookup and dispatch

All plugins use existing:

- `find_guest_contract` / `find_guest_contract_by_bundle` / contract resolution;
- generated caller construction;
- instance create/destroy;
- registry revision/generation validation;
- direct native/VM dispatch interfaces.

No hot-path allocation, lock, source check, manifest lookup, string lookup, or registry
branch is introduced.

### Reload

Reload is an acquisition capability:

- external plugin loaders retain their existing reload behavior;
- internal plugins are non-reloadable unless a future acquisition adapter can provide
  a real replacement through the same canonical transaction and lifecycle guarantees;
- callers see only the existing revision/generation behavior;
- the core registry does not inspect origin to decide dispatch behavior.

### Unload

Every bundle uses the existing canonical unload operation:

1. reject unload when dependency rules forbid it;
2. notify/quiesce through existing lifecycle callbacks;
3. stop new resolution and invalidate registry slots;
4. apply the existing call/lease/instance safety contract;
5. invoke the source-neutral lifecycle boundary for every directly or recursively
   invalidated bundle ID;
6. let each acquisition adapter release its backing resources and reload recipe.

The transaction refactor must not silently strengthen, weaken, or otherwise change
current external unload behavior. Any existing difference in live-instance policy is
recorded and tested before implementation. The lifecycle boundary may express the
safety capability required by backing resources, but it cannot branch on an
internal/external origin label. Changing the public unload contract, if ever required,
is a separate explicitly reviewed change rather than a hidden consequence of this
authoring redesign.

## 9. Documentation product

Documentation is part of the implementation, not a follow-up. The mdBook
`docs/SUMMARY.md` gains a multi-page **How polyplug works** section. It must explain
the complete architecture at two levels: the public application workflow and the
under-the-hood ABI/runtime pipeline.

### Required pages

Create a dedicated section with at least these pages:

1. `docs/how-it-works/overview.md`
   - internal plugin and external plugin definitions;
   - the rule that acquisition origin ends before registration;
   - the complete project pipeline diagram;
   - links to every deeper page.
2. `docs/how-it-works/generated-bindings.md`
   - generated guest provider bindings;
   - generated host caller bindings;
   - current default generation profiles;
   - the explicit internal-plugin generation option;
   - an external-only application example proving internal bindings are optional;
   - why an application supplying an internal plugin includes both roles;
   - which generated details remain private.
3. `docs/how-it-works/acquisition.md`
   - application responsibility: pass external bundle locations and internal
     implementations to polyplug;
   - loader responsibilities for external plugins;
   - generated guest provider binding responsibilities for internal plugins;
   - source-specific ownership without a source-specific plugin model.
4. `docs/how-it-works/registration.md`
   - existing `ManifestData`;
   - prepared-bundle staging;
   - existing descriptor/interface registration;
   - exact-set/version/function/dependency checks;
   - atomic publication and rollback.
5. `docs/how-it-works/registry-and-calls.md`
   - bundle and contract indices;
   - generated host caller resolution;
   - instance creation, dispatch, destruction;
   - revision/generation behavior;
   - why callers cannot observe acquisition origin.
6. `docs/how-it-works/lifecycle.md`
   - resource ownership;
   - reload capability;
   - unload, invalidation, drainage, and cleanup;
   - failure ownership and exact-once cleanup.
7. `docs/how-it-works/application-integration.md`
   - an external-only application example;
   - an optional internal-plugin generation and registration example;
   - the existing `Runtime` plus generated host caller binding responsibilities;
   - identical typed caller use after acquisition;
   - a separate clearly labelled CheatGear `PluginManager` example;
   - CheatGear Linux/Windows/mock plus external plugin example.

### Canonical project pipeline diagram

The overview page must contain this pipeline, rendered as a maintained Mermaid diagram
plus an accessible text equivalent:

```text
APPLICATION
    │
    ├─ always available when requested by the application:
    │      passes external plugin bundle/path ─> POLYPLUG LOADER
    │                                             └─ generated guest provider bindings
    │
    └─ optional; present only when internal plugin generation was enabled:
           passes internal implementation ─────> generated guest provider bindings
                                                       │
                                                       v
                                      CANONICAL PREPARED-BUNDLE TRANSACTION
                                      ManifestData
                                      PluginDescriptor
                                      GuestContractInterface
                                      validation + atomic publication
                                                       │
                                                       v
                                               CORE REGISTRY
                                      one bundle/contract/dependency model
                                                       │
                                                       v
                                      GENERATED HOST CALLER BINDINGS
                                                       │
                                                       v
                                               APPLICATION
                                      discover -> enable -> call -> unload
```

### Canonical application example

The canonical polyplug example uses the existing `Runtime` and generated bindings; it
must not present CheatGear's `PluginManager` as a polyplug API. The final generated
helper names are compile-tested, but the required responsibility flow is:

```rust
let runtime = Runtime::builder().build()?;

// External-only applications stop after this acquisition path.
runtime.load_bundle(Path::new("./plugins/decoder"))?;

// Optional; emitted only by the internal generation profile.
generated::internal::register(&runtime, || Platform::new())?;

// Generated host caller bindings perform typed discovery and caller construction.
let platform = generated::PlatformPluginCaller::find_by_bundle(
    &runtime,
    "platform_bundle",
)?;
let metadata = platform.metadata()?;
```

The generated typed discovery helper is part of the planned host caller binding
surface and wraps existing `find_guest_contract_by_bundle` plus caller construction; it
does not add a registry or manager. A second example may show CheatGear's application-
owned `PluginManager`, but it must be labelled as CheatGear code.

### Existing documentation updates

Update `docs/ARCHITECTURE.md`, `docs/ABI_ARCHITECTURE.md`, `docs/FEATURES.md`,
`docs/glossary.md`, `docs/generated-names.md`, `docs/EXAMPLES.md`, relevant API
reference pages, all six language guest/host pages, and all SDK READMEs/examples.
Update `docs/SUMMARY.md` so the multi-page section is part of the product book.

The glossary and generated-name reference must use only:

- internal plugin;
- external plugin;
- generated guest provider bindings;
- generated host caller bindings.

Legacy internal-plugin terms may appear only in migration notes or exact old symbol
names being removed. Documentation tests and repository search gates must reject their
use as current product terminology.

CheatGear's `docs/book/` receives the same application-facing model and its concrete
platform pipeline. Commit documentation sources only, never generated book output.

## 10. Implementation plan

### Phase A — freeze baseline and remove superseded experiment

- Record focused external and internal plugin behavior tests before refactoring.
- Revert only the uncommitted CheatGear handwritten manifest/IR experiment; preserve
  unrelated worktree changes.
- Establish source-search gates for prohibited parallel models and handwritten glue.
- Capture current generated output in every maintained language and classify each file
  as canonical host caller output, canonical external guest output, or legacy
  internal-plugin helper output. Golden compatibility applies byte-for-byte to the
  canonical files; the legacy internal files are intentionally removed only in Phase E.

### Phase B — extract the canonical transaction

- Factor the shared prepared-bundle transaction from `Runtime::load_manifest_with_source`
  and the current legacy internal-plugin begin/commit/abort path.
- Keep external parsing, signature verification, loader selection, and artifact
  acquisition outside the shared transaction.
- Route the existing external loader through the extracted transaction without changing
  its public API or bundle format.
- Prove identical external bundle descriptors, contract handles, revisions, dependency
  results, reload behavior, and unload cleanup before proceeding.
- Route direct and cascade unload through the same per-bundle lifecycle cleanup
  boundary without changing external loader behavior. Add a Rust cascade regression
  before internal-plugin migration.

### Phase C — Rust generated internal plugin bindings

- Add the separate internal-generation function/configuration type while leaving
  `GenerateConfig` and `generate(GenerateConfig)` unchanged.
- Add the one-bundle internal CLI profile with bundle-identity-namespaced output and
  duplicate generated-path rejection.
- Split canonical bundle parsing from external-only `loader`/`file` validation; retain
  the existing `ResolvedBundle` and reject missing acquisition fields in external mode.
- Generate the ordinary Rust contract implementation surface, typed discovery helper,
  and one registration call.
- Adapt the implementation into existing descriptors/interfaces privately and route it
  through the same transaction.
- Enforce consumed-on-attempt ownership and fresh-input retry semantics.
- Keep the certified public Rust internal-plugin bundle/factory API working during the
  private CheatGear checkpoint. Remove it only in Phase E.
- Prove old `GenerateConfig` struct literals still compile, canonical default files are
  byte-identical, internal mode emits namespaced output, and two internal bundles do not
  collide.

### Mandatory CheatGear checkpoint

- Migrate Linux, Windows, and mock implementations to the generated contract directly.
- Use one PluginManager insertion/enable path for external and internal plugins.
- Run only focused compilation and real platform metadata/read/write/independent-instance
  checks.
- Stop for owner manual validation before multiplying the design across other languages.

### Phase D — six-language parity

- Implement and privately certify the identical opt-in internal generation profile for
  C++, C#, Python, Lua, and JavaScript.
- Keep only minimum language-required roots/objects in each adapter.
- Keep every existing public SDK/internal-plugin API and legacy generated helper
  working throughout this phase; do not remove or rename them yet.
- Enforce the same consumed-on-attempt/fresh-retry ownership semantics in the new
  façades without changing legacy API retry behavior.
- Certify scalar, string, array, buffer, enum, multi-argument, nested-struct, error, and
  independent-instance behavior in every language.
- Prove external-only generation emits no new internal-plugin files or dependencies and
  two namespaced internal bundles compile together in every language.

### Phase E — atomic cutover and documentation

- Remove old author-visible internal-plugin APIs and legacy unconditionally generated
  internal helper files across all six languages together.
- Switch active API, generated names, tests, examples, and docs to internal/external
  terminology in the same cutover.
- Complete the multi-page **How polyplug works** documentation section defined above.
- Update architecture, feature, language, SDK, example, and CheatGear documentation.
- Document one canonical plugin pipeline with optional internal acquisition.
- Do not retain compatibility aliases or advertise mixed old/new language surfaces.

### Phase F — final certification

Only after owner approval of the CheatGear checkpoint:

- run canonical external regression suites;
- run six-language internal plugin registration suites;
- test default external-only host generation and explicit internal-plugin generation
  separately in all six languages;
- test unchanged legacy `GenerateConfig` construction, source-neutral internal bundle
  parsing, duplicate output rejection, and two-bundle namespaced imports;
- test failure at every transaction boundary and prove no partial publication;
- test external and internal plugins through identical
  find/create/dispatch/destroy/unload application code;
- test external reload unchanged;
- test source cleanup and leak-free failure/unload for all adapters;
- test direct and cascade unload with dependent internal plugins, live instances, and
  exact-once cleanup; test producer/commit/panic/exception failures and fresh retry in
  all six languages;
- run Linux and native Windows CheatGear end-to-end checks;
- benchmark steady dispatch against the certified baseline;
- run full project quality, docs, install, and CI gates once.

## 11. Compatibility and non-regression gates

The update is rejected if any of the following occurs:

- an existing external bundle or manifest format changes;
- external signature/path/loader policy moves into or is weakened by the common core;
- existing external loader reload or unload cleanup changes observably;
- registry IDs, handles, slot generations, dependency behavior, or caller semantics
  differ by acquisition source;
- a second bundle/contract registration schema or record appears;
- core registry records gain an origin discriminator;
- application dispatch branches on internal/external origin;
- generated internal plugin registration requires manual ABI, manifest, arena, bridge,
  or caller work;
- external-only generation changes canonical host caller/external guest file contents,
  emits new internal-plugin output, or requires internal-plugin runtime dependencies;
- failed registration publishes any partial bundle or leaks source resources;
- unload releases backing resources while calls, leases, or instances can still reach
  them;
- steady dispatch adds a source check, allocation, lock, name lookup, manifest work, or
  measurable regression beyond the existing performance policy.

## 12. Final acceptance statement

The work is complete when the following statement is literally true:

> Polyplug has one canonical plugin registration, registry, caller, instance, dispatch,
> dependency, and unload model. External plugins and internal plugins use different
> acquisition mechanisms but produce the same existing bundle and contract data.
> Neither the core registry nor application callers know or care where a registered
> plugin came from.

# PP-38 — Embedded Loading and Language-Parity Audit

This is the working audit ledger for Plane parent **PP-38**. Keep it until every
accepted finding is fixed, tested, documented, and closed in Plane. It is not
product documentation; product docs describe only the final current state.

## Non-negotiable direction

- Rust, C++, C#, Python, Lua, and JavaScript/QuickJS are maintained host and guest languages.
- Generally applicable features have equivalent semantics in every maintained language.
- Language mechanics live behind generators, SDKs, and loaders.
- One canonical bundle, contract, dependency, registration, lifecycle, and discovery path serves every source.
- Bundle initialization is transactional: publish the complete bundle once or publish nothing.
- Cross-compilation proves compile/link; native execution proves runtime behavior.
- Delegated findings are leads until independently verified.

## Embedded/source architecture under discussion

The runtime already has `BundleSource::Path`, `BundleSource::Code`, and
`BundleSource::Bytes`. JavaScript, Lua, and Python support in-memory source;
.NET supports assembly bytes. `EmbeddedBundle` and `EmbeddedContract` are a
separate Rust-only registration path and are candidates for removal.

Proposed loader-owned semantics:

| Guest implementation | Canonical source | Loader behavior |
|---|---|---|
| Lua | `Code` / UTF-8 `Bytes` | Create the bundle VM and evaluate the supplied chunk. |
| JavaScript/QuickJS | `Code` / UTF-8 `Bytes` | Create the bundle VM and evaluate the self-contained bundle. |
| Python | `Code` / UTF-8 `Bytes` | Initialize/isolate the module through the Python loader. |
| .NET/C# | `Bytes` | Load the assembly into the bundle-owned collectible ALC. |
| Rust/C++ dynamic | `Path` | Delegate to the operating-system native loader. |
| Rust/C++ packaged bytes | `Bytes` | Verify and materialize a private target artifact, then delegate to the OS loader. |
| Rust/C++ linked | proposed `Linked` source | Register an exact generated C-ABI init function/table whose code outlives the runtime. |

Equivalent logical lifecycle does not imply identical physical mechanics. A
linked native implementation can destroy instances and unregister contracts,
but code linked into the host executable cannot be unmapped independently.

### Native linked safety requirements

- Accept only the exact generated `extern "C"` init/function-table type.
- Never use an arbitrary object pointer, C++ object ABI, RTTI, STL type, or exception boundary.
- The linked code and table outlive the runtime registration.
- Destroy all instances and stop callbacks/threads before logical unload.
- Catch Rust panics and C++ exceptions before they cross the C ABI.
- Keep implementation/factory state bundle-owned; never process-global.

### Decisions awaiting owner approval

1. Whether native embedded delivery supports both linked code and packaged artifact bytes.
2. Whether logical unload without physical unmapping is the required linked-native contract.
3. Whether to expand the pre-1.0 ABI so every host SDK can submit Path, Code, Bytes, and Linked sources.

## Independently verified correctness findings

### PP-43 — Cross-language revision synchronization — critical

The runtime mutates a Rust `AtomicU64` revision counter. Generated Rust callers
perform an acquire atomic load, but generated Python, Lua, and JavaScript callers
read the exposed address as ordinary memory. Atomic writes mixed with non-atomic
foreign reads do not provide the synchronization required by cached-interface
revalidation.

Required result: one genuinely atomic mechanism across all host languages plus
concurrent load/reload/unload tests for each maintained host.

Evidence:

- `crates/polyplug/src/runtime_store.rs` — `publish`, `revision_ptr`
- `crates/polyplug_codegen/src/generators/rust.rs` — acquire load
- `crates/polyplug_codegen/src/generators/python.rs` — ctypes ordinary read
- `crates/polyplug_codegen/src/generators/lua.rs` — LuaJIT FFI ordinary read
- `crates/polyplug_codegen/src/generators/js_quickjs.rs` — Deno pointer-view ordinary read

### PP-44 — Transactional bundle preparation/commit — critical

Normal loader initialization publishes contracts one by one before final bundle
metadata/provider/function-count validation. Failures can leave partial
contracts, resident resources, or dependency declarations. The embedded Rust
side path is atomic, demonstrating the semantic mismatch.

Required result: loaders stage all registrations and resident ownership; the
runtime validates the full provider/dependency/metadata set and commits one
registry snapshot. Abort publishes nothing and reclaims once.

Evidence:

- `crates/polyplug/src/runtime.rs` — `load_manifest_with_source`
- `crates/polyplug/src/runtime_store.rs` — `register_guest_contract`, `declare_bundle_dependencies`
- `crates/polyplug_lua/src/loader.rs` — sequential registration and VM ownership
- `crates/polyplug_python/src/loader.rs` — sequential registration and leaked loader-data boxes

### PP-45 — Loader parity and failure cleanup — high

- QuickJS consumes only `registrations[0]` although generated bundles may return multiple contracts.
- Python path-backed initialization can return before popping the init-bundle stack.
- Runtime success does not verify that actual registered providers equal manifest `provides`.

Required result: identical multi-contract and cleanup behavior in all loaders,
with a two-contract success/failure fixture per language and central provider-set validation.

Evidence:

- `crates/polyplug_js/src/loader.rs` — first registration only
- `crates/polyplug_codegen/src/generators/js_quickjs.rs` — emits every plugin registration
- `crates/polyplug_python/src/lib.rs` — path versus source init-stack cleanup
- `crates/polyplug/src/runtime.rs` — post-load validation observes only registered contracts

### PP-46 — Global embedded factory state — high

Generated embedded Rust uses a module-global `OnceLock<EmbeddedFactories>` and
ignores subsequent installations. Two runtimes or unload/re-registration can
silently use the first factory table.

Required result: factories and implementation state belong to the registered
bundle/loader resident, independently per runtime and registration.

Evidence: `crates/polyplug_codegen/src/generators/rust.rs`.

### Additional verified architecture defects

- `bundle_manifests` survives unload and can retain a stale reload recipe.
- C++ bundles use the `native` loader but runtime metadata maps `native` to Rust.
- `BundleInitContext` documentation promises a longer path lifetime than native/.NET loaders provide.
- External and embedded registration disagree on null lifecycle callback validity.
- In-memory sources are forced through disk-oriented manifest `file/path` and signature assumptions.

These are included under PP-40/PP-42 and must become explicit repair items if
not absorbed by the canonical transaction/source redesign.

## Independently verified simplification findings

Tracked by **PP-47**:

1. `RuntimeLanguageBridge` plus Lua/JS bridge implementations form a disconnected, incomplete second host-contract architecture. The generated `HostContractInterface` path is canonical.
2. Native, JS, and Lua loader config types carry no effective setting.
3. Six generators duplicate bundle-manifest emission.
4. `ManifestData.bundle_dependencies` duplicates the canonical `[[dependency]]` grammar and has no real manifest consumer.
5. `BundleLoader::loader_language` is explicitly informational and unused.
6. `LogSink` wraps one closure implementation and only forwards `emit`.
7. Loader manifests repeat default `[lib].name`/`path`; Lua/Python repeat normal dependencies under dev-dependencies; Python repeats Cargo's default integration-test target; Rust host examples duplicate workspace paths.

### Rejected cleanup claim: loader `cdylib` outputs

The loader crates do not define binary targets. Their `cdylib` outputs are
shared libraries loaded by non-Rust hosts through exported C-ABI loader factory
functions. Keep `crate-type = ["rlib", "cdylib"]`. The redundant configuration
around those factories is removable; the shared-library artifact is required.

## External research conclusions

Primary-runtime documentation was researched through the Firecrawl CLI.

- Native raw in-memory PE/ELF/Mach-O mapping is not portable and bypasses normal relocation, dependency, signing, W^X, and teardown behavior.
- Native bytes should be verified and materialized privately before OS loading.
- A linked native table is safe only under the strict C-ABI and lifetime rules above.
- .NET assembly bytes naturally map to `AssemblyLoadContext.LoadFromStream`.
- CPython supports source/code execution through interpreter APIs but process-wide interpreter constraints remain.
- Lua/LuaJIT and QuickJS naturally accept source or runtime-supported bytecode; bytecode compatibility must be validated by the loader/runtime version.
- Physical mechanism parity is impossible; equivalent feature semantics are achievable and required.

## Plane source of truth

- PP-38 — parent: embedded parity redesign and audit
- PP-39 — universal language feature rules
- PP-40 — one loader-owned embedded bundle path
- PP-41 — deep architecture/loader audit
- PP-42 — implementation and certification
- PP-43 — atomic revision synchronization
- PP-44 — transactional preparation/registration
- PP-45 — loader parity and failure cleanup
- PP-46 — remove global embedded factory state
- PP-47 — remove disconnected/redundant loader code

## Closure requirements

- Owner-approved source/native design.
- Every accepted finding fixed or explicitly rejected with evidence.
- Six-language host/guest parity tests.
- Cross-compilation for supported targets and native Linux/Windows execution.
- Current-state product documentation rewritten without migration narrative.
- Full local gates and GitHub workflows green.
- Plane PP-38 and children contain immutable commit and CI evidence.

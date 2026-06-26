# Glossary

Canonical definitions for the terms used throughout the polyplug documentation.
Each term is defined here once; pages link to this glossary rather than redefining
terms inline.

- **ABI** — The frozen C application binary interface the host and guests
  communicate through, defined by the `polyplug_abi` crate. It comprises the two
  `#[no_mangle]` host exports (`polyplug_runtime_create` / `polyplug_runtime_destroy`)
  and the `HostApi` function-pointer table. It freezes at v1.0 (currently pre-1.0).

- **arena** — A per-call bump allocator threaded into guest dispatch as the
  `(arena_ptr, arena_alloc)` arguments. It backs cross-boundary allocations for a
  single call and is rewound (retain-and-rewind) between calls, then freed wholesale
  at teardown. It is never a shared cell, so same-VM reentrant dispatch stays correct.

- **bundle** — A loadable plugin package. The host loads a bundle at runtime; each
  bundle exports one or more guest contracts and must export `polyplug_init`. A bundle
  is identified by its `bundle_id`.

- **contract** — A versioned interface definition (authored in a `.toml`) that one
  side implements and the other calls. A guest contract is implemented by plugins; a
  host contract is implemented by the host.

- **descriptor** — The metadata (`PluginDescriptor`: name, version, contract id)
  passed to `register_guest_contract` alongside the interface so the runtime can
  index and resolve the contract.

- **epoch** — The crossbeam-epoch reclamation domain backing lock-free registry
  reads. Readers pin a guard and serve from an immutable published view; reload and
  unload defer reclamation of the superseded interface `Arc` and the dylib/VM until no
  reader remains pinned in the prior epoch.

- **guest** — The plugin side of the boundary. Guest code (Rust, C++, C#, Python,
  Lua, or JavaScript) implements guest contracts and is called by the host.

- **guest contract** — A contract implemented by plugins for the host to call.
  Registered during `polyplug_init` via `register_guest_contract`.

- **GuestContractInterface** — The `#[repr(C)]` interface struct a plugin provides
  for the host to call. It carries the dispatch entry points for one guest contract.

- **host** — The application that owns the `Runtime`, loads bundles, and calls into
  guest contracts. The host provides the `HostApi` to guests.

- **host contract** — A contract provided and implemented by the host for guests to
  call back into (for example logging, metrics, or config). Registered via
  `register_host_contract`.

- **HostApi** — The runtime's ABI function table provided to guests during
  `polyplug_init`: a `#[repr(C)]` struct of function pointers (184 bytes, `align = 8`).
  Guests call its fields to register contracts, allocate and free cross-boundary
  memory, log, create and destroy instances, and read the revision counter.

- **HostContractInterface** — The interface struct the host registers for a host
  contract — the concrete functions guests invoke when calling back into the host.

- **hot-reload** — Re-reading a bundle's on-disk source and swapping its live
  interface in place without unloading the bundle. Supported by the native (cdylib),
  Lua, and JS loaders; the Python and .NET loaders return `HotReloadDisabled`.

- **instance** — A live object produced by a guest contract's author factory.
  Instances are created and destroyed through the host-mediated `create_guest_instance`
  / `destroy_guest_instance` calls so the runtime can pin the epoch across the
  operation and attribute each live instance to its contract.

- **instance payload** — The per-instance state carried in
  `GuestContractInstance.data`: the `HostApi` pointer captured at creation plus the
  plugin's implementation object. It is never stored in a global or class-static.

- **loader** — The per-language component that maps a bundle into a runnable form and
  presents the C-ABI surface to the runtime: `polyplug_native`, `polyplug_python`,
  `polyplug_lua`, `polyplug_js`, and `polyplug_dotnet`.

- **peer dispatch** — One guest contract calling another guest contract. Native peers
  dispatch through the cached interface pointer; VM peers route by contract id (JS
  routes through the threaded bridge).

- **revision counter** — A `u64` the runtime bumps on every load, reload, and unload.
  The `HostApi.revision_counter` field returns a pointer to it; cached callers do one
  acquire load *through that pointer* before each dispatch and re-resolve when it
  changes, so a cached interface pointer never dangles after a reload or unload.

- **StringView** — The ABI string representation: a `(ptr, len)` pair over UTF-8
  bytes. Every string crossing the boundary is a `StringView` — never a null-terminated
  C string.

- **unload** — Removing a loaded bundle from the registry. The superseded interface
  `Arc` and the underlying dylib/VM are reclaimed through epoch-deferred reclamation
  once no reader is still pinned. Using a cached interface pointer after its owning
  bundle is unloaded is documented undefined behaviour — the host must quiesce first.

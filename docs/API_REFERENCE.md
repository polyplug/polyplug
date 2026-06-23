# API Reference (rustdoc)

The full Rust API — every public type, trait, and function across the workspace
crates — is published as generated `rustdoc` alongside this site.

> **[Open the rustdoc API reference →](/polyplug/api/polyplug/index.html)**

Start at the [`polyplug`](/polyplug/api/polyplug/index.html) crate (the runtime engine),
then branch into:

- [`polyplug_abi`](/polyplug/api/polyplug_abi/index.html) — the frozen ABI types
  (`HostApi`, `BundleInitContext`, `RuntimeConfig`, `AbiError`, …).
- [`polyplug_native`](/polyplug/api/polyplug_native/index.html),
  [`polyplug_python`](/polyplug/api/polyplug_python/index.html),
  [`polyplug_lua`](/polyplug/api/polyplug_lua/index.html),
  [`polyplug_js`](/polyplug/api/polyplug_js/index.html),
  [`polyplug_dotnet`](/polyplug/api/polyplug_dotnet/index.html) — the per-language
  loaders.
- [`polyplug_codegen`](/polyplug/api/polyplug_codegen/index.html) and
  [`polyplugc`](/polyplug/api/polyplugc/index.html) — the code-generation library and
  CLI.

## Building the API docs locally

The same docs are a single command away from any checkout:

```bash
cargo doc --workspace --no-deps --open
```

Drop `--open` to just build them under `target/doc/`. The hosted copy on this site is
produced by the documentation workflow with the identical invocation, so the local and
hosted references never drift.

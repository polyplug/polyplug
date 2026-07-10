# Development Workflow — Host Apps and Plugins

polyplug has exactly two kinds of developers, and `polyplugc` has exactly two
verbs for them:

- **`generate`** — emit the polyplug-specific code: contract glue for either
  side, plus the `manifest.toml` (guest side).
- **`validate`** — check things without generating: a contract `.toml`, or an
  assembled bundle directory (`--bundle-dir`).

Compiling, linking, and bundling use your own toolchain.

## Maintainer cross-build: Linux to Windows MSVC loaders

Build and final-link every loader for `x86_64-pc-windows-msvc` from a Linux
checkout:

```sh
cargo install cargo-xwin --locked
cargo install just --locked
LUA_LIB=/absolute/path/to/windows-luajit/lib just check-windows-msvc-loaders
```

This requires a Linux x86_64 Rust toolchain with the
`x86_64-pc-windows-msvc` target installed, `cargo-xwin`, `just`, and a real
x64 MSVC/COFF static `lua51.lib` in `LUA_LIB`. Build that library from the
matching LuaJIT source with `msvcbuild.bat static`; the CI Windows job produces
and passes this exact artifact to the Linux cross-build job. On its first run,
`cargo-xwin` downloads the Windows SDK/CRT sysroot it uses for MSVC linking.

The recipe runs `cargo xwin build`, not merely `check`, so the five loaders
(native, Python, Lua, QuickJS, and .NET) are final-linked for the Windows
target. Python uses CPython's stable `python3.dll` ABI at the loader's 3.11
floor, so compilation needs neither a target interpreter nor `PYO3_CROSS_*`
variables. A deployed Windows host still needs a compatible CPython
`python3.dll` on `PATH`.

The Lua command replaces the default vendored LuaJIT with `external-luajit`.
Cargo features unify across the dependency graph: another dependency enabling
`mlua/vendored` would reactivate `luajit-src`, whose MSVC build script cannot
run on Linux. Keep the cross-build graph free of that feature; inspect it with:

```sh
cargo tree -e features --target x86_64-pc-windows-msvc \
  -p polyplug_lua --no-default-features --features external-luajit
```

The output must not contain `mlua-sys`'s `vendored` feature or `luajit-src`.

---

## Pipeline 1 — App developer (adding plugin support to an app)

```
┌─ 1. DESIGN ────────────────────────────────────────────────────────┐
│  Write api.toml — the app's plugin API:                            │
│    [[plugin_contract]]  what plugins must implement                │
│    [[host_contract]]    services the app offers back (host.*)      │
│  Publish api.toml to plugin developers — it IS the contract.       │
└────────────────────────────────────────────────────────────────────┘
┌─ 2. GENERATE ──────────────────────────────────────────────────────┐
│  polyplugc generate --api api.toml --lang <lang> --out generated/  │
│  → typed host-side callers, contract IDs, host-contract            │
│    registration glue (interface factories)                         │
└────────────────────────────────────────────────────────────────────┘
┌─ 3. EMBED ─────────────────────────────────────────────────────────┐
│  Add the polyplug host SDK for your language and register a        │
│  loader per guest language you choose to support:                  │
│                                                                    │
│    let runtime = Runtime::builder()                                │
│        .loader(NativeLoader::new(NativeConfig {}))    // always    │
│        .loader(LuaLoader::new(LuaConfig::default()))  // opt-in    │
│        .loader(JsLoader::new(JsConfig {}))            // opt-in    │
│        .loader(PythonLoader::new(PythonConfig::default()))         │
│        .config(config)                                             │
│        .build()?;                                                  │
│                                                                    │
│    let vtable = create_host_logger_interface(Box::new(MyLogger));  │
│    runtime.register_host_contract(HOSTLOGGER_CONTRACT_ID, vtable)?;│
└────────────────────────────────────────────────────────────────────┘
┌─ 4. RUN ───────────────────────────────────────────────────────────┐
│  let bundles = scanner::scan_dirs(&[plugins_dir]);                 │
│  for (path, _manifest) in &bundles {                               │
│      runtime.load_bundle(path)?;       // reads manifest.toml      │
│  }                                                                 │
│  let mut decoder = find_contract::<PipelineDecoderContract>(       │
│      runtime, PIPELINE_DECODER_CONTRACT_ID);                       │
│  decoder.decode(input)?;               // single indirect call     │
└────────────────────────────────────────────────────────────────────┘
```

Working reference hosts for all six languages live in `examples/hosts/`.

---

## Pipeline 2 — Plugin developer (writing a plugin for that app)

```
┌─ 1. RECEIVE ──────────────────────────────────────────────────────┐
│  Get api.toml from the app developer.                             │
└───────────────────────────────────────────────────────────────────┘
┌─ 2. DECLARE ──────────────────────────────────────────────────────┐
│  Write bundle.toml:                                               │
│    [bundle]                                                       │
│    name    = "decoder"                                            │
│    version = "1.0.0"                                              │
│    api     = "../api.toml"                                        │
│    loader  = "native"        # or lua | python | js-quickjs |     │
│    file    = "libdecoder.so" #    dotnet                          │
│                                                                   │
│    [[plugin]]                                                     │
│    name       = "decoder"                                         │
│    implements = ["pipeline.Decoder@1.0"]                          │
└───────────────────────────────────────────────────────────────────┘
┌─ 3. GENERATE ─────────────────────────────────────────────────────┐
│  polyplugc generate --bundle bundle.toml --lang rust --out gen/   │
│  → guest glue: typed contract stubs + polyplug_init + dispatch    │
│    shims                                                          │
│  → manifest.toml — with the precomputed bundle_id                 │
│    (id == fnv1a_64(name); never hand-written, never edited)       │
└───────────────────────────────────────────────────────────────────┘
┌─ 4. IMPLEMENT + BUILD  ← 100% your toolchain, any flags ──────────┐
│    rust:   cargo build --release                → libdecoder.so   │
│    cpp:    c++ -std=c++20 -shared -fPIC …       → libdecoder.so   │
│    csharp: dotnet build -c Release              → decoder.dll     │
│    js:     rolldown index.ts --format iife                        │
│              --platform neutral --file bundle.js → bundle.js      │
│            (one flat self-contained file — npm deps included)     │
│    lua / python: nothing to build               → decoder.lua/.py │
└───────────────────────────────────────────────────────────────────┘
┌─ 5. ASSEMBLE ─────────────────────────────────────────────────────┐
│  Your build script copies two things into a folder:               │
│    dist/decoder/                                                  │
│    ├── manifest.toml      (from step 3, regenerate on version     │
│    │                       bumps — rerun generate)                │
│    └── libdecoder.so      (your artifact, named as manifest       │
│                            [file] declares)                       │
│  Multi-file plugins: see "Bundle layout" below.                   │
└───────────────────────────────────────────────────────────────────┘
┌─ 6. VALIDATE ─────────────────────────────────────────────────────┐
│  polyplugc validate --bundle-dir dist/decoder/                    │
│  Drives the runtime loader's own checks, so mistakes surface      │
│  here — not at load time inside the host app:                     │
│    • manifest parses with the loader's real parser                │
│    • id == fnv1a_64(name)            (tamper check)               │
│    • [file] resolves for this platform and the artifact exists    │
│    • artifact extension matches runtime                           │
│      (native → .so/.dylib/.dll, lua → .lua, python → .py,         │
│       js-quickjs → .js, dotnet → .dll)                            │
│    • version parses as major.minor[.patch]                        │
└───────────────────────────────────────────────────────────────────┘
┌─ 7. SIGN (optional) ──────────────────────────────────────────────┐
│  polyplugc keygen --out keys/            (once — keep signing.key  │
│                                           secret, ship nothing)    │
│  polyplugc sign --bundle-dir dist/decoder/ --key keys/signing.key  │
│  Writes dist/decoder/bundle.sig (detached Ed25519 over a digest    │
│  of every file). Verify any time with:                            │
│    polyplugc verify --bundle-dir dist/decoder/                     │
│  Hosts that set SignaturePolicy=Required reject unsigned/tampered  │
│  bundles at load; the public key travels in bundle.sig (TOFU).    │
└───────────────────────────────────────────────────────────────────┘
┌─ 8. SHIP ─────────────────────────────────────────────────────────┐
│  Send dist/decoder/ to app users — they drop it into the app's    │
│  plugins directory. Done.                                         │
└───────────────────────────────────────────────────────────────────┘
```

Working reference guests for all six languages live in `examples/guests/`,
and `examples/build_all.sh` shows real build + assemble commands for each.

---

## Bundle layout

A bundle is a directory: `manifest.toml` + the entry artifact named by its
`[file]` field, plus any extra files the plugin needs.

```
dist/decoder/
├── manifest.toml          required, generated — never hand-edited
├── libdecoder.so          entry artifact (manifest [file])
└── …                      optional extra files, per language below
```

- **Multi-platform native bundles** use a `[file]` table instead of a string;
  the loader resolves the entry for the current platform:

  ```toml
  [file]
  linux.x86_64  = "libdecoder.so"
  macos.aarch64 = "libdecoder.dylib"
  windows.x86_64 = "decoder.dll"
  ```

- **Lua** — the loader prepends the bundle dir to `package.path` and
  `package.cpath`, so the bundle may ship extra `.lua` modules (including the
  generated `guest/` glue, which is `require`'d at runtime) and C extension
  modules; `require "somedep"` finds them inside the bundle.
- **Python** — the loader prepends the bundle dir to `sys.path`, and
  `bundle_dir/site-packages/` too if present — ship the generated `.py` glue
  and pip dependencies inside the bundle.
- **JS (QuickJS)** — single flat `bundle.js` by design: rolldown bundles your
  TypeScript, the generated glue, and any pure-logic npm packages into one
  self-contained file. No `node_modules`, no imports at runtime.
- **C#** — ship NuGet dependency assemblies alongside the plugin assembly.
- **Rust / C++** — a single shared library; nothing extra.

---

## Where things come from — quick reference

| Artifact | Produced by | Edited by hand? |
|---|---|---|
| `api.toml` | app developer | yes — it's the API design |
| `bundle.toml` | plugin developer | yes — bundle identity & contracts |
| contract glue (both sides) | `polyplugc generate` | **never** — regenerate |
| `manifest.toml` | `polyplugc generate --bundle` | **never** — regenerate |
| compiled artifact | your toolchain | n/a |
| bundle dir | your build script (`cp`) | n/a |
| bundle correctness | `polyplugc validate --bundle-dir` | n/a |

Regenerating is idempotent — unchanged files are left untouched.

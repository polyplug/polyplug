# Linux-to-Windows MSVC Cross-Compilation

This is the canonical guide for final-linking PolyPlug's Windows loader DLLs
from Linux. It covers one supported build pair only.

## Supported build matrix

| Build host | Rust target | Windows architecture | Status |
|---|---|---|---|
| Linux `x86_64` | `x86_64-pc-windows-msvc` | x86_64 | Supported |

This guide does **not** describe a macOS host, a non-`x86_64` Linux host,
Windows GNU, 32-bit Windows, ARM Windows, or another Rust target.

The repository recipe final-links five loader crates. It does not build guest
bundles, assemble a host application, or execute the target DLLs.

| Loader crate | Manifest `loader` value | What it loads |
|---|---|---|
| `polyplug_native` | `native` | Compiled C-ABI DLL bundles, including generated Rust and C++ guests. |
| `polyplug_python` | `python` | CPython plugin source and its bundle-local Python package files. |
| `polyplug_lua` | `lua` | LuaJIT plugin source and bundle-local Lua modules. |
| `polyplug_js` | `js-quickjs` | A self-contained JavaScript bundle in the embedded QuickJS engine. |
| `polyplug_dotnet` | `dotnet` | .NET assemblies, including C# plugin DLLs. |

## Prerequisites on the Linux host

Run these commands from the repository root on an x86_64 Linux machine:

```sh
rustup target add x86_64-pc-windows-msvc
cargo install cargo-xwin --locked
cargo install just --locked
```

`cargo-xwin` obtains the Windows SDK/CRT sysroot it needs for MSVC linking on
its first use. The Lua loader has one additional, target-specific input:
`LUA_LIB` must name a Linux-visible directory containing an x64 MSVC/COFF
**static** LuaJIT library named `lua51.lib`.

### Build the required `lua51.lib`

Build the library on an x64 Windows machine in an MSVC developer command
prompt. LuaJIT's `msvcbuild.bat static` produces the required target library:

```bat
git clone --depth 1 https://github.com/LuaJIT/LuaJIT.git
cd LuaJIT\src
msvcbuild.bat static
```

Transfer `LuaJIT\src\lua51.lib` to the Linux build host without renaming it,
then set `LUA_LIB` to its containing directory. For example, after placing the
file at `$HOME/polyplug-msvc-luajit/lua51.lib`, verify the input with:

```sh
export LUA_LIB="$HOME/polyplug-msvc-luajit"
test -f "$LUA_LIB/lua51.lib"
```

The input must be LuaJIT's x64 MSVC static library. A `lua51.dll`, a MinGW
archive, a 32-bit library, or a regular Lua 5.1 library is not a replacement.
The cross-build links this library statically into the Lua loader.

## Build every loader

With `LUA_LIB` set to the directory containing `lua51.lib`, this is the one
repository command:

```sh
LUA_LIB="$HOME/polyplug-msvc-luajit" just check-windows-msvc-loaders
```

The `check-windows-msvc-loaders` name is historical: its recipe runs `cargo
xwin build`, so successful completion is a final-link check for all five
Windows loader DLLs.

To run the two recipe steps directly instead, use the same feature selection
and link settings:

```sh
LUA_LIB="$HOME/polyplug-msvc-luajit" LUA_LIB_NAME=lua51 LUA_LINK=static \
  cargo xwin build --target x86_64-pc-windows-msvc -p polyplug_lua \
  --no-default-features --features external-luajit
cargo xwin build --target x86_64-pc-windows-msvc \
  -p polyplug_native -p polyplug_python -p polyplug_js -p polyplug_dotnet
```

The first invocation is deliberately separate. `polyplug_lua` defaults to
`vendored-luajit`; `--no-default-features --features external-luajit` replaces
that default with the supplied target library. The feature names are mutually
exclusive.

## Using external LuaJIT in a consumer

A consumer that cross-builds `polyplug_lua` must select external mode in its
own `Cargo.toml`:

```toml
[dependencies]
polyplug_lua = { version = "0.1.3", default-features = false, features = ["external-luajit"] }
```

Use the same `LUA_LIB`, `LUA_LIB_NAME=lua51`, and `LUA_LINK=static` environment
settings when building that consumer for `x86_64-pc-windows-msvc`.

Cargo features unify across the resolved dependency graph. Another dependency
that enables `mlua/vendored` can re-enable `luajit-src`; that build script tries
to invoke the Windows-only LuaJIT MSVC build from Linux. Inspect the repository
recipe's feature graph with:

```sh
cargo tree -e features --target x86_64-pc-windows-msvc \
  -p polyplug_lua --no-default-features --features external-luajit
```

For a consumer workspace, inspect its complete target graph as well:

```sh
cargo tree -e features --target x86_64-pc-windows-msvc
```

Neither output may contain the `mlua-sys` `vendored` feature or `luajit-src`.
Remove the dependency feature that enables it; adding `external-luajit` does
not cancel a unified vendored feature.

## Outputs and deployment

Without `CARGO_TARGET_DIR`, Cargo writes the target loader DLLs under:

```text
target/x86_64-pc-windows-msvc/debug/
```

The relevant final artifacts are `polyplug_native.dll`, `polyplug_python.dll`,
`polyplug_lua.dll`, `polyplug_js.dll`, and `polyplug_dotnet.dll`. A custom
`CARGO_TARGET_DIR` changes only the target-directory prefix.

These DLLs are link artifacts, not a complete Windows distribution. Before
shipping a Windows host, check all of the following:

1. Build the host application for its intended Windows x64 deployment. Build
   every native plugin DLL and native dependency for Windows x64; a native guest
   may use a different Windows compiler ABI, such as MinGW, when its runtime DLL
   dependencies are deployed.
2. Ship and register only the loader DLLs that the host uses, together with the
   host application and plugin bundle directories.
3. Make each `manifest.toml` select the Windows x86_64 guest artifact and keep
   every bundle-local source/module beside its entry file.
4. Ship each runtime required by the selected loaders, listed below.
5. Exercise the assembled host and real plugin bundles on an x64 Windows
   machine before release.

### Target runtime requirements

| Loader | Windows deployment expectation |
|---|---|
| Native | The guest must be an x64 Windows DLL; across PolyPlug's C ABI it may be MSVC- or MinGW-built. Ship its transitive DLL dependencies, including the applicable compiler runtime DLLs, with the application or bundle according to the Windows loader's search rules. |
| Python | `polyplug_python` is built with PyO3 `abi3-py311` and links CPython's stable `python3.dll` ABI. No target interpreter or `PYO3_CROSS_*` setting is needed to cross-build it, but a deployed host needs a compatible CPython `python3.dll` available on `PATH`. The ABI floor is Python 3.11. |
| Lua | The cross-built loader contains the supplied static x64 LuaJIT library. Deploy the Lua entry source and its bundle-local `.lua` modules; any bundled native Lua extension must also be compatible with the Windows x64 target. |
| QuickJS | QuickJS is embedded through `polyplug_js`; no Node.js, Deno, or separately installed JavaScript engine is required. Deploy one self-contained JavaScript bundle, including its generated glue and JavaScript dependencies. |
| .NET | `polyplug_dotnet` initializes `hostfxr` at runtime. Install a compatible .NET runtime with discoverable `hostfxr`; the default loader configuration requires at least `net10.0`. A .NET SDK is not required merely to run the loader. |

## What CI proves

CI deliberately splits the proof into two jobs:

1. The native `windows` job builds the workspace and runs `cargo test --workspace
   --no-fail-fast` on `windows-latest`. It builds LuaJIT with `msvcbuild.bat
   static`, then uploads `LuaJIT\src\lua51.lib` as the
   `luajit-x86_64-pc-windows-msvc` artifact.
2. The Linux `windows-cross` job depends on that Windows job, downloads the
   artifact into `target/luajit-x86_64-pc-windows-msvc`, sets `LUA_LIB` to that
   directory, and runs `just check-windows-msvc-loaders`.

The second job proves that the Linux-hosted recipe can final-link each of the
five loader DLLs for `x86_64-pc-windows-msvc` with the handed-off MSVC LuaJIT
library. It does **not** execute those cross-built DLLs or prove that an
assembled Windows application can load real bundles. The first, native Windows
build-and-test job is the separate Windows execution/test proof; release
validation still needs a Windows deployment test of the actual host and bundles.

## Troubleshooting

### `luajit-src` or `vendored` appears in the feature tree

The build graph has leaked `mlua/vendored`. Configure `polyplug_lua` with
`default-features = false` and `features = ["external-luajit"]`, then use the
feature-tree commands above to find and remove the other dependency enabling
`mlua/vendored`. Cargo's additive feature unification means external mode cannot
override vendored mode.

### `lua51.lib` is missing or rejected

`LUA_LIB` is a directory, not the library filename; the recipe requires
`$LUA_LIB/lua51.lib`. Rebuild it with LuaJIT's `msvcbuild.bat static` in an x64
MSVC environment and transfer that exact file. Do not substitute a DLL, a GNU
archive, a 32-bit/ARM library, or a library built for a different Windows ABI.

### Windows cannot find `python3.dll`

This is a deployment problem, not a missing cross-build input. Install a
compatible CPython runtime and make its `python3.dll` discoverable through
`PATH` before starting the host. The cross-build's `abi3-py311` configuration
removes the need for a target interpreter only while linking.

### Linker reports an architecture or machine-type mismatch

The Linux cross-build's **link-time inputs** must be x64 MSVC/COFF: the Rust
target is `x86_64-pc-windows-msvc`, and `lua51.lib` comes from an x64 MSVC
LuaJIT build. Do not pass a MinGW, 32-bit, or ARM library to that MSVC link.
In contrast, a native guest DLL loaded at runtime only needs to target Windows
x64; a MinGW-built guest is valid when its runtime DLL dependencies are
deployed. Rebuild the mismatched artifact for the path that uses it.

## Related documentation

- [Development workflow](WORKFLOW.md)
- [Feature guide](FEATURES.md)
- [Windows-native bundle assembly examples](languages/rust-guest.md)

# Polyplug.Cli

dotnet global tool that exposes `polyplugc` — the polyplug contract code-generator CLI.
Generates host/guest bindings in Rust, C++, C#, Python, Lua, and JavaScript from a
`.toml` contract definition.

## Install

```sh
dotnet tool install -g Polyplug.Cli
```

## Usage

After installation `polyplugc` is available on PATH:

```sh
polyplugc generate --bundle api.toml --lang csharp --out src/generated
polyplugc validate --bundle api.toml
```

## Supported platforms

| Platform   | RID          |
|------------|--------------|
| Linux x64  | linux-x64    |
| macOS ARM  | macos-arm64  |
| Windows x64| win-x64      |

## Fully offline

All three `polyplugc` binaries are **embedded in the NuGet package**. No runtime
download or internet access is required after `dotnet tool install`.

## Build from source

```sh
cargo install polyplugc
```

Or download pre-built binaries from
<https://github.com/polyplug/polyplug/releases>.

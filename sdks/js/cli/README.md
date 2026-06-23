# @polyplug/cli

`polyplugc` — the polyplug code-generator CLI, distributed as an npm package with a **prebuilt binary embedded per-platform**. Fully offline — no download at install time or runtime.

## Install

```sh
# npm
npm install -g @polyplug/cli

# bun
bunx @polyplug/cli generate --help

# deno
deno install -A npm:@polyplug/cli
```

## Usage

```sh
polyplugc generate --bundle bundle.toml --lang rust --out src/generated
polyplugc validate --bundle-dir plugins/
polyplugc --help
```

## Supported platforms

| Platform        | Architecture | Package                        |
|----------------|-------------|-------------------------------|
| Linux           | x64          | `@polyplug/cli-linux-x64`     |
| macOS           | ARM64        | `@polyplug/cli-darwin-arm64`  |
| Windows         | x64          | `@polyplug/cli-win32-x64`     |

The binary for the current platform is embedded in its optional package. On install, npm/bun/deno selects only the right package — nothing is downloaded at runtime.

If your platform is not listed, build from source:

```sh
cargo install polyplugc
```

or download a prebuilt release from https://github.com/polyplug/polyplug/releases.

## How it works

Follows the [esbuild](https://esbuild.github.io/) / [biome](https://biomejs.dev/) pattern:

- `@polyplug/cli` is the main package; it declares `optionalDependencies` on one package per supported platform.
- Each platform package (`@polyplug/cli-linux-x64`, etc.) ships the binary for that platform.
- The `bin/polyplugc.mjs` shim resolves the installed platform package at runtime and `spawnSync`s the binary with `stdio: "inherit"`, forwarding the exit code exactly.
- There is zero network I/O anywhere in this package.

## License

MIT

# C++ — polyplug

C++ is a first-class host **and** guest in polyplug. As a host it links the
native runtime through a header-only binding — registry lookups and dispatch
land in the low single-digit nanoseconds (~2.4 ns measured), with zero-copy
`StringView` / `Buffer` arguments that never leave the host allocator. As a
guest it compiles to a native `cdylib` (`.so` / `.dylib` / `.dll`) and shares
the exact ABI structs the host uses, so host and plugin agree at the type level.

## Install

The C++ SDK is **header-only**. It lives under `sdks/cpp/`, split into four
include roots:

```
sdks/cpp/
├── abi/        polyplug/abi.hpp          — frozen C ABI structs + StringView helpers
├── host/       polyplug.hpp, polyplug/*  — RAII Runtime + Builder (host side)
├── guest/      polyplug_guest.hpp, …     — Contract base + POLYPLUG_GUEST_MAIN
└── loaders/    native/ python/ lua/ js/ dotnet/ — per-language loader registration
```

There is no C++ language-registry CLI. Obtain the SDK by **vendoring the
headers** into your project (copy `sdks/cpp/` and add `abi/`, `host/`, `guest/`,
and `loaders/<lang>/` to your include path) or by pulling them from a
[GitHub release](https://github.com/polyplug/polyplug/releases). Linking a host
also needs the prebuilt runtime libraries (`libpolyplug.so` and the loader
`.so`s) from the same release.

Install the `polyplugc` CLI — it generates host callers and guest glue from a
`.toml` contract:

```bash
cargo install polyplugc                                    # from crates.io
curl -fsSL https://polyplug.github.io/install.sh | bash    # prebuilt binary
```

Or grab a binary straight from the
[GitHub Releases](https://github.com/polyplug/polyplug/releases) page.

## Guides

- **[C++ — Host (app)](cpp-host.md)** — embed the runtime, register loaders,
  load plugins of any language, call contracts.
- **[C++ — Guest (plugin)](cpp-guest.md)** — write a C++ plugin, generate glue,
  build a `cdylib`, assemble and validate the bundle.

## Examples

Working, tested code lives in the repository:

- Host: `examples/hosts/cpp/` (`host.cpp` runs the full pipeline; `main.cpp`
  builds the hot-reload host). Registers all five loaders — native, JS
  (QuickJS), Lua, Python, .NET.
- Guests: `examples/guests/cpp/` — five `cdylib` plugins implementing the
  pipeline contracts (`decoder`, `transformer`, `encoder`, `reporter`,
  `validator`), built with `g++ -std=c++20 -fPIC -shared`.

Generated host callers for the examples are at
`examples/hosts/cpp/generated/`; generated guest glue for each plugin is at
`examples/guests/cpp/<plugin>/generated/`.

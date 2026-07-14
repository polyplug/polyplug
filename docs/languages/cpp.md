# C++ — polyplug

C++ works as both a host and a guest. As a host it links the native runtime
through a header-only binding. As a guest it compiles to a native `cdylib`
(`.so` / `.dylib` / `.dll`), and host and plugin share the same ABI types. For
measured overhead, see [Performance](../PERFORMANCE.md).

## Install

**CLI** — generates host callers and guest glue from an `api.toml` contract:

```bash
cargo install polyplugc
```

**SDK** — the C++ SDK is header-only. Vendor `sdks/cpp/` into your project (or
pull it from a [GitHub release](https://github.com/polyplug/polyplug/releases))
and add the include roots you need to your compiler's include path:

```text
sdks/cpp/
├── abi/        polyplug/abi.hpp     — frozen C ABI structs + StringView helpers
├── host/       polyplug.hpp         — RAII Runtime + Builder (host side)
├── guest/      polyplug_guest.hpp   — contract base + factory glue
└── loaders/    native/ python/ lua/ js/ dotnet/ — per-language loader registration
```

Linking a host or guest also needs the prebuilt runtime libraries
(`libpolyplug.so` and the loader `.so`s) from the same release.

## Guides

- **[C++ — Host (app)](cpp-host.md)** — embed the runtime, load plugins of any
  language, call contracts.
- **[C++ — Guest (plugin)](cpp-guest.md)** — write a C++ plugin, generate glue,
  build a `cdylib`, assemble and validate the bundle.

New to polyplug? Start with the [Quick Start](../QUICKSTART.md).

## Examples

- Host: `examples/hosts/cpp/` (`host.cpp`) — registers all five loaders and runs
  the full five-stage pipeline; `main.cpp` builds the hot-reload variant.
- Guests: `examples/guests/cpp/` — five `cdylib` plugins (`decoder`,
  `transformer`, `encoder`, `reporter`, `validator`).

Generated code lives under `examples/hosts/cpp/generated/` (host callers) and
`examples/guests/cpp/<plugin>/generated/` (guest glue).

## Internal plugin profile

External plugins use the standard bundle command. An application can instead
generate one internal profile with
`polyplugc generate --bundle bundle.toml --internal --lang cpp --out ./generated`.
It supplies ordinary C++ factories to generated guest provider bindings and
receives generated host caller bindings from the committed handles; registration,
calls, and unload then follow the same pipeline as an external plugin.

## Shared generated declarations

C++ keeps the default unified output. For a split project, emit or import
DomainTypes as `guest/domain.hpp` and GuestContracts as
`guest/guest_contracts.hpp`; the complete command and ownership rules are in
the [split-output guide](../CODE_GENERATION.md#tested-specifier-forms-for-every-maintained-language).

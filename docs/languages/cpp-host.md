# C++ — Host (app)

This guide walks through embedding the polyplug runtime in a C++ application,
loading plugins written in any supported language, and calling their contracts
via generated typed callers.

See also: [C++ overview](cpp.md) · [C++ — Guest (plugin)](cpp-guest.md)

---

## 1. Get the host SDK headers

The C++ host SDK is header-only. Add the host, ABI, and the loader include roots
you need to your compiler's include path, then include the single convenience
header:

```cpp
#include <polyplug.hpp>   // RAII Runtime + Builder, brings in abi/polyplug/abi.hpp
```

`polyplug.hpp` pulls in, in dependency order: `abi/polyplug/abi.hpp` (the C ABI
structs and `StringView` helpers), `polyplug/id.hpp` (compile-time
bundle/contract IDs), `polyplug/handle.hpp`, `polyplug/error.hpp`
(`HostException` + `throw_if_error`), and `polyplug/runtime.hpp` (the RAII
`Runtime`).

A C++ host can load guest plugins written in **any** of the six supported
languages — register the matching loader header(s) below. Each loader is a tiny
header exposing `polyplug::loaders::register_<lang>(rt)`:

```cpp
#include <polyplug_loaders_native.hpp>   // register_native — always needed
#include <polyplug_loaders_js.hpp>       // register_js     (QuickJS)
#include <polyplug_loaders_lua.hpp>      // register_lua
#include <polyplug_loaders_python.hpp>   // register_python(rt, "3.11")
#include <polyplug_loaders_dotnet.hpp>   // register_dotnet(rt, "10.0")
```

Compile against the C++17 standard or later. Link the runtime and the loaders
you register:

```bash
g++ -std=c++17 -O2 \
    -I.../sdks/cpp/host -I.../sdks/cpp/abi \
    -I.../sdks/cpp/loaders/native -I.../sdks/cpp/loaders/python \
    -I.../sdks/cpp/loaders/lua    -I.../sdks/cpp/loaders/js \
    -I.../sdks/cpp/loaders/dotnet \
    -L.../target/release/deps -Wl,-rpath,.../target/release/deps \
    -o host host.cpp \
    -lpolyplug -lpolyplug_native -lpolyplug_python \
    -lpolyplug_lua -lpolyplug_js -lpolyplug_dotnet
```

The example host's `Makefile` (`examples/hosts/cpp/Makefile`) is the canonical
build reference.

## 2. Install `polyplugc`

```bash
cargo install polyplugc
```

`polyplugc` generates the typed host callers from an `api.toml` contract
definition. Re-run it whenever the contract changes.

## 3. Obtain `api.toml`

`api.toml` is the shared contract definition authored once and consumed by both
hosts and guests. It declares the contracts your plugins implement and the types
they exchange. See `examples/api.toml` for a full example with five contracts,
a `LogLevel` enum, and a host-provided `host.logger` contract.

## 4. Generate host callers

```bash
polyplugc generate --api api.toml --lang cpp --out host/generated
```

This writes generated headers into `host/generated/host/`:

```
host/generated/
├── manifest.toml               generated marker (never edit)
└── host/
    ├── host_callers.hpp         RAII caller classes (one per contract)
    ├── host_contracts.hpp       host-contract base classes + contract-ID constants
    ├── interface_factories.hpp  create_<name>_interface helpers
    └── types.hpp                generated enums and structs (namespace polyplug_generated)
```

The caller class for contract `pipeline.Decoder` is `PipelineDecoderContract`;
its contract-ID constant is `PIPELINE_DECODER_CONTRACT_ID`. Never edit these
files — regenerate when the contract changes.

Include the generated headers from your host source:

```cpp
#include "generated/host/types.hpp"
#include "generated/host/host_contracts.hpp"
#include "generated/host/interface_factories.hpp"
#include "generated/host/host_callers.hpp"
```

## 5. Build the runtime and register loaders

`polyplug::Runtime` is an RAII wrapper around the native runtime, built through a
`Builder`. The destructor calls `polyplug_runtime_destroy`; operations throw
`std::runtime_error` carrying `get_last_error()` on failure.

```cpp
auto rt = polyplug::Runtime::builder().build();

polyplug::loaders::register_native(rt);
polyplug::loaders::register_python(rt);   // optional: default min_version "3.11"
polyplug::loaders::register_lua(rt);
polyplug::loaders::register_js(rt);
polyplug::loaders::register_dotnet(rt);   // optional: default min_framework "10.0"
```

The `Builder` supports `plugin_dir(path)` (scan a directory for bundles at
`build()` time), `compatibility(mode)`, `signature_policy(policy)`,
`trusted_keys(...)`, `config(cfg)`, and `on_reload(callback)`.

### Hot-reload callback (optional)

```cpp
auto rt = polyplug::Runtime::builder()
    .on_reload([](const ReloadPhase& phase) {
        switch (phase.phase_type) {
            case ReloadPhaseType::Preparing: /* … */ break;
            case ReloadPhaseType::Reloaded:  /* … */ break;
            case ReloadPhaseType::Failed:    /* … */ break;
            case ReloadPhaseType::Unloading: /* … */ break;
        }
    })
    .build();
```

`ReloadPhaseType` is an `enum class` — use `::`-scoped access. Hot-reload is
supported for **native** (`cdylib`), **Lua**, and **JavaScript (QuickJS)**
bundles. Python and .NET bundles do not hot-reload.

> Note: builder-time `plugin_dir()` auto-loading happens **before** the loaders
> above are registered, so register loaders first and then load bundles
> explicitly (Step 7) when you need non-native languages.

### Signature policy (optional)

```cpp
auto rt = polyplug::Runtime::builder()
    .signature_policy(SignaturePolicy::Required)
    .build();
```

`Required` rejects unsigned or tampered bundles. Pin specific keys with
`.trusted_keys({...})`. See [`TRUST_MODEL.md`](../TRUST_MODEL.md) for the full
signing model.

## 6. Register a host contract (optional)

If your `api.toml` defines a host contract (a service the host provides to
plugins), implement the generated base class and register it before loading
bundles:

```cpp
class ConsoleLogger : public HostLogger {
public:
    void log(StringView message) override {
        std::cout << "[plugin] " << polyplug::abi::to_string(message) << "\n";
    }
    void log_with_level(const polyplug_generated::LogLevel& level,
                        StringView message) override {
        std::cout << "[plugin] " << polyplug::abi::to_string(message) << "\n";
    }
};

const HostContractInterface* logger_iface =
    create_host_logger_interface(std::make_unique<ConsoleLogger>());
rt.register_host_contract(logger_iface);
```

> Note: the generated enum is `polyplug_generated::LogLevel` — fully qualify it
> to avoid ambiguity with the ABI's global `::LogLevel`.

## 7. Load bundles

Discover bundle directories (each holds a `manifest.toml`) and load each one.
`load_bundle` dispatches to the registered loader that matches the bundle's
`loader` field, so a single host serves plugins of every language.

```cpp
namespace fs = std::filesystem;
for (const auto& entry : fs::directory_iterator(plugin_path)) {
    if (!entry.is_directory()) continue;
    std::string manifest = entry.path().string() + "/manifest.toml";
    if (fs::exists(manifest)) {
        rt.load_bundle(entry.path().string());
    }
}
```

## 8. Resolve a contract and call it

Resolve a handle with `find_guest_contract(contract_id, min_version)`, check it
with `polyplug::is_valid`, then construct the generated RAII caller with
`XxxContract::create(handle, host)`:

```cpp
const HostApi* host = rt.host();

GuestContractHandle handle = rt.find_guest_contract(PIPELINE_DECODER_CONTRACT_ID, 0);
if (polyplug::is_valid(handle)) {
    if (auto decoder = PipelineDecoderContract::create(handle, host)) {
        StringView out = decoder->decode(StringView{
            reinterpret_cast<const uint8_t*>("name,value,42"), 13});
        std::cout << polyplug::abi::to_string(out) << "\n";  // DECODED:name|value|42
    }
}
```

The second argument to `find_guest_contract` is a minimum packed version
(`major << 16 | minor`); pass `0` to accept any version.

`XxxContract::create` resolves the interface, calls `create_guest_instance`, and
caches the runtime's revision counter — the RAII destructor calls
`destroy_guest_instance` and frees the caller's call arena. A returned
`StringView` borrows the caller's arena and is valid only until the next
arena-backed call on the same caller. Because the caller revalidates the cached
interface against the revision counter, a hot-reloaded plugin is picked up
automatically.

## Full reference

The C++ host example at `examples/hosts/cpp/host.cpp` is the primary reference:
it registers all five loaders, registers the `host.logger` contract with a
`ConsoleLogger`, scans `POLYPLUG_PLUGIN_PATH` for bundles, loads each one, and
runs the five-contract pipeline end to end. `examples/hosts/cpp/main.cpp` builds
the hot-reload variant. Generated callers live at
`examples/hosts/cpp/generated/`, and the build is driven by
`examples/hosts/cpp/Makefile`.

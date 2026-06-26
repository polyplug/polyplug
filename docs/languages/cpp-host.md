# C++ — Host (app)

Embed the polyplug runtime in a C++ application, load plugins written in any
supported language, and call their contracts through generated typed callers.

See also: [C++ overview](cpp.md) · [C++ — Guest (plugin)](cpp-guest.md) ·
[glossary](../glossary.md)

## 1. Install

Install the CLI, vendor the C++ SDK headers, and add the host, ABI, and loader
include roots to your compiler's include path:

```bash
cargo install polyplugc
```

```cpp
#include <polyplug.hpp>                  // RAII Runtime + Builder, brings in the ABI structs
#include <polyplug_loaders_native.hpp>   // register_native — always needed for native bundles
// add a loader header per guest language you want to support:
#include <polyplug_loaders_js.hpp>       // register_js     (QuickJS)
#include <polyplug_loaders_lua.hpp>      // register_lua
#include <polyplug_loaders_python.hpp>   // register_python
#include <polyplug_loaders_dotnet.hpp>   // register_dotnet
```

A C++ host can load guests written in any supported language — register the
matching loader when you build the runtime (step 3). Compile against C++17 or
later; link `-lpolyplug` plus the loaders you register. The example host's
`Makefile` (`examples/hosts/cpp/Makefile`) is the canonical build reference.

## 2. Generate host callers

Author or obtain the shared `api.toml` contract (see `examples/api.toml`), then
generate the typed callers. Re-run whenever the contract changes.

```bash
polyplugc generate --api api.toml --lang cpp --out host/generated
```

This writes `host/generated/host/` with the RAII caller classes, host-contract
base classes, contract-ID constants, interface factories, and generated types
(namespace `polyplug_generated`). Never edit these files. For the emitted symbol
names, see [Generated names](../generated-names.md).

Include the generated headers from your host source:

```cpp
#include "generated/host/types.hpp"
#include "generated/host/host_contracts.hpp"
#include "generated/host/interface_factories.hpp"
#include "generated/host/host_callers.hpp"
```

## 3. Build the runtime

Build the runtime through its `Builder`. Register one loader per guest language —
the registration call differs per loader, and Python and .NET take an optional
minimum-version string:

```cpp
auto rt = polyplug::Runtime::builder().build();

polyplug::loaders::register_native(rt);
polyplug::loaders::register_python(rt);   // optional 2nd arg, e.g. "3.11"
polyplug::loaders::register_lua(rt);
polyplug::loaders::register_js(rt);
polyplug::loaders::register_dotnet(rt);   // optional 2nd arg, e.g. "10.0"
```

The `Builder` also supports `plugin_dir(path)`, `compatibility(mode)`,
`config(cfg)`, `signature_policy(policy)`, `trusted_keys(...)`, and
`on_reload(callback)`. The full multi-loader host is
`examples/hosts/cpp/host.cpp`.

### Hot-reload callback (optional)

Pass `.on_reload(...)` to observe reload phases. Hot-reload applies to native,
Lua, and JS bundles — see [Hot Reload](../HOT_RELOAD_DESIGN.md).

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

### Signature policy (optional)

```cpp
auto rt = polyplug::Runtime::builder()
    .signature_policy(SignaturePolicy::Required)
    .build();
```

`Required` rejects unsigned or tampered bundles; pin specific keys with
`.trusted_keys({...})`. See the [Trust Model](../TRUST_MODEL.md).

## 4. Register a host contract (optional)

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

Fully qualify `polyplug_generated::LogLevel` — an unqualified `LogLevel` is
ambiguous with the ABI's global `::LogLevel`.

## 5. Load bundles

Discover bundle directories (each holds a `manifest.toml`) and load each one.
`load_bundle` dispatches to the registered loader matching the bundle's `loader`
field, so a single host serves plugins of every language.

```cpp
namespace fs = std::filesystem;
for (const auto& entry : fs::directory_iterator(plugin_path)) {
    if (!entry.is_directory()) continue;
    if (fs::exists(entry.path().string() + "/manifest.toml")) {
        rt.load_bundle(entry.path().string());
    }
}
```

Register loaders before loading bundles.

## 6. Call a contract

Resolve a handle with `find_guest_contract(contract_id, min_version)`, check it
with `polyplug::is_valid`, then construct the generated RAII caller:

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

The second argument to `find_guest_contract` is the minimum version to accept;
pass `0` for any version. A returned `StringView`
is valid only until the next call on the same caller — see
[Call Arena](../call-arena.md). A hot-reloaded plugin is picked up automatically — see
[Hot Reload](../HOT_RELOAD_DESIGN.md); on the caller's lifecycle and unload
safety, see [Unload](../UNLOAD_DESIGN.md).

## Full reference

`examples/hosts/cpp/host.cpp` registers all five loaders, a host contract, scans `POLYPLUG_PLUGIN_PATH` for bundles, loads each one,
and runs the five-stage pipeline end to end. Generated callers live at
`examples/hosts/cpp/generated/`; the build is driven by
`examples/hosts/cpp/Makefile`.

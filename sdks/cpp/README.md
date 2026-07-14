# polyplug C++ SDK

Build polyplug hosts and plugins in C++. The SDK is header-only: the host side
links the native runtime through a RAII `Runtime` binding, and the guest side
compiles to a native `cdylib` (`.so` / `.dylib` / `.dll`) sharing the same ABI
structs the host uses.

## Install

The C++ SDK is header-only. Vendor `sdks/cpp/` into your project (or pull it
from a [GitHub release](https://github.com/polyplug/polyplug/releases)) and add
the include roots you need:

```
sdks/cpp/
├── abi/        polyplug/abi.hpp     — frozen C ABI structs + StringView helpers
├── host/       polyplug.hpp         — RAII Runtime + Builder (host side)
├── guest/      polyplug_guest.hpp   — contract base + factory glue
└── loaders/    native/ python/ lua/ js/ dotnet/ — per-language loader registration
```

Linking a host or guest also needs the prebuilt runtime libraries
(`libpolyplug.so` and the loader `.so`s) from the same release. Requires C++17+.

Install the CLI to generate bindings:

```bash
cargo install polyplugc
```

## Generate bindings

```bash
polyplugc generate --bundle bundle.toml --lang cpp --out ./generated
```

## Shared generated declarations

The command remains unified by default. For a shared C++ declaration target,
emit or import DomainTypes as `guest/domain.hpp` and GuestContracts as
`guest/guest_contracts.hpp`; see the [canonical split-output guide][codegen].


## Internal plugins

Generate internal plugin bindings explicitly; the default command above remains
the external plugin profile:

```bash
polyplugc generate --bundle bundle.toml --internal --lang cpp --out ./generated
```

The bundle-identity-namespaced output contains generated guest provider bindings
in `guest/internal_plugin.hpp` and generated host caller bindings. The header uses
the namespace `polyplug_generated::bundle_<16-lowercase-hex-bundle-id>::internal_plugin`.
Register ordinary factories through the generated façade; the alias below stands for
the namespace generated for the bundle:

```cpp
#include "generated/internal/<bundle>-<bundle-id-hex>/guest/internal_plugin.hpp"

namespace bindings = polyplug_generated::bundle_0123456789abcdef;

auto registration = bindings::internal_plugin::register_internal_plugin(
    runtime,
    [](const HostApi* host) { return std::make_unique<DecoderImpl>(host); },
    [](const HostApi* host) { return std::make_unique<ValidatorImpl>(host); });

const uint64_t bundle_id = registration.internal_plugin_id;
```

The registrar consumes its provider factories on the attempt, validates the
exact manifest provider/function/dependency set, atomically publishes it, and
returns typed callers created from the committed handles. Use those callers
exactly like generated host callers discovered after loading an external plugin.
Before `Runtime::unload_bundle(bundle_id)`, the application must quiesce every
caller and destroy all guest instances for the bundle. Every committed internal
bundle is marked privately in `Runtime`; while stateful instances remain,
`unload_bundle` returns `InternalPluginInUse` and leaves the bundle live. After
destroying or resetting callers and destroying those instances, retry the unload
(subject to normal dependency checks). This refusal is a guard, not a replacement
for host quiescence. External unload paths may warn and proceed with live instances,
so they cannot use the internal guard. A successful unload invalidates those callers
and releases the generated provider binding state; callers must not be used afterward.

## Host application

```cpp
#include <polyplug/runtime.hpp>

auto runtime = polyplug::Runtime::Builder().PluginDir("./plugins").Build();
```

## Plugin author

Implement the generated `<Contract>GuestContract` class and export the author
factory. The `HostApi` pointer is captured per instance, never in a global:

```cpp
#include "generated/guest/init.hpp"

class DecoderImpl : public PipelineDecoderGuestContract {
public:
    explicit DecoderImpl(const HostApi* host) : host_(host) {}
    StringView decode(StringView input) override {
        return polyplug::alloc_string(host_, "DECODED:" + polyplug::abi::to_string(input));
    }
private:
    const HostApi* host_;
};

PipelineDecoderGuestContract* polyplug_create_decoder(const HostApi* host) {
    return new DecoderImpl(host);
}
```

## Learn more

- [C++ — Host guide][host] — embed the runtime, hot-reload, signing
- [C++ — Guest guide][guest] — generate → implement → build → bundle
- [C++ overview][overview] · [polyplug docs][docs] · [examples][examples]

[overview]: https://github.com/polyplug/polyplug/blob/main/docs/languages/cpp.md
[host]: https://github.com/polyplug/polyplug/blob/main/docs/languages/cpp-host.md
[guest]: https://github.com/polyplug/polyplug/blob/main/docs/languages/cpp-guest.md
[docs]: https://github.com/polyplug/polyplug/tree/main/docs
[examples]: https://github.com/polyplug/polyplug/tree/main/examples
[codegen]: https://github.com/polyplug/polyplug/blob/main/docs/CODE_GENERATION.md

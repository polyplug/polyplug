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

## Host application

```cpp
#include <polyplug/runtime.hpp>

auto runtime = polyplug::Runtime::Builder().PluginDir("./plugins").Build();

auto decoder = PipelineDecoder::Create(runtime);
if (decoder) {
    auto result = decoder->Decode(input);
}
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

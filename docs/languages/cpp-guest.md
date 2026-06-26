# C++ — Guest (plugin)

Write a polyplug plugin in C++: generate the ABI glue, build a native `cdylib`,
and assemble a bundle any polyplug host can load. New to polyplug? Start with the
[Quick Start](../QUICKSTART.md).

See also: [C++ overview](cpp.md) · [C++ — Host (app)](cpp-host.md) ·
[glossary](../glossary.md)

## 1. Install

Install the CLI, vendor `sdks/cpp/guest/` and `sdks/cpp/abi/` into your project
(or pull them from a [GitHub release](https://github.com/polyplug/polyplug/releases)),
and add both to your include path:

```bash
cargo install polyplugc
```

Building also links the prebuilt runtime library (`libpolyplug.so`) from the same
release.

## 2. Write the bundle manifest

`bundle.toml` declares the bundle name, target loader, the library file per
platform, and which contracts this bundle implements. The `api` field points at
the shared `api.toml` contract (see `examples/api.toml`). A C++ plugin compiles
to a native `cdylib`, so the loader is `native`:

```toml
# bundle.toml
[bundle]
name = "my_plugin"
version = "1.0.0"
api = "../api.toml"   # path to api.toml, relative to this file
loader = "native"

[bundle.file]
linux.x86_64   = "libmy_plugin.so"
macos.aarch64  = "libmy_plugin.dylib"
macos.x86_64   = "libmy_plugin.dylib"
windows.x86_64 = "my_plugin.dll"

[[plugin]]
name = "my_plugin"
implements = ["pipeline.Decoder@1.0"]
```

`implements` names each contract as `<namespace>.<Name>@<major_version>`. Add one
`[[plugin]]` section per plugin in the bundle. To declare a runtime dependency on
another contract, add a `[[dependency]]` section:

```toml
[[dependency]]
kind        = "contract"
contract    = "pipeline.Validator"
min_version = "1.0"
```

## 3. Generate the guest glue

```bash
polyplugc generate --bundle bundle.toml --lang cpp --out generated
```

This writes the contract base class(es), instance machinery, `polyplug_init`,
generated types, and a `manifest.toml` under `generated/`. Re-run
whenever `bundle.toml` or `api.toml` changes; never edit generated files. For the
emitted symbol names, see [Generated names](../generated-names.md).

## 4. Implement the plugin

Include the generated `init.hpp` (which pulls in the contract base and the ABI
structs), subclass the generated contract, and export the factory. Full source:
`examples/guests/cpp/decoder`.

```cpp
#include "generated/guest/init.hpp"
#include <algorithm>
#include <string>

namespace polyplug_plugin {

class DecoderImpl : public PipelineDecoderGuestContract {
public:
    explicit DecoderImpl(const HostApi* host) : host_(host) {}

    StringView decode(StringView input) override {
        std::string s(polyplug::abi::to_string(input));
        std::replace(s.begin(), s.end(), ',', '|');
        return polyplug::alloc_string(host_, "DECODED:" + s);
    }

private:
    const HostApi* host_;
};

PipelineDecoderGuestContract* polyplug_create_decoder(const HostApi* host) {
    return new DecoderImpl(host);
}

}  // namespace polyplug_plugin
```

- Capture the `const HostApi*` as a per-instance member and allocate every
  return value through it (`polyplug::alloc_string(host_, ...)`) — it is the
  [instance payload](../glossary.md).
- `polyplug::abi::to_string(input)` views a `StringView`, valid only for the call.
- The factory `polyplug_create_<plugin>` lives in the `polyplug_plugin`
  namespace; the generated `init.hpp` provides `polyplug_init`,
  `polyplug_abi_version`, and the interface tables. Contract base and factory
  names come from [Generated names](../generated-names.md).

To call a host contract (such as a logging service) from your plugin,
`#include "generated/guest/host_contracts.hpp"` and resolve the typed caller. See
`examples/guests/cpp/reporter/reporter.cpp` for the full pattern.

## 5. Build

Compile to a shared library, including the guest, abi, and generated directories,
and linking the runtime:

```bash
c++ -std=c++17 -fPIC -shared -O2 \
    -I path/to/sdks/cpp/guest -I path/to/sdks/cpp/abi -I generated \
    my_plugin.cpp \
    -L path/to/runtime -lpolyplug -Wl,-rpath,'$ORIGIN' \
    -o libmy_plugin.so
```

`-Wl,-rpath,'$ORIGIN'` lets the plugin find `libpolyplug.so` shipped next to it.
On macOS use `.dylib` and `-Wl,-rpath,@loader_path`; on Windows build
`my_plugin.dll` with the matching toolchain.

## 6. Assemble the bundle

Copy the built library and the runtime it links next to the generated
`manifest.toml`:

```
dist/my_plugin/
├── manifest.toml       # from generated/manifest.toml
├── libmy_plugin.so     # the compiled plugin
└── libpolyplug.so      # the runtime, resolved via $ORIGIN rpath
```

## 7. Validate the bundle

```bash
polyplugc validate --bundle-dir dist/my_plugin
```

This checks the manifest is consistent, the declared file is present for the
current platform, and the bundle conforms to the ABI rules.

## 8. Sign the bundle (optional)

If the target host enforces a signature policy, sign the bundle:

```bash
polyplugc keygen --out keys/           # generate keypair once; keep signing.key secret
polyplugc sign --bundle-dir dist/my_plugin --key keys/signing.key
polyplugc verify --bundle-dir dist/my_plugin
```

`sign` validates the bundle, then writes a detached `bundle.sig`. See the
[Trust Model](../TRUST_MODEL.md).

## Full reference

Reference plugins:

| Plugin | Path | Contract |
|---|---|---|
| decoder | `examples/guests/cpp/decoder/` | `pipeline.Decoder` |
| transformer | `examples/guests/cpp/transformer/` | `data.Transformer` (declares a dependency) |
| encoder | `examples/guests/cpp/encoder/` | `pipeline.Encoder` |
| reporter | `examples/guests/cpp/reporter/` | `data.Reporter` (calls a host contract) |
| validator | `examples/guests/cpp/validator/` | `pipeline.Validator` |

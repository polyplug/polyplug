# C++ — Guest (plugin)

This guide walks through writing a polyplug plugin in C++, generating the ABI
glue, building a native `cdylib`, and assembling a bundle ready for any polyplug
host.

See also: [C++ overview](cpp.md) · [C++ — Host (app)](cpp-host.md)

---

## 1. Install the SDK and CLI

The C++ guest SDK is **header-only**. Vendor `sdks/cpp/guest/` and
`sdks/cpp/abi/` into your project (or pull them from a
[GitHub release](https://github.com/polyplug/polyplug/releases)) and add both to
your include path. Building also links the prebuilt runtime library
(`libpolyplug.so`) from the same release.

Install the `polyplugc` CLI — it generates the guest glue from a `.toml`
contract:

```bash
cargo install polyplugc                                    # from crates.io
curl -fsSL https://polyplug.github.io/install.sh | bash    # prebuilt binary
```

Or grab a binary straight from the
[GitHub Releases](https://github.com/polyplug/polyplug/releases) page. C++ has no
language-registry CLI of its own.

## 2. Obtain `api.toml`

`api.toml` is the shared contract definition. Your plugin implements one or more
contracts declared there. Obtain it from the API owner or author your own; see
`examples/api.toml` for a reference.

## 3. Write `bundle.toml`

`bundle.toml` is the plugin developer's manifest. It declares the bundle name,
the target loader, the on-disk library file per platform, and which contracts
this bundle implements. A C++ plugin compiles to a native `cdylib`, so the
loader is `native`:

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

`implements` names each contract as `<namespace>.<Name>@<major_version>`. A
bundle can implement multiple contracts — add one `[[plugin]]` section per
plugin. To declare a runtime dependency on another contract, add a
`[[dependency]]` section:

```toml
[[dependency]]
kind        = "contract"
contract    = "pipeline.Validator"
min_version = "1.0"
```

## 4. Generate guest glue

```bash
polyplugc generate --bundle bundle.toml --lang cpp --out generated
```

This writes:

```
generated/
├── manifest.toml               ship-ready manifest (never edit)
└── guest/
    ├── contracts.hpp           the contract base class(es) you implement
    ├── interfaces.hpp          instance machinery + factory declarations
    ├── host_contracts.hpp      host-contract call helpers (if any)
    ├── init.hpp                polyplug_init ABI entry point + factory glue
    └── types.hpp               generated enums and structs
```

Re-run this command whenever `bundle.toml` or `api.toml` changes. Never edit
generated files — fix the contract and regenerate.

## 5. Implement the plugin

Create `my_plugin.cpp`. Include the generated `init.hpp` (which pulls in the
contract base and the ABI structs), subclass the generated contract, and export
the factory:

```cpp
#include "generated/guest/init.hpp"
#include <algorithm>
#include <string>
#include <string_view>

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
    // Host handle for this runtime, captured at instance creation.
    // Stored per instance — no DSO globals.
    const HostApi* host_;
};

// Factory called by the generated create_instance for every host-created
// instance. Ownership of the returned object transfers to the instance.
PipelineDecoderGuestContract* polyplug_create_decoder(const HostApi* host) {
    return new DecoderImpl(host);
}

}  // namespace polyplug_plugin
```

Key points:

- The contract base name follows the pattern `<Namespace><Name>GuestContract`
  (e.g. `PipelineDecoderGuestContract` for `pipeline.Decoder`).
- The factory name follows `polyplug_create_<plugin_name>` where `plugin_name`
  matches the `name` field in the `[[plugin]]` section. It lives in the
  `polyplug_plugin` namespace and receives the `const HostApi*` for this runtime.
- Capture the `HostApi*` as a per-instance member. Use
  `polyplug::alloc_string(host_, ...)` to allocate return values through the host
  allocator — never put cross-boundary data on the C++ heap and return it.
- `polyplug::abi::to_string(input)` / `polyplug::abi::to_string_view(input)`
  re-borrow a `StringView`; the view is valid only for the duration of the call.
- The generated `init.hpp` provides `polyplug_init`, `polyplug_abi_version`, and
  the static interface tables. You write only the impl class and the factory.

### Calling a host contract from a guest

If `api.toml` defines a host-provided contract (such as a logging service),
`#include "generated/guest/host_contracts.hpp"` and resolve the typed caller at
call time:

```cpp
// min_version is PACKED (major << 16 | minor): request major 1, minor 0.
std::optional<HostLoggerContract> logger =
    HostLoggerContract::from_host(host_, 0x00010000U);
if (logger && logger->is_valid()) {
    logger->log("[plugin] starting");
    logger->log_with_level(polyplug_generated::LogLevel::Info, "[plugin] step 1");
}
```

`from_host` resolves the contract dynamically — a host that did not register it
yields `nullopt` and the plugin proceeds without it. See
`examples/guests/cpp/reporter/reporter.cpp` for the full pattern.

## 6. Build

Compile to a shared library, including the guest + abi headers and the generated
directory, and linking the runtime:

```bash
c++ -std=c++20 -fPIC -shared -O2 \
    -I path/to/sdks/cpp/guest \
    -I path/to/sdks/cpp/abi \
    -I generated \
    my_plugin.cpp \
    -L path/to/runtime -lpolyplug \
    -Wl,-rpath,'$ORIGIN' \
    -o libmy_plugin.so
```

`-Wl,-rpath,'$ORIGIN'` lets the plugin find `libpolyplug.so` shipped next to it
in the bundle. On macOS use `.dylib` and `-Wl,-rpath,@loader_path`; on Windows
build `my_plugin.dll` with the matching toolchain.

## 7. Assemble the bundle

Copy the built library and the runtime it links into a bundle directory
alongside the generated `manifest.toml`:

```
dist/my_plugin/
├── manifest.toml       # from generated/manifest.toml
├── libmy_plugin.so     # the compiled plugin
└── libpolyplug.so      # the runtime, resolved via $ORIGIN rpath
```

## 8. Validate the bundle

```bash
polyplugc validate --bundle-dir dist/my_plugin
```

This checks that the manifest is consistent, the declared file is present for
the current platform, and the bundle conforms to the ABI rules.

## 9. Sign the bundle (optional)

If the target host enforces a signature policy, sign the bundle:

```bash
polyplugc keygen --out keys/           # generate keypair once; keep signing.key secret
polyplugc sign --bundle-dir dist/my_plugin --key keys/signing.key
polyplugc verify --bundle-dir dist/my_plugin
```

`sign` validates the bundle, then writes `dist/my_plugin/bundle.sig` — a
detached Ed25519 signature. The signer's public key travels in `bundle.sig`, so
the host needs no key distribution to verify integrity.

## Generated names reference

For a contract `namespace.Name@major`:

| Item | Generated name |
|---|---|
| Guest contract base | `NamespaceNameGuestContract` |
| Factory export | `polyplug_create_<plugin_name>` (in `polyplug_plugin`) |
| Host-contract caller | `HostNameContract` (in `host_contracts.hpp`) |

## Full reference

The five C++ guest plugins in `examples/guests/cpp/` cover the full range:

| Plugin | Path | Contract |
|---|---|---|
| decoder | `examples/guests/cpp/decoder/` | `pipeline.Decoder` |
| transformer | `examples/guests/cpp/transformer/` | `data.Transformer` |
| encoder | `examples/guests/cpp/encoder/` | `pipeline.Encoder` |
| reporter | `examples/guests/cpp/reporter/` | `data.Reporter` (calls host contract) |
| validator | `examples/guests/cpp/validator/` | `pipeline.Validator` |

The reporter plugin is the most instructive: it demonstrates calling a
host-provided contract (`host.logger`) from inside the guest implementation.
The transformer plugin demonstrates declaring a runtime dependency on another
contract (`pipeline.Validator`).

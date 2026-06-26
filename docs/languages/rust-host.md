# Rust — Host (app)

Embed the polyplug runtime in a Rust application, load plugins written in any
supported language, and call their contracts through generated typed callers.

See also: [Rust overview](rust.md) · [Rust — Guest (plugin)](rust-guest.md) ·
[glossary](../glossary.md)

## 1. Install

Install the CLI and add the runtime crates to your app's `Cargo.toml`:

```bash
cargo install polyplugc
```

```toml
[dependencies]
polyplug        = "0.1"
polyplug_abi    = "0.1"
polyplug_native = "0.1"   # always needed for native bundles
# add a loader per guest language you want to support:
polyplug_js     = "0.1"
polyplug_lua    = "0.1"
polyplug_python = "0.1"
polyplug_dotnet = "0.1"
```

## 2. Generate host callers

Author or obtain the shared `api.toml` contract (see `examples/api.toml`), then
generate the typed callers. Re-run whenever the contract changes.

```bash
polyplugc generate --api api.toml --lang rust --out host/generated
```

This writes `host/generated/host/` with the typed caller structs, host-contract
traits, contract-ID constants, interface factories, and generated types. Never
edit these files. For the emitted symbol names, see
[Generated names](../generated-names.md).

Include the module from your binary:

```rust
#[path = "host/generated/mod.rs"]
mod generated;

use generated::host::host_callers::*;
use generated::host::types::*;
```

## 3. Build the runtime

```rust
use polyplug::runtime::Runtime;
use polyplug_abi::runtime::RuntimeConfig;
use polyplug_abi::Compatibility;
use polyplug_native::{NativeConfig, NativeLoader};
use std::sync::Arc;

let config = RuntimeConfig {
    compatibility: Compatibility::Strict,
    hot_reload_enabled: true,
    ..Default::default()
};

let runtime: Arc<Runtime> = Runtime::builder()
    .loader(NativeLoader::new(NativeConfig {}))
    .config(config)
    .build()
    .expect("runtime build");
```

Register one loader per guest language. The config argument differs by loader —
a unit struct (`JsConfig {}`) where there are no options, `Config::default()`
otherwise:

```rust
let runtime: Arc<Runtime> = Runtime::builder()
    .loader(NativeLoader::new(NativeConfig {}))
    .loader(JsLoader::new(JsConfig {}))
    .loader(LuaLoader::new(LuaConfig::default()))
    .loader(PythonLoader::new(PythonConfig::default()))
    .loader(DotnetLoader::new(DotnetConfig::default()))
    .config(config)
    .build()
    .expect("runtime build");
```

The full multi-loader host is `examples/hosts/rust/src/main.rs`.

### Hot-reload callback (optional)

Pass `.on_reload(...)` to observe reload phases. Hot-reload applies to native,
Lua, and JS bundles — see [Hot Reload](../HOT_RELOAD_DESIGN.md).

```rust
use polyplug_abi::runtime::{ReloadPhase, ReloadPhaseType};

let runtime: Arc<Runtime> = Runtime::builder()
    .loader(NativeLoader::new(NativeConfig {}))
    .config(config)
    .on_reload(|_user_data: *mut core::ffi::c_void, phase: ReloadPhase| {
        match phase.phase_type {
            ReloadPhaseType::Preparing => eprintln!("[reload] preparing"),
            ReloadPhaseType::Reloaded  => eprintln!("[reload] reloaded"),
            ReloadPhaseType::Failed    => eprintln!("[reload] failed"),
            ReloadPhaseType::Unloading => eprintln!("[reload] unloading"),
        }
    })
    .build()
    .expect("runtime build");
```

### Signature policy (optional)

```rust
use polyplug_abi::runtime::SignaturePolicy;

let runtime: Arc<Runtime> = Runtime::builder()
    .loader(NativeLoader::new(NativeConfig {}))
    .config(config)
    .signature_policy(SignaturePolicy::Required)
    .build()
    .expect("runtime build");
```

`Required` rejects unsigned or tampered bundles. See the
[Trust Model](../TRUST_MODEL.md).

## 4. Register a host contract (optional)

If your `api.toml` defines a host contract (a service the host provides to
plugins), register it before loading bundles:

```rust
use polyplug_abi::HostContractInterface;
use generated::host::host_contracts::{HOSTLOGGER_CONTRACT_ID, HostLogger};
use generated::host::interface_factories::create_host_logger_interface;

struct ConsoleLogger;

impl HostLogger for ConsoleLogger {
    fn log(&self, message: &str) {
        println!("[plugin] {}", message);
    }

    fn log_with_level(&self, level: &LogLevel, message: &str) {
        println!("[plugin][{:?}] {}", level, message);
    }
}

let vtable: &'static HostContractInterface =
    create_host_logger_interface(Box::new(ConsoleLogger));
runtime
    .register_host_contract(HOSTLOGGER_CONTRACT_ID, vtable)
    .expect("register host contract");
```

## 5. Load bundles

```rust
use polyplug::loader::scanner;
use std::path::PathBuf;

let plugin_path: PathBuf = PathBuf::from("dist");
let scan: scanner::ScanResult = scanner::scan_dirs(std::slice::from_ref(&plugin_path));

for diagnostic in &scan.diagnostics {
    eprintln!("warning: {diagnostic}");
}

for (path, _manifest) in &scan.found {
    runtime.load_bundle(path).expect("load bundle");
}
```

`scan_dirs` discovers every `manifest.toml` under the given directories;
`load_bundle` loads the bundle at the given path.

## 6. Call a contract

```rust
use polyplug_abi::{GuestContractHandle, StringView};
use generated::host::types::PIPELINE_DECODER_CONTRACT_ID;
use generated::host::host_callers::PipelineDecoderContract;

let handle: GuestContractHandle = runtime
    .find_guest_contract(PIPELINE_DECODER_CONTRACT_ID, 0)
    .expect("contract not found");

let mut caller: PipelineDecoderContract =
    PipelineDecoderContract::new(handle, runtime.as_context_ptr())
        .expect("caller init");

let input = StringView { ptr: b"name,value,42".as_ptr(), len: 13 };
let result: StringView = caller.decode(input).expect("decode failed");

// SAFETY: result is a valid StringView in host-allocator memory, live for this scope.
let s: &str = unsafe {
    std::str::from_utf8(std::slice::from_raw_parts(result.ptr, result.len))
}.expect("utf8");
println!("{s}");   // DECODED:name|value|42
```

The second argument to `find_guest_contract` is the minimum version to accept;
pass `0` for any version. A hot-reloaded plugin
is picked up automatically — see [Hot Reload](../HOT_RELOAD_DESIGN.md).

## Full reference

`examples/hosts/rust/src/main.rs` registers all five loaders, a host contract,
scans a directory, loads every bundle, and runs a five-stage pipeline end to
end. Generated callers live at `examples/hosts/rust/generated/`.

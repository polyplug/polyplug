# Rust — Host (app)

This guide walks through embedding the polyplug runtime in a Rust application,
loading plugins written in any supported language, and calling their contracts
via generated typed callers.

See also: [Rust overview](rust.md) · [Rust — Guest (plugin)](rust-guest.md)

---

## 1. Add dependencies

```toml
# Cargo.toml
[dependencies]
polyplug        = "0.1"
polyplug_abi    = "0.1"
polyplug_native = "0.1"   # always needed for native bundles
# add whichever language loaders you want to support:
polyplug_js     = "0.1"
polyplug_lua    = "0.1"
polyplug_python = "0.1"
polyplug_dotnet = "0.1"
```

A Rust host can load guest plugins written in **any** of the six supported
languages — just register the matching loader(s) when building the runtime.

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
an enum, and a host-provided logging contract.

## 4. Generate host callers

```bash
polyplugc generate --api api.toml --lang rust --out host/generated
```

This writes three files into `host/generated/host/`:

```
host/generated/
├── mod.rs
└── host/
    ├── mod.rs
    ├── host_callers.rs        typed caller structs (one per contract)
    ├── host_contracts.rs      host-contract traits + contract-ID constants
    ├── interface_factories.rs create_<name>_interface helpers
    └── types.rs               generated enums and structs
```

The caller struct for contract `pipeline.Decoder` is `PipelineDecoderContract`;
its contract-ID constant is `PIPELINE_DECODER_CONTRACT_ID`. Never edit these
files — regenerate when the contract changes.

Include the generated module from your binary:

```rust
#[path = "host/generated/mod.rs"]
mod generated;

use generated::host::host_callers::*;
use generated::host::types::*;
```

## 5. Build and configure the runtime

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

To support plugins written in other languages, register their loaders:

```rust
use polyplug_js::{JsConfig, JsLoader};
use polyplug_lua::{LuaConfig, LuaLoader};
use polyplug_python::{PythonConfig, PythonLoader};
use polyplug_dotnet::{DotnetConfig, DotnetLoader};

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

`Runtime::builder()` returns an `Arc<Runtime>` — the `Arc` target has a stable
address for its lifetime, so generated callers that cache a `*const HostApi`
remain valid as long as the `Arc` is alive.

### Hot-reload callback (optional)

```rust
use polyplug_abi::runtime::{ReloadPhase, ReloadPhaseType};

let runtime: Arc<Runtime> = Runtime::builder()
    .loader(NativeLoader::new(NativeConfig {}))
    .config(config)
    .on_reload(|_user_data: *mut core::ffi::c_void, phase: ReloadPhase| {
        match phase.phase_type {
            ReloadPhaseType::Preparing => eprintln!("[HOT-RELOAD] Preparing"),
            ReloadPhaseType::Reloaded  => eprintln!("[HOT-RELOAD] Reloaded"),
            ReloadPhaseType::Failed    => eprintln!("[HOT-RELOAD] Failed"),
            ReloadPhaseType::Unloading => eprintln!("[HOT-RELOAD] Unloading"),
        }
    })
    .build()
    .expect("runtime build");
```

Hot-reload is supported for **native** (`cdylib`), **Lua**, and **JavaScript
(QuickJS)** bundles. Python and .NET bundles return
`RuntimeError::HotReloadDisabled` from `reload()` unconditionally.

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

`Required` rejects unsigned or tampered bundles. See
[`TRUST_MODEL.md`](../TRUST_MODEL.md) for the full signing model.

## 6. Register a host contract (optional)

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

## 7. Scan and load bundles

```rust
use polyplug::loader::scanner;
use std::path::PathBuf;

let plugin_path: PathBuf = PathBuf::from("dist");
let scan: scanner::ScanResult = scanner::scan_dirs(core::slice::from_ref(&plugin_path));

for diagnostic in &scan.diagnostics {
    eprintln!("warning: {diagnostic}");
}

let bundles: Vec<(PathBuf, _)> = scan.found;
for (path, _manifest) in &bundles {
    runtime
        .load_bundle(path)
        .expect("load bundle");
}
```

`scan_dirs` discovers every `manifest.toml` under the given directories.
`load_bundle` dispatches to the registered loader that matches the bundle's
`loader` field.

## 8. Resolve a contract and call it

```rust
use polyplug_abi::GuestContractHandle;
use generated::host::types::PIPELINE_DECODER_CONTRACT_ID;
use generated::host::host_callers::PipelineDecoderContract;

let handle: GuestContractHandle = runtime
    .find_guest_contract(PIPELINE_DECODER_CONTRACT_ID, 0)
    .expect("contract not found");

let mut caller: PipelineDecoderContract =
    PipelineDecoderContract::new(handle, runtime.as_context_ptr())
        .expect("caller init");

let input = polyplug_abi::StringView {
    ptr: b"name,value,42".as_ptr(),
    len: 13,
};
let result: polyplug_abi::StringView = caller.decode(input).expect("decode failed");

// SAFETY: result is a valid StringView in host-allocator memory, live for this scope.
let s: &str = unsafe {
    std::str::from_utf8(std::slice::from_raw_parts(result.ptr, result.len))
}.expect("utf8");
println!("{s}");   // DECODED:name|value|42
```

The second argument to `find_guest_contract` is a minimum packed version
(`major << 16 | minor`); pass `0` to accept any version.

Generated callers cache the resolved `GuestContractInterface` pointer and
validate it against the runtime's revision counter on every call, so a
hot-reloaded plugin is picked up automatically without re-resolving the handle.

## Full reference

The Rust host example at `examples/hosts/rust/src/main.rs` is the primary
reference: it registers all six loaders, a host contract, scans a directory,
loads every bundle it finds, and runs a five-contract pipeline end to end.
Generated callers live at `examples/hosts/rust/generated/`.

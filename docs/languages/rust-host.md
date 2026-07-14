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

```rust,ignore
#[path = "host/generated/mod.rs"]
mod generated;

use generated::host::host_callers::*;
use generated::host::types::*;
```

## 3. Build the runtime

```rust,ignore
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

Register one loader per guest language. Loaders with no options construct directly;
other loaders use their concrete configuration type:

```rust,ignore
let runtime: Arc<Runtime> = Runtime::builder()
    .loader(NativeLoader::new(NativeConfig {}))
    .loader(JsLoader::new())
    .loader(LuaLoader::new())
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

```rust,ignore
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

```rust,ignore
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

### Inspect loaded bundles and contracts

`Runtime::bundle_descriptors()` snapshots the currently loaded bundles; each
Rust `BundleDescriptor` includes its `BundleOrigin`, dependencies, and runtime.
`Runtime::registered_contract_descriptors()` snapshots each live contract handle
with its owning bundle and plugin descriptor:

```rust,ignore
for bundle in runtime.bundle_descriptors() {
    println!("{}: {:?}", bundle.name, bundle.origin);
}
let contracts = runtime.registered_contract_descriptors();
```

`BundleOrigin::{Internal, Path(_), Code, Bytes}` is payload-free acquisition
metadata: `Code` does not retain source text and `Bytes` does not retain bundle
bytes. These snapshots contain loaded/registered state, not an application
enabled flag. Applications own any per-plugin enable/initialize policy; see
[Runtime lifecycle is not application enablement](../ARCHITECTURE.md#runtime-lifecycle-is-not-application-enablement).

## 4. Register a host contract (optional)

If your `api.toml` defines a host contract (a service the host provides to
plugins), register it before loading bundles:

```rust,ignore
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

```rust,ignore
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

```rust,ignore
use polyplug_abi::{GuestContractHandle, StringView};
use generated::host::types::PIPELINE_DECODER_CONTRACT_ID;
use generated::host::host_callers::PipelineDecoderContract;

let handle: GuestContractHandle = runtime
    .find_guest_contract(PIPELINE_DECODER_CONTRACT_ID, 0)
    .expect("contract not found");

let mut caller: PipelineDecoderContract =
    PipelineDecoderContract::new(handle, Arc::clone(&runtime))
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

Generated callers retain the supplied `Arc<Runtime>`. A caller therefore keeps
its runtime alive after the application drops its original `Arc`, while normal
unload and reload revision checks still invalidate stale contract handles.

## Full reference

`examples/hosts/rust/src/main.rs` registers all five loaders, a host contract,
scans a directory, loads every bundle, and runs a five-stage pipeline end to
end. Generated callers live at `examples/hosts/rust/generated/`.

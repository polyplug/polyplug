# Application integration

An application supplies plugin inputs to Polyplug, then uses generated host caller
bindings. It never needs separate application paths for an **external plugin** and
an **internal plugin**. Acquisition is the only origin-specific step; registration,
lookup, caller construction, instance management, dispatch, and unload follow the
same pipeline described in [Overview](overview.md).

This page uses Rust names for clarity. Other generated language profiles provide the
same two roles:

- **generated guest provider bindings** expose an internal implementation through the
  canonical guest ABI and perform its typed registration;
- **generated host caller bindings** resolve a registered contract, manage its guest
  instance, and expose ordinary typed method calls to the application.

The generated names come from the API and bundle metadata. In the examples,
`PlatformPluginContract` and `PLATFORM_PLUGIN_CONTRACT_ID` are the generated host
caller and contract identifier for `platform.Plugin`.

## External-only application

An external-only application generates host caller bindings from its API definition
and configures the runtime with the loaders it intends to use. It does not generate
internal bindings or provide an internal implementation.

```bash
polyplugc generate --api platform-api.toml --lang rust --out src/generated
```

```rust,ignore
use std::{path::Path, sync::Arc};

use generated::host::{
    host_callers::{ContractError, PlatformPluginContract},
    types::PLATFORM_PLUGIN_CONTRACT_ID,
};
use polyplug::Runtime;
use polyplug_native::{NativeConfig, NativeLoader};
use polyplug_utils::BundleId;

let runtime: Arc<Runtime> = Runtime::builder()
    .loader(NativeLoader::new(NativeConfig {}))
    .build()?;

// The external loader acquires and validates this bundle before it enters the
// canonical registration pipeline.
runtime.load_bundle(Path::new("./plugins/external-platform"))?;

let bundle_id = BundleId::new("external_platform");
let handle = runtime.find_guest_contract_by_bundle(
    bundle_id.id(),
    PLATFORM_PLUGIN_CONTRACT_ID,
    0,
)?;
let mut platform = PlatformPluginContract::new(handle, Arc::clone(&runtime))
    .ok_or("platform.Plugin was not registered by external_platform")?;

let metadata = platform.metadata()?;
```

`Runtime` owns loader selection, external bundle acquisition, validation,
registration, registry visibility, dependency and lifecycle coordination. The
generated host caller binding owns the typed contract surface and its guest-instance
lifetime; it retains the runtime so that dispatch and destruction use the same
runtime that owns the handle. Application code supplies policy such as which bundle
to select and when to enable, call, or unload it.

The `BundleId`, handle, and caller above are all canonical registry data. No external
loader object is carried into the application call site.

## Optional internal plugin

Internal bindings are opt-in and are generated for one bundle at a time. They produce
matching generated guest provider bindings and generated host caller bindings under a
bundle-identity namespace. Generate them only when the application supplies an
internal implementation:

```bash
polyplugc generate --bundle platform.toml --internal --lang rust --out src/generated
```

The generated provider input contains ordinary typed factories. The generated
registration function consumes that input, adapts the implementations privately, and
returns the canonical bundle identity plus the exact typed callers committed by the
transaction.

```rust,ignore
use generated::guest::domain::InternalProviders;
use generated::guest::init::register;

let registration = register(
    Arc::clone(&runtime),
    InternalProviders {
        platform_plugin_platform_plugin: platform_provider_factory(),
    },
)?;

println!("registered bundle {}", registration.bundle_id.id());
let mut platform = registration.platform_plugin_platform_plugin;
let metadata = platform.metadata()?;
```

The names inside `InternalProviders` and `Registration` are generated from the bundle
and the provided contracts; the generated profile gives the application their exact
names. The application does not hand-write a manifest, ABI callback, arena, bridge,
or registry record.

Registration validates the full generated provider set before publishing it. On
success, the returned callers are built from the exact committed handles. On failure,
Polyplug rolls the attempt back atomically, releases uncommitted provider ownership,
and makes no partial bundle visible. A failed input has been consumed; retry with a
fresh generated provider input.

## Identical use after acquisition

After either example has produced `platform`, application code calls the same typed
host caller binding. It does not inspect where the plugin came from.

```rust,ignore
fn start_platform(platform: &mut PlatformPluginContract) -> Result<(), ContractError> {
    platform.initialize()?;
    let metadata = platform.metadata()?;
    println!("using {}", metadata.name);
    Ok(())
}

start_platform(&mut platform)?;
```

A caller resolves through the runtime registry, creates and destroys its instance,
and dispatches typed methods. When a bundle is reloaded or unloaded, callers observe
the normal registry revision and lifecycle behavior; they do not receive an
acquisition-origin branch. Before unload, the application must stop using the caller
and release it according to its language's ownership rules. See
[Registry and calls](registry-and-calls.md) and [Lifecycle](lifecycle.md) for the
handle, instance, reload, and unload rules.

## CheatGear `PluginManager` example (CheatGear application code)

`PluginManager` is a CheatGear application type, not a Polyplug API. CheatGear uses
one `Runtime` and one manager insertion/enable path for its selected platform provider
and for discovered external bundles.

| Deployment or test selection | Internal provider supplied to the generated registration | External bundles |
| --- | --- | --- |
| Linux | CheatGear's Linux `platform.Plugin` provider factory | `PluginManager` scans the configured plugin directory and loads each selected external bundle into the same runtime. |
| Windows | CheatGear's Windows `platform.Plugin` provider factory | The same scan/load and insertion path accepts Windows-compatible external bundles. |
| Fixture or unit test | A fresh mock `platform.Plugin` provider factory for that test | The manager can still discover external bundles; the mock caller and external callers use the same enable and dispatch path. |

Conceptually, the manager construction is:

```rust,ignore
// CheatGear code, not Polyplug API.
let runtime = create_runtime()?;
let registration = generated::guest::init::register(
    Arc::clone(runtime.runtime()),
    generated::guest::domain::InternalProviders {
        platform_plugin_platform_plugin: selected_platform_provider(),
    },
)?;

let mut plugins = PluginManager::new_with_platform_registration(
    plugin_directory,
    runtime,
    registration,
)?;

// Construction inserts the generated platform caller, discovers external bundles in
// plugin_directory, then the existing manager policy selects and enables callers.
let platform = plugins.enable_default_platform_plugin()?;
platform.with_caller(PlatformPluginContract::metadata)?;
```

The Linux and Windows factories provide the operating-system implementation; a test
passes a fresh mock factory instead. In every case, the generated registration returns
a typed `platform.Plugin` caller. `PluginManager` records that caller and callers from
external bundles with the same metadata, enable, dispatch, reload where supported, and
unload policy. CheatGear therefore keeps its application policy while Polyplug remains
responsible for the shared runtime pipeline.

## Responsibility boundary

| Application | Polyplug runtime and generated bindings |
| --- | --- |
| Select external bundle locations and, optionally, internal implementations. | Acquire the selected input and preserve the backing resources required by that acquisition. |
| Choose application policy for discovery, enablement, selection, and shutdown. | Validate and atomically register complete bundles; maintain the registry and dependency state. |
| Use generated typed callers as ordinary application objects. | Construct typed callers, create/dispatch/destroy instances, coordinate reload where supported, and unload safely. |

For the preceding stages, continue with [Acquisition](acquisition.md),
[Registration](registration.md), and [Generated bindings](generated-bindings.md).

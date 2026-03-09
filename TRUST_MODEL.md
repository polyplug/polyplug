# Trust Model — polyplug

This document defines the security boundaries, dependency enforcement mechanisms, and trust assumptions of the polyplug runtime platform.

## Overview

In the polyplug ecosystem, the trust model defines how bundles interact and how the runtime ensures that these interactions follow the declared architecture. The model is built on three pillars: bundle identity, declared dependencies, and an enforcement window. Polyplug is designed for performance first, meaning it prioritizes catch-at-load-time configuration errors over expensive runtime-only checks.

## Bundle Identity

Every plugin bundle is uniquely identified by a `bundle_id`. This ID is the FNV1a-64 hash of the bundle name string provided in the `bundle.toml`.

- **Unique per deployment**: Bundle names must be unique within a single application deployment to avoid ID collisions.
- **Implicit context**: The `bundle_id` is baked into the generated code by `polyplugc`.
- **System context**: A `bundle_id` of `0` represents a lack of bundle context, typically used by the host application or internal runtime operations.

## Declared Dependencies

Dependencies are explicitly declared in the `bundle.toml` file under the `[[dependency]]` section. This declaration serves as a contract between the bundle and the runtime.

- **Explicit access**: Bundles can only resolve contracts they have explicitly declared as dependencies.
- **Verification**: The `declare_deps(bundle_id, contract_ids)` call registers these requirements in the runtime's internal registry.
- **Enforcement**: If a bundle attempts to resolve a contract it did not declare, the resolution fails immediately.

## Enforcement Window

Dependency enforcement is a load-time check. It is strictly applied during the initialization phase.

- **The Window**: Enforcement applies only when `polyplug_init()` is running (indicated by a non-zero `INIT_BUNDLE_ID`).
- **The Reason**: Init-time enforcement catches misconfigured dependency graphs before the application starts its main loop.
- **Hot-path performance**: Runtime calls like `find_by_contract` made from within a running plugin *after* initialization are not subject to enforcement. This avoids adding latency to the hot path, as the architecture assumes that if the initialization passed, the graph is valid and trusted.

## Multi-impl Resolution

Polyplug supports multiple bundles implementing the same contract. The resolution logic follows these rules:

- **Registration Order**: `find_by_contract` returns the first implementation registered in the system.
- **Specific Lookup**: `find_by_bundle` allows a caller to request a contract from a specific bundle.
- **Enumeration**: `find_all_by_contract` provides a list of all providers for a given contract.
- **Prioritization**: No priority or weighting system exists in the current version; implementations are treated equally based on their registration sequence.

## Threat Model

The polyplug trust model is a software architecture tool, not a hardware-enforced security boundary.

- **What it protects against**: Accidental access to undeclared dependencies during initialization. It catches `bundle.toml` misconfigurations and ensures that the dependency graph is complete and understood at load time.
- **What it does NOT protect against**: Malicious plugins that intentionally bypass the initialization phase, runtime memory tampering, cross-process attacks, or memory corruption. Polyplug assumes that all loaded plugins are trusted code within the same process.

## ABI Freeze Notice

The polyplug core ABI was re-frozen at Epic 9.7 to ensure long-term stability for host and guest integrations. The frozen surface includes:

- **HostVTable**: A 56-byte structure containing 7 function pointers.
- **PluginVTable / PluginHandle / PluginDescriptor / PluginRegistrar**
- **AbiError / StringView / Buffer**

Any new functionality must be introduced through the extension system (`get_extension`) to maintain ABI compatibility without breaking existing integrations.

## Future Work

The trust model will evolve alongside the platform's capabilities:

- **Hot-reload (Epic 10)**: Reloading a bundle will increment its generation counter. Stale handles from previous generations will be rejected by the `resolve_guard` to prevent use-after-unload errors.
- **Scripting Bindings (Epics 10/11)**: Host and guest libraries for Python and Lua will integrate with the trust model, using `ctypes` and FFI to maintain the same dependency enforcement rules.
- **Priority Resolution**: Future versions may introduce a weighting system for multi-implementation resolution, allowing developers to prefer specific providers.

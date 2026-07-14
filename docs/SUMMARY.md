# Summary

[Introduction](README.md)

# Getting Started

- [Quick Start](QUICKSTART.md)
- [Examples](EXAMPLES.md)
- [Development Workflow](WORKFLOW.md)
- [Code generation and split output](CODE_GENERATION.md)
- [Linux-to-Windows MSVC Cross-Compilation](CROSS_COMPILATION.md)

# How polyplug works

- [Overview](how-it-works/overview.md)
- [Generated bindings](how-it-works/generated-bindings.md)
- [Acquisition](how-it-works/acquisition.md)
- [Registration](how-it-works/registration.md)
- [Registry and calls](how-it-works/registry-and-calls.md)
- [Lifecycle](how-it-works/lifecycle.md)
- [Application integration](how-it-works/application-integration.md)

# Languages

- [Rust](languages/rust.md)
  - [Host (app)](languages/rust-host.md)
  - [Guest (plugin)](languages/rust-guest.md)
- [C++](languages/cpp.md)
  - [Host (app)](languages/cpp-host.md)
  - [Guest (plugin)](languages/cpp-guest.md)
- [C#](languages/csharp.md)
  - [Host (app)](languages/csharp-host.md)
  - [Guest (plugin)](languages/csharp-guest.md)
- [Python](languages/python.md)
  - [Host (app)](languages/python-host.md)
  - [Guest (plugin)](languages/python-guest.md)
- [Lua](languages/lua.md)
  - [Host (app)](languages/lua-host.md)
  - [Guest (plugin)](languages/lua-guest.md)
- [JavaScript](languages/js.md)
  - [Host (app)](languages/js-host.md)
  - [Guest (plugin)](languages/js-guest.md)

# Concepts & Architecture

- [Architecture Overview](ARCHITECTURE.md)
- [The Instance Model](ARCHITECTURE_CLARIFICATIONS.md)
- [Plugin Interface Design](PLUGIN_INTERFACE_DESIGN.md)
- [Hot Reload](HOT_RELOAD_DESIGN.md)
- [Unload & Reclamation](UNLOAD_DESIGN.md)
- [Host Contracts](HOST_CONTRACTS.md)
- [Native String Helpers](DESIGN_DECISIONS.md)
- [Call Arena](call-arena.md)

# Reference

- [Generated Names](generated-names.md)
- [CLI (`polyplugc`)](cli.md)
- [ABI Types](abi_types.md)
- [ABI Architecture](ABI_ARCHITECTURE.md)
- [SDK Helpers](SDK_HELPERS.md)
- [Reload Limitations](RELOAD_LIMITATIONS.md)
- [Feature Guide](FEATURES.md)
- [API Reference (rustdoc)](API_REFERENCE.md)
- [Glossary](glossary.md)

# Security & Trust

- [Trust Model](TRUST_MODEL.md)
- [Security Policy](security-policy.md)

# Operations

- [Debugging Native Crashes](DEBUGGING_NATIVE_CRASHES.md)
- [Performance](PERFORMANCE.md)
- [Profiling](PROFILING.md)


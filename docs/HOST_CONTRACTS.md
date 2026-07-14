# Host contracts

A host contract is a service the host implements for guests to call back into —
logging, metrics, configuration, resource access, event emission. It is the
inverse of a guest contract: where the host calls a guest contract the plugin
implements, a guest calls a host contract the *host* implements. Together they
make the boundary bidirectional.

See the [glossary](glossary.md) for `HostApi`, `HostContractInterface`,
`GuestContractInterface`, and *host contract*. For the per-language mechanics of
registering one (host side) and calling one (guest side), follow the per-language
**Host** and **Guest** guides in the [Languages section](README.md) — this page is
about the model and the rules, not the step-by-step.

## How host contracts differ from guest contracts

| Aspect | Guest contract | Host contract |
|---|---|---|
| Direction | Host calls guest | Guest calls host |
| Implementation | Plugin implements | Host implements |
| Registration | Via `polyplug_init` → `register_guest_contract` | Via `register_host_contract` |
| Discovery | Host finds the plugin | Plugin queries the host |
| Use case | Plugin functionality | Host services (logging, metrics, config) |

A host contract name **must** start with the `host.` prefix. The prefix feeds the
contract-id hash (`host_contract:<name>@<major>` vs `guest_contract:<name>@<major>`),
so `host.logger` and a hypothetical `plugin.logger` can never collide. The id
derivation lives in [`TRUST_MODEL.md`](TRUST_MODEL.md); the generated constant
names (`{NS_TYPE}_CONTRACT_ID`) live in [Generated names](generated-names.md).

```toml
# api.toml — host contract names MUST start with "host."
[[host_contract]]
name = "host.logger"
version = "1.0.0"

[[host_contract.functions]]
name = "log"
params = [{ name = "message", type = "StringView" }]
return = "void"
```

## How the implementation is carried

A host contract factory holds no static or thread-local state. The registrant's
implementation pointer is stored in the `user_data` field of
`HostContractInterface` (offset 40), and `create_instance` / `destroy_instance`
recover it via `(*this).user_data`. The runtime never reads, writes, or frees the
pointee — it only stores the pointer. (C# and Python keep an additional
managed-side reference to the implementation object by documented necessity, so
the GC does not collect it while the runtime holds the raw `user_data` pointer.)

## Singleton vs per-instance

Each `[[host_contract]]` carries a `singleton` flag (defaults to `false` — i.e.
per-instance). It controls the **runtime's** instance caching:

- `singleton = true` — the runtime creates the instance once (lazily, on the first
  `get_host_contract`) and hands the same `HostContractInstance` to every plugin
  caller (cached in `singleton_instances`).
- `singleton = false` — the runtime calls the provider's `create_instance` once per
  `get_host_contract` caller, so each caller receives its own instance and
  `destroy_instance` reclaims it.

Whether distinct instances actually hold **independent state** also depends on the
provider's `create_instance`:

- **Lua and JavaScript (Deno) host providers** build a fresh implementation from a
  registered factory per `create_instance` and key it by a non-zero instance id, so
  `singleton = false` yields genuinely independent per-instance state. The Deno
  provider uses native dispatch via `Deno.UnsafeCallback` (the SDK's
  `buildHostContractInterface`); see
  `sdks/lua/host/tests/test_host_contract_per_instance.lua` and
  `sdks/js/host/tests/host_contract_provider_test.ts`. These LuaJIT/Deno host-SDK
  suites have no cargo coverage (the Deno host SDK runs only under Deno, the lua
  provider only under LuaJIT), so CI runs them through `just test-host-lua` /
  `just test-host-js` in the `test` job — the same recipes a developer runs locally.
- **Native host providers (Rust/C++/C#) and the Python provider** carry a single
  implementation pointer through `user_data`, so `create_instance` returns that same
  implementation regardless of the flag — they are single-implementation by design.
  Use `singleton = true` for them to make the intent explicit.

## Version negotiation

A guest requests a host contract by minimum minor version; the runtime resolves it
against what the host registered:

1. The guest specifies the minimum minor version it requires.
2. The host returns the interface if its minor version `>=` the requested minor.
3. The major version must match exactly.
4. On any incompatibility the guest receives `null` and must handle it gracefully.

```rust,ignore
// Guest side — request with minimum minor version 2.
let logger = unsafe { HostLoggerCaller::from_host(host, 2) };

// host implements 1.3, guest needs >= 1.2  → success
// host implements 1.1, guest needs >= 1.2  → None
// host implements 2.0, guest needs >= 1.2  → None (major mismatch)
```

Because the host may not implement a requested contract at all, a host-contract
caller is **always** optional on the guest side. Treat a missing or invalid caller
as a normal path, not an error:

```rust,ignore
let logger = unsafe { HostLoggerCaller::from_host(host, 1) };
if let Some(logger) = logger {
    if logger.is_valid() {
        logger.log("processing input")?;
    }
}
// Continue even if the host provides no logger.
```

The call itself can still fail; propagate or absorb the error per language
convention (`Result` in Rust/C++/C#, exceptions in Python/JS, returned status in
Lua) — never let a failed host call abort work that does not depend on it.

## Best practices

- **Make host contracts optional.** A plugin must work when the host implements no
  host contract at all — guard every call as shown above.
- **Use descriptive names.** `host.logger`, `host.metrics.recorder` — not
  `host.func1`.
- **Keep interfaces small.** One focused, single-responsibility contract per concern
  (`host.logger` separate from `host.metrics`), not one catch-all contract.
- **Document side effects.** State what each function does to host state and how the
  host may filter or discard the call.
- **Respect VM threading.** For VM-based hosts the runtime serializes state access —
  Python acquires the GIL per call, Lua and JavaScript serialize context access by
  mutex. Keep implementations thread-safe and avoid long-running operations that
  block other callers.

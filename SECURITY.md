# Security Policy

## Reporting a vulnerability

**Please do not open a public issue for security vulnerabilities.**

Report privately through GitHub's private vulnerability reporting:

> Repository **Security** tab → **Report a vulnerability**
> (<https://github.com/polyplug/polyplug/security/advisories/new>)

This opens a private advisory visible only to you and the maintainers, and lets
us coordinate a fix and (if warranted) a CVE before any public disclosure.

Please include:

- The affected component and version (`polyplug` runtime, a specific loader, a
  codegen target, or one of the published SDKs).
- A description of the issue and its impact.
- Reproduction steps or a proof-of-concept, ideally a minimal bundle + host.
- Any suggested fix or mitigation.

We aim to acknowledge a report within **5 business days** and to keep you updated
as we investigate. As a pre-1.0 project this is best-effort, not a contractual
SLA. Coordinated disclosure is appreciated — we will credit reporters who want
credit once a fix is released.

## Supported versions

polyplug is pre-1.0; the ABI and APIs may change between minor versions. Security
fixes land on `main` and ship in the next `0.1.x` release.

| Version | Supported |
|---|---|
| latest `0.1.x` | ✅ |
| older `0.1.x` | ❌ (upgrade to the latest) |

## Scope — what is and isn't a vulnerability

polyplug is an **architecture-enforcement runtime, not a security sandbox.** Read
[`docs/TRUST_MODEL.md`](docs/TRUST_MODEL.md) for the full trust model. The trust
boundaries are deliberate, so the following are **by design and not
vulnerabilities**:

- **Plugins run in-process with full host privileges.** A loaded bundle shares
  the host's address space and can read/write host memory, call syscalls, and
  crash the process. polyplug is for *trusted* plugins; isolate untrusted code at
  the OS level (containers, separate processes).
- **A plugin crash takes down the host.** There is no in-process fault isolation
  between a plugin and its host — this is an explicit non-goal.
- **No runtime resource limits / watchdog.** Per-call timeouts and resource caps
  are a host-side concern, not a runtime feature.
- **`SignaturePolicy::Off` (the default) loads unsigned bundles.** Signature
  enforcement is opt-in; running with it off is a host configuration choice.

In scope (please **do** report):

- Memory-safety defects reachable **without** a malicious in-process plugin —
  e.g. a use-after-free, data race, or out-of-bounds in the runtime, a loader, or
  generated code triggerable by a *well-behaved* bundle or by host inputs.
- A bundle that bypasses **declared-dependency enforcement** or impersonates
  another bundle's identity despite manifest validation.
- A way to defeat **bundle signing** under `SignaturePolicy::Required` — e.g.
  loading a tampered or unsigned bundle without the documented error, or forging
  a `bundle.sig` that verifies against a different bundle's contents.
- A use-after-unload / use-after-reload that is reachable while honoring the
  documented quiesce-before-unload contract.
- Vulnerabilities in the published SDK packages (PyPI, npm, crates.io, NuGet,
  LuaRocks) or in the release/CI supply chain.

When in doubt, report it privately — we would rather triage an out-of-scope
report than miss a real one.

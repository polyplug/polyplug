# polyplugc CLI

`polyplugc` generates the language-specific glue for a contract and validates,
signs, and verifies the bundles you ship. It has five subcommands.

| Command | Purpose |
|---|---|
| `generate` | Generate generated host caller bindings, external plugin guest provider bindings, or an explicit internal plugin profile. |
| `validate` | Validate an `api.toml` / `bundle.toml`, or an assembled bundle directory. |
| `keygen` | Generate an Ed25519 keypair for bundle signing. |
| `sign` | Sign a bundle directory, writing a `bundle.sig`. |
| `verify` | Verify a bundle directory's `bundle.sig`. |

Run `polyplugc <command> --help` for the same detail at the terminal.

## generate

```bash
polyplugc generate [OPTIONS] --lang <LANG> --out <OUT>
```

| Flag | Required | Description |
|---|---|---|
| `--api <API>` | one of `--api` / `--bundle` | Path to `api.toml`. Generates host caller bindings. |
| `--bundle <BUNDLE>` | one of `--api` / `--bundle` | Path to `bundle.toml`. Without `--internal`, generates ordinary external-plugin guest provider bindings, `polyplug_init`, dispatch shims, and `manifest.toml`. |
| `--internal` | only with `--bundle` | Generates the explicit internal plugin profile: bundle-identity-namespaced generated guest provider bindings and generated host caller bindings. No bundle artifact is acquired or synthesized. |
| `-l`, `--lang <LANG>` | yes | Target language: `rust`, `cpp`, `csharp`, `python`, `lua`, or `js-quickjs`. |
| `-o`, `--out <OUT>` | yes | Primary output directory. Bindings stay here. Without layout flags, all partitions remain in this unified tree. |
| `--domain-types-out <ROOT>` | paired with `--domain-types-import` | Emits the public DomainTypes partition under `ROOT`; generated consumers import the supplied specifier. |
| `--domain-types-import <IMPORT>` | optional | With `--domain-types-out`, identifies emitted DomainTypes. Alone, selects ImportOnly: emit no DomainTypes and import an existing canonical package/module. |
| `--domain-types-omit` | optional | Emits and references no DomainTypes. It conflicts with both DomainTypes flags. |
| `--guest-contracts-out <ROOT>` | paired with `--guest-contracts-import` | Emits the GuestContracts declarations under `ROOT`; generated consumers import the supplied specifier. |
| `--guest-contracts-import <IMPORT>` | optional | With `--guest-contracts-out`, identifies emitted GuestContracts. Alone, selects ImportOnly. |
| `--guest-contracts-omit` | optional | Emits and references no GuestContracts. It conflicts with both GuestContracts flags. |

The default remains fully unified. Split output is opt-in; each language validates
its import specifier before generation, and generation rejects a binding that
references an omitted or unimportable partition. See [Code generation and split
output](CODE_GENERATION.md) for the semantic partition model, all six accepted
import forms, ordinary and internal commands, and multi-root write limits.

## validate

```bash
polyplugc validate [OPTIONS]
```

| Flag | Description |
|---|---|
| `--api <API>` | Path to an `api.toml` to validate. |
| `--bundle <BUNDLE>` | Path to a `bundle.toml` to validate. |
| `--bundle-dir <BUNDLE_DIR>` | Path to an assembled bundle directory (`manifest.toml` + entry artifact). Validated against the runtime loader's own manifest machinery, so the CLI accepts exactly what the runtime would. |

## keygen

```bash
polyplugc keygen --out <OUT>
```

| Flag | Required | Description |
|---|---|---|
| `-o`, `--out <OUT>` | yes | Directory to write the keypair into. Writes `signing.key` (private, `0o600` on Unix — keep secret) and `verifying.key` (public — distribute or embed). |

## sign

```bash
polyplugc sign --bundle-dir <BUNDLE_DIR> --key <KEY>
```

Runs the same checks as `validate --bundle-dir`, then computes the canonical
bundle digest and writes a detached `bundle.sig`.

| Flag | Required | Description |
|---|---|---|
| `--bundle-dir <BUNDLE_DIR>` | yes | Path to the assembled bundle directory. |
| `--key <KEY>` | yes | Path to the signing key produced by `keygen`. |

## verify

```bash
polyplugc verify --bundle-dir <BUNDLE_DIR>
```

Exits zero on success, non-zero on failure.

| Flag | Required | Description |
|---|---|---|
| `--bundle-dir <BUNDLE_DIR>` | yes | Path to the assembled bundle directory. |

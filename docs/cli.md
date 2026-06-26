# polyplugc CLI

`polyplugc` generates the language-specific glue for a contract and validates,
signs, and verifies the bundles you ship. It has five subcommands.

| Command | Purpose |
|---|---|
| `generate` | Generate host- or guest-side code from an `api.toml` or `bundle.toml`. |
| `validate` | Validate an `api.toml` / `bundle.toml`, or an assembled bundle directory. |
| `keygen` | Generate an Ed25519 keypair for bundle signing. |
| `sign` | Sign a bundle directory, writing a `bundle.sig`. |
| `verify` | Verify a bundle directory's `bundle.sig`. |

Run `polyplugc <command> --help` for the same detail at the terminal.

## generate

```
polyplugc generate [OPTIONS] --lang <LANG> --out <OUT>
```

| Flag | Required | Description |
|---|---|---|
| `--api <API>` | one of `--api` / `--bundle` | Path to `api.toml`. Generates host-side code (typed callers + registration glue). |
| `--bundle <BUNDLE>` | one of `--api` / `--bundle` | Path to `bundle.toml`. Generates guest-side code (contract stubs, `polyplug_init`, dispatch shims, and a `manifest.toml`). |
| `-l`, `--lang <LANG>` | yes | Target language: `rust`, `cpp`, `csharp`, `python`, `lua`, `js-quickjs`. |
| `-o`, `--out <OUT>` | yes | Output directory for the generated files. |

The symbols each language emits are listed in [Generated names](generated-names.md).

## validate

```
polyplugc validate [OPTIONS]
```

| Flag | Description |
|---|---|
| `--api <API>` | Path to an `api.toml` to validate. |
| `--bundle <BUNDLE>` | Path to a `bundle.toml` to validate. |
| `--bundle-dir <BUNDLE_DIR>` | Path to an assembled bundle directory (`manifest.toml` + entry artifact). Validated against the runtime loader's own manifest machinery, so the CLI accepts exactly what the runtime would. |

## keygen

```
polyplugc keygen --out <OUT>
```

| Flag | Required | Description |
|---|---|---|
| `-o`, `--out <OUT>` | yes | Directory to write the keypair into. Writes `signing.key` (private, `0o600` on Unix — keep secret) and `verifying.key` (public — distribute or embed). |

## sign

```
polyplugc sign --bundle-dir <BUNDLE_DIR> --key <KEY>
```

Runs the same checks as `validate --bundle-dir`, then computes the canonical
bundle digest and writes a detached `bundle.sig`.

| Flag | Required | Description |
|---|---|---|
| `--bundle-dir <BUNDLE_DIR>` | yes | Path to the assembled bundle directory. |
| `--key <KEY>` | yes | Path to the signing key produced by `keygen`. |

## verify

```
polyplugc verify --bundle-dir <BUNDLE_DIR>
```

Exits zero on success, non-zero on failure.

| Flag | Required | Description |
|---|---|---|
| `--bundle-dir <BUNDLE_DIR>` | yes | Path to the assembled bundle directory. |

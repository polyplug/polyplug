# Code generation and split output

`polyplugc generate` turns a validated `api.toml` or `bundle.toml` into source
code. The schema is directional: `[[guest_contract]]` declares functionality a
guest plugin implements for its host, while `[[host_contract]]` declares
functionality the host implements for guests. `plugin_contract` is **not** a
supported schema key. Renaming an existing `[[plugin_contract]]` table to
`[[guest_contract]]` is a breaking schema migration: validation reports:
“`[[plugin_contract]]` is invalid; use `[[guest_contract]]` instead.” Update
every table and nested table to use `guest_contract` (for example,
`[[guest_contract.functions]]`). IDs remain
namespaced as `guest_contract:<name>@<major>`; only the schema spelling changed.

For every accepted `api.toml` key, nesting rule, validation rule, and target-language `langs` attribute wrapper, see the [`api.toml` schema reference](API_TOML.md).

## Profiles and the default layout

The normal commands remain the all-in-one default:

```sh
# Application-side typed callers from the API.
polyplugc generate --api api.toml --lang rust --out generated/host

# Ordinary external guest bindings, entry points, and manifest from one bundle.
polyplugc generate --bundle bundle.toml --lang rust --out generated/guest

# An internal profile has both generated guest providers and host callers.
polyplugc generate --bundle bundle.toml --internal --lang rust --out generated/internal
```

`--internal` is valid only with `--bundle`. It opts into generated registration
for one application-owned bundle; it does not replace the ordinary external
bundle profile. In all three commands, omitting every layout flag preserves the
established unified tree and generated file names.

## The three semantic partitions

A split layout separates generated source by meaning, not by language-specific
file name:

| Partition | Contains | Owner and public role |
|---|---|---|
| **Bindings** | ABI adapters, marshaling, typed callers, entry points, registrars, manifests, and module glue | Generated implementation detail; the CLI keeps it under `--out`. |
| **DomainTypes** | API-defined structs, enums, and flags | Application-owned data model shared by providers and callers. |
| **GuestContracts** | Guest-facing traits, interfaces, and contract declarations | The typed implementation boundary a guest provider implements. |

The generated `types.rs` (and analogous `types.hpp`, `.cs`, `.py`, `.lua`, or
`.ts`) inside **Bindings** is private ABI machinery. It may contain C-layout
views, pointer-sized handles, conversion helpers, or return arenas. Do not
re-export it as application domain data and do not hand-author replacements.
Public domain values come only from **DomainTypes**; provider traits/interfaces
come only from **GuestContracts**. This is what lets an application put its
stable domain model in one package while generated ABI glue remains private.

### Library configuration migration

The output-layout change is source-breaking for Rust callers that construct
`GenerateConfig` with a struct literal. The migration requires two source
edits: remove the former `out_dir` field and add
`layout: OutputLayout::unified()` to preserve the old unified output:

```rust,ignore
use polyplug_codegen::{GenerateConfig, Lang, OutputLayout, Side, generate};
use std::path::PathBuf;

// Before this API change:
let old_config = GenerateConfig {
    api_toml: PathBuf::from("api.toml"),
    out_dir: PathBuf::from("generated"),
    lang: Lang::Rust,
    side: Side::Guest,
};

// After both required source edits:
let config = GenerateConfig {
    api_toml: PathBuf::from("api.toml"),
    lang: Lang::Rust,
    side: Side::Guest,
    layout: OutputLayout::unified(),
};
let output = generate(config)?;
```

The `old_config` form no longer compiles because `out_dir` was removed and
`layout` is required. Select `OutputLayout::unified()` for the previous
all-in-one tree or construct an explicit split layout. `generate` and the
`generate_internal_*` functions are the supported library entry points;
low-level `generate_ir` is crate-private for tests and is not a public
customization API.

## Selecting an output destination

`OutputLayout` in the code-generation library is the language-independent
three-part model:

```rust,ignore
use polyplug_codegen::{
    Lang, OutputDestination::{Emit, ImportOnly, Inline, Omit}, OutputLayout,
    PolyplugcError, ValidatedImport,
};

fn split_layout() -> Result<OutputLayout, PolyplugcError> {
    let unified = OutputLayout::unified();
    assert_eq!(unified.bindings, Inline);
    let declarations_are_unneeded = OutputLayout {
        bindings: Omit,
        domain_types: Omit,
        guest_contracts: Omit,
    };
    assert_eq!(declarations_are_unneeded.guest_contracts, Omit);
    assert_eq!(declarations_are_unneeded.bindings, Omit);
    Ok(OutputLayout {
        bindings: Inline,
        domain_types: Emit {
            root: "crates/common/src/generated".into(),
            import: ValidatedImport::parse(Lang::Rust, "common::domain")?,
        },
        guest_contracts: ImportOnly {
            import: ValidatedImport::parse(Lang::Rust, "common::guest_contracts")?,
        },
    })
}
```

The library applies those four destinations (`Inline`, `Emit`, `ImportOnly`,
and `Omit`) to every `OutputLayout` partition. The CLI deliberately keeps
Bindings inline and exposes separate-root, ImportOnly, and omit controls only
for DomainTypes and GuestContracts.

- **Inline** keeps a partition beneath the primary `--out` root. This is the
  default for all three partitions and is the only CLI destination for
  Bindings.
- **Emit** writes a partition under a separate root. On the CLI, pass both its
  `--*-out` root and `--*-import` specifier; generated consumers import that
  specifier instead of a local declaration.
- **ImportOnly** emits no partition files. On the CLI, pass only `--*-import`.
  It means another package already owns that semantic partition and the
  generated bindings must import it.
- **Omit** emits and references nothing for that partition. Use it only when
  the selected profile has no generated consumer of that declaration. The CLI
  spellings are `--domain-types-omit` and `--guest-contracts-omit`; each
  conflicts with the matching `--*-out` and `--*-import` flags.

The generator validates every partition reference before it writes. A binding
that needs an omitted partition is rejected; a cross-root reference without an
import is rejected; and an import for the wrong target language is rejected.
Import specifiers reject empty values, controls, and path traversal (and Rust
also requires a valid Rust module path).

## CLI: emit and import-only

Here is an ordinary external guest split: bindings remain in `generated/guest`,
while canonical declarations are emitted in two separately owned roots.

```sh
polyplugc generate --bundle bundle.toml --lang rust --out generated/guest \
  --domain-types-out crates/common/src/generated \
  --domain-types-import common::domain \
  --guest-contracts-out crates/common/src/generated \
  --guest-contracts-import common::guest_contracts
```

The matching ImportOnly generation emits only the binding partition. It is
appropriate for a consumer crate after `common` has already generated the two
canonical declaration partitions:

```sh
polyplugc generate --bundle bundle.toml --internal --lang rust --out crates/core/src/generated \
  --domain-types-import common::domain \
  --guest-contracts-import common::guest_contracts
```

Use the same two pairs with an ordinary `--bundle` command or an `--internal`
command. API-side host generation can likewise split the domain partition when
host callers need application-defined values. A selected profile never invents
a partition it does not contain; an output root may therefore receive no files
for a declaration category absent from the contract.

### Tested specifier forms for every maintained language

These are the accepted forms used by the six-language layout matrix. Substitute
the appropriate roots for your package layout; the specifiers are consumed by
generated source, not resolved as shell paths.

| Language (`--lang`) | DomainTypes `--domain-types-import` | GuestContracts `--guest-contracts-import` |
|---|---|---|
| Rust (`rust`) | `common::domain` | `common::guest_contracts` |
| C++ (`cpp`) | `guest/domain.hpp` | `guest/guest_contracts.hpp` |
| C# (`csharp`) | `Common.Domain` | `Common.GuestContracts` |
| Python (`python`) | `common.domain` | `common.guest_contracts` |
| Lua (`lua`) | `common.domain` | `common.guest_contracts` |
| JavaScript / QuickJS (`js-quickjs`) | `@app/domain` | `@app/guest-contracts` |

For example, these six ordinary guest commands each use their language's native
import syntax while retaining the same semantic layout:

```sh
polyplugc generate --bundle bundle.toml --lang rust --out generated/rust --domain-types-out common/rust --domain-types-import common::domain --guest-contracts-out common/rust --guest-contracts-import common::guest_contracts
polyplugc generate --bundle bundle.toml --lang cpp --out generated/cpp --domain-types-out common/cpp --domain-types-import guest/domain.hpp --guest-contracts-out common/cpp --guest-contracts-import guest/guest_contracts.hpp
polyplugc generate --bundle bundle.toml --lang csharp --out generated/csharp --domain-types-out common/csharp --domain-types-import Common.Domain --guest-contracts-out common/csharp --guest-contracts-import Common.GuestContracts
polyplugc generate --bundle bundle.toml --lang python --out generated/python --domain-types-out common/python --domain-types-import common.domain --guest-contracts-out common/python --guest-contracts-import common.guest_contracts
polyplugc generate --bundle bundle.toml --lang lua --out generated/lua --domain-types-out common/lua --domain-types-import common.domain --guest-contracts-out common/lua --guest-contracts-import common.guest_contracts
polyplugc generate --bundle bundle.toml --lang js-quickjs --out generated/js --domain-types-out common/js --domain-types-import @app/domain --guest-contracts-out common/js --guest-contracts-import @app/guest-contracts
```

Each form is also valid with `--internal` after `--bundle bundle.toml`. Keep the
specifier stable and make the separately generated root available to that
language's normal compiler/module resolver.

## Rust: common, platform, and core

An internal Rust application commonly has three crates:

```text
common/    owns generated DomainTypes and GuestContracts once
platform/  owns the handwritten Platform implementation
core/      owns generated bindings, Runtime, registration, and callers
```

`common/build.rs` can emit only canonical declarations with the Rust library
API. `Bindings: Omit` emits no host callers, adapters, providers, manifests,
or ABI files. When either semantic partition is inline, generation instead
emits one assembly `mod.rs` beside the semantic files. That root uses portable
relative module declarations for inline partitions and stable named imports for
external partitions. It owns fingerprint checks for every non-omitted semantic
module.

If an imported guest-contract partition has signatures using generated domain
types, `DomainTypes` must also be an import: an inline or omitted domain
partition would create incompatible nominal types. Primitive-only imported
contracts have no domain-partition edge.


```rust,ignore
use std::{env, path::PathBuf};

use polyplug_codegen::{
    InternalRustGenerateConfig, OutputDestination, OutputLayout, generate_internal_rust,
    write_output,
};

fn main() {
    println!("cargo:rerun-if-changed=../bundle.toml");
    println!("cargo:rerun-if-changed=../api.toml");
    let output_root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("src/generated");
    let output = generate_internal_rust(InternalRustGenerateConfig {
        bundle_toml: "../bundle.toml".into(),
        layout: OutputLayout {
            bindings: OutputDestination::Omit,
            domain_types: OutputDestination::Inline,
            guest_contracts: OutputDestination::Inline,
        },
    })
    .expect("generate declarations");
    write_output(&output, &output_root).expect("write declarations");
}
```

For a bundle whose `[bundle] name` is `platform`, the identity namespace is
`platform-3e4bd4e31c5c3ad2`; use the directory generated for the actual bundle.
`common/src/lib.rs` module-includes the generated root exactly once, then
reexports its public modules:

```rust,ignore
#[path = "generated/internal/platform-3e4bd4e31c5c3ad2/mod.rs"]
mod generated;

pub use generated::domain;
pub use generated::guest_contracts;
```

`platform/src/lib.rs` implements the declarations directly—there is no ABI
wrapper or duplicate domain model:

```rust,ignore
use common::{
    domain::Envelope,
    guest_contracts::PlatformPluginContract,
};
use polyplug_guest::GuestError;

pub struct Platform;

impl PlatformPluginContract for Platform {
    fn cycle(&self, value: Envelope) -> Result<Envelope, GuestError> {
        Ok(value)
    }
}
```

`core/build.rs` uses ImportOnly, so it emits only bindings that import the
canonical `common` modules:

```rust,ignore
use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=../bundle.toml");
    println!("cargo:rerun-if-changed=../api.toml");
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let status = Command::new(env::var("POLYPLUGC").unwrap_or_else(|_| "polyplugc".into()))
        .args([
            "generate", "--bundle", "../bundle.toml", "--internal", "--lang", "rust",
            "--out", "src/generated",
            "--domain-types-import", "common::domain",
            "--guest-contracts-import", "common::guest_contracts",
        ])
        .current_dir(root)
        .status()
        .expect("run polyplugc");
    assert!(status.success(), "polyplugc failed");
}
```

`core` includes the generated `mod.rs` root with a portable source-relative path:

```rust,ignore
#[path = "generated/internal/platform-3e4bd4e31c5c3ad2/mod.rs"]
mod generated;
```

The generated root compares every non-omitted internal partition fingerprint,
including imported semantic modules. A mixed generation therefore fails during
compilation; applications do not add their own assertion.


The core crate then registers `platform::Platform` through the generated
internal registration façade and uses the returned callers. `platform` depends
on `common`; `core` depends on both `common` and `platform`; `common` never
depends on `core`.

## Write safety and multi-root limits

Before writing, generation validates the semantic layout and partition
references, generated relative paths and containment, and collisions among the
complete target set. That preflight happens before it creates output
directories or replaces generated files. It also rejects an existing directory
where a file belongs and existing non-directory ancestors.

Each changed generated file is written to a temporary file in its destination
directory and renamed into place. A successful replacement therefore does not
expose a partially written version of that file.

This is not a file-set or cross-root transaction. Directory creation and
per-file writes occur after preflight and can fail after earlier files have
already been replaced, including when several partitions use different roots.
Polyplug does not roll those replacements back. Treat every generated root as
one build input and regenerate it from the same API or bundle.

## Further reading

- [CLI reference](cli.md) for every command and validation option.
- [Generated bindings](how-it-works/generated-bindings.md) for runtime-facing
  provider and caller roles.
- [Workflow](WORKFLOW.md) for authoring, validation, and bundle assembly.

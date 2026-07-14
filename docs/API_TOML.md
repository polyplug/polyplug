# `api.toml` reference

`api.toml` is the complete, language-neutral contract from which `polyplugc`
generates domain values, typed caller surfaces, and guest implementation
surfaces. It is not a bundle manifest: bundle identity, artifact selection, and
plugin providers belong in [`bundle.toml`](cli.md#generate).

This page is the schema reference. Start with [Quick Start](QUICKSTART.md) for
a small working contract and [Code generation and split output](CODE_GENERATION.md)
for where each generated projection belongs.

## API root

An API file has four array-of-table keys and one optional root customization
table:

| Key | TOML form | Purpose |
| --- | --- | --- |
| `types` | `[[types]]` | ABI-safe named struct types. |
| `enum` | `[[enum]]` | Integer enums and bitflags. |
| `guest_contract` | `[[guest_contract]]` | Operations implemented by a guest plugin and called by the host. |
| `host_contract` | `[[host_contract]]` | Operations implemented by the host and called by guest plugins. |
| `langs` | `[langs.<language>]` | Optional target-language source metadata for the API root. |

Use these **singular** array names exactly. `[[plugin_contract]]` and
`[[contract]]` are rejected; rename them (and nested tables such as
`[[plugin_contract.functions]]`) to `guest_contract`. Unknown keys are
rejected at every schema level.

Place root `langs` tables at document root, conventionally before the first
`[[...]]` declaration:

```toml
[langs.rust]
attributes = ["allow(clippy::missing_docs_in_private_items)"]

[[types]]
name = "Point"
```

There is no required root metadata and an otherwise empty `api.toml` is valid.
A root rule is emitted only in generated projections that have a root-level
source location for the selected target.

## Struct types

Each `[[types]]` declaration defines one named, by-value ABI struct.

| Key | Required | Default | Meaning |
| --- | --- | --- | --- |
| `name` | yes | — | Struct identifier. |
| `fields` | no | `[]` | Fields, written as `[[types.fields]]` tables or TOML inline tables. |
| `docs` | no | absent | Documentation for the generated type. |
| `langs` | no | absent | Per-language attributes for the type. |

```toml
[[types]]
name = "Point"
docs = "A two-dimensional point."

[types.langs.rust]
attributes = ["repr(C)"]

[[types.fields]]
name = "x"
type = "f32"
docs = "Horizontal coordinate."

[types.fields.langs.cpp]
attributes = ["deprecated(\"use position\")"]
```

### Fields

A `[[types.fields]]` entry has these keys:

| Key | Required | Default | Meaning |
| --- | --- | --- | --- |
| `name` | yes | — | Field identifier. |
| `type` | yes | — | A supported type reference. |
| `docs` | no | absent | Documentation for the generated field. |
| `langs` | no | absent | Per-language attributes for the field. |

A field is physically part of its containing type. It cannot be declared as a
top-level `[[fields]]` table.

### Type references

`type` accepts the primitive names `u8`, `u16`, `u32`, `u64`, `i8`, `i16`,
`i32`, `i64`, `f32`, `f64`, and `bool`; ABI values `StringView`, `Buffer`,
`Ptr` (also accepted as `ptr`), and `Void` (also accepted as `void`); a named
`[[types]]` or `[[enum]]` declaration from the same file; or `Array<T>` for one
non-array element type `T`.

`Array<T>` is desugared into a generated `ArrayOf_T` `{ items, len }` wrapper
and uses the return arena at the generated boundary. Nested arrays such as
`Array<Array<u8>>` are rejected. `Void`/`void` is normalized to no return when
used as a function return; avoid it for a field or parameter.

## Enums and variants

Each `[[enum]]` defines an ABI integer enum.

| Key | Required | Default | Meaning |
| --- | --- | --- | --- |
| `name` | yes | — | Enum identifier. It must not collide with a type name. |
| `repr` | yes | — | ABI storage: `u8`, `u16`, `u32`, or `u64`. |
| `bitflag` | no | `false` | Generate flag semantics instead of an ordinary enum. |
| `variants` | no | `[]` | Entries written as `[[enum.variants]]`. |
| `docs` | no | absent | Documentation for the generated enum. |
| `langs` | no | absent | Per-language attributes for the enum. |

Each `[[enum.variants]]` entry requires `name` and `value`; it optionally has
`docs` and `langs`:

```toml
[[enum]]
name = "Access"
repr = "u32"
bitflag = true
docs = "Requested access modes."

[enum.langs.csharp]
attributes = ["Flags"]

[[enum.variants]]
name = "Read"
value = "1"
docs = "Read access."

[enum.variants.langs.python]
attributes = ["deprecated(\"example metadata\")"]

[[enum.variants]]
name = "Write"
value = "Read << 1"
```

A value is a validated integer expression. It may contain decimal, `0x` hex,
or `0b` binary integer literals; `|`, `<<`, `~`, parentheses, whitespace; and
a previously declared variant name. A reference to a later variant, an unknown
identifier, or a chained variant reference is rejected. Declare variants in
the order their expressions require.

## Guest contracts

A `[[guest_contract]]` is the public operation surface a guest plugin
implements. The host receives typed callers for it; a generated guest
projection receives the trait, interface, or equivalent implementation surface.

| Key | Required | Default | Meaning |
| --- | --- | --- | --- |
| `name` | yes | — | Dotted contract identifier, for example `image.Decoder`. |
| `version` | yes | — | Contract version. Its major component participates in identity. |
| `functions` | no | `[]` | Entries written as `[[guest_contract.functions]]`. |
| `docs` | no | absent | Documentation for the generated contract declaration. |
| `langs` | no | absent | Per-language attributes for the contract type/interface. |

The guest contract ID is derived from `guest_contract:<name>@<major>`. Changing
minor or patch does not change that ID; adding functions changes the generated
surface, so append compatible functions rather than reorder existing ones.

```toml
[[guest_contract]]
name = "image.Decoder"
version = "1.2.0"
docs = "Decodes image data."

[guest_contract.langs.lua]
attributes = ["contract_metadata"]
```

## Host contracts

A `[[host_contract]]` is an API the host provides to guests. A generated guest
projection receives typed host-call helpers; the application supplies the host
implementation through its host registration path.

| Key | Required | Default | Meaning |
| --- | --- | --- | --- |
| `name` | yes | — | Dotted identifier beginning with `host.`, for example `host.logger`. |
| `version` | yes | — | Contract version. Its major component participates in identity. |
| `singleton` | no | `false` | Whether all callers receive the same host-contract instance. |
| `functions` | no | `[]` | Entries written as `[[host_contract.functions]]`. |
| `docs` | no | absent | Documentation for the generated contract declaration. |
| `langs` | no | absent | Per-language attributes for the contract type/interface. |

The host contract ID is derived from `host_contract:<name>@<major>`. Host
contract names must start with `host.` and cannot duplicate a guest contract or
another host contract.

```toml
[[host_contract]]
name = "host.logger"
version = "1"
singleton = true
docs = "Writes diagnostic messages."

[host_contract.langs.csharp]
attributes = ["Obsolete(\"example metadata\")"]
```

## Functions, parameters, and returns

Functions are nested under exactly one contract. The same schema is used below
`guest_contract` and `host_contract`.

| Node | TOML form | Keys |
| --- | --- | --- |
| Function | `[[guest_contract.functions]]` or `[[host_contract.functions]]` | `name` (required), `params` (default `[]`), `return` (optional), `docs` (optional), `langs` (optional). |
| Parameter | `[[...functions.params]]` | `name` and `type` (required), `docs` and `langs` (optional). |
| Return, compact | `return = "Type"` | Type only. |
| Return, expanded | `[...functions.return]` | `type` (required), `docs` and `langs` (optional). |

Use the expanded return table whenever the return itself needs documentation or
language metadata. An inline return table also permits `docs`, but TOML inline
tables cannot be extended with a nested `[...return.langs.<language>]` table.

```toml
[[guest_contract.functions]]
name = "decode"
docs = "Decodes a byte buffer."

[guest_contract.functions.langs.javascript]
attributes = ["internal"]

[[guest_contract.functions.params]]
name = "encoded"
type = "Buffer"
docs = "Encoded input bytes."

[guest_contract.functions.params.langs.rust]
attributes = ["allow(unused_variables)"]

[guest_contract.functions.return]
type = "Array<Point>"
docs = "Decoded points."

[guest_contract.functions.return.langs.cpp]
attributes = ["nodiscard"]
```

An omitted `return` means no return. `return = "void"` and `return = "Void"`
also normalize to no return; their expanded form is still useful when the
no-return operation needs return-site docs or metadata.

Function IDs are zero-based and follow declaration order within their contract.
Function names must be unique within that contract. Parameter names must be
valid identifiers; the parser currently does not infer names or types.

## Documentation

Every authored declaration that can appear in generated source accepts optional
`docs`: types, fields, enums, enum variants, guest contracts, host contracts,
functions, parameters, and expanded returns. `docs` is never part of a contract
ID, ABI layout, compatibility version, or generated manifest.

Documentation line endings normalize to LF. Tabs and ordinary line breaks are
allowed. Other C0 control characters and C1 control characters are rejected so
the text can be rendered safely in all maintained targets. A compact string
return has no place for `docs`; use its expanded form.

## Defaults and validation

The parser validates the complete schema before code generation.

- Optional collections default to empty: `types`, `enum`, `guest_contract`,
  `host_contract`, `fields`, `variants`, `functions`, and `params`.
- `bitflag` and host-contract `singleton` default to `false`; `docs` and
  `langs` are absent unless supplied.
- An identifier for a type, field, function, parameter, enum, or variant must
  match `[A-Za-z_][A-Za-z0-9_]*` and must not be reserved by polyplug or any
  maintained target language. Contract names use one or more such segments
  separated by dots.
- Type references must resolve in the API file or be a supported builtin. Enum
  names cannot collide with type names.
- Contract versions parse numeric major, minor, and patch components. Omitted
  minor and patch default to zero; minor and patch may not exceed `65535`.
  Canonically write `major.minor.patch`.
- Guest contract names are unique. Host contract names are unique, begin with
  `host.`, and cannot duplicate a guest contract name. Function names are
  unique within a contract.
- Attribute values must be non-empty after trimming and must be one physical
  line. Unknown language keys and unknown keys within a language table are
  rejected.

## Language customization

`langs` lets a contract author attach target-specific source metadata to every
customizable API node. It is deliberately a closed six-language schema:
`rust`, `cpp`, `csharp`, `python`, `lua`, and `javascript`. The JavaScript key
selects the `js-quickjs` generator.

For `cpp`, `csharp`, `python`, `lua`, and `javascript`, each language entry
contains exactly one optional key, `attributes`. Rust accepts `attributes` too
and additionally supports the typed keys `derives`, `serde`, `primary_name`,
`aliases`, `default`, `empty_sequence_as_null`, and `tagged_enum` at their
applicable nodes; expanded returns accept only `attributes`. See
[Rust semantic rules](#rust-semantic-rules) for the key-by-key node rules.
An `attributes` value is an ordered string array containing the **inner
contents** of an attribute. Do not include delimiters such as `#[...]`,
`[[...]]`, `[...]`, or `@...`; LangPrint supplies the target- and site-specific
wrapper.

```toml
[guest_contract.langs.rust]
attributes = ["derive(Debug)", "allow(dead_code)"]

[guest_contract.langs.csharp]
attributes = ["EditorBrowsable(EditorBrowsableState.Never)"]
```

Rules do not inherit. A root rule does not become a type rule, a type rule does
not become a field rule, and an absent language entry emits nothing. Multiple
entries preserve their authored order.

### Attachment paths for every customizable node

Every row below accepts the same six `[...langs.<language>]` tables and the
common `attributes = ["inner contents"]` array. Rust typed semantic keys are
also accepted on the non-return nodes identified in
[Keys, defaults, and applicable nodes](#keys-defaults-and-applicable-nodes);
the expanded-return row accepts only `attributes` for every language.

| Authored node | `langs` table path | LangPrint declaration site |
| --- | --- | --- |
| API root | `[langs.<language>]` | Root |
| Type | `[types.langs.<language>]` | Type |
| Field | `[types.fields.langs.<language>]` | Field |
| Enum | `[enum.langs.<language>]` | Enum |
| Enum variant | `[enum.variants.langs.<language>]` | Variant |
| Guest contract | `[guest_contract.langs.<language>]` | Type |
| Host contract | `[host_contract.langs.<language>]` | Type |
| Function | `[guest_contract.functions.langs.<language>]` or `[host_contract.functions.langs.<language>]` | Function |
| Parameter | `[...functions.params.langs.<language>]` | Parameter |
| Expanded return | `[...functions.return.langs.<language>]` | Return |

These are paths relative to the latest enclosing array-table item. For example,
`[types.fields.langs.rust]` belongs immediately after the selected
`[[types.fields]]`, not after a different type's field.

### LangPrint wrappers by target

For attribute body `example(value)`, the generated line is exactly the form in
this table. `Root` means API-root metadata; all other declaration sites are
shown as `non-root` where their wrapper is identical.

| API key / generator | Root | Type, field, enum, variant, contract, function, parameter | Return |
| --- | --- | --- | --- |
| `rust` | `#![example(value)]` | `#[example(value)]` | `#[example(value)]` |
| `cpp` | `// [[langprint::root(example(value))]]` | `[[example(value)]]` | `[[example(value)]]` |
| `csharp` | `[assembly: example(value)]` | `[example(value)]` | `[return: example(value)]` |
| `python` | `# @langprint Root: example(value)` | `@example(value)` only for type, enum, and function; `# @langprint Field/Variant/Parameter: example(value)` for field, variant, and parameter | `# @langprint Return: example(value)` |
| `lua` | `---@langprint Root: example(value)` | `---@langprint Type/Field/Enum/Variant/Function/Parameter: example(value)` | `---@langprint Return: example(value)` |
| `javascript` (`js-quickjs`) | `/** @langprint Root: example(value) */` | `/** @langprint Type/Field/Enum/Variant/Function/Parameter: example(value) */` | `/** @langprint Return: example(value) */` |

The spelled site names in the comment forms are significant: `Root`, `Type`,
`Field`, `Enum`, `Variant`, `Function`, `Parameter`, and `Return`. Guest and
host contracts both use the `Type` site.

### Grammar-impossible metadata forms

LangPrint uses a language-native attribute only where that target/site has an
attribute grammar it can express. The following outputs are intentionally
metadata comments, not executable attributes:

- C++ API-root metadata: `// [[langprint::root(<contents>)]]`.
- Python root, field, variant, parameter, and return metadata:
  `# @langprint <Site>: <contents>`. Python type, enum, and function metadata
  uses the real decorator form `@<contents>`.
- Every Lua site: `---@langprint <Site>: <contents>`.
- Every JavaScript/QuickJS site: `/** @langprint <Site>: <contents> */`.

Do not mistake a metadata comment for a compiler-recognized annotation. It
preserves the authored text in the generated source but cannot change target
compiler behavior by itself.

### Trusted source injection

Attribute contents are source text, not a portable annotation vocabulary.
`polyplugc` verifies only that each string is non-blank after trimming and has
no `\n` or `\r`; it does not parse, escape, validate, or translate the body for
the target language. LangPrint adds only the wrappers above.

Treat an `api.toml` containing `langs` as trusted source input, subject to the
same review and repository controls as generated source. Supply bodies valid
for the selected declaration site and target compiler. An attribute accepted
by TOML can still cause the target compiler to reject the generated file.
Never accept unreviewed attribute bodies from an untrusted plugin or network
source.

## Rust semantic rules

`[...langs.rust]` supports the general `attributes` rule described above and
the typed Rust rules in this section. These rules change only Rust generated
source; they do not change an API's contract IDs, C ABI layout, manifest, or
the output for another language. All omitted collection values are empty,
booleans are `false`, and optional scalar values are absent.

### Keys, defaults, and applicable nodes

| Key | TOML type | Default | Applicable node | Generated effect |
| --- | --- | --- | --- | --- |
| `attributes` | string array | `[]` | Every customizable node | The normal LangPrint Rust attributes. |
| `derives` | string array | `[]` | API root, type, field, enum, enum variant, guest/host contract, function, or parameter; consumed for `[[types]]` and `[[enum]]` declarations | Ordered, duplicate-suppressed entries appended to generated `derive(...)` lists. |
| `serde` | string | absent | `[[enum]]` only | Selects an enum serialization policy. |
| `primary_name` | string | absent | `[[enum.variants]]` only | The primary human-readable name for dual serde. |
| `aliases` | string array | `[]` | `[[enum.variants]]` only | Additional accepted human-readable names for dual serde. |
| `default` | boolean | `false` | `[[enum.variants]]` only | Marks the generated domain enum's default variant. |
| `empty_sequence_as_null` | boolean | `false` | `[[types.fields]]` only | Gives an `Array<T>` domain field null/empty sequence serde behavior. |
| `tagged_enum` | table | absent | `[[types]]` only | Projects a flat ABI struct into a Rust domain enum. |

`derives` strings are emitted as authored and are not parsed as Rust paths.
The lowering step rejects blank or multi-line derive strings, removes duplicate
strings while retaining the first occurrence, and does not add dependencies or
imports. For example, use fully qualified derive paths when that is preferable:

```toml
[types.langs.rust]
derives = ["serde::Serialize", "serde::Deserialize"]
```

The generator keeps its mandatory derives and appends each distinct authored
derive. Ordinary domain structs start with `Debug`, `Clone`, and `PartialEq`;
ordinary domain enums start with `Debug`, `Clone`, `Copy`, `PartialEq`, and
`Eq`. A tagged projection deliberately starts without `Copy`/`Eq`, because its
payload can be non-copy or floating-point. The ABI-flat generated declarations
also retain their ABI-required derives.

### Enum names, defaults, and dual serde

`serde` has one supported policy:

```toml
[enum.langs.rust]
serde = "human-name-binary-discriminant"
```

It is valid only on an ordinary (non-`bitflag`) `[[enum]]`. It makes the Rust
domain enum use two compatible representations:

| Serializer mode | Serialize | Deserialize |
| --- | --- | --- |
| Human-readable (for example JSON or YAML) | The variant's `primary_name`, or its authored enum-variant name when `primary_name` is absent | The primary name, every `aliases` value, and the authored name when it is the primary name |
| Non-human-readable (for example postcard) | The exact unsigned ABI discriminant at the enum's authored `repr` width (`u8`, `u16`, `u32`, or `u64`) | The same width and the exact authored discriminant |

The policy generates `serde::Serialize` and `serde::Deserialize` itself.
Therefore `derives` for that enum must not also include exactly `Serialize` or
`Deserialize`; doing so is a validation error. The parser's duplicate guard
recognizes those exact strings; qualified equivalents are emitted verbatim and
can still produce a Rust duplicate-implementation error, so do not add either
trait in any form.

The generated implementations use direct `serde::...` references, so the crate
compiling the domain projection needs a direct `serde` dependency. `derives`
never causes Cargo dependencies to be added; serde derives additionally require
the dependency's `derive` feature when the chosen derive path requires it.

`primary_name`, `aliases`, and an enum-variant `default` are accepted only on
`[[enum.variants]]`. `primary_name` and `aliases` participate in the dual-serde
policy above; the flat ABI enum continues to use its authored variant names and
integer discriminants. At most one variant in an enum may set `default = true`.
When that enum has a generated domain projection (which dual serde creates),
the generator adds `Default` if necessary and emits `#[default]` on that
unit-like domain variant. A `default` rule by itself does not create a separate
domain enum projection.

```toml
[[enum]]
name = "Kind"
repr = "u32"
[enum.langs.rust]
serde = "human-name-binary-discriminant"
derives = ["Ord"]

[[enum.variants]]
name = "Empty"
value = "17"
[enum.variants.langs.rust]
primary_name = "none"
aliases = ["empty"]
default = true
```

### Tagged enum domain projection

`tagged_enum` keeps the ABI-facing type flat while presenting a Rust sum type
to internal-profile or split-domain code. It is a rule on a declared
`[[types]]` item:

```toml
[[enum]]
name = "Kind"
repr = "u32"
[[enum.variants]]
name = "Empty"
value = "0"
[[enum.variants]]
name = "Boolean"
value = "1"
[[enum.variants]]
name = "Text"
value = "2"

[[types]]
name = "Value"
[types.langs.rust]
derives = ["serde::Serialize", "serde::Deserialize"]
tagged_enum = { tag_field = "kind", variants = [
  { tag = "Empty", name = "None", default = true },
  { tag = "Boolean", name = "Bool", payload = "bool_value" },
  { tag = "Text", name = "String", payload = "string_value" },
] }

[[types.fields]]
name = "kind"
type = "Kind"
[[types.fields]]
name = "bool_value"
type = "bool"
[[types.fields]]
name = "string_value"
type = "StringView"
```

The nested table has these keys:

| Path | Key | Type | Required | Default | Meaning |
| --- | --- | --- | --- | --- | --- |
| `tagged_enum` | `tag_field` | string | yes | — | Name of the flat struct field containing the discriminator enum. |
| `tagged_enum` | `variants` | array of inline tables | no | `[]` | One mapping for every discriminator variant. |
| `tagged_enum.variants[]` | `tag` | string | yes | — | Authored discriminator enum-variant name. |
| `tagged_enum.variants[]` | `name` | string | no | `tag` value | Rust domain enum-variant name. |
| `tagged_enum.variants[]` | `payload` | string | no | absent | One non-tag flat field carried by this domain variant. |
| `tagged_enum.variants[]` | `default` | boolean | no | `false` | Marks one unit domain variant as the Rust default. |

The discriminator field must be a named, ordinary `[[enum]]`, not a primitive,
ABI builtin, or bitflag. The mapping must cover each discriminator variant
exactly once. A payload must name an existing non-discriminator field and no
payload field may be reused. At most one mapping may be `default`, and a default
mapping must be unit-like (it cannot name a payload).

The normal `types.rs` ABI declaration remains the authored `#[repr(C)]` flat
struct with its tag and all possible payload fields. Rust internal-profile
output and a Rust `DomainTypes` output partition additionally emit `domain.rs`
with the projected enum. Its adapters read the tag to form the mapped domain
variant; converting back starts from a zeroed ABI struct, writes the selected
tag, and writes only that variant's payload field. This is why `tagged_enum`
does not alter the wire layout or other language projections.

### Empty sequence as null

Use `empty_sequence_as_null = true` only on a struct field whose API type is
`Array<T>`:

```toml
[[types]]
name = "Settings"

[[types.fields]]
name = "filters"
type = "Array<StringView>"
[types.fields.langs.rust]
empty_sequence_as_null = true
```

The Rust domain field is a `Vec<T>` (as all `Array<T>` domain fields are). The
generator adds a serde default plus custom deserialize and serialize helpers:
missing or explicit `null` input deserializes to an empty `Vec`; an empty `Vec`
serializes as `null`; and a non-empty `Vec` serializes as a sequence. The ABI
array wrapper remains unchanged. The helper code refers directly to
`serde::{Deserialize, Serializer, ...}`, so this rule requires a direct
`serde` dependency in the crate compiling `domain.rs`.

### Rust semantic validation errors

Rust semantic rule validation is schema validation, before generation. It
rejects unknown keys in a Rust rule table and reports these rule-specific
errors:

- blank or multi-line `derives` entries;
- an unsupported `serde` value;
- `serde` outside an enum, `tagged_enum` outside a type, or
  `empty_sequence_as_null` outside a field;
- `primary_name`, `aliases`, or enum-variant `default` outside an enum variant;
- dual serde on a bitflag, or dual serde combined with the exact `Serialize` or
  `Deserialize` derive name;
- more than one default enum variant;
- `empty_sequence_as_null` on a field other than `Array<T>`;
- a generated array wrapper selected as a tagged type, an absent or invalid
  tag field, a bitflag tag enum, incomplete or duplicate tag mappings, a
  missing/reused/tag payload field, more than one tagged default, or a tagged
  default that has a payload.

These rules do not make Rust source injected through `attributes` safe or
portable; the trusted-source rules in [Trusted source injection](#trusted-source-injection)
still apply.

## Generated projections and output layout

The schema is directional, while types and enums are shared domain values:

- `[[types]]` and `[[enum]]` become generated **DomainTypes** wherever a
  selected profile needs them.
- `[[guest_contract]]` produces host-side typed callers and a guest-side public
  implementation declaration (trait/interface/equivalent). The latter is the
  **GuestContracts** partition in a split layout.
- `[[host_contract]]` produces the host-provided operation surface and guest
  host-call helpers. It is not a guest implementation contract and therefore
  does not create a GuestContracts declaration partition.

With the default unified layout, the generated bindings, domain types, and
applicable guest contracts remain in the normal language-specific output tree.
With a split layout, `DomainTypes` and `GuestContracts` may be emitted in
separate owned roots or imported from another package; bindings remain under
`--out`. `langs` follows the declaration it customizes, not the output root:
a rule is rendered wherever that node's public projection is emitted. An
`ImportOnly` or `Omit` partition has no local declaration to annotate.

See [Code generation and split output](CODE_GENERATION.md) for the precise
`Inline`, `Emit`, `ImportOnly`, and `Omit` rules and per-language import forms.

## Complete multi-language example

This syntactically complete API exercises every declaration class and all six
language keys. The individual attribute bodies are deliberately examples;
replace them with metadata valid for the target and declaration site you use.

```toml
# Root rules are document-root tables.
[langs.rust]
attributes = ["allow(dead_code)"]
[langs.cpp]
attributes = ["maybe_unused"]
[langs.csharp]
attributes = ["CLSCompliant(true)"]
[langs.python]
attributes = ["generated_root"]
[langs.lua]
attributes = ["generated_root"]
[langs.javascript]
attributes = ["generated_root"]

[[types]]
name = "Point"
docs = "A two-dimensional point."
[types.langs.rust]
attributes = ["repr(C)"]

[[types.fields]]
name = "x"
type = "f32"
docs = "Horizontal coordinate."
[types.fields.langs.cpp]
attributes = ["deprecated(\"example\")"]

[[enum]]
name = "Access"
repr = "u32"
bitflag = true
docs = "Requested access modes."
[enum.langs.csharp]
attributes = ["Flags"]

[[enum.variants]]
name = "Read"
value = "1"
docs = "Read access."
[enum.variants.langs.python]
attributes = ["deprecated(\"example\")"]

[[enum.variants]]
name = "Write"
value = "Read << 1"

[[guest_contract]]
name = "geometry.Source"
version = "1.0.0"
docs = "Supplies points to the host."
[guest_contract.langs.lua]
attributes = ["contract_metadata"]

[[guest_contract.functions]]
name = "points"
docs = "Returns available points."
[guest_contract.functions.langs.javascript]
attributes = ["internal"]

[[guest_contract.functions.params]]
name = "limit"
type = "u32"
docs = "Maximum result count."
[guest_contract.functions.params.langs.rust]
attributes = ["allow(unused_variables)"]

[guest_contract.functions.return]
type = "Array<Point>"
docs = "The returned points."
[guest_contract.functions.return.langs.cpp]
attributes = ["nodiscard"]

[[host_contract]]
name = "host.logger"
version = "1.0.0"
singleton = true
docs = "Receives guest diagnostics."
[host_contract.langs.csharp]
attributes = ["Obsolete(\"example metadata\")"]

[[host_contract.functions]]
name = "log"
docs = "Writes one message."
[host_contract.functions.langs.python]
attributes = ["host_callback"]

[[host_contract.functions.params]]
name = "message"
type = "StringView"
[host_contract.functions.params.langs.lua]
attributes = ["message_metadata"]

[host_contract.functions.return]
type = "void"
docs = "Completion status."
[host_contract.functions.return.langs.javascript]
attributes = ["void_result"]
```

## Invalid examples

These forms are rejected by the schema or violate its required layout:

```toml
# Wrong: legacy name. Use [[guest_contract]].
[[plugin_contract]]
name = "image.Decoder"
version = "1.0.0"

# Wrong: fields must be nested below a [[types]] item.
[[fields]]
name = "x"
type = "f32"

# Wrong: host names require the host. prefix.
[[host_contract]]
name = "logger"
version = "1.0.0"

# Wrong: enum repr is unsigned only.
[[enum]]
name = "Mode"
repr = "i32"

# Wrong: wrapper punctuation belongs to LangPrint, not attributes.
[langs.rust]
attributes = ["#[derive(Clone)]"]

# Wrong: attribute contents must be one non-empty physical line.
[langs.cpp]
attributes = ["\n"]
```

This is also invalid TOML design for expanded return metadata: an inline table
cannot be reopened to add a nested language table.

```toml
[[guest_contract.functions]]
name = "decode"
return = { type = "Buffer" }

# Do not try to append return.langs here; use [guest_contract.functions.return]
# for the return from the start instead.
```

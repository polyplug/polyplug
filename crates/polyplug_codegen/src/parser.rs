//! Parser — TOML schema parsing for polyplugc.
//!
//! Parses `api.toml` and `bundle.toml` using serde+toml.
//! Produces raw AST structs that are later lowered to `ValidatedIr`.

use core::iter::once;
use core::mem;
use std::borrow::Cow;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;

use crate::Lang;
use crate::PlatformKey;
use crate::PolyplugcError;
use crate::ResolvedBundleFile;
use crate::error::SourceLocation;
use crate::ir::AbiBuiltin;
use crate::ir::CustomizableNode;
use crate::ir::EnumDef;
use crate::ir::EnumVariant;
use crate::ir::LanguageAttributes;
use crate::ir::LanguageRules;
use crate::ir::PrimitiveType;
use crate::ir::ReprType;
use crate::ir::ResolvedBundle;
use crate::ir::ResolvedContract;
use crate::ir::ResolvedDependency;
use crate::ir::ResolvedField;
use crate::ir::ResolvedFunction;
use crate::ir::ResolvedHostContract;
use crate::ir::ResolvedParam;
use crate::ir::ResolvedPlugin;
use crate::ir::ResolvedType;
use crate::ir::ResolvedTypeRef;
use crate::ir::RustEnumSerdePolicy;
use crate::ir::RustLanguageRules;
use crate::ir::RustTaggedEnum;
use crate::ir::RustTaggedEnumVariant;
use crate::ir::ValidatedIr;
use crate::ir::Version;
use crate::ir::array_element_name;
use crate::ir::resolve_type_ref;
use crate::reserved;
use polyplug_utils::bundle_id;
use polyplug_utils::guest_contract_id;
use polyplug_utils::host_contract_id;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawApiSchema {
    #[serde(default)]
    pub types: Vec<RawType>,
    #[serde(rename = "enum", default)]
    pub r#enum: Vec<RawEnum>,
    #[serde(default)]
    pub guest_contract: Vec<RawGuestContract>,
    #[serde(default)]
    pub host_contract: Vec<RawHostContract>,
    #[serde(default)]
    pub langs: Option<RawLanguageRules>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawLanguageRules {
    #[serde(default)]
    pub rust: Option<RawRustLanguageRules>,
    #[serde(default)]
    pub cpp: Option<RawLanguageAttributes>,
    #[serde(default)]
    pub csharp: Option<RawLanguageAttributes>,
    #[serde(default)]
    pub python: Option<RawLanguageAttributes>,
    #[serde(default)]
    pub lua: Option<RawLanguageAttributes>,
    #[serde(default)]
    pub javascript: Option<RawLanguageAttributes>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawRustLanguageRules {
    #[serde(default)]
    pub attributes: Option<toml::Spanned<Vec<String>>>,
    #[serde(default)]
    pub derives: Vec<String>,
    #[serde(default)]
    pub serde: Option<String>,
    #[serde(default)]
    pub primary_name: Option<String>,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub default: bool,
    #[serde(default)]
    pub empty_sequence_as_null: bool,
    #[serde(default)]
    pub tagged_enum: Option<RawRustTaggedEnum>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawRustTaggedEnum {
    pub tag_field: String,
    #[serde(default)]
    pub variants: Vec<RawRustTaggedEnumVariant>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawRustTaggedEnumVariant {
    pub tag: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub payload: Option<String>,
    #[serde(default)]
    pub default: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawLanguageAttributes {
    #[serde(default)]
    pub attributes: Option<toml::Spanned<Vec<String>>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawUnspannedLanguageRules {
    #[serde(default)]
    pub rust: Option<RawUnspannedLanguageAttributes>,
    #[serde(default)]
    pub cpp: Option<RawUnspannedLanguageAttributes>,
    #[serde(default)]
    pub csharp: Option<RawUnspannedLanguageAttributes>,
    #[serde(default)]
    pub python: Option<RawUnspannedLanguageAttributes>,
    #[serde(default)]
    pub lua: Option<RawUnspannedLanguageAttributes>,
    #[serde(default)]
    pub javascript: Option<RawUnspannedLanguageAttributes>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawUnspannedLanguageAttributes {
    #[serde(default)]
    pub attributes: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawType {
    pub name: String,
    #[serde(default)]
    pub fields: Vec<RawField>,
    #[serde(default)]
    pub docs: Option<toml::Spanned<String>>,
    #[serde(default)]
    pub langs: Option<RawLanguageRules>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawField {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: toml::Spanned<String>,
    #[serde(default)]
    pub docs: Option<toml::Spanned<String>>,
    #[serde(default)]
    pub langs: Option<RawLanguageRules>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawGuestContract {
    pub name: String,
    pub version: toml::Spanned<String>,
    #[serde(default)]
    pub functions: Vec<RawFunction>,
    #[serde(default)]
    pub docs: Option<toml::Spanned<String>>,
    #[serde(default)]
    pub langs: Option<RawLanguageRules>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawHostContract {
    pub name: String,
    pub version: toml::Spanned<String>,
    #[serde(default)]
    pub singleton: bool,
    #[serde(default)]
    pub functions: Vec<RawFunction>,
    #[serde(default)]
    pub docs: Option<toml::Spanned<String>>,
    #[serde(default)]
    pub langs: Option<RawLanguageRules>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawFunction {
    pub name: String,
    #[serde(default)]
    pub params: Vec<RawParam>,
    #[serde(rename = "return", default)]
    pub returns: Option<toml::Spanned<RawReturn>>,
    #[serde(default)]
    pub docs: Option<toml::Spanned<String>>,
    #[serde(default)]
    pub langs: Option<RawLanguageRules>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum RawReturn {
    Type(String),
    Table(RawReturnTable),
}

impl RawReturn {
    fn ty(&self) -> &str {
        match self {
            RawReturn::Type(ty) => ty,
            RawReturn::Table(table) => &table.ty,
        }
    }

    fn docs(&self) -> Option<&str> {
        match self {
            RawReturn::Type(_) => None,
            RawReturn::Table(table) => table.docs.as_deref(),
        }
    }

    fn langs(&self) -> Option<&RawUnspannedLanguageRules> {
        match self {
            RawReturn::Type(_) => None,
            RawReturn::Table(table) => table.langs.as_ref(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawReturnTable {
    #[serde(rename = "type")]
    pub ty: String,
    #[serde(default)]
    pub docs: Option<String>,
    #[serde(default)]
    pub langs: Option<RawUnspannedLanguageRules>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawParam {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: toml::Spanned<String>,
    #[serde(default)]
    pub docs: Option<toml::Spanned<String>>,
    #[serde(default)]
    pub langs: Option<RawLanguageRules>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawEnumVariant {
    pub name: String,
    pub value: String,
    #[serde(default)]
    pub docs: Option<toml::Spanned<String>>,
    #[serde(default)]
    pub langs: Option<RawLanguageRules>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawEnum {
    pub name: String,
    pub repr: String,
    #[serde(default)]
    pub bitflag: bool,
    #[serde(default)]
    pub variants: Vec<RawEnumVariant>,
    #[serde(default)]
    pub docs: Option<toml::Spanned<String>>,
    #[serde(default)]
    pub langs: Option<RawLanguageRules>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawBundleSchema {
    pub bundle: RawBundleMeta,
    #[serde(default)]
    pub plugin: Vec<RawBundlePlugin>,
    #[serde(default, rename = "dependency")]
    pub dependencies: Vec<RawDependency>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawBundleMeta {
    pub name: String,
    pub version: toml::Spanned<String>,
    #[serde(default)]
    pub api: Option<String>,
    #[serde(default)]
    pub loader: String,
    #[serde(default)]
    pub file: RawBundleFile,
    #[serde(default)]
    pub needs_reinit_on_dep_reload: bool,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum RawBundleFile {
    Single(String),
    PlatformMap(HashMap<String, HashMap<String, String>>),
}

impl Default for RawBundleFile {
    fn default() -> Self {
        RawBundleFile::Single(String::new())
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawBundlePlugin {
    pub name: String,
    #[serde(default)]
    pub implements: Vec<String>,
    #[serde(default)]
    pub optional: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawDependency {
    pub kind: String,
    pub contract: String,
    pub min_version: String,
    #[serde(default)]
    pub bundle: Option<String>,
}

// ─── Diagnostic helpers ───────────────────────────────────────────────────────

/// Convert a byte offset into `source` to a 1-based (line, col) pair.
fn byte_offset_to_line_col(source: &str, byte_offset: usize) -> (usize, usize) {
    let clamped: usize = byte_offset.min(source.len());
    let prefix: &str = &source[..clamped];
    let line: usize = prefix.bytes().filter(|&b| b == b'\n').count() + 1;
    let col: usize = match prefix.rfind('\n') {
        Some(last_nl) => clamped - last_nl,
        None => clamped + 1,
    };
    (line, col)
}

/// Build a `SourceLocation` from a byte span start and the source text.
fn location_from_span(file: &str, source: &str, span_start: usize) -> SourceLocation {
    let (line, col): (usize, usize) = byte_offset_to_line_col(source, span_start);
    SourceLocation {
        file: file.to_owned(),
        line,
        col,
    }
}

/// Normalize documentation line endings and reject control characters that cannot
/// be represented safely by every generated language.
fn normalize_docs(
    docs: Option<&toml::Spanned<String>>,
    file: &str,
    source: &str,
) -> Result<Option<String>, PolyplugcError> {
    let Some(docs) = docs else {
        return Ok(None);
    };

    for (offset, character) in docs.get_ref().char_indices() {
        let code: u32 = character as u32;
        let allowed: bool = matches!(character, '\t' | '\n' | '\r')
            || (code >= 0x20 && !(0x7F..=0x9F).contains(&code));
        if !allowed {
            return Err(PolyplugcError::InvalidDocumentation {
                character,
                location: Some(location_from_span(file, source, docs.span().start + offset)),
            });
        }
    }

    Ok(Some(
        docs.get_ref().replace("\r\n", "\n").replace('\r', "\n"),
    ))
}

/// Normalize documentation that originates from an untagged TOML value. Serde
/// cannot preserve a value span while decoding an untagged enum, so this follows
/// the same validation policy without a location.
fn normalize_unspanned_docs(docs: Option<&str>) -> Result<Option<String>, PolyplugcError> {
    let Some(docs) = docs else {
        return Ok(None);
    };
    for character in docs.chars() {
        let code: u32 = character as u32;
        let allowed: bool = matches!(character, '\t' | '\n' | '\r')
            || (code >= 0x20 && !(0x7F..=0x9F).contains(&code));
        if !allowed {
            return Err(PolyplugcError::InvalidDocumentation {
                character,
                location: None,
            });
        }
    }
    Ok(Some(docs.replace("\r\n", "\n").replace('\r', "\n")))
}

/// Lower and validate one language entry's attribute contents.
fn lower_language_attributes(
    raw: Option<&RawLanguageAttributes>,
    lang: Lang,
    node: CustomizableNode,
    file: &str,
    source: &str,
) -> Result<Option<LanguageAttributes>, PolyplugcError> {
    let Some(raw) = raw else {
        return Ok(None);
    };

    let attributes: &[String] = raw
        .attributes
        .as_ref()
        .map(toml::Spanned::get_ref)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let mut lowered: Vec<String> = Vec::with_capacity(attributes.len());
    for value in attributes {
        let reason: Option<&str> = if value.trim().is_empty() {
            Some("attribute contents must not be empty")
        } else if value.contains(['\n', '\r']) {
            Some("attribute contents must be a single line")
        } else {
            None
        };
        if let Some(reason) = reason {
            return Err(PolyplugcError::InvalidLanguageAttribute {
                language: lang.schema_key().to_owned(),
                node: node.label().to_owned(),
                attribute: value.to_owned(),
                reason: reason.to_owned(),
                location: Box::new(location_from_span(
                    file,
                    source,
                    raw.attributes
                        .as_ref()
                        .map(|attributes| attributes.span().start)
                        .unwrap_or_default(),
                )),
            });
        }
        lowered.push(value.to_owned());
    }
    if lowered.is_empty() {
        Ok(None)
    } else {
        Ok(Some(LanguageAttributes {
            attributes: lowered,
        }))
    }
}

/// Lower the six closed language entries for one authored API node.
fn lower_langs(
    raw: Option<&RawLanguageRules>,
    node: CustomizableNode,
    file: &str,
    source: &str,
) -> Result<LanguageRules, PolyplugcError> {
    let Some(raw) = raw else {
        return Ok(LanguageRules::default());
    };
    let (rust, rust_semantics) = lower_rust_language_rules(raw.rust.as_ref(), node, file, source)?;
    Ok(LanguageRules {
        rust,
        rust_semantics,
        cpp: lower_language_attributes(raw.cpp.as_ref(), Lang::Cpp, node, file, source)?,
        csharp: lower_language_attributes(raw.csharp.as_ref(), Lang::CSharp, node, file, source)?,
        python: lower_language_attributes(raw.python.as_ref(), Lang::Python, node, file, source)?,
        lua: lower_language_attributes(raw.lua.as_ref(), Lang::Lua, node, file, source)?,
        javascript: lower_language_attributes(
            raw.javascript.as_ref(),
            Lang::JsQuickJs,
            node,
            file,
            source,
        )?,
    })
}

fn lower_rust_language_rules(
    raw: Option<&RawRustLanguageRules>,
    node: CustomizableNode,
    file: &str,
    source: &str,
) -> Result<(Option<LanguageAttributes>, Option<RustLanguageRules>), PolyplugcError> {
    let Some(raw) = raw else {
        return Ok((None, None));
    };
    let rust_attributes = lower_language_attributes(
        Some(&RawLanguageAttributes {
            attributes: raw.attributes.clone(),
        }),
        Lang::Rust,
        node,
        file,
        source,
    )?;
    let attributes = rust_attributes.clone().unwrap_or_default();
    let mut derives = Vec::new();
    for derive in &raw.derives {
        if derive.trim().is_empty() || derive.contains(['\n', '\r']) {
            return Err(PolyplugcError::ValidationFailed {
                message: format!(
                    "Rust derive on {} must be a non-empty single token",
                    node.label()
                ),
            });
        }
        if !derives.contains(derive) {
            derives.push(derive.clone());
        }
    }
    let serde = match raw.serde.as_deref() {
        None => None,
        Some("human-name-binary-discriminant") => {
            Some(RustEnumSerdePolicy::HumanNameBinaryDiscriminant)
        }
        Some(value) => {
            return Err(PolyplugcError::ValidationFailed {
                message: format!("unsupported Rust enum serde policy `{value}`"),
            });
        }
    };
    let tagged_enum = raw.tagged_enum.as_ref().map(|projection| RustTaggedEnum {
        tag_field: projection.tag_field.clone(),
        variants: projection
            .variants
            .iter()
            .map(|variant| RustTaggedEnumVariant {
                tag: variant.tag.clone(),
                name: variant.name.clone().unwrap_or_else(|| variant.tag.clone()),
                payload: variant.payload.clone(),
                default: variant.default,
            })
            .collect(),
    });
    if raw.empty_sequence_as_null && node != CustomizableNode::Field {
        return Err(PolyplugcError::ValidationFailed {
            message: "Rust `empty_sequence_as_null` is only valid on a field".to_owned(),
        });
    }
    if tagged_enum.is_some() && node != CustomizableNode::Type {
        return Err(PolyplugcError::ValidationFailed {
            message: "Rust `tagged_enum` is only valid on a type".to_owned(),
        });
    }
    if serde.is_some() && node != CustomizableNode::Enum {
        return Err(PolyplugcError::ValidationFailed {
            message: "Rust `serde` is only valid on an enum".to_owned(),
        });
    }
    if (raw.primary_name.is_some() || !raw.aliases.is_empty() || raw.default)
        && node != CustomizableNode::EnumVariant
    {
        return Err(PolyplugcError::ValidationFailed {
            message:
                "Rust `primary_name`, `aliases`, and `default` are only valid on an enum variant"
                    .to_owned(),
        });
    }
    let semantics = RustLanguageRules {
        attributes: attributes.attributes,
        derives,
        serde,
        primary_name: raw.primary_name.clone(),
        aliases: raw.aliases.clone(),
        default: raw.default,
        empty_sequence_as_null: raw.empty_sequence_as_null,
        tagged_enum,
    };
    Ok((
        rust_attributes,
        (semantics != RustLanguageRules::default()).then_some(semantics),
    ))
}

/// Lower the return-table language rules, whose untagged TOML representation
/// cannot preserve spans for nested values.
fn lower_unspanned_language_attributes(
    raw: Option<&RawUnspannedLanguageAttributes>,
    lang: Lang,
    node: CustomizableNode,
    file: &str,
    source: &str,
    return_span: usize,
) -> Result<Option<LanguageAttributes>, PolyplugcError> {
    let Some(raw) = raw else {
        return Ok(None);
    };

    for value in &raw.attributes {
        let reason: Option<&str> = if value.trim().is_empty() {
            Some("attribute contents must not be empty")
        } else if value.contains(['\n', '\r']) {
            Some("attribute contents must be a single line")
        } else {
            None
        };
        if let Some(reason) = reason {
            return Err(PolyplugcError::InvalidLanguageAttribute {
                language: lang.schema_key().to_owned(),
                node: node.label().to_owned(),
                attribute: value.to_owned(),
                reason: reason.to_owned(),
                location: Box::new(location_from_span(file, source, return_span)),
            });
        }
    }
    if raw.attributes.is_empty() {
        Ok(None)
    } else {
        Ok(Some(LanguageAttributes {
            attributes: raw.attributes.clone(),
        }))
    }
}

/// Lower return-table language rules while retaining the enclosing return-table
/// source span for diagnostics from TOML's unspanned nested values.
fn lower_unspanned_langs(
    raw: Option<&RawUnspannedLanguageRules>,
    node: CustomizableNode,
    file: &str,
    source: &str,
    return_span: usize,
) -> Result<LanguageRules, PolyplugcError> {
    let Some(raw) = raw else {
        return Ok(LanguageRules::default());
    };

    Ok(LanguageRules {
        rust_semantics: None,
        rust: lower_unspanned_language_attributes(
            raw.rust.as_ref(),
            Lang::Rust,
            node,
            file,
            source,
            return_span,
        )?,
        cpp: lower_unspanned_language_attributes(
            raw.cpp.as_ref(),
            Lang::Cpp,
            node,
            file,
            source,
            return_span,
        )?,
        csharp: lower_unspanned_language_attributes(
            raw.csharp.as_ref(),
            Lang::CSharp,
            node,
            file,
            source,
            return_span,
        )?,
        python: lower_unspanned_language_attributes(
            raw.python.as_ref(),
            Lang::Python,
            node,
            file,
            source,
            return_span,
        )?,
        lua: lower_unspanned_language_attributes(
            raw.lua.as_ref(),
            Lang::Lua,
            node,
            file,
            source,
            return_span,
        )?,
        javascript: lower_unspanned_language_attributes(
            raw.javascript.as_ref(),
            Lang::JsQuickJs,
            node,
            file,
            source,
            return_span,
        )?,
    })
}

/// Parse a version from a spanned TOML field, enriching a `VersionOverflow`
/// error with the source location of the offending `version = "..."` field.
fn parse_version_spanned(
    spanned: &toml::Spanned<String>,
    file: &str,
    source: &str,
) -> Result<Version, PolyplugcError> {
    match Version::parse(spanned.get_ref().as_str()) {
        Ok(version) => Ok(version),
        Err(PolyplugcError::VersionOverflow {
            component,
            value,
            version_str,
            suggestion,
            ..
        }) => Err(PolyplugcError::VersionOverflow {
            component,
            value,
            version_str,
            location: Some(location_from_span(file, source, spanned.span().start)),
            suggestion,
        }),
        Err(other) => Err(other),
    }
}

/// Compute the Levenshtein edit distance between two strings (bounded to 10 to
/// avoid quadratic cost on very long inputs).
fn edit_distance(a: &str, b: &str) -> usize {
    const BOUND: usize = 10;
    let a: &[u8] = a.as_bytes();
    let b: &[u8] = b.as_bytes();
    if a.len() > BOUND * 4 || b.len() > BOUND * 4 {
        return BOUND + 1;
    }
    let m: usize = a.len();
    let n: usize = b.len();
    let mut dp: Vec<usize> = (0..=n).collect();
    for i in 1..=m {
        let mut prev: usize = dp[0];
        dp[0] = i;
        for j in 1..=n {
            let temp: usize = dp[j];
            dp[j] = if a[i - 1] == b[j - 1] {
                prev
            } else {
                1 + prev.min(dp[j]).min(dp[j - 1])
            };
            prev = temp;
        }
    }
    dp[n]
}

/// Return the closest match to `type_ref` among a combined list of type names
/// (user-defined types, enum names, and builtin type names), if the edit
/// distance is ≤ `max_dist`.  Returns `None` when no close match exists.
fn nearest_type_suggestion(type_ref: &str, candidates: &[&str]) -> Option<String> {
    const MAX_DIST: usize = 2;
    let mut best_dist: usize = MAX_DIST + 1;
    let mut best: Option<&str> = None;
    for &candidate in candidates {
        let d: usize = edit_distance(type_ref, candidate);
        if d < best_dist {
            best_dist = d;
            best = Some(candidate);
        }
    }
    best.map(|s| s.to_owned())
}

/// Return the closest match to `repr` among the valid repr strings
/// (u8, u16, u32, u64) if edit distance is ≤ 2.
fn nearest_repr_suggestion(repr: &str) -> Option<String> {
    nearest_type_suggestion(repr, &["u8", "u16", "u32", "u64"])
}

// ─── Type-ref resolution with diagnostics ────────────────────────────────────

/// All type names that are always valid (primitives + ABI builtins).  Used for
/// "did you mean?" suggestions when a type reference is unknown.
const BUILTIN_TYPE_NAMES: &[&str] = &[
    "u8",
    "u16",
    "u32",
    "u64",
    "i8",
    "i16",
    "i32",
    "i64",
    "f32",
    "f64",
    "bool",
    "StringView",
    "Buffer",
    "Ptr",
    "Void",
];

/// Resolve a type reference with source location and suggestion enrichment.
fn resolve_type_ref_spanned(
    spanned_ty: &toml::Spanned<String>,
    contract: &str,
    all_known_names: &[String],
    file: &str,
    source: &str,
) -> Result<ResolvedTypeRef, PolyplugcError> {
    let ty: &str = spanned_ty.get_ref().as_str();
    resolve_type_ref(ty, contract, all_known_names).map_err(|_| {
        let location: Option<SourceLocation> =
            Some(location_from_span(file, source, spanned_ty.span().start));
        // Build candidate list: builtins + user-defined types/enums.
        let mut candidates: Vec<&str> = BUILTIN_TYPE_NAMES.to_vec();
        for name in all_known_names {
            candidates.push(name.as_str());
        }
        let suggestion: Option<String> = nearest_type_suggestion(ty, &candidates);
        PolyplugcError::UnknownType {
            type_ref: ty.to_owned(),
            contract: contract.to_owned(),
            location,
            suggestion,
        }
    })
}

/// Normalize source line endings before TOML parsing so documentation and TOML
/// syntax have one canonical newline representation.
fn normalize_source_line_endings(source: &str) -> Cow<'_, str> {
    if source.contains('\r') {
        Cow::Owned(source.replace("\r\n", "\n").replace('\r', "\n"))
    } else {
        Cow::Borrowed(source)
    }
}
// ─── Parse entry points ───────────────────────────────────────────────────────

#[derive(Clone, Copy)]
enum BundleParseMode {
    External,
    Internal,
}

pub fn parse_api(path: &Path) -> Result<ValidatedIr, PolyplugcError> {
    let content: String = fs::read_to_string(path).map_err(|e| PolyplugcError::ReadFailed {
        path: path.to_string_lossy().into_owned(),
        source: e,
    })?;
    parse_api_str_with_file(&content, &path.to_string_lossy())
}

#[allow(dead_code)]
pub fn parse_api_str(content: &str) -> Result<ValidatedIr, PolyplugcError> {
    parse_api_str_with_file(content, "<input>")
}

fn api_schema_error(e: toml::de::Error, file: &str, source: &str) -> PolyplugcError {
    let message: String = match e.message() {
        message if message.contains("unknown field `plugin_contract`") => {
            "`[[plugin_contract]]` is invalid; use `[[guest_contract]]` instead".to_owned()
        }
        message if message.contains("unknown field `contract`") => {
            "`[[contract]]` is invalid; use `[[guest_contract]]` instead".to_owned()
        }
        message if message.contains("unknown field") => format!(
            "{message}; valid top-level API keys are `[[types]]`, `[[enum]]`, `[[guest_contract]]`, and `[[host_contract]]`"
        ),
        message => message.to_owned(),
    };
    let location: Option<SourceLocation> = e
        .span()
        .map(|span| location_from_span(file, source, span.start));
    PolyplugcError::TomlParseError { message, location }
}

fn parse_api_str_with_file(content: &str, file: &str) -> Result<ValidatedIr, PolyplugcError> {
    let content: Cow<'_, str> = normalize_source_line_endings(content);
    let raw: RawApiSchema = toml::from_str(content.as_ref())
        .map_err(|e| api_schema_error(e, file, content.as_ref()))?;
    lower_api(raw, content.as_ref(), file)
}

#[allow(dead_code)]
pub fn parse_bundle_str(content: &str) -> Result<ValidatedIr, PolyplugcError> {
    parse_bundle_str_with_file(content, "<input>")
}

fn parse_bundle_str_with_file(content: &str, file: &str) -> Result<ValidatedIr, PolyplugcError> {
    let content: Cow<'_, str> = normalize_source_line_endings(content);
    let raw: RawBundleSchema = toml::from_str(content.as_ref()).map_err(|e| {
        let location: Option<SourceLocation> = e
            .span()
            .map(|span| location_from_span(file, content.as_ref(), span.start));
        PolyplugcError::TomlParseError {
            message: e.message().to_owned(),
            location,
        }
    })?;
    lower_bundle(raw, content.as_ref(), file, BundleParseMode::External)
}

pub fn parse_bundle_with_api(path: &Path) -> Result<ValidatedIr, PolyplugcError> {
    parse_bundle_with_api_mode(path, BundleParseMode::External)
}

/// Parse one bundle plus its API schema for the internal Rust generation profile.
///
/// Internal bundles share all canonical metadata and provider validation while
/// deliberately omitting external loader/artifact acquisition validation.
pub fn parse_bundle_with_api_internal(path: &Path) -> Result<ValidatedIr, PolyplugcError> {
    parse_bundle_with_api_mode(path, BundleParseMode::Internal)
}

fn parse_bundle_with_api_mode(
    path: &Path,
    mode: BundleParseMode,
) -> Result<ValidatedIr, PolyplugcError> {
    let content: String = fs::read_to_string(path).map_err(|e| PolyplugcError::ReadFailed {
        path: path.to_string_lossy().into_owned(),
        source: e,
    })?;
    let content: String = normalize_source_line_endings(&content).into_owned();
    let file: String = path.to_string_lossy().into_owned();
    let raw: RawBundleSchema = toml::from_str(&content).map_err(|e| {
        let location: Option<SourceLocation> = e
            .span()
            .map(|span| location_from_span(&file, &content, span.start));
        PolyplugcError::TomlParseError {
            message: e.message().to_owned(),
            location,
        }
    })?;

    let bundle_dir: &Path = path.parent().unwrap_or_else(|| Path::new("."));

    let api_ir: ValidatedIr = if let Some(ref api_path_str) = raw.bundle.api {
        let api_path: PathBuf = bundle_dir.join(api_path_str);
        parse_api(&api_path)?
    } else {
        ValidatedIr {
            types: Vec::new(),
            enums: Vec::new(),
            contracts: Vec::new(),
            host_contracts: Vec::new(),
            bundle: None,
            langs: LanguageRules::default(),
        }
    };
    check_bundle_name_conflict(&raw.bundle.name, &api_ir.contracts)?;

    let bundle_ir: ValidatedIr = lower_bundle(raw, &content, &file, mode)?;
    Ok(ValidatedIr {
        types: api_ir.types,
        enums: api_ir.enums,
        contracts: api_ir.contracts,
        host_contracts: api_ir.host_contracts,
        bundle: bundle_ir.bundle,
        langs: api_ir.langs,
    })
}

fn check_bundle_name_conflict(
    bundle_name: &str,
    contracts: &[ResolvedContract],
) -> Result<(), PolyplugcError> {
    for contract in contracts {
        if contract.name == bundle_name {
            return Err(PolyplugcError::BundleNameConflict {
                bundle_name: bundle_name.to_owned(),
            });
        }
    }
    Ok(())
}

fn validate_enum_value_expr(
    expr: &str,
    enum_name: &str,
    variant_name: &str,
    declared_variants: &[String],
) -> Result<(), PolyplugcError> {
    let chars: Vec<char> = expr.chars().collect();
    let len: usize = chars.len();
    let mut i: usize = 0;
    while i < len {
        let c: char = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c.is_ascii_digit() {
            if c == '0' && i + 1 < len && (chars[i + 1] == 'x' || chars[i + 1] == 'X') {
                i += 2;
                while i < len && chars[i].is_ascii_hexdigit() {
                    i += 1;
                }
            } else if c == '0' && i + 1 < len && (chars[i + 1] == 'b' || chars[i + 1] == 'B') {
                i += 2;
                while i < len && (chars[i] == '0' || chars[i] == '1') {
                    i += 1;
                }
            } else {
                while i < len && chars[i].is_ascii_digit() {
                    i += 1;
                }
            }
            continue;
        }
        if c.is_alphabetic() || c == '_' {
            let start: usize = i;
            while i < len && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let ident: String = chars[start..i].iter().collect();
            if declared_variants.contains(&ident) {
                continue;
            }
            if chars[start].is_uppercase() {
                return Err(PolyplugcError::EnumForwardRef {
                    enum_name: enum_name.to_owned(),
                    variant_name: variant_name.to_owned(),
                    ref_name: ident,
                });
            }
            return Err(PolyplugcError::EnumInvalidValueExpr {
                enum_name: enum_name.to_owned(),
                variant_name: variant_name.to_owned(),
                expr: expr.to_owned(),
            });
        }
        if c == '<' {
            if i + 1 < len && chars[i + 1] == '<' {
                i += 2;
                continue;
            }
            return Err(PolyplugcError::EnumInvalidValueExpr {
                enum_name: enum_name.to_owned(),
                variant_name: variant_name.to_owned(),
                expr: expr.to_owned(),
            });
        }
        if c == '|' {
            i += 1;
            continue;
        }
        if c == '~' {
            i += 1;
            continue;
        }
        if c == '(' || c == ')' {
            i += 1;
            continue;
        }
        return Err(PolyplugcError::EnumInvalidValueExpr {
            enum_name: enum_name.to_owned(),
            variant_name: variant_name.to_owned(),
            expr: expr.to_owned(),
        });
    }
    Ok(())
}

fn check_enum_chained_refs(
    enum_name: &str,
    variants: &[EnumVariant],
) -> Result<(), PolyplugcError> {
    let expr_contains_variant_ref = |expr: &str, variant_names: &[&str]| -> bool {
        let chars: Vec<char> = expr.chars().collect();
        let len: usize = chars.len();
        let mut j: usize = 0;
        while j < len {
            if chars[j].is_alphabetic() || chars[j] == '_' {
                let start: usize = j;
                while j < len && (chars[j].is_alphanumeric() || chars[j] == '_') {
                    j += 1;
                }
                let ident: String = chars[start..j].iter().collect();
                if variant_names.contains(&ident.as_str()) {
                    return true;
                }
            } else {
                j += 1;
            }
        }
        false
    };

    let all_names: Vec<&str> = variants.iter().map(|v| v.name.as_str()).collect();

    for variant in variants {
        let chars: Vec<char> = variant.value.chars().collect();
        let len: usize = chars.len();
        let mut j: usize = 0;
        while j < len {
            if chars[j].is_alphabetic() || chars[j] == '_' {
                let start: usize = j;
                while j < len && (chars[j].is_alphanumeric() || chars[j] == '_') {
                    j += 1;
                }
                let ref_name: String = chars[start..j].iter().collect();
                if all_names.contains(&ref_name.as_str()) {
                    if let Some(ref_variant) = variants.iter().find(|v| v.name == ref_name) {
                        if expr_contains_variant_ref(&ref_variant.value, &all_names) {
                            return Err(PolyplugcError::EnumChainedRef {
                                enum_name: enum_name.to_owned(),
                                variant_name: variant.name.clone(),
                                ref_name,
                            });
                        }
                    }
                }
            } else {
                j += 1;
            }
        }
    }
    Ok(())
}

/// Return `true` if `name` is a valid identifier: `[A-Za-z_][A-Za-z0-9_]*`.
fn is_valid_identifier(name: &str) -> bool {
    let mut chars: core::str::Chars<'_> = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c: char| c.is_ascii_alphanumeric() || c == '_')
}

/// Reject `name` if it is a reserved keyword in any target language (or a
/// polyplug-reserved name). Such names flow verbatim into generated source and
/// would produce uncompilable output, so they must be caught at parse time.
fn validate_not_reserved(name: &str, kind: &str, context: &str) -> Result<(), PolyplugcError> {
    match reserved::reserved_in(name) {
        Some(languages) => Err(PolyplugcError::ReservedIdentifier {
            kind: kind.to_owned(),
            name: name.to_owned(),
            context: context.to_owned(),
            languages,
            location: None,
        }),
        None => Ok(()),
    }
}

/// Validate a plain identifier (function/param name). Names flow verbatim into
/// generated source, so invalid identifiers must be rejected before codegen.
fn validate_identifier(name: &str, kind: &str, context: &str) -> Result<(), PolyplugcError> {
    if !is_valid_identifier(name) {
        return Err(PolyplugcError::InvalidIdentifier {
            kind: kind.to_owned(),
            name: name.to_owned(),
            context: context.to_owned(),
            location: None,
        });
    }
    validate_not_reserved(name, kind, context)
}

/// Validate an identifier emitted only into Rust domain declarations.
fn validate_rust_identifier(name: &str, kind: &str, context: &str) -> Result<(), PolyplugcError> {
    if !is_valid_identifier(name) {
        return Err(PolyplugcError::InvalidIdentifier {
            kind: kind.to_owned(),
            name: name.to_owned(),
            context: context.to_owned(),
            location: None,
        });
    }
    if let Some(languages) = reserved::reserved_in(name)
        && languages
            .split(", ")
            .any(|language| language == "Rust" || language == "polyplug")
    {
        return Err(PolyplugcError::ReservedIdentifier {
            kind: kind.to_owned(),
            name: name.to_owned(),
            context: context.to_owned(),
            languages,
            location: None,
        });
    }
    Ok(())
}

/// Validate a (possibly dotted) contract name. Each dot-separated segment must
/// be a valid identifier — e.g. `pipeline.Decoder`, `host.fs.reader`.
fn validate_contract_name(name: &str, kind: &str) -> Result<(), PolyplugcError> {
    if name.is_empty() || !name.split('.').all(is_valid_identifier) {
        return Err(PolyplugcError::InvalidIdentifier {
            kind: kind.to_owned(),
            name: name.to_owned(),
            context: name.to_owned(),
            location: None,
        });
    }
    // Each dot-separated segment becomes an identifier in generated code (module
    // path, class name, function prefix), so no segment may be a reserved word.
    for segment in name.split('.') {
        validate_not_reserved(segment, kind, name)?;
    }
    Ok(())
}

/// Validate names within a contract: the contract name itself, every function
/// name (also checking for duplicates), and every parameter name.
fn validate_contract_members(
    contract_name: &str,
    kind: &str,
    functions: &[RawFunction],
) -> Result<(), PolyplugcError> {
    validate_contract_name(contract_name, kind)?;
    let mut seen_functions: Vec<&str> = Vec::with_capacity(functions.len());
    for raw_fn in functions {
        validate_identifier(&raw_fn.name, "function", contract_name)?;
        if seen_functions.contains(&raw_fn.name.as_str()) {
            return Err(PolyplugcError::DuplicateFunctionName {
                contract: contract_name.to_owned(),
                function: raw_fn.name.clone(),
                first_defined_at: None,
            });
        }
        seen_functions.push(&raw_fn.name);
        for p in &raw_fn.params {
            validate_identifier(&p.name, "parameter", &raw_fn.name)?;
        }
    }
    Ok(())
}

/// Synthesize the `{ items: u64, len: u64 }` wrapper struct for every `ArrayOf_*`
/// reference the `Array<T>` desugar produced (fields, params, returns). `items` is
/// the arena address of the element block, `len` the element count. Deterministic
/// first-seen order; each wrapper emitted once and never for a name a user already
/// defined.
fn collect_array_wrapper_types(
    types: &[ResolvedType],
    contracts: &[ResolvedContract],
    host_contracts: &[ResolvedHostContract],
) -> Vec<ResolvedType> {
    fn note(names: &mut Vec<String>, ty: &ResolvedTypeRef) {
        if let ResolvedTypeRef::UserDefined(n) = ty
            && array_element_name(n).is_some()
            && !names.contains(n)
        {
            names.push(n.clone());
        }
    }
    let mut names: Vec<String> = Vec::new();
    for t in types {
        for f in &t.fields {
            note(&mut names, &f.ty);
        }
    }
    for c in contracts {
        for f in &c.functions {
            for p in &f.params {
                note(&mut names, &p.ty);
            }
            if let Some(r) = &f.returns {
                note(&mut names, r);
            }
        }
    }
    for c in host_contracts {
        for f in &c.functions {
            for p in &f.params {
                note(&mut names, &p.ty);
            }
            if let Some(r) = &f.returns {
                note(&mut names, r);
            }
        }
    }
    names
        .into_iter()
        .filter(|n| !types.iter().any(|t| &t.name == n))
        .map(|name| ResolvedType {
            name,
            fields: vec![
                ResolvedField {
                    name: "items".to_owned(),
                    ty: ResolvedTypeRef::Primitive(PrimitiveType::U64),
                    docs: None,
                    langs: LanguageRules::default(),
                },
                ResolvedField {
                    name: "len".to_owned(),
                    ty: ResolvedTypeRef::Primitive(PrimitiveType::U64),
                    docs: None,
                    langs: LanguageRules::default(),
                },
            ],
            docs: None,
            langs: LanguageRules::default(),
        })
        .collect()
}

/// Stable topological sort of `types` so every struct is emitted AFTER the
/// user-defined types it references by field (nested structs and `ArrayOf_*`
/// wrappers). Types with no cross-dependency keep their original relative order,
/// so output for contracts without a struct-embedding-array (or out-of-order
/// struct) field is byte-identical to the input order. A dependency naming a type
/// not in the set (a primitive, an enum, an ABI builtin) imposes no ordering.
/// By-value ABI structs cannot form cycles (infinite size), and the `in_progress`
/// guard makes a hypothetical cycle terminate rather than recurse forever.
fn topologically_order_types(types: Vec<ResolvedType>) -> Vec<ResolvedType> {
    let type_names: HashSet<&str> = types
        .iter()
        .map(|t: &ResolvedType| t.name.as_str())
        .collect();
    let mut ordered: Vec<ResolvedType> = Vec::with_capacity(types.len());
    let mut emitted: HashSet<String> = HashSet::new();
    let mut in_progress: HashSet<String> = HashSet::new();
    for t in &types {
        visit_type_deps(
            &t.name,
            &types,
            &type_names,
            &mut emitted,
            &mut in_progress,
            &mut ordered,
        );
    }
    ordered
}

/// DFS post-order visit for [`topologically_order_types`]: emit `name`'s
/// user-defined field dependencies (that are themselves in the type set) before
/// `name` itself. `emitted` skips already-placed types; `in_progress` breaks any
/// cycle.
fn visit_type_deps(
    name: &str,
    types: &[ResolvedType],
    type_names: &HashSet<&str>,
    emitted: &mut HashSet<String>,
    in_progress: &mut HashSet<String>,
    ordered: &mut Vec<ResolvedType>,
) {
    if emitted.contains(name) || in_progress.contains(name) {
        return;
    }
    let Some(ty) = types.iter().find(|t: &&ResolvedType| t.name == name) else {
        return;
    };
    in_progress.insert(name.to_owned());
    for f in &ty.fields {
        if let ResolvedTypeRef::UserDefined(dep) = &f.ty
            && type_names.contains(dep.as_str())
        {
            visit_type_deps(dep, types, type_names, emitted, in_progress, ordered);
        }
    }
    in_progress.remove(name);
    emitted.insert(name.to_owned());
    ordered.push(ty.clone());
}

fn validate_rust_semantic_rules(
    types: &[ResolvedType],
    enums: &[EnumDef],
) -> Result<(), PolyplugcError> {
    for enum_def in enums {
        let default_count = enum_def
            .variants
            .iter()
            .filter(|variant| variant.langs.rust().is_some_and(|rule| rule.default))
            .count();
        if default_count > 1 {
            return Err(PolyplugcError::ValidationFailed {
                message: format!(
                    "Rust enum `{}` has more than one default variant",
                    enum_def.name
                ),
            });
        }
        let mut serialized_names: HashSet<&str> = HashSet::new();
        for variant in &enum_def.variants {
            let rules = variant.langs.rust();
            let primary_name = rules
                .and_then(|rules| rules.primary_name.as_deref())
                .unwrap_or(&variant.name);
            let aliases = rules
                .map(|rules| rules.aliases.iter().map(String::as_str))
                .into_iter()
                .flatten();
            for serialized_name in once(primary_name).chain(aliases) {
                if !serialized_names.insert(serialized_name) {
                    return Err(PolyplugcError::ValidationFailed {
                        message: format!(
                            "Rust enum `{}` reuses serialized name `{serialized_name}`",
                            enum_def.name
                        ),
                    });
                }
            }
        }
        let Some(rules) = enum_def.langs.rust() else {
            continue;
        };
        if rules.serde.is_some() && enum_def.bitflag {
            return Err(PolyplugcError::ValidationFailed {
                message: format!(
                    "Rust serde policy requires ordinary enum `{}`",
                    enum_def.name
                ),
            });
        }
        if rules.serde.is_some()
            && rules
                .derives
                .iter()
                .any(|derive| derive == "Serialize" || derive == "Deserialize")
        {
            return Err(PolyplugcError::ValidationFailed {
                message: format!(
                    "Rust enum `{}` dual serde owns Serialize and Deserialize",
                    enum_def.name
                ),
            });
        }
    }
    for ty in types {
        for field in &ty.fields {
            if field
                .langs
                .rust()
                .is_some_and(|rules| rules.empty_sequence_as_null)
                && !matches!(&field.ty, ResolvedTypeRef::UserDefined(name) if array_element_name(name).is_some())
            {
                return Err(PolyplugcError::ValidationFailed {
                    message: format!(
                        "Rust `empty_sequence_as_null` field `{}.{}` must be an Array<T>",
                        ty.name, field.name
                    ),
                });
            }
        }
        let Some(projection) = ty.langs.rust().and_then(|rules| rules.tagged_enum.as_ref()) else {
            continue;
        };
        if array_element_name(&ty.name).is_some() {
            return Err(PolyplugcError::ValidationFailed {
                message: format!(
                    "Rust tagged_enum cannot target generated type `{}`",
                    ty.name
                ),
            });
        }
        let tag_field = ty
            .fields
            .iter()
            .find(|field| field.name == projection.tag_field)
            .ok_or_else(|| PolyplugcError::ValidationFailed {
                message: format!(
                    "Rust tagged_enum type `{}` has no tag field `{}`",
                    ty.name, projection.tag_field
                ),
            })?;
        let ResolvedTypeRef::UserDefined(tag_name) = &tag_field.ty else {
            return Err(PolyplugcError::ValidationFailed {
                message: format!(
                    "Rust tagged_enum tag field `{}` must name an ordinary enum",
                    projection.tag_field
                ),
            });
        };
        let tag_enum = enums
            .iter()
            .find(|item| item.name == *tag_name)
            .ok_or_else(|| PolyplugcError::ValidationFailed {
                message: format!(
                    "Rust tagged_enum tag field `{}` must name an ordinary enum",
                    projection.tag_field
                ),
            })?;
        if tag_enum.bitflag {
            return Err(PolyplugcError::ValidationFailed {
                message: format!("Rust tagged_enum tag enum `{tag_name}` must not be a bitflag"),
            });
        }
        if projection.variants.len() != tag_enum.variants.len() {
            return Err(PolyplugcError::ValidationFailed {
                message: format!(
                    "Rust tagged_enum `{}` must map every tag variant exactly once",
                    ty.name
                ),
            });
        }
        let mut tags = HashSet::new();
        let mut names = HashSet::new();
        let mut payloads = HashSet::new();
        let mut defaults = 0usize;
        for mapping in &projection.variants {
            validate_rust_identifier(&mapping.name, "Rust tagged_enum variant", &ty.name)?;
            if !names.insert(mapping.name.as_str()) {
                return Err(PolyplugcError::ValidationFailed {
                    message: format!(
                        "Rust tagged_enum `{}` reuses projected variant name `{}`",
                        ty.name, mapping.name
                    ),
                });
            }
            if !tag_enum
                .variants
                .iter()
                .any(|variant| variant.name == mapping.tag)
                || !tags.insert(mapping.tag.as_str())
            {
                return Err(PolyplugcError::ValidationFailed {
                    message: format!(
                        "Rust tagged_enum `{}` maps tag `{}` incorrectly",
                        ty.name, mapping.tag
                    ),
                });
            }
            if mapping.default && mapping.payload.is_some() {
                return Err(PolyplugcError::ValidationFailed {
                    message: format!(
                        "Rust tagged_enum `{}` default variant must be unit",
                        ty.name
                    ),
                });
            }
            if let Some(payload) = &mapping.payload {
                let field = ty
                    .fields
                    .iter()
                    .find(|field| field.name == *payload)
                    .ok_or_else(|| PolyplugcError::ValidationFailed {
                        message: format!(
                            "Rust tagged_enum `{}` payload field `{payload}` does not exist",
                            ty.name
                        ),
                    })?;
                if field.name == projection.tag_field || !payloads.insert(payload.as_str()) {
                    return Err(PolyplugcError::ValidationFailed {
                        message: format!("Rust tagged_enum `{}` reuses field `{payload}`", ty.name),
                    });
                }
            }
            defaults += usize::from(mapping.default);
        }
        if defaults > 1 {
            return Err(PolyplugcError::ValidationFailed {
                message: format!(
                    "Rust tagged_enum `{}` has more than one default variant",
                    ty.name
                ),
            });
        }
    }
    Ok(())
}

fn lower_api(raw: RawApiSchema, source: &str, file: &str) -> Result<ValidatedIr, PolyplugcError> {
    let langs: LanguageRules =
        lower_langs(raw.langs.as_ref(), CustomizableNode::Api, file, source)?;
    let known_type_names: Vec<String> = raw.types.iter().map(|t| t.name.clone()).collect();

    let known_enum_names: Vec<String> = raw.r#enum.iter().map(|e| e.name.clone()).collect();
    for name in &known_enum_names {
        if known_type_names.contains(name) {
            return Err(PolyplugcError::EnumNameCollision {
                name: name.clone(),
                suggestion: None,
            });
        }
    }

    let all_known_names: Vec<String> = known_type_names
        .iter()
        .chain(known_enum_names.iter())
        .cloned()
        .collect();

    let mut resolved_types: Vec<ResolvedType> = Vec::new();
    for raw_type in &raw.types {
        validate_identifier(&raw_type.name, "type", &raw_type.name)?;
        let docs: Option<String> = normalize_docs(raw_type.docs.as_ref(), file, source)?;
        let mut fields: Vec<ResolvedField> = Vec::new();
        for field in &raw_type.fields {
            validate_identifier(&field.name, "field", &raw_type.name)?;
            let ty: ResolvedTypeRef = resolve_type_ref_spanned(
                &field.ty,
                &raw_type.name,
                &all_known_names,
                file,
                source,
            )?;
            fields.push(ResolvedField {
                name: field.name.clone(),
                ty,
                docs: normalize_docs(field.docs.as_ref(), file, source)?,
                langs: lower_langs(field.langs.as_ref(), CustomizableNode::Field, file, source)?,
            });
        }
        resolved_types.push(ResolvedType {
            name: raw_type.name.clone(),
            fields,
            docs,
            langs: lower_langs(
                raw_type.langs.as_ref(),
                CustomizableNode::Type,
                file,
                source,
            )?,
        });
    }

    let mut resolved_enums: Vec<EnumDef> = Vec::new();
    for raw_enum in &raw.r#enum {
        validate_identifier(&raw_enum.name, "enum", &raw_enum.name)?;
        let docs: Option<String> = normalize_docs(raw_enum.docs.as_ref(), file, source)?;
        let repr: ReprType = match ReprType::parse(&raw_enum.repr) {
            Some(r) => r,
            None => {
                let suggestion: Option<String> = nearest_repr_suggestion(&raw_enum.repr);
                return Err(PolyplugcError::EnumInvalidRepr {
                    enum_name: raw_enum.name.clone(),
                    repr: raw_enum.repr.clone(),
                    suggestion,
                });
            }
        };
        let mut declared: Vec<String> = Vec::new();
        let mut variants: Vec<EnumVariant> = Vec::new();
        for raw_variant in &raw_enum.variants {
            validate_identifier(&raw_variant.name, "enum variant", &raw_enum.name)?;
            validate_enum_value_expr(
                &raw_variant.value,
                &raw_enum.name,
                &raw_variant.name,
                &declared,
            )?;
            declared.push(raw_variant.name.clone());
            variants.push(EnumVariant {
                name: raw_variant.name.clone(),
                value: raw_variant.value.clone(),
                docs: normalize_docs(raw_variant.docs.as_ref(), file, source)?,
                langs: lower_langs(
                    raw_variant.langs.as_ref(),
                    CustomizableNode::EnumVariant,
                    file,
                    source,
                )?,
            });
        }
        check_enum_chained_refs(&raw_enum.name, &variants)?;
        resolved_enums.push(EnumDef {
            name: raw_enum.name.clone(),
            repr,
            bitflag: raw_enum.bitflag,
            variants,
            docs,
            langs: lower_langs(
                raw_enum.langs.as_ref(),
                CustomizableNode::Enum,
                file,
                source,
            )?,
        });
    }

    // Reject duplicate guest-contract names (mirrors the host-contract check below).
    let mut seen_guest_contracts: Vec<&str> = Vec::with_capacity(raw.guest_contract.len());
    for raw_guest_contract in &raw.guest_contract {
        if seen_guest_contracts.contains(&raw_guest_contract.name.as_str()) {
            return Err(PolyplugcError::DuplicateContractName {
                name: raw_guest_contract.name.clone(),
                first_defined_at: None,
            });
        }
        seen_guest_contracts.push(&raw_guest_contract.name);
    }

    let mut resolved_contracts: Vec<ResolvedContract> = Vec::new();
    for raw_guest_contract in &raw.guest_contract {
        validate_contract_members(
            &raw_guest_contract.name,
            "guest contract",
            &raw_guest_contract.functions,
        )?;
        let version: Version = parse_version_spanned(&raw_guest_contract.version, file, source)?;
        let contract_id: u64 = guest_contract_id(&raw_guest_contract.name, version.major);
        let docs: Option<String> = normalize_docs(raw_guest_contract.docs.as_ref(), file, source)?;

        let mut functions: Vec<ResolvedFunction> = Vec::new();
        for (function_id, raw_fn) in raw_guest_contract.functions.iter().enumerate() {
            let mut params: Vec<ResolvedParam> = Vec::new();
            for p in &raw_fn.params {
                let ty: ResolvedTypeRef = resolve_type_ref_spanned(
                    &p.ty,
                    &raw_guest_contract.name,
                    &all_known_names,
                    file,
                    source,
                )?;
                params.push(ResolvedParam {
                    name: p.name.clone(),
                    ty,
                    docs: normalize_docs(p.docs.as_ref(), file, source)?,
                    langs: lower_langs(p.langs.as_ref(), CustomizableNode::Param, file, source)?,
                });
            }
            let return_docs: Option<String> = normalize_unspanned_docs(
                raw_fn
                    .returns
                    .as_ref()
                    .and_then(|raw_return| RawReturn::docs(raw_return.get_ref())),
            )?;
            let returns: Option<ResolvedTypeRef> = raw_fn
                .returns
                .as_ref()
                .map(|raw_return| {
                    resolve_type_ref(
                        raw_return.get_ref().ty(),
                        &raw_guest_contract.name,
                        &all_known_names,
                    )
                })
                .transpose()?
                // An explicit `return = "void"` means "no return"; normalize it to
                // None so all generators treat it uniformly as a void function.
                .filter(|ty: &ResolvedTypeRef| {
                    !matches!(ty, ResolvedTypeRef::AbiType(AbiBuiltin::Void))
                });
            functions.push(ResolvedFunction {
                name: raw_fn.name.clone(),
                function_id: function_id as u32,
                params,
                returns,
                docs: normalize_docs(raw_fn.docs.as_ref(), file, source)?,
                return_docs,
                langs: lower_langs(
                    raw_fn.langs.as_ref(),
                    CustomizableNode::Function,
                    file,
                    source,
                )?,
                return_langs: lower_unspanned_langs(
                    raw_fn
                        .returns
                        .as_ref()
                        .and_then(|raw_return| RawReturn::langs(raw_return.get_ref())),
                    CustomizableNode::Return,
                    file,
                    source,
                    raw_fn
                        .returns
                        .as_ref()
                        .map(|raw_return| raw_return.span().start)
                        .unwrap_or_default(),
                )?,
            });
        }

        resolved_contracts.push(ResolvedContract {
            name: raw_guest_contract.name.clone(),
            contract_id,
            version,
            functions,
            docs,
            langs: lower_langs(
                raw_guest_contract.langs.as_ref(),
                CustomizableNode::GuestContract,
                file,
                source,
            )?,
        });
    }

    let mut resolved_host_contracts: Vec<ResolvedHostContract> = Vec::new();
    for raw_host_contract in &raw.host_contract {
        validate_contract_members(
            &raw_host_contract.name,
            "host contract",
            &raw_host_contract.functions,
        )?;
        let version: Version = parse_version_spanned(&raw_host_contract.version, file, source)?;
        let contract_id: u64 = host_contract_id(&raw_host_contract.name, version.major);
        let docs: Option<String> = normalize_docs(raw_host_contract.docs.as_ref(), file, source)?;

        let mut functions: Vec<ResolvedFunction> = Vec::new();
        for (function_id, raw_fn) in raw_host_contract.functions.iter().enumerate() {
            let mut params: Vec<ResolvedParam> = Vec::new();
            for p in &raw_fn.params {
                let ty: ResolvedTypeRef = resolve_type_ref_spanned(
                    &p.ty,
                    &raw_host_contract.name,
                    &all_known_names,
                    file,
                    source,
                )?;
                params.push(ResolvedParam {
                    name: p.name.clone(),
                    ty,
                    docs: normalize_docs(p.docs.as_ref(), file, source)?,
                    langs: lower_langs(p.langs.as_ref(), CustomizableNode::Param, file, source)?,
                });
            }
            let return_docs: Option<String> = normalize_unspanned_docs(
                raw_fn
                    .returns
                    .as_ref()
                    .and_then(|raw_return| RawReturn::docs(raw_return.get_ref())),
            )?;
            let returns: Option<ResolvedTypeRef> = raw_fn
                .returns
                .as_ref()
                .map(|raw_return| {
                    resolve_type_ref(
                        raw_return.get_ref().ty(),
                        &raw_host_contract.name,
                        &all_known_names,
                    )
                })
                .transpose()?
                // An explicit `return = "void"` means "no return"; normalize it to
                // None so all generators treat it uniformly as a void function.
                .filter(|ty: &ResolvedTypeRef| {
                    !matches!(ty, ResolvedTypeRef::AbiType(AbiBuiltin::Void))
                });
            functions.push(ResolvedFunction {
                name: raw_fn.name.clone(),
                function_id: function_id as u32,
                params,
                returns,
                docs: normalize_docs(raw_fn.docs.as_ref(), file, source)?,
                return_docs,
                langs: lower_langs(
                    raw_fn.langs.as_ref(),
                    CustomizableNode::Function,
                    file,
                    source,
                )?,
                return_langs: lower_unspanned_langs(
                    raw_fn
                        .returns
                        .as_ref()
                        .and_then(|raw_return| RawReturn::langs(raw_return.get_ref())),
                    CustomizableNode::Return,
                    file,
                    source,
                    raw_fn
                        .returns
                        .as_ref()
                        .map(|raw_return| raw_return.span().start)
                        .unwrap_or_default(),
                )?,
            });
        }

        resolved_host_contracts.push(ResolvedHostContract {
            name: raw_host_contract.name.clone(),
            contract_id,
            version,
            singleton: raw_host_contract.singleton,
            functions,
            docs,
            langs: lower_langs(
                raw_host_contract.langs.as_ref(),
                CustomizableNode::HostContract,
                file,
                source,
            )?,
        });
    }

    let guest_contract_names: Vec<&str> = raw
        .guest_contract
        .iter()
        .map(|contract| contract.name.as_str())
        .collect();
    for raw_host_contract in &raw.host_contract {
        if !raw_host_contract.name.starts_with("host.") {
            return Err(PolyplugcError::HostContractNameMissingPrefix {
                name: raw_host_contract.name.clone(),
            });
        }
        if guest_contract_names.contains(&raw_host_contract.name.as_str()) {
            return Err(PolyplugcError::DuplicateContractName {
                name: raw_host_contract.name.clone(),
                first_defined_at: None,
            });
        }
    }
    let mut seen_host_names: Vec<&str> = Vec::new();
    for raw_host_contract in &raw.host_contract {
        if seen_host_names.contains(&raw_host_contract.name.as_str()) {
            return Err(PolyplugcError::DuplicateContractName {
                name: raw_host_contract.name.clone(),
                first_defined_at: None,
            });
        }
        seen_host_names.push(&raw_host_contract.name);
    }

    // Emit the synthesized `{ items, len }` wrapper struct for each `Array<T>`
    // the desugar referenced (see `array_wrapper_name`). Appended after the
    // user types so generators see them as ordinary structs; the arena element
    // marshaling is emitted at the return boundary.
    let array_wrappers: Vec<ResolvedType> = collect_array_wrapper_types(
        &resolved_types,
        &resolved_contracts,
        &resolved_host_contracts,
    );
    resolved_types.extend(array_wrappers);
    // Order types so every struct follows the user-defined types it references by
    // field. The C-family generators (Lua `ffi.cdef`, C++ headers, Python `ctypes`
    // `_fields_`) require a type to be declared before it is used as a field, and
    // the desugar appends `ArrayOf_*` wrappers AFTER the user structs — so a struct
    // with an `Array<T>` field would reference an as-yet-undeclared wrapper.
    let resolved_types: Vec<ResolvedType> = topologically_order_types(resolved_types);
    validate_rust_semantic_rules(&resolved_types, &resolved_enums)?;

    Ok(ValidatedIr {
        types: resolved_types,
        enums: resolved_enums,
        contracts: resolved_contracts,
        host_contracts: resolved_host_contracts,
        bundle: None,
        langs,
    })
}

/// Reject bundles where two `[[plugin]]` entries share a name.
///
/// Multiple plugins MAY implement the same contract in one bundle — that is a valid
/// multi-provider bundle (e.g. several decoders behind one `pipeline.Decoder`
/// contract, each fronting a different backend). The generators handle that by
/// emitting the shared contract-id constant once and one plugin-named interface per
/// provider. Plugin names, though, key every per-provider symbol
/// (`{PLUGIN}_INTERFACE`, `{PLUGIN}_FNS`, `{PLUGIN}_create_instance`); two plugins
/// with the same name would collide on all of them, and are indistinguishable to a
/// reader anyway. Reject that here, before any code is generated.
fn validate_bundle_plugin_uniqueness(
    plugins: &[ResolvedPlugin],
    bundle_name: &str,
) -> Result<(), PolyplugcError> {
    let mut seen_plugin: HashSet<&str> = HashSet::new();
    for plugin in plugins {
        if !seen_plugin.insert(plugin.name.as_str()) {
            let symbol: String = plugin.name.to_uppercase().replace('.', "_");
            return Err(PolyplugcError::ValidationFailed {
                message: format!(
                    "bundle `{bundle_name}`: two [[plugin]] entries are both named `{}` — plugin names must be unique within a bundle (generated symbols like {symbol}_INTERFACE would collide)",
                    plugin.name,
                ),
            });
        }
    }
    Ok(())
}

fn lower_bundle(
    raw: RawBundleSchema,
    source: &str,
    file: &str,
    mode: BundleParseMode,
) -> Result<ValidatedIr, PolyplugcError> {
    let bundle_version: Version = parse_version_spanned(&raw.bundle.version, file, source)?;
    let mut plugins: Vec<ResolvedPlugin> = Vec::new();
    for raw_plugin in &raw.plugin {
        plugins.push(ResolvedPlugin {
            name: raw_plugin.name.clone(),
            implements: raw_plugin.implements.clone(),
            optional: raw_plugin.optional.clone(),
        });
    }
    validate_bundle_plugin_uniqueness(&plugins, &raw.bundle.name)?;
    let dep_bundle_id: u64 = bundle_id(&raw.bundle.name);
    let mut resolved_deps: Vec<ResolvedDependency> = Vec::new();
    for dep in &raw.dependencies {
        // The contract_id must use the same major version the API schema uses when it
        // resolves the same contract (guest_contract_id encodes major in the hash).
        // Use min_version.major so the dep's contract_id matches the resolved contract.
        let dep_major: u32 = Version::parse(&dep.min_version)
            .map(|v| v.major)
            .unwrap_or(0);
        let contract_id_val: u64 = guest_contract_id(&dep.contract, dep_major);
        let resolved: ResolvedDependency = if dep.kind == "bundle" {
            let bundle_name: String = dep.bundle.clone().unwrap_or_default();
            let bundle_id_val: u64 = bundle_id(&bundle_name);
            ResolvedDependency::ByBundle {
                bundle: bundle_name,
                bundle_id: bundle_id_val,
                contract: dep.contract.clone(),
                contract_id: contract_id_val,
                min_version: Version::parse(&dep.min_version)
                    .map(|v| v.major)
                    .unwrap_or(0),
            }
        } else {
            ResolvedDependency::ByContract {
                contract: dep.contract.clone(),
                contract_id: contract_id_val,
                min_version: Version::parse(&dep.min_version)
                    .map(|v| v.major)
                    .unwrap_or(0),
            }
        };
        resolved_deps.push(resolved);
    }
    let loader: String = raw.bundle.loader.to_lowercase();
    let resolved_file: ResolvedBundleFile = match mode {
        BundleParseMode::Internal => ResolvedBundleFile::Absent,
        BundleParseMode::External => {
            if loader.is_empty() {
                return Err(PolyplugcError::ValidationFailed {
                    message: "bundle.loader field is required".to_owned(),
                });
            }
            let is_native: bool = loader == "rust" || loader == "cpp" || loader == "native";
            match &raw.bundle.file {
                RawBundleFile::PlatformMap(os_map) if is_native => {
                    let mut map: HashMap<PlatformKey, String> = HashMap::new();
                    for (os, arch_map) in os_map {
                        for (arch, path) in arch_map {
                            map.insert(
                                PlatformKey {
                                    os: os.clone(),
                                    arch: arch.clone(),
                                },
                                path.clone(),
                            );
                        }
                    }
                    ResolvedBundleFile::PlatformMap(map)
                }
                RawBundleFile::Single(path) if !path.is_empty() && !is_native => {
                    ResolvedBundleFile::Single(path.clone())
                }
                RawBundleFile::PlatformMap(_) if !is_native => {
                    return Err(PolyplugcError::ValidationFailed {
                        message: format!(
                            "loader '{}' requires a flat file field (file = \"path\"), not [bundle.file] table",
                            loader
                        ),
                    });
                }
                RawBundleFile::Single(path) if is_native && !path.is_empty() => {
                    return Err(PolyplugcError::ValidationFailed {
                        message: format!(
                            "loader '{}' requires [bundle.file] table with platform entries, not a flat file field",
                            loader
                        ),
                    });
                }
                _ => {
                    return Err(PolyplugcError::ValidationFailed {
                        message: "bundle.file field is required".to_string(),
                    });
                }
            }
        }
    };
    let resolved_loader: String = match mode {
        BundleParseMode::External => raw.bundle.loader.clone(),
        BundleParseMode::Internal => String::new(),
    };
    // Suppress unused warning — `source` is accepted for API consistency with lower_api
    // but bundle lowering currently has no field-level type refs to resolve.
    let _: &str = source;
    Ok(ValidatedIr {
        types: Vec::new(),
        enums: Vec::new(),
        contracts: Vec::new(),
        host_contracts: Vec::new(),
        bundle: Some(ResolvedBundle {
            name: raw.bundle.name.clone(),
            version: bundle_version,
            loader: resolved_loader,
            file: resolved_file,
            bundle_id: dep_bundle_id,
            plugins,
            dependencies: resolved_deps,
            needs_reinit_on_dep_reload: raw.bundle.needs_reinit_on_dep_reload,
        }),
        langs: LanguageRules::default(),
    })
}

const _: () = {
    let _ = mem::size_of::<HashMap<String, String>>();
};

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;

    const SAMPLE_API: &str = "[[guest_contract]]\nname = \"image.decode\"\nversion = \"1.0.0\"\n\n[[guest_contract.functions]]\nname = \"decode\"\n\n[[guest_contract.functions]]\nname = \"supported_formats\"\n    return = \"StringView\"";

    const SAMPLE_BUNDLE: &str = "[bundle]\nname = \"image-plugin\"\nversion = \"1.0.0\"\nloader = \"python\"\nfile = \"test.py\"\n\n[[plugin]]\nname = \"jpeg_decoder\"\nimplements = [\"image.decode@1.0\"]";

    #[test]
    fn parse_canonical_guest_contract_preserves_contract_id() {
        let ir: ValidatedIr = parse_api_str(SAMPLE_API).expect("parse canonical guest contract");
        assert_eq!(ir.contracts.len(), 1);
        assert_eq!(ir.contracts[0].name, "image.decode");
        assert_eq!(ir.contracts[0].contract_id, 18_154_885_241_241_252_316);
        assert_eq!(ir.contracts[0].functions.len(), 2);
        assert_eq!(ir.contracts[0].functions[0].function_id, 0);
        assert_eq!(ir.contracts[0].functions[1].function_id, 1);
    }

    fn assert_rejected_legacy_or_unknown_table(toml: &str, invalid_table: &str) {
        let result: Result<ValidatedIr, PolyplugcError> = parse_api_str(toml);
        match result {
            Err(PolyplugcError::TomlParseError { message, .. }) => {
                assert!(
                    message.contains(invalid_table),
                    "unexpected message: {message}"
                );
                assert!(
                    message.contains("[[guest_contract]]"),
                    "diagnostic must name the canonical table: {message}"
                );
            }
            other => panic!("expected legacy or unknown table rejection, got {other:?}"),
        }
    }

    #[test]
    fn parse_legacy_plugin_contract_rejected() {
        assert_rejected_legacy_or_unknown_table(
            "[[plugin_contract]]\nname = \"test.add\"\nversion = \"1.0.0\"\n",
            "[[plugin_contract]]",
        );
    }

    #[test]
    fn parse_legacy_contract_rejected() {
        assert_rejected_legacy_or_unknown_table(
            "[[contract]]\nname = \"test.add\"\nversion = \"1.0.0\"\n",
            "[[contract]]",
        );
    }

    #[test]
    fn parse_unknown_top_level_table_rejected() {
        assert_rejected_legacy_or_unknown_table(
            "[[guest_contrat]]\nname = \"test.add\"\nversion = \"1.0.0\"\n",
            "guest_contrat",
        );
    }

    #[test]
    fn parse_minimal_bundle() {
        let ir: ValidatedIr = parse_bundle_str(SAMPLE_BUNDLE).expect("parse bundle");
        assert!(ir.bundle.is_some());
        let bundle: &ResolvedBundle = ir.bundle.as_ref().expect("bundle");
        assert_eq!(bundle.name, "image-plugin");
        assert_eq!(bundle.plugins.len(), 1);
        assert_eq!(bundle.plugins[0].implements[0], "image.decode@1.0");
    }

    #[test]
    fn bundle_accepts_two_plugins_implementing_same_contract() {
        // Multiple providers of one contract is a valid bundle (e.g. two decoders
        // behind pipeline.Decoder, each fronting a different backend). The api forbids
        // two contracts of the same name, so both providers share the identical
        // contract id — the generator emits that const once and one interface each.
        let toml: &str = "[bundle]\nname = \"multi-bundle\"\nversion = \"1.0.0\"\nloader = \"python\"\nfile = \"test.py\"\n\n[[plugin]]\nname = \"decoder_a\"\nimplements = [\"pipeline.Decoder@1.0\"]\n\n[[plugin]]\nname = \"decoder_b\"\nimplements = [\"pipeline.Decoder@1.0\"]";
        let ir: ValidatedIr =
            parse_bundle_str(toml).expect("multiple providers of one contract must be accepted");
        let bundle: &ResolvedBundle = ir.bundle.as_ref().expect("bundle");
        assert_eq!(bundle.plugins.len(), 2, "both providers retained");
        assert_eq!(bundle.plugins[0].implements, bundle.plugins[1].implements);
    }

    #[test]
    fn bundle_rejects_duplicate_plugin_names() {
        // Two plugins named `decoder` → DECODER_INTERFACE / DECODER_FNS would collide.
        let toml: &str = "[bundle]\nname = \"dup-bundle\"\nversion = \"1.0.0\"\nfile = \"test.so\"\n\n[[plugin]]\nname = \"decoder\"\nimplements = [\"pipeline.Decoder@1.0\"]\n\n[[plugin]]\nname = \"decoder\"\nimplements = [\"pipeline.Encoder@1.0\"]";
        let err: PolyplugcError =
            parse_bundle_str(toml).expect_err("duplicate plugin names must be rejected");
        let msg: String = format!("{err}");
        assert!(
            msg.contains("both named `decoder`") && msg.contains("DECODER_INTERFACE"),
            "message must name the plugin and the colliding symbol: {msg}"
        );
    }

    #[test]
    fn bundle_allows_distinct_plugins_and_contracts() {
        // Two plugins, two distinct contracts, distinct names → no collision.
        let toml: &str = "[bundle]\nname = \"ok-bundle\"\nversion = \"1.0.0\"\nloader = \"python\"\nfile = \"test.py\"\n\n[[plugin]]\nname = \"decoder\"\nimplements = [\"pipeline.Decoder@1.0\"]\n\n[[plugin]]\nname = \"encoder\"\nimplements = [\"pipeline.Encoder@1.0\"]";
        let ir: ValidatedIr = parse_bundle_str(toml).expect("distinct plugins must be accepted");
        assert_eq!(ir.bundle.as_ref().expect("bundle").plugins.len(), 2);
    }

    #[test]
    fn bundle_name_conflicts_with_contract_name() {
        let contracts: Vec<ResolvedContract> = vec![ResolvedContract {
            name: "test.add".to_owned(),
            contract_id: 0,
            version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
            functions: Vec::new(),
            docs: None,
            langs: LanguageRules::default(),
        }];
        let result: Result<(), PolyplugcError> = check_bundle_name_conflict("test.add", &contracts);
        assert!(
            matches!(result, Err(PolyplugcError::BundleNameConflict { .. })),
            "expected BundleNameConflict, got {result:?}",
        );
    }

    #[test]
    fn bundle_name_no_conflict_with_contract_names() {
        let contracts: Vec<ResolvedContract> = vec![ResolvedContract {
            name: "test.add".to_owned(),
            contract_id: 0,
            version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
            functions: Vec::new(),
            docs: None,
            langs: LanguageRules::default(),
        }];
        let result: Result<(), PolyplugcError> =
            check_bundle_name_conflict("image_bundle", &contracts);
        assert!(result.is_ok(), "expected Ok, got {result:?}");
    }

    #[test]
    fn parse_raw_enum_deserializes() {
        let toml_str: &str = "[[enum]]\nname = \"Status\"\nrepr = \"u32\"\n\n[[enum.variants]]\nname = \"Ok\"\nvalue = \"0\"";
        let raw: RawApiSchema = toml::from_str(toml_str).expect("deserialize");
        assert_eq!(raw.r#enum.len(), 1);
        assert_eq!(raw.r#enum[0].name, "Status");
        assert_eq!(raw.r#enum[0].repr, "u32");
        assert_eq!(raw.r#enum[0].variants[0].name, "Ok");
        assert_eq!(raw.r#enum[0].variants[0].value, "0");
    }

    #[test]
    fn test_enum_forward_ref_rejected() {
        let declared: Vec<String> = vec!["A".to_owned()];
        let result: Result<(), PolyplugcError> =
            validate_enum_value_expr("C | 1", "MyEnum", "B", &declared);
        assert!(
            matches!(result, Err(PolyplugcError::EnumForwardRef { ref ref_name, .. }) if ref_name == "C"),
            "expected EnumForwardRef for C, got {result:?}",
        );
    }

    #[test]
    fn test_enum_chained_ref_rejected() {
        let variants: Vec<EnumVariant> = vec![
            EnumVariant {
                name: "A".to_owned(),
                value: "1".to_owned(),
                docs: None,
                langs: LanguageRules::default(),
            },
            EnumVariant {
                name: "B".to_owned(),
                value: "A | 1".to_owned(),
                docs: None,
                langs: LanguageRules::default(),
            },
            EnumVariant {
                name: "C".to_owned(),
                value: "B | 2".to_owned(),
                docs: None,
                langs: LanguageRules::default(),
            },
        ];
        let result: Result<(), PolyplugcError> = check_enum_chained_refs("MyEnum", &variants);
        assert!(
            matches!(result, Err(PolyplugcError::EnumChainedRef { ref variant_name, ref ref_name, .. }) if variant_name == "C" && ref_name == "B"),
            "expected EnumChainedRef for C->B, got {result:?}",
        );
    }

    #[test]
    fn test_enum_name_collision_with_type() {
        let type_names: Vec<String> = vec!["Status".to_owned()];
        let enum_names: Vec<String> = vec!["Status".to_owned()];
        let collision: bool = enum_names.iter().any(|n| type_names.contains(n));
        assert!(collision, "expected name collision detected");
    }

    #[test]
    fn test_enum_invalid_repr_rejected() {
        let result: Option<ReprType> = ReprType::parse("i32");
        assert!(result.is_none(), "i32 should not be a valid ReprType");
    }

    #[test]
    fn test_enum_valid_bitflag_expr() {
        let declared_a: Vec<String> = vec![];
        let r_a: Result<(), PolyplugcError> =
            validate_enum_value_expr("0", "Flags", "A", &declared_a);
        assert!(r_a.is_ok(), "A=0 should be valid, got {r_a:?}");

        let declared_b: Vec<String> = vec!["A".to_owned()];
        let r_b: Result<(), PolyplugcError> =
            validate_enum_value_expr("1", "Flags", "B", &declared_b);
        assert!(r_b.is_ok(), "B=1 should be valid, got {r_b:?}");

        let declared_c: Vec<String> = vec!["A".to_owned(), "B".to_owned()];
        let r_c: Result<(), PolyplugcError> =
            validate_enum_value_expr("1 << 1", "Flags", "C", &declared_c);
        assert!(r_c.is_ok(), "C=1<<1 should be valid, got {r_c:?}");

        let declared_d: Vec<String> = vec!["A".to_owned(), "B".to_owned(), "C".to_owned()];
        let r_d: Result<(), PolyplugcError> =
            validate_enum_value_expr("B | C", "Flags", "D", &declared_d);
        assert!(r_d.is_ok(), "D=B|C should be valid, got {r_d:?}");
    }

    #[test]
    fn test_parse_api_with_enums() {
        let toml_str: &str = "[[enum]]\nname = \"Status\"\nrepr = \"u32\"\n\n[[enum.variants]]\nname = \"Ok\"\nvalue = \"0\"\n\n[[enum.variants]]\nname = \"Err\"\nvalue = \"1\"";
        let ir: ValidatedIr = parse_api_str(toml_str).expect("parse");
        assert_eq!(ir.enums.len(), 1);
        assert_eq!(ir.enums[0].name, "Status");
        assert_eq!(ir.enums[0].variants.len(), 2);
        assert_eq!(ir.enums[0].variants[0].name, "Ok");
        assert_eq!(ir.enums[0].variants[1].value, "1");
    }

    #[test]
    fn parse_bundle_with_dependency() {
        let toml: &str = concat!(
            "[bundle]\nname = \"audio-engine\"\nversion = \"1.0.0\"\nloader = \"python\"\nfile = \"test.py\"\n\n",
            "[[plugin]]\nname = \"decoder\"\nversion = \"1.0.0\"\nimplements = [\"audio.decode@1.0\"]\n\n",
            "[[dependency]]\nkind = \"contract\"\ncontract = \"audio-decoder\"\nmin_version = \"1.0\"\n"
        );
        let ir: ValidatedIr = parse_bundle_str(toml).expect("parse bundle with dep");
        let bundle: &ResolvedBundle = ir.bundle.as_ref().expect("bundle");
        assert_eq!(bundle.name, "audio-engine");
    }

    #[test]
    fn parse_host_contract_valid() {
        let toml: &str = "[[host_contract]]\nname = \"host.logger\"\nversion = \"1.0.0\"\n\n[[host_contract.functions]]\nname = \"log\"\n[[host_contract.functions.params]]\nname = \"message\"\ntype = \"StringView\"";
        let ir: ValidatedIr = parse_api_str(toml).expect("parse host contract");
        assert_eq!(ir.contracts.len(), 0);
    }

    #[test]
    fn parse_host_contract_missing_prefix_rejected() {
        let toml: &str = "[[host_contract]]\nname = \"logger\"\nversion = \"1.0.0\"\n";
        let result: Result<ValidatedIr, PolyplugcError> = parse_api_str(toml);
        assert!(
            matches!(result, Err(PolyplugcError::HostContractNameMissingPrefix { ref name }) if name == "logger"),
            "expected HostContractNameMissingPrefix for 'logger', got {result:?}",
        );
    }

    #[test]
    fn parse_host_contract_duplicate_with_guest_contract_rejected() {
        let toml: &str = concat!(
            "[[guest_contract]]\nname = \"host.logger\"\nversion = \"1.0.0\"\n\n",
            "[[host_contract]]\nname = \"host.logger\"\nversion = \"1.0.0\"\n"
        );
        let result: Result<ValidatedIr, PolyplugcError> = parse_api_str(toml);
        assert!(
            matches!(result, Err(PolyplugcError::DuplicateContractName { ref name, .. }) if name == "host.logger"),
            "expected DuplicateContractName for 'host.logger', got {result:?}",
        );
    }

    #[test]
    fn parse_host_contract_duplicate_within_host_contracts_rejected() {
        let toml: &str = concat!(
            "[[host_contract]]\nname = \"host.logger\"\nversion = \"1.0.0\"\n\n",
            "[[host_contract]]\nname = \"host.logger\"\nversion = \"2.0.0\"\n"
        );
        let result: Result<ValidatedIr, PolyplugcError> = parse_api_str(toml);
        assert!(
            matches!(result, Err(PolyplugcError::DuplicateContractName { ref name, .. }) if name == "host.logger"),
            "expected DuplicateContractName for 'host.logger', got {result:?}",
        );
    }

    #[test]
    fn parse_both_contract_types_valid() {
        let toml: &str = concat!(
            "[[guest_contract]]\nname = \"image.decode\"\nversion = \"1.0.0\"\n\n",
            "[[host_contract]]\nname = \"host.logger\"\nversion = \"1.0.0\"\n"
        );
        let ir: ValidatedIr = parse_api_str(toml).expect("parse both contract types");
        assert_eq!(ir.contracts.len(), 1);
        assert_eq!(ir.contracts[0].name, "image.decode");
    }

    #[test]
    fn parse_host_contract_invalid_version_rejected() {
        let toml: &str = "[[host_contract]]\nname = \"host.logger\"\nversion = \"invalid\"\n";
        let result: Result<ValidatedIr, PolyplugcError> = parse_api_str(toml);
        assert!(
            matches!(result, Err(PolyplugcError::ValidationFailed { .. })),
            "expected ValidationFailed for invalid version format, got {result:?}",
        );
    }

    #[test]
    fn parse_host_contract_version_overflow_rejected() {
        let toml: &str = "[[host_contract]]\nname = \"host.logger\"\nversion = \"4294967296.0\"\n";
        let result: Result<ValidatedIr, PolyplugcError> = parse_api_str(toml);
        assert!(
            matches!(result, Err(PolyplugcError::ValidationFailed { .. })),
            "expected ValidationFailed for version overflow, got {result:?}",
        );
    }

    // ─── Reserved-word rejection ────────────────────────────────────────────

    #[test]
    fn parse_function_named_reserved_keyword_rejected() {
        // `class` is a keyword in Python and C++ — generated code would not compile.
        let toml: &str = concat!(
            "[[guest_contract]]\nname = \"image.decode\"\nversion = \"1.0.0\"\n\n",
            "[[guest_contract.functions]]\nname = \"class\"\n"
        );
        let result: Result<ValidatedIr, PolyplugcError> = parse_api_str(toml);
        match result {
            Err(PolyplugcError::ReservedIdentifier {
                ref kind,
                ref name,
                ref languages,
                ..
            }) => {
                assert_eq!(name, "class");
                assert_eq!(kind, "function");
                assert!(
                    languages.contains("Python") || languages.contains("C++"),
                    "languages should mention Python/C++, got: {languages}"
                );
            }
            other => panic!("expected ReservedIdentifier for `class`, got {other:?}"),
        }
    }

    #[test]
    fn parse_field_named_reserved_keyword_rejected() {
        // `end` is a Lua keyword.
        let toml: &str = concat!(
            "[[types]]\nname = \"Frame\"\n",
            "[[types.fields]]\nname = \"end\"\ntype = \"u32\"\n"
        );
        let result: Result<ValidatedIr, PolyplugcError> = parse_api_str(toml);
        match result {
            Err(PolyplugcError::ReservedIdentifier {
                ref kind,
                ref name,
                ref languages,
                ..
            }) => {
                assert_eq!(name, "end");
                assert_eq!(kind, "field");
                assert!(
                    languages.contains("Lua"),
                    "languages should mention Lua, got: {languages}"
                );
            }
            other => panic!("expected ReservedIdentifier for `end`, got {other:?}"),
        }
    }

    #[test]
    fn parse_enum_variant_named_reserved_keyword_rejected() {
        // `def` is a Python keyword.
        let toml: &str = concat!(
            "[[enum]]\nname = \"Kind\"\nrepr = \"u32\"\n\n",
            "[[enum.variants]]\nname = \"def\"\nvalue = \"0\"\n"
        );
        let result: Result<ValidatedIr, PolyplugcError> = parse_api_str(toml);
        match result {
            Err(PolyplugcError::ReservedIdentifier {
                ref kind,
                ref name,
                ref languages,
                ..
            }) => {
                assert_eq!(name, "def");
                assert_eq!(kind, "enum variant");
                assert!(
                    languages.contains("Python"),
                    "languages should mention Python, got: {languages}"
                );
            }
            other => panic!("expected ReservedIdentifier for `def`, got {other:?}"),
        }
    }

    #[test]
    fn parse_contract_segment_named_reserved_keyword_rejected() {
        // A dotted contract segment that is reserved (`int` is a C++ keyword).
        let toml: &str = "[[guest_contract]]\nname = \"image.int\"\nversion = \"1.0.0\"\n";
        let result: Result<ValidatedIr, PolyplugcError> = parse_api_str(toml);
        assert!(
            matches!(result, Err(PolyplugcError::ReservedIdentifier { ref name, .. }) if name == "int"),
            "expected ReservedIdentifier for `int`, got {result:?}",
        );
    }

    #[test]
    fn parse_polyplug_prefixed_function_rejected() {
        let toml: &str = concat!(
            "[[guest_contract]]\nname = \"image.decode\"\nversion = \"1.0.0\"\n\n",
            "[[guest_contract.functions]]\nname = \"polyplug_init\"\n"
        );
        let result: Result<ValidatedIr, PolyplugcError> = parse_api_str(toml);
        assert!(
            matches!(result, Err(PolyplugcError::ReservedIdentifier { ref name, ref languages, .. }) if name == "polyplug_init" && languages.contains("polyplug")),
            "expected ReservedIdentifier for `polyplug_init`, got {result:?}",
        );
    }

    #[test]
    fn parse_normal_names_still_pass() {
        // Control: ordinary names must continue to parse cleanly.
        let toml: &str = concat!(
            "[[types]]\nname = \"Frame\"\n",
            "[[types.fields]]\nname = \"width\"\ntype = \"u32\"\n\n",
            "[[enum]]\nname = \"LogLevel\"\nrepr = \"u32\"\n\n",
            "[[enum.variants]]\nname = \"Debug\"\nvalue = \"0\"\n\n",
            "[[guest_contract]]\nname = \"pipeline.Decoder\"\nversion = \"1.0.0\"\n\n",
            "[[guest_contract.functions]]\nname = \"decode\"\n"
        );
        let ir: ValidatedIr = parse_api_str(toml).expect("normal names must parse");
        assert_eq!(ir.contracts.len(), 1);
    }

    // ─── Diagnostic helpers unit tests ──────────────────────────────────────

    #[test]
    fn byte_offset_to_line_col_first_line() {
        // "hello\nworld"
        // offset 0 → line 1, col 1
        let (line, col): (usize, usize) = byte_offset_to_line_col("hello\nworld", 0);
        assert_eq!(line, 1, "line");
        assert_eq!(col, 1, "col");
    }

    #[test]
    fn byte_offset_to_line_col_second_line() {
        // "hello\nworld"
        // offset 6 (start of "world") → line 2, col 1
        let (line, col): (usize, usize) = byte_offset_to_line_col("hello\nworld", 6);
        assert_eq!(line, 2, "line");
        assert_eq!(col, 1, "col");
    }

    #[test]
    fn byte_offset_to_line_col_mid_line() {
        // "ab\ncd\nef"  →  bytes: a0 b1 \n2 c3 d4 \n5 e6 f7
        // offset 6 (start of "ef") → line 3, col 1
        let (line, col): (usize, usize) = byte_offset_to_line_col("ab\ncd\nef", 6);
        assert_eq!(line, 3, "line");
        assert_eq!(col, 1, "col");
    }

    #[test]
    fn byte_offset_to_line_col_within_line() {
        // "abc\ndef"
        // offset 5 ('e') → line 2, col 2
        let (line, col): (usize, usize) = byte_offset_to_line_col("abc\ndef", 5);
        assert_eq!(line, 2, "line");
        assert_eq!(col, 2, "col");
    }

    #[test]
    fn edit_distance_identical() {
        assert_eq!(edit_distance("u32", "u32"), 0);
    }

    #[test]
    fn edit_distance_one_substitution() {
        assert_eq!(edit_distance("u33", "u32"), 1);
    }

    #[test]
    fn edit_distance_insertion() {
        assert_eq!(edit_distance("Striing", "String"), 1);
    }

    #[test]
    fn nearest_type_suggestion_close_match() {
        let candidates: &[&str] = &["u8", "u16", "u32", "u64", "StringView"];
        // "u33" is 1 edit from "u32" and "u64", but "u32" comes first.
        let suggestion: Option<String> = nearest_type_suggestion("u33", candidates);
        assert!(suggestion.is_some(), "expected a suggestion");
        let s: String = suggestion.expect("some");
        assert!(s == "u32" || s == "u64", "expected u32 or u64, got {s}");
    }

    #[test]
    fn nearest_type_suggestion_no_close_match() {
        let candidates: &[&str] = &["u8", "u16", "u32"];
        let suggestion: Option<String> = nearest_type_suggestion("CompletelyDifferent", candidates);
        assert!(suggestion.is_none(), "expected no suggestion");
    }

    #[test]
    fn nearest_repr_suggestion_finds_u32() {
        let suggestion: Option<String> = nearest_repr_suggestion("u33");
        assert_eq!(suggestion.as_deref(), Some("u32"));
    }

    #[test]
    fn nearest_repr_suggestion_no_suggestion_for_i32() {
        // "i32" has edit distance 1 from "u32", which IS within our threshold.
        // This is intentional — "i32" could be a signed-integer mistake.
        let suggestion: Option<String> = nearest_repr_suggestion("i32");
        assert!(suggestion.is_some(), "expected suggestion for i32");
    }

    #[test]
    fn array_return_desugars_to_wrapper_struct() {
        // `Array<Foo>` desugars to a synthesized `ArrayOf_Foo { items, len }` struct
        // and the return resolves to that wrapper. The element marshaling is emitted
        // by the generators at the return boundary.
        let api: &str = "[[types]]\nname = \"Foo\"\nfields = [{ name = \"a\", type = \"u32\" }, { name = \"s\", type = \"StringView\" }]\n\n[[guest_contract]]\nname = \"x.C\"\nversion = \"1.0.0\"\n\n[[guest_contract.functions]]\nname = \"list\"\nreturn = \"Array<Foo>\"";
        let ir: ValidatedIr = parse_api_str(api).expect("parse Array<Foo>");
        let wrapper: &ResolvedType = ir
            .types
            .iter()
            .find(|t: &&ResolvedType| t.name == "ArrayOf_Foo")
            .expect("ArrayOf_Foo wrapper synthesized");
        assert_eq!(wrapper.fields.len(), 2, "wrapper has items + len");
        assert_eq!(wrapper.fields[0].name, "items");
        assert_eq!(wrapper.fields[1].name, "len");
        let returns: &Option<ResolvedTypeRef> = &ir.contracts[0].functions[0].returns;
        assert!(
            matches!(returns, Some(ResolvedTypeRef::UserDefined(n)) if n == "ArrayOf_Foo"),
            "return resolves to the wrapper: {returns:?}"
        );
    }

    #[test]
    fn array_wrapper_emitted_once_across_multiple_uses() {
        let api: &str = "[[types]]\nname = \"Foo\"\nfields = [{ name = \"a\", type = \"u32\" }]\n\n[[guest_contract]]\nname = \"x.C\"\nversion = \"1.0.0\"\n\n[[guest_contract.functions]]\nname = \"a\"\nreturn = \"Array<Foo>\"\n\n[[guest_contract.functions]]\nname = \"b\"\nreturn = \"Array<Foo>\"";
        let ir: ValidatedIr = parse_api_str(api).expect("parse");
        let count: usize = ir
            .types
            .iter()
            .filter(|t: &&ResolvedType| t.name == "ArrayOf_Foo")
            .count();
        assert_eq!(count, 1, "wrapper struct emitted exactly once");
    }

    #[test]
    fn array_of_unknown_element_is_rejected() {
        let api: &str = "[[guest_contract]]\nname = \"x.C\"\nversion = \"1.0.0\"\n\n[[guest_contract.functions]]\nname = \"list\"\nreturn = \"Array<Nope>\"";
        assert!(
            parse_api_str(api).is_err(),
            "Array<Unknown> must be rejected"
        );
    }

    #[test]
    fn nested_array_is_rejected() {
        let api: &str = "[[types]]\nname = \"Foo\"\nfields = [{ name = \"a\", type = \"u32\" }]\n\n[[guest_contract]]\nname = \"x.C\"\nversion = \"1.0.0\"\n\n[[guest_contract.functions]]\nname = \"list\"\nreturn = \"Array<Array<Foo>>\"";
        assert!(
            parse_api_str(api).is_err(),
            "nested arrays must be rejected"
        );
    }
    #[test]
    fn lowers_language_rules_for_every_authored_node() {
        let api: &str = r#"
[langs.rust]
attributes = ["derive(Clone)"]
[langs.cpp]
attributes = ["nodiscard"]
[langs.csharp]
attributes = ["Serializable"]
[langs.python]
attributes = ["dataclass"]
[langs.lua]
attributes = ["metatable"]
[langs.javascript]
attributes = ["sealed"]

[[types]]
name = "Packet"
[types.langs.cpp]
attributes = ["alignas(8)"]
[[types.fields]]
name = "code"
type = "u32"
[types.fields.langs.csharp]
attributes = ["JsonPropertyName(\"code\")"]

[[enum]]
name = "Mode"
repr = "u32"
[enum.langs.python]
attributes = ["enum.unique"]
[[enum.variants]]
name = "Fast"
value = "1"
[enum.variants.langs.lua]
attributes = ["fast"]

[[guest_contract]]
name = "pipeline.Decoder"
version = "1.0.0"
[guest_contract.langs.javascript]
attributes = ["public"]
[[guest_contract.functions]]
name = "decode"
[guest_contract.functions.langs.rust]
attributes = ["inline"]
[guest_contract.functions.return]
type = "u32"
[guest_contract.functions.return.langs.csharp]
attributes = ["return:MarshalAs(UnmanagedType.U4)"]
[[guest_contract.functions.params]]
name = "input"
type = "StringView"
[guest_contract.functions.params.langs.cpp]
attributes = ["const"]

[[host_contract]]
name = "host.logger"
version = "1.0.0"
[host_contract.langs.python]
attributes = ["protocol"]
[[host_contract.functions]]
name = "level"
[host_contract.functions.langs.lua]
attributes = ["method"]
[host_contract.functions.return]
type = "u32"
[host_contract.functions.return.langs.rust]
attributes = ["must_use"]
[[host_contract.functions.params]]
name = "message"
type = "StringView"
[host_contract.functions.params.langs.javascript]
attributes = ["readonly"]
"#;

        let ir: ValidatedIr = parse_api_str(api).expect("all language rules parse");
        assert_eq!(
            ir.langs
                .for_lang(Lang::Rust)
                .expect("root rust rules")
                .attributes,
            ["derive(Clone)"]
        );
        assert_eq!(
            ir.langs
                .for_lang(Lang::JsQuickJs)
                .expect("root javascript rules")
                .attributes,
            ["sealed"]
        );
        assert_eq!(
            ir.types[0]
                .langs
                .for_lang(Lang::Cpp)
                .expect("type cpp rules")
                .attributes,
            ["alignas(8)"]
        );
        assert_eq!(
            ir.types[0].fields[0]
                .langs
                .for_lang(Lang::CSharp)
                .expect("field csharp rules")
                .attributes,
            ["JsonPropertyName(\"code\")"]
        );
        assert_eq!(
            ir.enums[0]
                .langs
                .for_lang(Lang::Python)
                .expect("enum python rules")
                .attributes,
            ["enum.unique"]
        );
        assert_eq!(
            ir.enums[0].variants[0]
                .langs
                .for_lang(Lang::Lua)
                .expect("variant lua rules")
                .attributes,
            ["fast"]
        );

        let guest: &ResolvedContract = &ir.contracts[0];
        assert_eq!(
            guest
                .langs
                .for_lang(Lang::JsQuickJs)
                .expect("guest contract javascript rules")
                .attributes,
            ["public"]
        );
        assert_eq!(
            guest.functions[0]
                .langs
                .for_lang(Lang::Rust)
                .expect("guest function rust rules")
                .attributes,
            ["inline"]
        );
        assert_eq!(
            guest.functions[0].params[0]
                .langs
                .for_lang(Lang::Cpp)
                .expect("guest parameter cpp rules")
                .attributes,
            ["const"]
        );
        assert_eq!(
            guest.functions[0]
                .return_langs
                .for_lang(Lang::CSharp)
                .expect("guest return csharp rules")
                .attributes,
            ["return:MarshalAs(UnmanagedType.U4)"]
        );

        let host: &ResolvedHostContract = &ir.host_contracts[0];
        assert_eq!(
            host.langs
                .for_lang(Lang::Python)
                .expect("host contract python rules")
                .attributes,
            ["protocol"]
        );
        assert_eq!(
            host.functions[0]
                .langs
                .for_lang(Lang::Lua)
                .expect("host function lua rules")
                .attributes,
            ["method"]
        );
        assert_eq!(
            host.functions[0].params[0]
                .langs
                .for_lang(Lang::JsQuickJs)
                .expect("host parameter javascript rules")
                .attributes,
            ["readonly"]
        );
        assert_eq!(
            host.functions[0]
                .return_langs
                .for_lang(Lang::Rust)
                .expect("host return rust rules")
                .attributes,
            ["must_use"]
        );
    }

    #[test]
    fn language_rules_allow_optional_subsets_and_unchanged_apis() {
        let with_subset: &str = r#"
[[types]]
name = "Packet"
[types.langs.rust]
attributes = ["repr(C)"]
"#;
        let subset_ir: ValidatedIr = parse_api_str(with_subset).expect("optional subset parses");
        assert!(subset_ir.langs.for_lang(Lang::Rust).is_none());
        assert!(subset_ir.types[0].langs.for_lang(Lang::Cpp).is_none());
        assert_eq!(
            subset_ir.types[0]
                .langs
                .for_lang(Lang::Rust)
                .expect("rust subset")
                .attributes,
            ["repr(C)"]
        );

        let unchanged_ir: ValidatedIr =
            parse_api_str(SAMPLE_API).expect("existing APIs without langs still parse");
        assert!(unchanged_ir.langs.for_lang(Lang::Rust).is_none());
        assert!(
            unchanged_ir.contracts[0]
                .langs
                .for_lang(Lang::Cpp)
                .is_none()
        );
        assert!(
            unchanged_ir.contracts[0].functions[0]
                .return_langs
                .for_lang(Lang::Lua)
                .is_none()
        );
    }

    #[test]
    fn language_rules_reject_unknown_keys_with_source_location() {
        let api: &str = "[langs.go]\nattributes = [\"tag\"]\n";
        let err: PolyplugcError = parse_api_str(api).expect_err("unknown language must fail");
        match err {
            PolyplugcError::TomlParseError {
                message,
                location: Some(location),
            } => {
                assert!(message.contains("go"), "unexpected diagnostic: {message}");
                assert_eq!(location.line, 1);
            }
            other => panic!("expected located unknown-language rejection, got {other:?}"),
        }
    }

    #[test]
    fn language_rules_reject_invalid_placement_with_source_location() {
        let api: &str = r#"
[[guest_contract]]
name = "pipeline.Decoder"
version = "1.0.0"

[[guest_contract.functions]]
name = "decode"
[guest_contract.functions.params.langs.rust]
attributes = ["inline"]
"#;
        let err: PolyplugcError = parse_api_str(api).expect_err("misplaced langs must fail");
        match err {
            PolyplugcError::TomlParseError {
                location: Some(location),
                ..
            } => assert_eq!(location.line, 8),
            other => panic!("expected located invalid-placement rejection, got {other:?}"),
        }
    }

    #[test]
    fn language_rules_reject_empty_and_multiline_attributes_with_locations() {
        for (attribute, reason) in [("\"   \"", "empty"), ("\"first\\nsecond\"", "single line")] {
            let api: String = format!("[langs.rust]\nattributes = [{attribute}]\n");
            let err: PolyplugcError =
                parse_api_str(&api).expect_err("invalid attribute contents must fail");
            match err {
                PolyplugcError::InvalidLanguageAttribute {
                    language,
                    node,
                    reason: actual_reason,
                    location,
                    ..
                } => {
                    assert_eq!(language, "rust");
                    assert_eq!(node, "API root");
                    assert!(
                        actual_reason.contains(reason),
                        "unexpected reason: {actual_reason}"
                    );
                    assert_eq!(location.line, 2);
                }
                other => panic!("expected invalid language attribute, got {other:?}"),
            }
        }
    }

    #[test]
    fn expanded_return_attribute_uses_the_return_table_source_line() {
        let api = "[[guest_contract]]\nname = \"pipeline.Decoder\"\nversion = \"1.0.0\"\n[[guest_contract.functions]]\nname = \"decode\"\nreturn = { type = \"u8\", langs = { rust = { attributes = [\"first\\nsecond\"] } } }\n";
        let err = parse_api_str(api).expect_err("multiline return attribute must fail");
        match err {
            PolyplugcError::InvalidLanguageAttribute {
                node,
                reason,
                location,
                ..
            } => {
                assert_eq!(node, "return");
                assert!(
                    reason.contains("single line"),
                    "unexpected reason: {reason}"
                );
                assert_eq!(location.line, 6);
            }
            other => panic!("expected located return attribute rejection, got {other:?}"),
        }
    }
}

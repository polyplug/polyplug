//! Parser — TOML schema parsing for polyplugc.
//!
//! Parses `api.toml` and `bundle.toml` using serde+toml.
//! Produces raw AST structs that are later lowered to `ValidatedIr`.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

use crate::ir::EnumDef;
use crate::ir::EnumVariant;
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
use crate::ir::ValidatedIr;
use crate::ir::Version;
use crate::ir::compute_bundle_id;
use crate::ir::compute_contract_id;
use crate::ir::compute_host_contract_id;
use crate::ir::resolve_type_ref;
use polyplug_codegen::PlatformKey;
use polyplug_codegen::PolyplugcError;
use polyplug_codegen::ResolvedBundleFile;
use polyplug_codegen::error::SourceLocation;

#[derive(Debug, Deserialize)]
pub(crate) struct RawApiSchema {
    #[serde(default)]
    pub types: Vec<RawType>,
    #[serde(rename = "enum", default)]
    pub r#enum: Vec<RawEnum>,
    #[serde(default, alias = "contract")]
    pub plugin_contract: Vec<RawContract>,
    #[serde(default)]
    pub host_contract: Vec<RawHostContract>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawType {
    pub name: String,
    #[serde(default)]
    pub fields: Vec<RawField>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawField {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: toml::Spanned<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawContract {
    pub name: String,
    pub version: toml::Spanned<String>,
    #[serde(default)]
    pub functions: Vec<RawFunction>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawHostContract {
    pub name: String,
    pub version: toml::Spanned<String>,
    #[serde(default)]
    pub singleton: bool,
    #[serde(default)]
    pub functions: Vec<RawFunction>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawFunction {
    pub name: String,
    #[serde(default)]
    pub params: Vec<RawParam>,
    #[serde(rename = "return", default)]
    pub returns: Option<toml::Spanned<String>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawParam {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: toml::Spanned<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawEnumVariant {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawEnum {
    pub name: String,
    pub repr: String,
    #[serde(default)]
    pub bitflag: bool,
    #[serde(default)]
    pub variants: Vec<RawEnumVariant>,
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
    pub runtime: String,
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
    pub version: toml::Spanned<String>,
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

/// Parse a version from a spanned TOML field, enriching a `VersionOverflow`
/// error with the source location of the offending `version = "..."` field.
fn parse_version_spanned(
    spanned: &toml::Spanned<String>,
    file: &str,
    source: &str,
) -> core::result::Result<Version, PolyplugcError> {
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
) -> core::result::Result<ResolvedTypeRef, PolyplugcError> {
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

// ─── Parse entry points ───────────────────────────────────────────────────────

pub fn parse_api(path: &Path) -> core::result::Result<ValidatedIr, PolyplugcError> {
    let content: String =
        std::fs::read_to_string(path).map_err(|e| PolyplugcError::ReadFailed {
            path: path.to_string_lossy().into_owned(),
            source: e,
        })?;
    parse_api_str_with_file(&content, &path.to_string_lossy())
}

pub fn parse_api_str(content: &str) -> core::result::Result<ValidatedIr, PolyplugcError> {
    parse_api_str_with_file(content, "<input>")
}

fn parse_api_str_with_file(
    content: &str,
    file: &str,
) -> core::result::Result<ValidatedIr, PolyplugcError> {
    if content.contains("[[contract]]") && !content.contains("[[plugin_contract]]") {
        eprintln!("warning: [[contract]] is deprecated, use [[plugin_contract]] instead");
    }
    let raw: RawApiSchema = toml::from_str(content).map_err(|e| {
        let location: Option<SourceLocation> = e
            .span()
            .map(|span| location_from_span(file, content, span.start));
        PolyplugcError::TomlParseError {
            message: e.message().to_owned(),
            location,
        }
    })?;
    lower_api(raw, content, file)
}

#[allow(dead_code)]
pub fn parse_bundle_str(content: &str) -> core::result::Result<ValidatedIr, PolyplugcError> {
    parse_bundle_str_with_file(content, "<input>")
}

fn parse_bundle_str_with_file(
    content: &str,
    file: &str,
) -> core::result::Result<ValidatedIr, PolyplugcError> {
    let raw: RawBundleSchema = toml::from_str(content).map_err(|e| {
        let location: Option<SourceLocation> = e
            .span()
            .map(|span| location_from_span(file, content, span.start));
        PolyplugcError::TomlParseError {
            message: e.message().to_owned(),
            location,
        }
    })?;
    lower_bundle(raw, content, file)
}

pub fn parse_bundle_with_api(path: &Path) -> core::result::Result<ValidatedIr, PolyplugcError> {
    let content: String =
        std::fs::read_to_string(path).map_err(|e| PolyplugcError::ReadFailed {
            path: path.to_string_lossy().into_owned(),
            source: e,
        })?;
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

    let bundle_dir: &std::path::Path = path.parent().unwrap_or_else(|| std::path::Path::new("."));

    let api_ir: ValidatedIr = if let Some(ref api_path_str) = raw.bundle.api {
        let api_path: std::path::PathBuf = bundle_dir.join(api_path_str);
        parse_api(&api_path)?
    } else {
        ValidatedIr {
            types: Vec::new(),
            enums: Vec::new(),
            contracts: Vec::new(),
            host_contracts: Vec::new(),
            bundle: None,
        }
    };
    check_bundle_name_conflict(&raw.bundle.name, &api_ir.contracts)?;

    let bundle_ir: ValidatedIr = lower_bundle(raw, &content, &file)?;
    Ok(ValidatedIr {
        types: api_ir.types,
        enums: api_ir.enums,
        contracts: api_ir.contracts,
        host_contracts: api_ir.host_contracts,
        bundle: bundle_ir.bundle,
    })
}

fn check_bundle_name_conflict(
    bundle_name: &str,
    contracts: &[ResolvedContract],
) -> core::result::Result<(), PolyplugcError> {
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
) -> core::result::Result<(), PolyplugcError> {
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
) -> core::result::Result<(), PolyplugcError> {
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
fn validate_not_reserved(
    name: &str,
    kind: &str,
    context: &str,
) -> core::result::Result<(), PolyplugcError> {
    match polyplug_codegen::reserved::reserved_in(name) {
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
fn validate_identifier(
    name: &str,
    kind: &str,
    context: &str,
) -> core::result::Result<(), PolyplugcError> {
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

/// Validate a (possibly dotted) contract name. Each dot-separated segment must
/// be a valid identifier — e.g. `pipeline.Decoder`, `host.fs.reader`.
fn validate_contract_name(name: &str, kind: &str) -> core::result::Result<(), PolyplugcError> {
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
) -> core::result::Result<(), PolyplugcError> {
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

fn lower_api(
    raw: RawApiSchema,
    source: &str,
    file: &str,
) -> core::result::Result<ValidatedIr, PolyplugcError> {
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
            });
        }
        resolved_types.push(ResolvedType {
            name: raw_type.name.clone(),
            fields,
        });
    }

    let mut resolved_enums: Vec<EnumDef> = Vec::new();
    for raw_enum in &raw.r#enum {
        validate_identifier(&raw_enum.name, "enum", &raw_enum.name)?;
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
            });
        }
        check_enum_chained_refs(&raw_enum.name, &variants)?;
        resolved_enums.push(EnumDef {
            name: raw_enum.name.clone(),
            repr,
            bitflag: raw_enum.bitflag,
            variants,
        });
    }

    // Reject duplicate plugin-contract names (mirrors the host-contract dup check below).
    let mut seen_plugin_contracts: Vec<&str> = Vec::with_capacity(raw.plugin_contract.len());
    for raw_contract in &raw.plugin_contract {
        if seen_plugin_contracts.contains(&raw_contract.name.as_str()) {
            return Err(PolyplugcError::DuplicateContractName {
                name: raw_contract.name.clone(),
                first_defined_at: None,
            });
        }
        seen_plugin_contracts.push(&raw_contract.name);
    }

    let mut resolved_contracts: Vec<ResolvedContract> = Vec::new();
    for raw_contract in &raw.plugin_contract {
        validate_contract_members(&raw_contract.name, "contract", &raw_contract.functions)?;
        let version: Version = parse_version_spanned(&raw_contract.version, file, source)?;
        let contract_id: u64 = compute_contract_id(&raw_contract.name, version.major);

        let mut functions: Vec<ResolvedFunction> = Vec::new();
        for (function_id, raw_fn) in raw_contract.functions.iter().enumerate() {
            let mut params: Vec<ResolvedParam> = Vec::new();
            for p in &raw_fn.params {
                let ty: ResolvedTypeRef = resolve_type_ref_spanned(
                    &p.ty,
                    &raw_contract.name,
                    &all_known_names,
                    file,
                    source,
                )?;
                params.push(ResolvedParam {
                    name: p.name.clone(),
                    ty,
                });
            }
            let returns: Option<ResolvedTypeRef> = raw_fn
                .returns
                .as_ref()
                .map(|spanned| {
                    resolve_type_ref_spanned(
                        spanned,
                        &raw_contract.name,
                        &all_known_names,
                        file,
                        source,
                    )
                })
                .transpose()?
                // An explicit `return = "void"` means "no return"; normalize it to
                // None so all generators treat it uniformly as a void function.
                .filter(|ty: &ResolvedTypeRef| {
                    !matches!(ty, ResolvedTypeRef::AbiType(crate::ir::AbiBuiltin::Void))
                });
            functions.push(ResolvedFunction {
                name: raw_fn.name.clone(),
                function_id: function_id as u32,
                params,
                returns,
            });
        }

        resolved_contracts.push(ResolvedContract {
            name: raw_contract.name.clone(),
            contract_id,
            version,
            functions,
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
        let contract_id: u64 = compute_host_contract_id(&raw_host_contract.name, version.major);

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
                });
            }
            let returns: Option<ResolvedTypeRef> = raw_fn
                .returns
                .as_ref()
                .map(|spanned| {
                    resolve_type_ref_spanned(
                        spanned,
                        &raw_host_contract.name,
                        &all_known_names,
                        file,
                        source,
                    )
                })
                .transpose()?
                // An explicit `return = "void"` means "no return"; normalize it to
                // None so all generators treat it uniformly as a void function.
                .filter(|ty: &ResolvedTypeRef| {
                    !matches!(ty, ResolvedTypeRef::AbiType(crate::ir::AbiBuiltin::Void))
                });
            functions.push(ResolvedFunction {
                name: raw_fn.name.clone(),
                function_id: function_id as u32,
                params,
                returns,
            });
        }

        resolved_host_contracts.push(ResolvedHostContract {
            name: raw_host_contract.name.clone(),
            contract_id,
            version,
            singleton: raw_host_contract.singleton,
            functions,
        });
    }

    let plugin_contract_names: Vec<&str> = raw
        .plugin_contract
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    for raw_host_contract in &raw.host_contract {
        if !raw_host_contract.name.starts_with("host.") {
            return Err(PolyplugcError::HostContractNameMissingPrefix {
                name: raw_host_contract.name.clone(),
            });
        }
        if plugin_contract_names.contains(&raw_host_contract.name.as_str()) {
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

    Ok(ValidatedIr {
        types: resolved_types,
        enums: resolved_enums,
        contracts: resolved_contracts,
        host_contracts: resolved_host_contracts,
        bundle: None,
    })
}

fn lower_bundle(
    raw: RawBundleSchema,
    source: &str,
    file: &str,
) -> core::result::Result<ValidatedIr, PolyplugcError> {
    let bundle_version: Version = parse_version_spanned(&raw.bundle.version, file, source)?;
    let mut plugins: Vec<ResolvedPlugin> = Vec::new();
    for raw_plugin in &raw.plugin {
        let plugin_version: Version = parse_version_spanned(&raw_plugin.version, file, source)?;
        plugins.push(ResolvedPlugin {
            name: raw_plugin.name.clone(),
            version: plugin_version,
            implements: raw_plugin.implements.clone(),
            optional: raw_plugin.optional.clone(),
        });
    }
    let dep_bundle_id: u64 = compute_bundle_id(&raw.bundle.name);
    let mut resolved_deps: Vec<ResolvedDependency> = Vec::new();
    for dep in &raw.dependencies {
        // The contract_id must use the same major version the API schema uses when it
        // resolves the same contract (compute_contract_id encodes major in the hash).
        // Use min_version.major so the dep's contract_id matches the resolved contract.
        let dep_major: u32 = Version::parse(&dep.min_version)
            .map(|v| v.major)
            .unwrap_or(0);
        let contract_id_val: u64 = compute_contract_id(&dep.contract, dep_major);
        let resolved: ResolvedDependency = if dep.kind == "bundle" {
            let bundle_name: String = dep.bundle.clone().unwrap_or_default();
            let bundle_id_val: u64 = compute_bundle_id(&bundle_name);
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
    let runtime: String = raw.bundle.runtime.to_lowercase();
    let is_native: bool = runtime == "rust" || runtime == "cpp" || runtime == "native";
    let resolved_file: ResolvedBundleFile = match &raw.bundle.file {
        RawBundleFile::PlatformMap(os_map) if is_native => {
            let mut map: std::collections::HashMap<PlatformKey, String> =
                std::collections::HashMap::new();
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
                    "runtime '{}' requires a flat file field (file = \"path\"), not [bundle.file] table",
                    runtime
                ),
            });
        }
        RawBundleFile::Single(_) if is_native => {
            return Err(PolyplugcError::ValidationFailed {
                message: format!(
                    "runtime '{}' requires [bundle.file] table with platform entries, not a flat file field",
                    runtime
                ),
            });
        }
        _ => {
            return Err(PolyplugcError::ValidationFailed {
                message: "bundle.file field is required".to_string(),
            });
        }
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
            runtime: raw.bundle.runtime.clone(),
            file: resolved_file,
            bundle_id: dep_bundle_id,
            plugins,
            dependencies: resolved_deps,
            needs_reinit_on_dep_reload: raw.bundle.needs_reinit_on_dep_reload,
        }),
    })
}

const _: () = {
    let _ = core::mem::size_of::<HashMap<String, String>>();
};

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;

    const SAMPLE_API: &str = "[[plugin_contract]]\nname = \"image.decode\"\nversion = \"1.0.0\"\n\n[[plugin_contract.functions]]\nname = \"decode\"\n\n[[plugin_contract.functions]]\nname = \"supported_formats\"\n    return = \"StringView\"";

    const SAMPLE_BUNDLE: &str = "[bundle]\nname = \"image-plugin\"\nversion = \"1.0.0\"\nfile = \"test.so\"\n\n[[plugin]]\nname = \"jpeg_decoder\"\nversion = \"1.0.0\"\nimplements = [\"image.decode@1.0\"]";

    #[test]
    fn parse_minimal_api() {
        let ir: ValidatedIr = parse_api_str(SAMPLE_API).expect("parse api");
        assert_eq!(ir.contracts.len(), 1);
        assert_eq!(ir.contracts[0].name, "image.decode");
        assert_eq!(ir.contracts[0].functions.len(), 2);
        assert_eq!(ir.contracts[0].functions[0].function_id, 0);
        assert_eq!(ir.contracts[0].functions[1].function_id, 1);
    }

    #[test]
    fn parse_api_with_deprecated_contract_syntax() {
        let deprecated_api: &str = "[[contract]]\nname = \"test.add\"\nversion = \"1.0.0\"\n";
        let ir: ValidatedIr = parse_api_str(deprecated_api).expect("parse deprecated api");
        assert_eq!(ir.contracts.len(), 1);
        assert_eq!(ir.contracts[0].name, "test.add");
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
        }];
        let result: core::result::Result<(), PolyplugcError> =
            check_bundle_name_conflict("test.add", &contracts);
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
        }];
        let result: core::result::Result<(), PolyplugcError> =
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
        let result: core::result::Result<(), PolyplugcError> =
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
            },
            EnumVariant {
                name: "B".to_owned(),
                value: "A | 1".to_owned(),
            },
            EnumVariant {
                name: "C".to_owned(),
                value: "B | 2".to_owned(),
            },
        ];
        let result: core::result::Result<(), PolyplugcError> =
            check_enum_chained_refs("MyEnum", &variants);
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
        let r_a: core::result::Result<(), PolyplugcError> =
            validate_enum_value_expr("0", "Flags", "A", &declared_a);
        assert!(r_a.is_ok(), "A=0 should be valid, got {r_a:?}");

        let declared_b: Vec<String> = vec!["A".to_owned()];
        let r_b: core::result::Result<(), PolyplugcError> =
            validate_enum_value_expr("1", "Flags", "B", &declared_b);
        assert!(r_b.is_ok(), "B=1 should be valid, got {r_b:?}");

        let declared_c: Vec<String> = vec!["A".to_owned(), "B".to_owned()];
        let r_c: core::result::Result<(), PolyplugcError> =
            validate_enum_value_expr("1 << 1", "Flags", "C", &declared_c);
        assert!(r_c.is_ok(), "C=1<<1 should be valid, got {r_c:?}");

        let declared_d: Vec<String> = vec!["A".to_owned(), "B".to_owned(), "C".to_owned()];
        let r_d: core::result::Result<(), PolyplugcError> =
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
            "[bundle]\nname = \"audio-engine\"\nversion = \"1.0.0\"\nfile = \"test.so\"\n\n",
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
        let result: core::result::Result<ValidatedIr, PolyplugcError> = parse_api_str(toml);
        assert!(
            matches!(result, Err(PolyplugcError::HostContractNameMissingPrefix { ref name }) if name == "logger"),
            "expected HostContractNameMissingPrefix for 'logger', got {result:?}",
        );
    }

    #[test]
    fn parse_host_contract_duplicate_with_plugin_contract_rejected() {
        let toml: &str = concat!(
            "[[plugin_contract]]\nname = \"host.logger\"\nversion = \"1.0.0\"\n\n",
            "[[host_contract]]\nname = \"host.logger\"\nversion = \"1.0.0\"\n"
        );
        let result: core::result::Result<ValidatedIr, PolyplugcError> = parse_api_str(toml);
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
        let result: core::result::Result<ValidatedIr, PolyplugcError> = parse_api_str(toml);
        assert!(
            matches!(result, Err(PolyplugcError::DuplicateContractName { ref name, .. }) if name == "host.logger"),
            "expected DuplicateContractName for 'host.logger', got {result:?}",
        );
    }

    #[test]
    fn parse_both_contract_types_valid() {
        let toml: &str = concat!(
            "[[plugin_contract]]\nname = \"image.decode\"\nversion = \"1.0.0\"\n\n",
            "[[host_contract]]\nname = \"host.logger\"\nversion = \"1.0.0\"\n"
        );
        let ir: ValidatedIr = parse_api_str(toml).expect("parse both contract types");
        assert_eq!(ir.contracts.len(), 1);
        assert_eq!(ir.contracts[0].name, "image.decode");
    }

    #[test]
    fn parse_host_contract_invalid_version_rejected() {
        let toml: &str = "[[host_contract]]\nname = \"host.logger\"\nversion = \"invalid\"\n";
        let result: core::result::Result<ValidatedIr, PolyplugcError> = parse_api_str(toml);
        assert!(
            matches!(result, Err(PolyplugcError::ValidationFailed { .. })),
            "expected ValidationFailed for invalid version format, got {result:?}",
        );
    }

    #[test]
    fn parse_host_contract_version_overflow_rejected() {
        let toml: &str = "[[host_contract]]\nname = \"host.logger\"\nversion = \"4294967296.0\"\n";
        let result: core::result::Result<ValidatedIr, PolyplugcError> = parse_api_str(toml);
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
            "[[plugin_contract]]\nname = \"image.decode\"\nversion = \"1.0.0\"\n\n",
            "[[plugin_contract.functions]]\nname = \"class\"\n"
        );
        let result: core::result::Result<ValidatedIr, PolyplugcError> = parse_api_str(toml);
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
        let result: core::result::Result<ValidatedIr, PolyplugcError> = parse_api_str(toml);
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
        let result: core::result::Result<ValidatedIr, PolyplugcError> = parse_api_str(toml);
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
        let toml: &str = "[[plugin_contract]]\nname = \"image.int\"\nversion = \"1.0.0\"\n";
        let result: core::result::Result<ValidatedIr, PolyplugcError> = parse_api_str(toml);
        assert!(
            matches!(result, Err(PolyplugcError::ReservedIdentifier { ref name, .. }) if name == "int"),
            "expected ReservedIdentifier for `int`, got {result:?}",
        );
    }

    #[test]
    fn parse_polyplug_prefixed_function_rejected() {
        let toml: &str = concat!(
            "[[plugin_contract]]\nname = \"image.decode\"\nversion = \"1.0.0\"\n\n",
            "[[plugin_contract.functions]]\nname = \"polyplug_init\"\n"
        );
        let result: core::result::Result<ValidatedIr, PolyplugcError> = parse_api_str(toml);
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
            "[[plugin_contract]]\nname = \"pipeline.Decoder\"\nversion = \"1.0.0\"\n\n",
            "[[plugin_contract.functions]]\nname = \"decode\"\n"
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
}

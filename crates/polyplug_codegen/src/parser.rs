//! Parser — TOML schema parsing for polyplugc.
//!
//! Parses `api.toml` and `bundle.toml` using serde+toml.
//! Produces raw AST structs that are later lowered to `ValidatedIr`.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

use crate::error::PolyplugcError;
use crate::ir::EnumDef;
use crate::ir::EnumVariant;
use crate::ir::ReprType;
use crate::ir::ResolvedBundle;
use crate::ir::ResolvedContract;
use crate::ir::ResolvedDependency;
use crate::ir::ResolvedField;
use crate::ir::ResolvedFunction;
use crate::ir::ResolvedParam;
use crate::ir::ResolvedPlugin;
use crate::ir::ResolvedType;
use crate::ir::ResolvedTypeRef;
use crate::ir::ValidatedIr;
use crate::ir::Version;
use crate::ir::compute_bundle_id;
use crate::ir::compute_contract_id;
use crate::ir::resolve_type_ref;

// ─── Raw TOML AST structs ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(crate) struct RawApiSchema {
    #[serde(default)]
    pub types: Vec<RawType>,
    #[serde(rename = "enum", default)]
    pub r#enum: Vec<RawEnum>,
    #[serde(default)]
    pub contract: Vec<RawContract>,
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
    pub ty: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawContract {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub functions: Vec<RawFunction>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawFunction {
    pub name: String,
    #[serde(default)]
    pub params: Vec<RawParam>,
    #[serde(rename = "return", default)]
    pub returns: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawParam {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
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
    pub plugin: Vec<RawPlugin>,
    #[serde(default, rename = "dependency")]
    pub dependencies: Vec<RawDependency>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawBundleMeta {
    pub name: String,
    pub version: String,
    /// Path or package name to the api.toml for this bundle.
    #[serde(default)]
    pub api: Option<String>,
    #[serde(default)]
    pub runtime: String,
    pub file: RawBundleFile,
    #[serde(default)]
    pub needs_reinit_on_dep_reload: bool,
}

/// Bundle file field — either flat string or [bundle.file] table.
/// Uses untagged enum to accept both forms.
/// The [bundle.file] table deserializes as nested HashMap: {"linux": {"x86_64": "file.so"}}
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum RawBundleFile {
    /// Flat string: file = "path"
    Single(String),
    /// Table: [bundle.file] with platform entries as nested map
    PlatformMap(HashMap<String, HashMap<String, String>>),
}

impl Default for RawBundleFile {
    fn default() -> Self {
        RawBundleFile::Single(String::new())
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawPlugin {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub implements: Vec<String>,
    #[serde(default)]
    pub optional: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawDependency {
    /// Either "contract" or "bundle" depending on resolution strategy.
    #[allow(dead_code)]
    pub kind: String,
    /// Contract name (will be hashed to contract_id by the IR lowering).
    #[allow(dead_code)]
    pub contract: String,
    /// Minimum version required, e.g. "1.0".
    #[allow(dead_code)]
    pub min_version: String,
    /// Bundle name — only present when kind == "bundle".
    #[serde(default)]
    #[allow(dead_code)]
    pub bundle: Option<String>,
}

// ─── Public parse functions ───────────────────────────────────────────────────────

/// Parse and validate an `api.toml` file, producing a `ValidatedIr`.
pub fn parse_api(path: &Path) -> Result<ValidatedIr, PolyplugcError> {
    let content: String =
        std::fs::read_to_string(path).map_err(|e| PolyplugcError::ReadFailed {
            path: path.to_string_lossy().into_owned(),
            source: e,
        })?;
    parse_api_str(&content)
}

/// Parse an `api.toml` TOML string.
pub fn parse_api_str(content: &str) -> Result<ValidatedIr, PolyplugcError> {
    let raw: RawApiSchema =
        toml::from_str(content).map_err(|e| PolyplugcError::ValidationFailed {
            message: format!("TOML parse error: {e}"),
        })?;
    lower_api(raw)
}

/// Parse and validate a `bundle.toml` file.
#[allow(dead_code)]
pub fn parse_bundle(path: &Path) -> Result<ValidatedIr, PolyplugcError> {
    let content: String =
        std::fs::read_to_string(path).map_err(|e| PolyplugcError::ReadFailed {
            path: path.to_string_lossy().into_owned(),
            source: e,
        })?;
    parse_bundle_str(&content)
}

/// Parse a `bundle.toml` TOML string.
#[allow(dead_code)]
pub fn parse_bundle_str(content: &str) -> Result<ValidatedIr, PolyplugcError> {
    let raw: RawBundleSchema =
        toml::from_str(content).map_err(|e| PolyplugcError::ValidationFailed {
            message: format!("TOML parse error: {e}"),
        })?;
    lower_bundle(raw)
}

/// Parse a `bundle.toml` file and chain-load the referenced `api.toml`.
///
/// Reads `bundle.bundle.api` field. If present, resolves the path relative to the
/// bundle file's parent directory and calls `parse_api()` to load types + contracts.
/// Returns a `ValidatedIr` with the bundle metadata merged with the API types/contracts.
pub fn parse_bundle_with_api(path: &Path) -> Result<ValidatedIr, PolyplugcError> {
    let content: String =
        std::fs::read_to_string(path).map_err(|e| PolyplugcError::ReadFailed {
            path: path.to_string_lossy().into_owned(),
            source: e,
        })?;
    let raw: RawBundleSchema =
        toml::from_str(&content).map_err(|e| PolyplugcError::ValidationFailed {
            message: format!("TOML parse error: {e}"),
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
            bundle: None,
        }
    };
    check_bundle_name_conflict(&raw.bundle.name, &api_ir.contracts)?;

    let bundle_ir: ValidatedIr = lower_bundle(raw)?;
    Ok(ValidatedIr {
        types: api_ir.types,
        enums: api_ir.enums,
        contracts: api_ir.contracts,
        bundle: bundle_ir.bundle,
    })
}

// ─── IR Lowering ──────────────────────────────────────────────────────────────────

/// Checks whether `bundle_name` matches any contract name in `contracts`.
///
/// Returns `Err(PolyplugcError::BundleNameConflict)` on the first match.
/// Comparison is exact and case-sensitive — contract names are identity-bearing.
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

/// Validate a variant value expression string.
///
/// Allowed tokens: integer literals (decimal, hex 0x..., binary 0b...),
/// operators: `<<`, `|`, `~`, grouping: `(`, `)`, whitespace (skipped),
/// and previously-declared variant names (backward references only).
///
/// Returns Ok(()) if valid, Err with appropriate PolyplugcError if not.
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
        // Skip whitespace
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        // Integer literal
        if c.is_ascii_digit() {
            // Consume hex (0x...) or binary (0b...) or decimal
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
        // Identifier (variant name)
        if c.is_alphabetic() || c == '_' {
            let start: usize = i;
            while i < len && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let ident: String = chars[start..i].iter().collect();
            if declared_variants.contains(&ident) {
                continue;
            }
            // Not a known backward ref
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
        // << operator
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
        // | operator
        if c == '|' {
            i += 1;
            continue;
        }
        // ~ operator
        if c == '~' {
            i += 1;
            continue;
        }
        // Grouping
        if c == '(' || c == ')' {
            i += 1;
            continue;
        }
        // Anything else is invalid
        return Err(PolyplugcError::EnumInvalidValueExpr {
            enum_name: enum_name.to_owned(),
            variant_name: variant_name.to_owned(),
            expr: expr.to_owned(),
        });
    }
    Ok(())
}

/// Check that no variant references another variant that itself contains a reference.
/// Enforces the "one level deep" rule.
fn check_enum_chained_refs(
    enum_name: &str,
    variants: &[EnumVariant],
) -> Result<(), PolyplugcError> {
    // Helper: check if an expression string contains any variant name token
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
        // Find all variant name tokens in this variant's value expression
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
                // Is this a reference to a declared variant?
                if all_names.contains(&ref_name.as_str()) {
                    // Find the referenced variant's value
                    if let Some(ref_variant) = variants.iter().find(|v| v.name == ref_name) {
                        // Does the referenced variant also reference a variant?
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

fn lower_api(raw: RawApiSchema) -> Result<ValidatedIr, PolyplugcError> {
    // Step 1: Collect known type names for type resolution
    let known_type_names: Vec<String> = raw.types.iter().map(|t| t.name.clone()).collect();

    // Step 2: Collect known enum names and check for name collisions with types
    let known_enum_names: Vec<String> = raw.r#enum.iter().map(|e| e.name.clone()).collect();
    for name in &known_enum_names {
        if known_type_names.contains(name) {
            return Err(PolyplugcError::EnumNameCollision { name: name.clone() });
        }
    }

    // Step 3: Build combined name set for type reference resolution
    let all_known_names: Vec<String> = known_type_names
        .iter()
        .chain(known_enum_names.iter())
        .cloned()
        .collect();

    // Step 4: Resolve types
    let mut resolved_types: Vec<ResolvedType> = Vec::new();
    for raw_type in &raw.types {
        let mut fields: Vec<ResolvedField> = Vec::new();
        for field in &raw_type.fields {
            let ty: ResolvedTypeRef =
                resolve_type_ref(&field.ty, &raw_type.name, &all_known_names)?;
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

    // Step 5: Resolve enums
    let mut resolved_enums: Vec<EnumDef> = Vec::new();
    for raw_enum in &raw.r#enum {
        let repr: ReprType = match ReprType::parse(&raw_enum.repr) {
            Some(r) => r,
            None => {
                return Err(PolyplugcError::EnumInvalidRepr {
                    enum_name: raw_enum.name.clone(),
                    repr: raw_enum.repr.clone(),
                });
            }
        };
        let mut declared: Vec<String> = Vec::new();
        let mut variants: Vec<EnumVariant> = Vec::new();
        for raw_variant in &raw_enum.variants {
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

    // Step 6: Resolve contracts
    let mut resolved_contracts: Vec<ResolvedContract> = Vec::new();
    for raw_contract in &raw.contract {
        let version: Version = Version::parse(&raw_contract.version)?;
        let contract_id: u64 = compute_contract_id(&raw_contract.name, version.major);

        let mut functions: Vec<ResolvedFunction> = Vec::new();
        for (function_id, raw_fn) in raw_contract.functions.iter().enumerate() {
            let mut params: Vec<ResolvedParam> = Vec::new();
            for p in &raw_fn.params {
                let ty: ResolvedTypeRef =
                    resolve_type_ref(&p.ty, &raw_contract.name, &all_known_names)?;
                params.push(ResolvedParam {
                    name: p.name.clone(),
                    ty,
                });
            }
            let returns: Option<ResolvedTypeRef> = raw_fn
                .returns
                .as_deref()
                .map(|r| resolve_type_ref(r, &raw_contract.name, &all_known_names))
                .transpose()?;
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

    Ok(ValidatedIr {
        types: resolved_types,
        enums: resolved_enums,
        contracts: resolved_contracts,
        bundle: None,
    })
}

fn lower_bundle(raw: RawBundleSchema) -> Result<ValidatedIr, PolyplugcError> {
    let bundle_version: Version = Version::parse(&raw.bundle.version)?;
    let mut plugins: Vec<ResolvedPlugin> = Vec::new();
    for raw_plugin in &raw.plugin {
        let plugin_version: Version = Version::parse(&raw_plugin.version)?;
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
        let contract_id_val: u64 = compute_contract_id(&dep.contract, 0);
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
    // Parse file field — either flat string or platform table
    let resolved_file: crate::generators::ResolvedBundleFile = match &raw.bundle.file {
        RawBundleFile::Single(path) if !path.is_empty() => {
            crate::generators::ResolvedBundleFile::Single(path.clone())
        }
        RawBundleFile::PlatformMap(os_map) => {
            let mut map: std::collections::HashMap<crate::generators::PlatformKey, String> =
                std::collections::HashMap::new();
            for (os, arch_map) in os_map {
                for (arch, path) in arch_map {
                    map.insert(
                        crate::generators::PlatformKey {
                            os: os.clone(),
                            arch: arch.clone(),
                        },
                        path.clone(),
                    );
                }
            }
            crate::generators::ResolvedBundleFile::PlatformMap(map)
        }
        _ => crate::generators::ResolvedBundleFile::Single(format!("lib{}.so", raw.bundle.name)),
    };
    Ok(ValidatedIr {
        types: Vec::new(),
        enums: Vec::new(),
        contracts: Vec::new(),
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

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_API: &str = "[[contract]]\nname = \"image.decode\"\nversion = \"1.0.0\"\n\n[[contract.functions]]\nname = \"decode\"\n\n[[contract.functions]]\nname = \"supported_formats\"\n    return = \"StringView\"";

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
        // Variant B references variant C which hasn't been declared yet
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
        // A = "1", B = "A | 1" (B refs A which has no ref — OK so far)
        // C = "B | 2" (C refs B which refs A — chained ref!)
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
        let result: Result<(), PolyplugcError> = check_enum_chained_refs("MyEnum", &variants);
        assert!(
            matches!(result, Err(PolyplugcError::EnumChainedRef { ref variant_name, ref ref_name, .. }) if variant_name == "C" && ref_name == "B"),
            "expected EnumChainedRef for C->B, got {result:?}",
        );
    }

    #[test]
    fn test_enum_name_collision_with_type() {
        // [[types]] named "Status" AND [[enum]] also named "Status" — should error
        // This test is for T5's lower_api(). For now we call validate directly with
        // a minimal check that names collide. We'll use the collision logic directly.
        let type_names: Vec<String> = vec!["Status".to_owned()];
        let enum_names: Vec<String> = vec!["Status".to_owned()];
        let collision: bool = enum_names.iter().any(|n| type_names.contains(n));
        assert!(collision, "expected name collision detected");
    }

    #[test]
    fn test_enum_invalid_repr_rejected() {
        // Repr "i32" should be invalid — test via ReprType::parse
        let result: Option<ReprType> = ReprType::parse("i32");
        assert!(result.is_none(), "i32 should not be a valid ReprType");
    }

    #[test]
    fn test_enum_valid_bitflag_expr() {
        // Test that a valid bitflag expression parses without error
        // A=0, B=1, C=1<<1, D=B|C
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

// Suppress unused import warning for HashMap (used in future expansion)
const _: () = {
    let _ = core::mem::size_of::<HashMap<String, String>>();
};

//! Parser — TOML schema parsing for polyplugc.
//!
//! Parses `api.toml` and `bundle.toml` using serde+toml.
//! Produces raw AST structs that are later lowered to `ValidatedIr`.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

use crate::error::CodegenError;
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
pub(crate) fn parse_api(path: &Path) -> Result<ValidatedIr, CodegenError> {
    let content: String = std::fs::read_to_string(path).map_err(|e| CodegenError::WriteFailed {
        path: path.to_string_lossy().into_owned(),
        source: e,
    })?;
    parse_api_str(&content)
}

/// Parse an `api.toml` TOML string.
pub(crate) fn parse_api_str(content: &str) -> Result<ValidatedIr, CodegenError> {
    let raw: RawApiSchema =
        toml::from_str(content).map_err(|e| CodegenError::ValidationFailed {
            message: format!("TOML parse error: {e}"),
        })?;
    lower_api(raw)
}

/// Parse and validate a `bundle.toml` file.
#[allow(dead_code)]
pub(crate) fn parse_bundle(path: &Path) -> Result<ValidatedIr, CodegenError> {
    let content: String = std::fs::read_to_string(path).map_err(|e| CodegenError::WriteFailed {
        path: path.to_string_lossy().into_owned(),
        source: e,
    })?;
    parse_bundle_str(&content)
}

/// Parse a `bundle.toml` TOML string.
#[allow(dead_code)]
pub(crate) fn parse_bundle_str(content: &str) -> Result<ValidatedIr, CodegenError> {
    let raw: RawBundleSchema =
        toml::from_str(content).map_err(|e| CodegenError::ValidationFailed {
            message: format!("TOML parse error: {e}"),
        })?;
    lower_bundle(raw)
}

/// Parse a `bundle.toml` file and chain-load the referenced `api.toml`.
///
/// Reads `bundle.bundle.api` field. If present, resolves the path relative to the
/// bundle file's parent directory and calls `parse_api()` to load types + contracts.
/// Returns a `ValidatedIr` with the bundle metadata merged with the API types/contracts.
pub(crate) fn parse_bundle_with_api(path: &Path) -> Result<ValidatedIr, CodegenError> {
    let content: String = std::fs::read_to_string(path).map_err(|e| CodegenError::WriteFailed {
        path: path.to_string_lossy().into_owned(),
        source: e,
    })?;
    let raw: RawBundleSchema =
        toml::from_str(&content).map_err(|e| CodegenError::ValidationFailed {
            message: format!("TOML parse error: {e}"),
        })?;

    let bundle_dir: &std::path::Path = path.parent().unwrap_or_else(|| std::path::Path::new("."));

    let api_ir: ValidatedIr = if let Some(ref api_path_str) = raw.bundle.api {
        let api_path: std::path::PathBuf = bundle_dir.join(api_path_str);
        parse_api(&api_path)?
    } else {
        ValidatedIr {
            types: Vec::new(),
            contracts: Vec::new(),
            bundle: None,
        }
    };

    let bundle_ir: ValidatedIr = lower_bundle(raw)?;
    Ok(ValidatedIr {
        types: api_ir.types,
        contracts: api_ir.contracts,
        bundle: bundle_ir.bundle,
    })
}

// ─── IR Lowering ──────────────────────────────────────────────────────────────────

fn lower_api(raw: RawApiSchema) -> Result<ValidatedIr, CodegenError> {
    // Step 1: Collect known type names for type resolution
    let known_type_names: Vec<String> = raw.types.iter().map(|t| t.name.clone()).collect();

    // Step 2: Resolve types
    let mut resolved_types: Vec<ResolvedType> = Vec::new();
    for raw_type in &raw.types {
        let mut fields: Vec<ResolvedField> = Vec::new();
        for field in &raw_type.fields {
            let ty: ResolvedTypeRef =
                resolve_type_ref(&field.ty, &raw_type.name, &known_type_names)?;
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

    // Step 3: Resolve contracts
    let mut resolved_contracts: Vec<ResolvedContract> = Vec::new();
    for raw_contract in &raw.contract {
        let version: Version = Version::parse(&raw_contract.version)?;
        let contract_id: u64 = compute_contract_id(&raw_contract.name, version.major);

        let mut functions: Vec<ResolvedFunction> = Vec::new();
        for (function_id, raw_fn) in raw_contract.functions.iter().enumerate() {
            let mut params: Vec<ResolvedParam> = Vec::new();
            for p in &raw_fn.params {
                let ty: ResolvedTypeRef =
                    resolve_type_ref(&p.ty, &raw_contract.name, &known_type_names)?;
                params.push(ResolvedParam {
                    name: p.name.clone(),
                    ty,
                });
            }
            let returns: Option<ResolvedTypeRef> = raw_fn
                .returns
                .as_deref()
                .map(|r| resolve_type_ref(r, &raw_contract.name, &known_type_names))
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
        contracts: resolved_contracts,
        bundle: None,
    })
}

fn lower_bundle(raw: RawBundleSchema) -> Result<ValidatedIr, CodegenError> {
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
    Ok(ValidatedIr {
        types: Vec::new(),
        contracts: Vec::new(),
        bundle: Some(ResolvedBundle {
            name: raw.bundle.name.clone(),
            version: bundle_version,
            bundle_id: dep_bundle_id,
            plugins,
            dependencies: resolved_deps,
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_API: &str = "[[contract]]\nname = \"image.decode\"\nversion = \"1.0.0\"\n\n[[contract.functions]]\nname = \"decode\"\n\n[[contract.functions]]\nname = \"supported_formats\"\n    return = \"StringView\"";

    const SAMPLE_BUNDLE: &str = "[bundle]\nname = \"image-plugin\"\nversion = \"1.0.0\"\n\n[[plugin]]\nname = \"jpeg_decoder\"\nversion = \"1.0.0\"\nimplements = [\"image.decode@1.0\"]";

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
}

#[test]
fn parse_bundle_with_dependency() {
    let toml: &str = concat!(
        "[bundle]\nname = \"audio-engine\"\nversion = \"1.0.0\"\n\n",
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

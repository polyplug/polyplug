//! Generators — CodeGenerator trait and language dispatch.

use std::path::PathBuf;

pub(crate) mod cpp;
pub(crate) mod csharp;
pub(crate) mod js_quickjs;
pub(crate) mod lua;
pub(crate) mod python;
pub(crate) mod rust;

use crate::ir::ResolvedDependency;
use crate::ir::ValidatedIr;
use polyplug_codegen::PolyplugcError;

/// Check if a runtime is a native runtime.
pub fn is_native_runtime(runtime: &str) -> bool {
    runtime.to_lowercase() == "native"
}

/// Emit the `[[dependency]]` tables for a bundle manifest.
///
/// This is the single, canonical emitter shared by every language generator so that
/// all generators produce byte-identical manifest dependency tables. The output emits
/// the full union of fields the runtime manifest parser (`RawManifestDependency`)
/// requires:
///
/// - `kind` — REQUIRED (no serde default); `"contract"` or `"bundle"`.
/// - `contract` — the contract name.
/// - `contract_id` — hex `0x{:016X}`; the runtime defaults it to 0 only when absent,
///   so it is always emitted to keep `ByContract` deps resolvable.
/// - `bundle` / `bundle_id` — emitted only for `ByBundle`; without `bundle_id` the
///   runtime drops the dependency with a warning.
/// - `min_version` — a quoted TOML string `"{major}.0"` (the parser deserializes it
///   into a `String`; a bare integer fails to parse).
pub(crate) fn emit_manifest_dependencies(dependencies: &[ResolvedDependency]) -> String {
    let mut dep_toml: String = String::new();
    for dep in dependencies {
        dep_toml.push_str("\n[[dependency]]\n");
        match dep {
            ResolvedDependency::ByContract {
                contract,
                contract_id,
                min_version,
            } => {
                dep_toml.push_str("kind = \"contract\"\n");
                dep_toml.push_str(&format!("contract = \"{contract}\"\n"));
                dep_toml.push_str(&format!("contract_id = 0x{contract_id:016X}\n"));
                dep_toml.push_str(&format!("min_version = \"{min_version}.0\"\n"));
            }
            ResolvedDependency::ByBundle {
                bundle,
                bundle_id,
                contract,
                contract_id,
                min_version,
            } => {
                dep_toml.push_str("kind = \"bundle\"\n");
                dep_toml.push_str(&format!("bundle = \"{bundle}\"\n"));
                dep_toml.push_str(&format!("bundle_id = 0x{bundle_id:016X}\n"));
                dep_toml.push_str(&format!("contract = \"{contract}\"\n"));
                dep_toml.push_str(&format!("contract_id = 0x{contract_id:016X}\n"));
                dep_toml.push_str(&format!("min_version = \"{min_version}.0\"\n"));
            }
        }
    }
    dep_toml
}

/// Format the manifest file field based on ResolvedBundleFile.
pub(crate) fn format_manifest_file_field(file: &polyplug_codegen::ResolvedBundleFile) -> String {
    match file {
        polyplug_codegen::ResolvedBundleFile::Single(path) => format!("file = \"{path}\""),
        polyplug_codegen::ResolvedBundleFile::PlatformMap(map) => {
            let mut lines: Vec<String> = Vec::with_capacity(map.len() + 1);
            lines.push(String::from("[file]"));
            let mut entries: Vec<(&str, &str, &str)> = map
                .iter()
                .map(|(k, v)| (k.os.as_str(), k.arch.as_str(), v.as_str()))
                .collect();
            entries.sort();
            for (os, arch, path) in entries {
                lines.push(format!("{os}.{arch} = \"{path}\""));
            }
            lines.join("\n")
        }
    }
}

/// A single generated file (path + content).
#[derive(Debug)]
pub(crate) struct GeneratedFile {
    /// Relative output path.
    pub path: PathBuf,
    /// Generated source code.
    pub content: String,
    /// If true, always write this file (skip cache check).
    /// Used for manifest.toml which must always be regenerated.
    pub force_regenerate: bool,
}
/// Collection of generated files.
#[derive(Debug, Default)]
pub(crate) struct GeneratedFiles {
    pub files: Vec<GeneratedFile>,
}

/// Trait for language-specific code generators.
pub(crate) trait CodeGenerator {
    /// Generate host-side caller code for an app developer.
    fn generate_host(
        &self,
        ir: &ValidatedIr,
        files: &mut GeneratedFiles,
    ) -> Result<(), PolyplugcError>;

    /// Generate guest-side SDK + ABI wrappers for plugin developers.
    fn generate_guest(
        &self,
        ir: &ValidatedIr,
        files: &mut GeneratedFiles,
    ) -> Result<(), PolyplugcError>;
}

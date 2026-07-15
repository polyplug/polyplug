//! Generators — CodeGenerator trait and language dispatch.

pub(crate) mod attributes;
pub mod cpp;
pub mod csharp;
pub(crate) mod docs;
pub mod js_quickjs;
pub mod lua;
pub mod python;
pub mod rust;

use crate::GenerateOutput;
pub use crate::GeneratedFile;
use crate::OutputLayout;
use crate::PolyplugcError;
use crate::ResolvedBundleFile;
pub type GeneratedFiles = GenerateOutput;
use crate::ir::ResolvedContract;
use crate::ir::ResolvedDependency;
use crate::ir::ValidatedIr;

/// Arena buffer length (bytes) emitted by every language generator.
pub const CALL_ARENA_BUF_LEN: usize = 512;
/// Convert contract and declaration path segments into their canonical PascalCase
/// identifier form.
pub(crate) fn canonical_pascal_case(value: &str) -> String {
    value
        .split(['.', '_', '-'])
        .filter(|segment| !segment.is_empty())
        .map(|segment| {
            let mut characters = segment.chars();
            match characters.next() {
                Some(first) => first.to_uppercase().collect::<String>() + characters.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

/// Fingerprint every semantic input that contributes to an internal profile.
pub(crate) fn internal_generation_fingerprint(ir: &ValidatedIr) -> u64 {
    let mut fingerprint: u64 = 0xcbf2_9ce4_8422_2325;
    let mut hash = |text: &str| {
        for byte in text.bytes() {
            fingerprint ^= u64::from(byte);
            fingerprint = fingerprint.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    hash(&format!("{:?}", ir.types));
    hash(&format!("{:?}", ir.enums));
    hash(&format!("{:?}", ir.contracts));
    hash(&format!("{:?}", ir.host_contracts));
    hash(&format!("{:?}", ir.langs));
    if let Some(bundle) = &ir.bundle {
        hash(&format!(
            "{:?}{:?}{:?}{:?}{:?}",
            bundle.name,
            bundle.version,
            bundle.loader,
            bundle.bundle_id,
            bundle.needs_reinit_on_dep_reload
        ));
        hash(&format!("{:?}", bundle.plugins));
        hash(&format!("{:?}", bundle.dependencies));
        match &bundle.file {
            ResolvedBundleFile::Absent => hash("absent"),
            ResolvedBundleFile::Single(path) => hash(path),
            ResolvedBundleFile::PlatformMap(files) => {
                let mut entries: Vec<(&str, &str, &str)> = files
                    .iter()
                    .map(|(key, path)| (key.os.as_str(), key.arch.as_str(), path.as_str()))
                    .collect();
                entries.sort_unstable();
                for (os, arch, path) in entries {
                    hash(os);
                    hash(arch);
                    hash(path);
                }
            }
        }
    } else {
        hash("no-bundle");
    }
    fingerprint
}

/// Collect every contract in `ir.contracts` whose `contract_id` appears in the
/// bundle's declared dependencies.  Returns an empty vec when there is no bundle
/// or when no dependency matches any known contract.
pub(crate) fn collect_peer_contracts(ir: &ValidatedIr) -> Vec<&ResolvedContract> {
    let deps: &[ResolvedDependency] = match ir.bundle.as_ref() {
        Some(b) => &b.dependencies,
        None => return Vec::new(),
    };

    ir.contracts
        .iter()
        .filter(|c: &&ResolvedContract| {
            deps.iter().any(|d: &ResolvedDependency| {
                let dep_contract_id: u64 = match d {
                    ResolvedDependency::ByContract { contract_id, .. } => *contract_id,
                    ResolvedDependency::ByBundle { contract_id, .. } => *contract_id,
                };
                dep_contract_id == c.contract_id
            })
        })
        .collect()
}

/// Return the `min_version` (major) for a dependency whose `contract_id` matches
/// `target_contract_id`.  Returns 0 when no match is found; callers guard against
/// an empty peer set before reaching this.
pub(crate) fn peer_min_version(ir: &ValidatedIr, target_contract_id: u64) -> u32 {
    let deps: &[ResolvedDependency] = match ir.bundle.as_ref() {
        Some(b) => &b.dependencies,
        None => return 0,
    };
    for d in deps {
        match d {
            ResolvedDependency::ByContract {
                contract_id,
                min_version,
                ..
            } if *contract_id == target_contract_id => return *min_version,
            ResolvedDependency::ByBundle {
                contract_id,
                min_version,
                ..
            } if *contract_id == target_contract_id => return *min_version,
            _ => {}
        }
    }
    0
}

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
pub(crate) fn format_manifest_file_field(file: &ResolvedBundleFile) -> String {
    match file {
        ResolvedBundleFile::Absent => String::new(),
        ResolvedBundleFile::Single(path) => format!("file = \"{path}\""),
        ResolvedBundleFile::PlatformMap(map) => {
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

/// Trait for language-specific code generators.
pub trait CodeGenerator {
    /// Generate host-side caller code for an app developer.
    fn generate_host(
        &self,
        ir: &ValidatedIr,
        layout: &OutputLayout,
        files: &mut GeneratedFiles,
    ) -> Result<(), PolyplugcError>;

    /// Generate guest-side SDK + ABI wrappers for plugin developers.
    fn generate_guest(
        &self,
        ir: &ValidatedIr,
        layout: &OutputLayout,
        files: &mut GeneratedFiles,
    ) -> Result<(), PolyplugcError>;

    /// Apply this language's canonical output layout after ordinary generation.
    fn apply_output_layout(
        &self,
        ir: &ValidatedIr,
        side: crate::Side,
        layout: &OutputLayout,
        files: &mut GeneratedFiles,
    ) -> Result<(), PolyplugcError>;
}

#[cfg(test)]
mod tests {
    use super::canonical_pascal_case;

    #[test]
    fn canonical_pascal_case_tokenizes_contract_segments() {
        assert_eq!(
            canonical_pascal_case("game_engine.Plugin"),
            "GameEnginePlugin"
        );
        assert_eq!(
            canonical_pascal_case("game-engine_plugin"),
            "GameEnginePlugin"
        );
    }
}

//! Generators — CodeGenerator trait and language dispatch.

use std::path::PathBuf;

pub(crate) mod cpp;
pub(crate) mod csharp;
pub(crate) mod js_quickjs;
pub(crate) mod lua;
pub(crate) mod python;
pub(crate) mod rust;

use crate::ir::ValidatedIr;
use polyplug_codegen::PolyplugcError;

/// Check if a runtime is a native runtime.
pub fn is_native_runtime(runtime: &str) -> bool {
    runtime.to_lowercase() == "native"
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
    #[allow(dead_code)]
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

    /// Language identifier used in file extensions and header comments.
    #[allow(dead_code)]
    fn language_name(&self) -> &'static str;
}

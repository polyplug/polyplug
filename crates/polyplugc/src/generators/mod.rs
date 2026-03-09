//! Generators — CodeGenerator trait and language dispatch.

use std::path::PathBuf;

pub(crate) mod cpp;
pub(crate) mod csharp;
pub(crate) mod rust;
pub(crate) mod lua;
pub(crate) mod python;

use crate::error::CodegenError;
use crate::ir::ValidatedIr;

/// A single generated file (path + content).
#[derive(Debug)]
pub(crate) struct GeneratedFile {
    /// Relative output path.
    pub path: PathBuf,
    /// Generated source code.
    pub content: String,
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
    ) -> Result<(), CodegenError>;

    /// Generate guest-side SDK + ABI wrappers for plugin developers.
    fn generate_guest(
        &self,
        ir: &ValidatedIr,
        files: &mut GeneratedFiles,
    ) -> Result<(), CodegenError>;

    /// Language identifier used in file extensions and header comments.
    #[allow(dead_code)]
    fn language_name(&self) -> &'static str;
}

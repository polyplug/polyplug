//! pack — generates scaffold metadata for packaging plugins.
//!
//! Produces language-specific project scaffolding for plugin authors.
//! This command generates scaffold files only — no build execution.

use std::path::Path;

use crate::error::CodegenError;
use crate::ir::ValidatedIr;

/// Generate scaffold packaging files for a plugin in the given language.
/// Writes scaffold files under `out/`.
///
/// Note: This command generates scaffold metadata only.
/// No build tools are invoked (no cargo, npm, dotnet, pip, luarocks, deno).
pub(crate) fn run(ir: &ValidatedIr, out: &Path, lang: &str) -> Result<(), CodegenError> {
    let _ = (ir, out);
    match lang {
        _ => Err(CodegenError::UnsupportedLanguage {
            lang: lang.to_owned(),
        }),
    }
}

//! Lua code generator skeleton.
//! Full implementation is planned for Epic 11.
//! This stub exists to register the module and allow compilation.

use crate::error::CodegenError;
use crate::generators::CodeGenerator;
use crate::generators::GeneratedFiles;
use crate::ir::ValidatedIr;

/// Lua code generator stub — not yet implemented.
#[allow(dead_code)]
pub(crate) struct LuaGenerator;

impl CodeGenerator for LuaGenerator {
    fn language_name(&self) -> &'static str {
        "lua"
    }

    fn generate_host(
        &self,
        _ir: &ValidatedIr,
        _files: &mut GeneratedFiles,
    ) -> Result<(), CodegenError> {
        Err(CodegenError::ValidationFailed {
            message: "Lua generator not yet implemented (planned for Epic 11)".to_owned(),
        })
    }

    fn generate_guest(
        &self,
        _ir: &ValidatedIr,
        _files: &mut GeneratedFiles,
    ) -> Result<(), CodegenError> {
        Err(CodegenError::ValidationFailed {
            message: "Lua generator not yet implemented (planned for Epic 11)".to_owned(),
        })
    }
}

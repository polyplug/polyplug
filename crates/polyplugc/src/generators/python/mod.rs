//! Python code generator skeleton.
//! Full implementation is planned for Epic 10.
//! This stub exists to register the module and allow compilation.

use crate::error::CodegenError;
use crate::generators::CodeGenerator;
use crate::generators::GeneratedFiles;
use crate::ir::ValidatedIr;

/// Python code generator stub — not yet implemented.
#[allow(dead_code)]
pub(crate) struct PythonGenerator;

impl CodeGenerator for PythonGenerator {
    fn language_name(&self) -> &'static str {
        "python"
    }

    fn generate_host(
        &self,
        _ir: &ValidatedIr,
        _files: &mut GeneratedFiles,
    ) -> Result<(), CodegenError> {
        Err(CodegenError::ValidationFailed {
            message: "Python generator not yet implemented (planned for Epic 10)".to_owned(),
        })
    }

    fn generate_guest(
        &self,
        _ir: &ValidatedIr,
        _files: &mut GeneratedFiles,
    ) -> Result<(), CodegenError> {
        Err(CodegenError::ValidationFailed {
            message: "Python generator not yet implemented (planned for Epic 10)".to_owned(),
        })
    }
}

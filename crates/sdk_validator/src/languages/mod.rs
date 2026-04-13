//! Language-specific validation modules.
//!
//! Each language has its own validator that uses ast-grep CLI to detect
//! method/function definitions in SDK files. Lua uses tree-sitter instead
//! since ast-grep doesn't support Lua.

pub mod cpp;
pub mod csharp;
pub mod js;
pub mod lua;
pub mod python;
pub mod rust;

pub use cpp::CppValidator;
pub use csharp::CSharpValidator;
pub use js::JsValidator;
pub use lua::LuaValidator;
pub use python::PythonValidator;
pub use rust::RustValidator;

use serde::Serialize;

use crate::ast_grep::{AstGrepRunner, Language, NamingConvention};

/// Result of validating a single struct's methods in a language SDK.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ValidationResult {
    /// Name of the struct being validated (e.g., "StringView").
    pub struct_name: String,
    /// Name of the language (e.g., "csharp").
    pub language: String,
    /// Methods that were found in the SDK.
    pub found_methods: Vec<String>,
    /// Methods that are missing from the SDK.
    pub missing_methods: Vec<String>,
}

impl ValidationResult {
    /// Create a new validation result.
    pub fn new(struct_name: String, language: String) -> Self {
        Self {
            struct_name,
            language,
            found_methods: Vec::new(),
            missing_methods: Vec::new(),
        }
    }

    /// Check if all required methods are present.
    #[allow(dead_code)]
    pub fn is_complete(&self) -> bool {
        self.missing_methods.is_empty()
    }

    /// Get the completion percentage (0-100).
    #[allow(dead_code)]
    pub fn completion_percentage(&self) -> u8 {
        let total: usize = self.found_methods.len() + self.missing_methods.len();
        if total == 0 {
            return 100;
        }
        let found: usize = self.found_methods.len();
        ((found * 100) / total) as u8
    }
}

/// Trait for language-specific SDK validators.
///
/// Each language implementation uses ast-grep CLI to detect method/function
/// definitions in SDK files and reports which methods are found or missing.
pub trait LanguageValidator {
    /// Get the language name for this validator.
    fn language_name(&self) -> &'static str;

    /// Get the ast-grep Language enum value.
    fn ast_grep_language(&self) -> Language;

    /// Get the naming convention for this language.
    fn naming_convention(&self) -> NamingConvention;

    /// Validate that the required methods exist in the SDK files.
    ///
    /// # Arguments
    ///
    /// * `runner` - The ast-grep runner to use for pattern matching.
    /// * `struct_name` - The name of the struct being validated (e.g., "StringView").
    /// * `required_methods` - The list of method names that should be present.
    /// * `target_files` - The SDK files to search for method definitions.
    ///
    /// # Returns
    ///
    /// A `ValidationResult` indicating which methods were found and which are missing.
    fn validate(
        &self,
        runner: &AstGrepRunner,
        struct_name: &str,
        required_methods: &[String],
        target_files: &[String],
    ) -> ValidationResult;
}

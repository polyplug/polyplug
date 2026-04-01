//! Language generators — item-by-item code generation.
//!
//! This module defines the `CodeGenerator` trait for generating language-specific
//! bindings from ABI items. Each generator processes one item at a time, enabling
//! fine-grained control and composability.

pub mod cpp;
pub mod csharp;
pub mod js;
pub mod lua;
pub mod python;

pub use cpp::CppGenerator;
pub use csharp::CSharpGenerator;
pub use js::JsGenerator;
pub use lua::LuaGenerator;
pub use python::PythonGenerator;

use crate::data::{ConstInfo, EnumInfo, FunctionInfo, StructInfo, UnionInfo};

/// Context passed to code generators during generation.
#[derive(Debug, Clone, Default)]
pub struct GenerationContext {
    /// Namespace or module name for the generated code.
    pub namespace: Option<String>,
    /// Whether to generate documentation comments.
    pub generate_docs: bool,
    /// Whether to generate helper functions (e.g., FNV-1a hash).
    pub generate_helpers: bool,
}

impl GenerationContext {
    /// Create a new generation context with default settings.
    pub fn new() -> Self {
        GenerationContext {
            namespace: None,
            generate_docs: true,
            generate_helpers: true,
        }
    }

    /// Set the namespace for generated code.
    pub fn with_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.namespace = Some(namespace.into());
        self
    }
}

/// Trait for language-specific code generators.
///
/// Each method generates code for a single ABI item. Implementations should
/// produce idiomatic code for the target language while maintaining ABI compatibility.
pub trait CodeGenerator {
    /// Generate code for a constant.
    fn generate_const(&self, item: &ConstInfo, ctx: &GenerationContext) -> String;

    /// Generate code for a struct.
    fn generate_struct(&self, item: &StructInfo, ctx: &GenerationContext) -> String;

    /// Generate code for an enum.
    fn generate_enum(&self, item: &EnumInfo, ctx: &GenerationContext) -> String;

    /// Generate code for a union.
    fn generate_union(&self, item: &UnionInfo, ctx: &GenerationContext) -> String;

    /// Generate code for a function.
    fn generate_function(&self, item: &FunctionInfo, ctx: &GenerationContext) -> String;

    /// Return the file extension for this language (e.g., "hpp", "cs", "py").
    fn file_extension(&self) -> &'static str;

    /// Return the language name for identification.
    fn language_name(&self) -> &'static str;

    /// Generate the file header (includes, imports, etc.).
    fn generate_header(&self, ctx: &GenerationContext) -> String;

    /// Generate the file footer (closing namespaces, etc.).
    fn generate_footer(&self, _ctx: &GenerationContext) -> String {
        String::new()
    }
}
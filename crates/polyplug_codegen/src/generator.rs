//! CodeGenerator trait for item-by-item ABI code generation.

use crate::context::GenerationContext;
use crate::data::{ConstInfo, EnumInfo, FieldInfo, FunctionInfo, Item, StructInfo, UnionInfo};

/// Trait for generating language-specific ABI bindings.
///
/// Implementations generate code for each ABI item type (const, struct, enum, union, function)
/// using the provided generation context for type mappings and formatting.
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

    /// Get the file extension for generated files.
    fn file_extension(&self) -> &'static str;

    /// Get the language name for headers/comments.
    fn language_name(&self) -> &'static str;

    /// Generate file header (auto-generated warning, includes, etc.).
    fn generate_header(&self, _ctx: &GenerationContext) -> String {
        String::new()
    }

    /// Generate file footer (namespace closes, etc.).
    fn generate_footer(&self, _ctx: &GenerationContext) -> String {
        String::new()
    }

    /// Generate code for a single item, dispatching to the appropriate method.
    fn generate_item(&self, item: &Item, ctx: &GenerationContext) -> String {
        match item {
            Item::Const(c) => self.generate_const(c, ctx),
            Item::Struct(s) => self.generate_struct(s, ctx),
            Item::Enum(e) => self.generate_enum(e, ctx),
            Item::Union(u) => self.generate_union(u, ctx),
            Item::Function(f) => self.generate_function(f, ctx),
        }
    }

    /// Generate a documentation comment for the target language.
    fn format_doc(&self, doc: &str, indent: usize, ctx: &GenerationContext) -> String {
        let indent_str: String = ctx.indent_str.repeat(indent);
        let lines: Vec<&str> = doc.lines().collect();
        if lines.len() == 1 {
            self.format_single_line_doc(&indent_str, lines[0])
        } else {
            self.format_multi_line_doc(&indent_str, &lines)
        }
    }

    /// Format a single-line documentation comment.
    fn format_single_line_doc(&self, indent_str: &str, line: &str) -> String;

    /// Format a multi-line documentation comment.
    fn format_multi_line_doc(&self, indent_str: &str, lines: &[&str]) -> String;

    /// Generate a field declaration for the target language.
    fn generate_field(&self, field: &FieldInfo, ctx: &GenerationContext) -> String;
}

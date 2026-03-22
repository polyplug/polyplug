//! ABI Code Generator — `AbiGenerator` trait and supporting types.
//!
//! This module provides the foundation for generating ABI bindings in multiple
//! target languages. Language-specific generators implement the `AbiGenerator`
//! trait to produce host and guest SDK code.

mod cpp;
mod csharp;
mod js;
mod lua;
mod parser;
mod python;

pub use cpp::CppGenerator;
pub use csharp::CSharpGenerator;
pub use js::JsGenerator;
pub use lua::LuaGenerator;
pub use parser::{AbiParser, ParseError};
pub use python::PythonGenerator;

use std::path::PathBuf;

// ─── Generated File Types ─────────────────────────────────────────────────────

/// A single generated file (path + content).
#[derive(Debug, Clone)]
pub struct GeneratedFile {
    /// Relative output path.
    pub path: PathBuf,
    /// Generated source code.
    pub content: String,
}

/// Collection of generated files.
#[derive(Debug, Default)]
pub struct GeneratedFiles {
    /// The generated files.
    pub files: Vec<GeneratedFile>,
}

impl GeneratedFiles {
    /// Create an empty collection.
    pub fn new() -> GeneratedFiles {
        GeneratedFiles::default()
    }

    /// Add a generated file.
    pub fn push(&mut self, file: GeneratedFile) {
        self.files.push(file);
    }
}

// ─── ABI Type Information ─────────────────────────────────────────────────────

/// Information about a constant extracted from the ABI.
#[derive(Debug, Clone)]
pub struct ConstantInfo {
    /// Constant name (e.g., "POLYPLUG_ABI_VERSION").
    pub name: String,
    /// Constant value as a string (for language-specific formatting).
    pub value: String,
    /// Constant type (e.g., "u32").
    pub type_name: String,
}

/// Information about a struct field extracted from the ABI.
#[derive(Debug, Clone)]
pub struct FieldInfo {
    /// Field name.
    pub name: String,
    /// Field type name.
    pub type_name: String,
    /// Optional documentation comment.
    pub doc: Option<String>,
}

/// Information about a struct extracted from the ABI.
#[derive(Debug, Clone)]
pub struct StructInfo {
    /// Struct name (e.g., "StringView").
    pub name: String,
    /// Struct fields.
    pub fields: Vec<FieldInfo>,
    /// Optional documentation comment.
    pub doc: Option<String>,
}

/// Information about an enum variant extracted from the ABI.
#[derive(Debug, Clone)]
pub struct VariantInfo {
    /// Variant name.
    pub name: String,
    /// Optional discriminant value.
    pub value: Option<i64>,
    /// Optional documentation comment.
    pub doc: Option<String>,
}

/// Information about an enum extracted from the ABI.
#[derive(Debug, Clone)]
pub struct EnumInfo {
    /// Enum name (e.g., "DispatchType").
    pub name: String,
    /// Enum variants.
    pub variants: Vec<VariantInfo>,
    /// Optional documentation comment.
    pub doc: Option<String>,
}

/// Information about a union variant extracted from the ABI.
#[derive(Debug, Clone)]
pub struct UnionVariantInfo {
    /// Variant name.
    pub name: String,
    /// Variant type name.
    pub type_name: String,
    /// Optional documentation comment.
    pub doc: Option<String>,
}

/// Information about a union extracted from the ABI.
#[derive(Debug, Clone)]
pub struct UnionInfo {
    /// Union name (e.g., "PluginDispatch").
    pub name: String,
    /// Union variants.
    pub variants: Vec<UnionVariantInfo>,
    /// Optional documentation comment.
    pub doc: Option<String>,
}

/// Information about a function extracted from the ABI.
#[derive(Debug, Clone)]
pub struct FunctionInfo {
    /// Function name.
    pub name: String,
    /// Return type name.
    pub return_type: String,
    /// Optional documentation comment.
    pub doc: Option<String>,
}

/// Complete ABI type information extracted from `polyplug_abi`.
///
/// This struct holds all the type information needed to generate bindings
/// in any target language. Language-specific generators use this data to
/// produce idiomatic code.
#[derive(Debug, Clone, Default)]
pub struct AbiInfo {
    /// ABI constants (version, error codes).
    pub constants: Vec<ConstantInfo>,
    /// ABI structs (StringView, Buffer, etc.).
    pub structs: Vec<StructInfo>,
    /// ABI enums (DispatchType).
    pub enums: Vec<EnumInfo>,
    /// ABI unions (PluginDispatch).
    pub unions: Vec<UnionInfo>,
    /// ABI functions (FNV-1a hash helpers).
    pub functions: Vec<FunctionInfo>,
}

impl AbiInfo {
    /// Create an empty `AbiInfo`.
    pub fn new() -> AbiInfo {
        AbiInfo::default()
    }

    /// Add a constant.
    pub fn add_constant(&mut self, constant: ConstantInfo) {
        self.constants.push(constant);
    }

    /// Add a struct.
    pub fn add_struct(&mut self, struct_info: StructInfo) {
        self.structs.push(struct_info);
    }

    /// Add an enum.
    pub fn add_enum(&mut self, enum_info: EnumInfo) {
        self.enums.push(enum_info);
    }

    /// Add a union.
    pub fn add_union(&mut self, union_info: UnionInfo) {
        self.unions.push(union_info);
    }

    /// Add a function.
    pub fn add_function(&mut self, function: FunctionInfo) {
        self.functions.push(function);
    }
}

// ─── AbiGenerator Trait ───────────────────────────────────────────────────────

/// Trait for language-specific ABI code generators.
///
/// Each target language (C#, Python, Lua, etc.) implements this trait to
/// generate idiomatic bindings for the polyplug ABI types.
///
/// # Example Implementation
///
/// ```ignore
/// struct CSharpGenerator;
///
/// impl AbiGenerator for CSharpGenerator {
///     fn generate_constants(&self, info: &AbiInfo) -> String {
///         // Generate C# constant definitions
///     }
///
///     fn generate_structs(&self, info: &AbiInfo) -> String {
///         // Generate C# struct definitions
///     }
///
///     // ... other methods
/// }
/// ```
pub trait AbiGenerator {
    /// Generate constant definitions for the target language.
    ///
    /// This includes ABI version, error codes, and other constants.
    fn generate_constants(&self, info: &AbiInfo) -> String;

    /// Generate struct definitions for the target language.
    ///
    /// This includes all `#[repr(C)]` structs from the ABI:
    /// StringView, Buffer, AbiError, PluginHandle, HostContext, etc.
    fn generate_structs(&self, info: &AbiInfo) -> String;

    /// Generate enum definitions for the target language.
    ///
    /// This includes DispatchType and any other C-style enums.
    fn generate_enums(&self, info: &AbiInfo) -> String;

    /// Generate union definitions for the target language.
    ///
    /// This includes PluginDispatch and any other unions.
    fn generate_unions(&self, info: &AbiInfo) -> String;

    /// Generate helper functions for the target language.
    ///
    /// This includes FNV-1a hash implementations and string helpers.
    fn generate_helpers(&self, info: &AbiInfo) -> String;

    /// Return the file extension for this language (e.g., "cs", "py", "lua").
    fn file_extension(&self) -> &'static str;

    /// Return the output directory name for this language (e.g., "csharp", "python").
    fn output_dir(&self) -> &'static str;

    /// Generate all ABI bindings and return the collection of files.
    ///
    /// The default implementation calls each generate_* method and combines
    /// the results into a single file. Implementations may override this
    /// to produce multiple files.
    fn generate(&self, info: &AbiInfo) -> GeneratedFiles {
        let mut files: GeneratedFiles = GeneratedFiles::new();

        let mut content: String = String::new();
        content.push_str(&self.generate_constants(info));
        content.push_str(&self.generate_structs(info));
        content.push_str(&self.generate_enums(info));
        content.push_str(&self.generate_unions(info));
        content.push_str(&self.generate_helpers(info));

        let filename: String = format!("abi.{}", self.file_extension());
        files.push(GeneratedFile {
            path: PathBuf::from(filename),
            content,
        });

        files
    }
}

// ─── Formatting Helpers ───────────────────────────────────────────────────────

/// Indent a string by the specified number of spaces.
///
/// # Arguments
/// * `s` - The string to indent
/// * `spaces` - Number of spaces to indent each line
///
/// # Returns
/// A new string with each line indented.
pub fn indent(s: &str, spaces: usize) -> String {
    let indent_str: String = " ".repeat(spaces);
    s.lines()
        .map(|line: &str| {
            if line.is_empty() {
                String::new()
            } else {
                format!("{}{}", indent_str, line)
            }
        })
        .collect::<Vec<String>>()
        .join("\n")
}

/// Convert a snake_case identifier to PascalCase.
///
/// # Arguments
/// * `s` - The snake_case string to convert
///
/// # Returns
/// A PascalCase string.
pub fn to_pascal_case(s: &str) -> String {
    s.split('_')
        .map(|word: &str| {
            let mut chars: core::str::Chars<'_> = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => {
                    let first_upper: String = first.to_uppercase().collect();
                    let rest_lower: String = chars.as_str().to_lowercase();
                    format!("{}{}", first_upper, rest_lower)
                }
            }
        })
        .collect()
}

/// Convert a PascalCase identifier to snake_case.
///
/// # Arguments
/// * `s` - The PascalCase string to convert
///
/// # Returns
/// A snake_case string.
pub fn to_snake_case(s: &str) -> String {
    let mut result: String = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                result.push('_');
            }
            result.push(c.to_lowercase().next().unwrap_or(c));
        } else {
            result.push(c);
        }
    }
    result
}

/// Generate a documentation comment for the target language.
///
/// # Arguments
/// * `doc` - The documentation text
/// * `prefix` - The comment prefix (e.g., "///", "//", "#")
///
/// # Returns
/// A formatted documentation comment string.
pub fn format_doc_comment(doc: &str, prefix: &str) -> String {
    doc.lines()
        .map(|line: &str| format!("{} {}", prefix, line))
        .collect::<Vec<String>>()
        .join("\n")
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_files_push() {
        let mut files: GeneratedFiles = GeneratedFiles::new();
        assert!(files.files.is_empty());

        files.push(GeneratedFile {
            path: PathBuf::from("test.cs"),
            content: String::from("// test"),
        });

        assert_eq!(files.files.len(), 1);
        assert_eq!(files.files[0].path, PathBuf::from("test.cs"));
    }

    #[test]
    fn abi_info_add_types() {
        let mut info: AbiInfo = AbiInfo::new();
        assert!(info.constants.is_empty());
        assert!(info.structs.is_empty());
        assert!(info.enums.is_empty());
        assert!(info.unions.is_empty());

        info.add_constant(ConstantInfo {
            name: String::from("ABI_OK"),
            value: String::from("0"),
            type_name: String::from("u32"),
        });
        assert_eq!(info.constants.len(), 1);

        info.add_struct(StructInfo {
            name: String::from("StringView"),
            fields: vec![],
            doc: None,
        });
        assert_eq!(info.structs.len(), 1);

        info.add_enum(EnumInfo {
            name: String::from("DispatchType"),
            variants: vec![],
            doc: None,
        });
        assert_eq!(info.enums.len(), 1);

        info.add_union(UnionInfo {
            name: String::from("PluginDispatch"),
            variants: vec![],
            doc: None,
        });
        assert_eq!(info.unions.len(), 1);
    }

    #[test]
    fn indent_empty_string() {
        let result: String = indent("", 4);
        assert!(result.is_empty());
    }

    #[test]
    fn indent_single_line() {
        let result: String = indent("hello", 4);
        assert_eq!(result, "    hello");
    }

    #[test]
    fn indent_multiple_lines() {
        let result: String = indent("line1\nline2\nline3", 2);
        assert_eq!(result, "  line1\n  line2\n  line3");
    }

    #[test]
    fn indent_preserves_empty_lines() {
        let result: String = indent("line1\n\nline3", 2);
        assert_eq!(result, "  line1\n\n  line3");
    }

    #[test]
    fn to_pascal_case_simple() {
        assert_eq!(to_pascal_case("hello_world"), "HelloWorld");
        assert_eq!(to_pascal_case("abi_error"), "AbiError");
        assert_eq!(to_pascal_case("string_view"), "StringView");
    }

    #[test]
    fn to_pascal_case_single_word() {
        assert_eq!(to_pascal_case("hello"), "Hello");
        assert_eq!(to_pascal_case("WORLD"), "World");
    }

    #[test]
    fn to_snake_case_simple() {
        assert_eq!(to_snake_case("HelloWorld"), "hello_world");
        assert_eq!(to_snake_case("AbiError"), "abi_error");
        assert_eq!(to_snake_case("StringView"), "string_view");
    }

    #[test]
    fn to_snake_case_single_word() {
        assert_eq!(to_snake_case("Hello"), "hello");
        assert_eq!(to_snake_case("WORLD"), "w_o_r_l_d");
    }

    #[test]
    fn format_doc_comment_single_line() {
        let result: String = format_doc_comment("Hello world", "///");
        assert_eq!(result, "/// Hello world");
    }

    #[test]
    fn format_doc_comment_multiple_lines() {
        let result: String = format_doc_comment("Line 1\nLine 2", "///");
        assert_eq!(result, "/// Line 1\n/// Line 2");
    }
}

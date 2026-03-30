//! ABI Code Generator — `AbiGenerator` trait and supporting types.
//!
//! This module provides the foundation for generating ABI bindings in multiple
//! target languages. Language-specific generators implement the `AbiGenerator`
//! trait to produce host and guest SDK code.

mod abi_generator;
mod abi_type_info;
mod laguages;
mod parser;
mod utils;

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

fn main() {}

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

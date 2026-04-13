//! Lua SDK validator using tree-sitter.
//!
//! This module provides validation for Lua SDK files, detecting
//! function definitions (global functions and table methods) that
//! match the required method names.

use std::collections::HashSet;
use std::path::Path;

use thiserror::Error;
use tree_sitter::Parser;

use crate::languages::ValidationResult;

#[derive(Debug, Error)]
pub enum LuaValidatorError {
    #[error("Failed to set tree-sitter language: {0}")]
    Language(String),

    #[error("Failed to parse Lua file: {path}")]
    #[allow(dead_code)]
    Parse {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("Failed to read file: {path}")]
    #[allow(dead_code)]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

/// Validator for Lua SDK files.
///
/// Detects function definitions in Lua source files using tree-sitter.
/// Supports both global functions (`function name()`) and table methods
/// (`function M.name()` or `function M:name()`).
pub struct LuaValidator {
    parser: Parser,
}

impl LuaValidator {
    /// Create a new Lua validator.
    pub fn new() -> Result<Self, LuaValidatorError> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_lua::LANGUAGE.into())
            .map_err(|e| LuaValidatorError::Language(e.to_string()))?;
        Ok(Self { parser })
    }

    /// Extract function names from Lua source code.
    fn extract_function_names(&mut self, source: &str) -> HashSet<String> {
        let mut function_names: HashSet<String> = HashSet::new();

        let tree = match self.parser.parse(source, None) {
            Some(tree) => tree,
            None => return function_names,
        };

        let root = tree.root_node();
        self.collect_function_names(&root, source, &mut function_names);

        function_names
    }

    /// Recursively collect function names from the AST.
    fn collect_function_names(
        &self,
        node: &tree_sitter::Node,
        source: &str,
        names: &mut HashSet<String>,
    ) {
        if node.kind() == "function_declaration" {
            if let Some(name_node) = node.child_by_field_name("name") {
                self.extract_function_name(&name_node, source, names);
            }
        } else if node.kind() == "function_definition" {
            // Anonymous function assigned to variable: local to_str = function() end
            // This is handled by the parent assignment
        }

        // Recurse into children
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.collect_function_names(&child, source, names);
        }
    }

    /// Extract function name from a name node.
    fn extract_function_name(
        &self,
        name_node: &tree_sitter::Node,
        source: &str,
        names: &mut HashSet<String>,
    ) {
        let name_text = name_node.utf8_text(source.as_bytes()).unwrap_or("");

        // Handle dot-separated names: M.to_str -> to_str
        // Handle colon-separated names: M:to_str -> to_str
        let final_name = if name_text.contains('.') {
            name_text.split('.').next_back().unwrap_or(name_text)
        } else if name_text.contains(':') {
            name_text.split(':').next_back().unwrap_or(name_text)
        } else {
            name_text
        };

        names.insert(final_name.to_string());
    }

    /// Validate that the required methods exist in the SDK files.
    pub fn validate(
        &mut self,
        struct_name: &str,
        required_methods: &[String],
        target_files: &[String],
    ) -> ValidationResult {
        let mut result =
            ValidationResult::new(struct_name.to_string(), self.language_name().to_string());

        // Collect all function names from all target files
        let mut all_function_names: HashSet<String> = HashSet::new();

        for file_path_str in target_files {
            let file_path: &Path = Path::new(file_path_str);

            if !file_path.exists() {
                continue;
            }

            let source: String = match std::fs::read_to_string(file_path) {
                Ok(s) => s,
                Err(_) => continue,
            };

            let function_names = self.extract_function_names(&source);
            all_function_names.extend(function_names);
        }

        // Check each required method
        for method_name in required_methods {
            if all_function_names.contains(method_name) {
                result.found_methods.push(method_name.clone());
            } else {
                result.missing_methods.push(method_name.clone());
            }
        }

        result
    }

    /// Get the language name for this validator.
    pub fn language_name(&self) -> &'static str {
        "lua"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_temp_lua_file(content: &str) -> NamedTempFile {
        let mut file = NamedTempFile::with_suffix(".lua").expect("Failed to create temp file");
        file.write_all(content.as_bytes())
            .expect("Failed to write temp file");
        file.flush().expect("Failed to flush temp file");
        file
    }

    #[test]
    fn test_lua_validator_new() {
        let validator: LuaValidator = LuaValidator::new().expect("Failed to create validator");
        assert_eq!(validator.language_name(), "lua");
    }

    #[test]
    fn test_lua_validator_detects_global_function() {
        let lua_code = r#"
function to_str(sv)
    return ""
end
"#;
        let file = create_temp_lua_file(lua_code);
        let mut validator = LuaValidator::new().expect("Failed to create validator");

        let result = validator.validate(
            "StringView",
            &["to_str".to_string()],
            &[file.path().to_string_lossy().to_string()],
        );

        assert!(result.found_methods.contains(&"to_str".to_string()));
        assert!(!result.missing_methods.contains(&"to_str".to_string()));
    }

    #[test]
    fn test_lua_validator_detects_table_method_dot() {
        let lua_code = r#"
local M = {}

function M.to_str(sv)
    return ""
end

return M
"#;
        let file = create_temp_lua_file(lua_code);
        let mut validator = LuaValidator::new().expect("Failed to create validator");

        let result = validator.validate(
            "StringView",
            &["to_str".to_string()],
            &[file.path().to_string_lossy().to_string()],
        );

        assert!(result.found_methods.contains(&"to_str".to_string()));
    }

    #[test]
    fn test_lua_validator_detects_table_method_colon() {
        let lua_code = r#"
local M = {}

function M:to_str()
    return ""
end

return M
"#;
        let file = create_temp_lua_file(lua_code);
        let mut validator = LuaValidator::new().expect("Failed to create validator");

        let result = validator.validate(
            "StringView",
            &["to_str".to_string()],
            &[file.path().to_string_lossy().to_string()],
        );

        assert!(result.found_methods.contains(&"to_str".to_string()));
    }

    #[test]
    fn test_lua_validator_detects_multiple_methods() {
        let lua_code = r#"
local M = {}

function M.to_str(sv)
    return ""
end

function M.starts_with(sv, prefix)
    return true
end

function M.strip_prefix(sv, prefix)
    return ""
end

function M.split(sv, delimiter)
    return {}
end

return M
"#;
        let file = create_temp_lua_file(lua_code);
        let mut validator = LuaValidator::new().expect("Failed to create validator");

        let result = validator.validate(
            "StringView",
            &[
                "to_str".to_string(),
                "starts_with".to_string(),
                "ends_with".to_string(),
                "strip_prefix".to_string(),
                "split".to_string(),
            ],
            &[file.path().to_string_lossy().to_string()],
        );

        assert!(result.found_methods.contains(&"to_str".to_string()));
        assert!(result.found_methods.contains(&"starts_with".to_string()));
        assert!(result.found_methods.contains(&"strip_prefix".to_string()));
        assert!(result.found_methods.contains(&"split".to_string()));
        assert!(result.missing_methods.contains(&"ends_with".to_string()));
        assert!(!result.is_complete());
    }

    #[test]
    fn test_lua_validator_missing_method() {
        let lua_code = r#"
function to_str(sv)
    return ""
end
"#;
        let file = create_temp_lua_file(lua_code);
        let mut validator = LuaValidator::new().expect("Failed to create validator");

        let result = validator.validate(
            "StringView",
            &["ends_with".to_string()],
            &[file.path().to_string_lossy().to_string()],
        );

        assert!(result.missing_methods.contains(&"ends_with".to_string()));
        assert!(!result.is_complete());
    }

    #[test]
    fn test_lua_validator_nonexistent_file() {
        let mut validator = LuaValidator::new().expect("Failed to create validator");

        let result = validator.validate(
            "StringView",
            &["to_str".to_string()],
            &["/nonexistent/file.lua".to_string()],
        );

        assert!(result.missing_methods.contains(&"to_str".to_string()));
    }

    #[test]
    fn test_lua_validator_completion_percentage() {
        let lua_code = r#"
function to_str(sv)
    return ""
end

function starts_with(sv, prefix)
    return true
end
"#;
        let file = create_temp_lua_file(lua_code);
        let mut validator = LuaValidator::new().expect("Failed to create validator");

        let result = validator.validate(
            "StringView",
            &[
                "to_str".to_string(),
                "starts_with".to_string(),
                "ends_with".to_string(),
            ],
            &[file.path().to_string_lossy().to_string()],
        );

        assert_eq!(result.completion_percentage(), 66);
    }

    #[test]
    fn test_lua_validator_all_methods_found() {
        let lua_code = r#"
function to_str(sv)
    return ""
end

function starts_with(sv, prefix)
    return true
end
"#;
        let file = create_temp_lua_file(lua_code);
        let mut validator = LuaValidator::new().expect("Failed to create validator");

        let result = validator.validate(
            "StringView",
            &["to_str".to_string(), "starts_with".to_string()],
            &[file.path().to_string_lossy().to_string()],
        );

        assert!(result.is_complete());
        assert_eq!(result.completion_percentage(), 100);
    }

    #[test]
    fn test_lua_validator_local_function() {
        let lua_code = r#"
local function to_str(sv)
    return ""
end
"#;
        let file = create_temp_lua_file(lua_code);
        let mut validator = LuaValidator::new().expect("Failed to create validator");

        let result = validator.validate(
            "StringView",
            &["to_str".to_string()],
            &[file.path().to_string_lossy().to_string()],
        );

        assert!(result.found_methods.contains(&"to_str".to_string()));
    }

    #[test]
    fn test_lua_validator_nested_table_method() {
        let lua_code = r#"
local helpers = {}

function helpers.string.to_str(sv)
    return ""
end
"#;
        let file = create_temp_lua_file(lua_code);
        let mut validator = LuaValidator::new().expect("Failed to create validator");

        let result = validator.validate(
            "StringView",
            &["to_str".to_string()],
            &[file.path().to_string_lossy().to_string()],
        );

        assert!(result.found_methods.contains(&"to_str".to_string()));
    }
}

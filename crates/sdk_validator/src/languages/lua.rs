//! Lua SDK validator using tree-sitter.
//!
//! ast-grep does not support Lua, so this validator walks the tree-sitter
//! AST directly. It detects:
//! - declarations: `function name()`, `function M.name()`, `function M:name()`,
//!   `local function name()`, nested names (`function helpers.string.name()`)
//! - assignments: `M.name = function(...) end`, `name = function(...) end`,
//!   `local name = function(...) end`

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use tree_sitter::{Node, Parser, Tree};

use crate::ast_grep::AstGrepRunner;
use crate::error::ValidatorError;
use crate::languages::LanguageValidator;

/// Validator for Lua SDK files.
pub struct LuaValidator {
    parser: Parser,
    /// Function names per already-parsed file.
    names_by_file: HashMap<PathBuf, HashSet<String>>,
}

impl LuaValidator {
    /// Create a new Lua validator.
    ///
    /// # Errors
    ///
    /// Returns [`ValidatorError::LuaInit`] if the tree-sitter Lua grammar
    /// cannot be loaded.
    pub fn new() -> Result<Self, ValidatorError> {
        let mut parser: Parser = Parser::new();
        parser
            .set_language(&tree_sitter_lua::LANGUAGE.into())
            .map_err(|e| ValidatorError::LuaInit {
                message: e.to_string(),
            })?;
        Ok(Self {
            parser,
            names_by_file: HashMap::new(),
        })
    }

    /// Parse a file and extract all function names defined in it.
    fn extract_function_names(&mut self, file: &Path) -> Result<HashSet<String>, ValidatorError> {
        let source: String =
            std::fs::read_to_string(file).map_err(|source| ValidatorError::FileRead {
                path: file.to_path_buf(),
                source,
            })?;

        let tree: Tree =
            self.parser
                .parse(&source, None)
                .ok_or_else(|| ValidatorError::LuaParse {
                    path: file.to_path_buf(),
                })?;

        let mut names: HashSet<String> = HashSet::new();
        collect_function_names(&tree.root_node(), &source, &mut names);
        Ok(names)
    }
}

/// Recursively collect function names from the AST.
fn collect_function_names(node: &Node<'_>, source: &str, names: &mut HashSet<String>) {
    if node.kind() == "function_declaration" {
        if let Some(name_node) = node.child_by_field_name("name") {
            insert_function_name(&name_node, source, names);
        }
    } else if node.kind() == "assignment_statement" {
        collect_assigned_functions(node, source, names);
    }

    let mut cursor: tree_sitter::TreeCursor<'_> = node.walk();
    for child in node.children(&mut cursor) {
        collect_function_names(&child, source, names);
    }
}

/// Collect names from `M.name = function(...) end` style assignments.
///
/// Pairs the i-th variable with the i-th value and records the variable name
/// when the assigned value is a `function_definition`. This also covers
/// `local name = function(...) end`, since tree-sitter-lua wraps local
/// declarations in a `variable_declaration` containing an
/// `assignment_statement`.
fn collect_assigned_functions(node: &Node<'_>, source: &str, names: &mut HashSet<String>) {
    let mut variables: Vec<Node<'_>> = Vec::new();
    let mut values: Vec<Node<'_>> = Vec::new();

    let mut cursor: tree_sitter::TreeCursor<'_> = node.walk();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "variable_list" => {
                let mut list_cursor: tree_sitter::TreeCursor<'_> = child.walk();
                variables.extend(child.children_by_field_name("name", &mut list_cursor));
            }
            "expression_list" => {
                let mut list_cursor: tree_sitter::TreeCursor<'_> = child.walk();
                values.extend(child.children_by_field_name("value", &mut list_cursor));
            }
            _ => {}
        }
    }

    for (variable, value) in variables.iter().zip(values.iter()) {
        if value.kind() == "function_definition" {
            insert_function_name(variable, source, names);
        }
    }
}

/// Extract the final name segment from a name node and record it.
///
/// Handles `M.to_str` and `M:to_str` by taking the last segment.
fn insert_function_name(name_node: &Node<'_>, source: &str, names: &mut HashSet<String>) {
    // Source comes from read_to_string, so it is valid UTF-8.
    let name_text: &str = name_node.utf8_text(source.as_bytes()).unwrap_or("");

    let final_name: &str = if name_text.contains('.') {
        name_text.split('.').next_back().unwrap_or(name_text)
    } else if name_text.contains(':') {
        name_text.split(':').next_back().unwrap_or(name_text)
    } else {
        name_text
    };

    if !final_name.is_empty() {
        names.insert(final_name.to_string());
    }
}

impl LanguageValidator for LuaValidator {
    fn language_name(&self) -> &'static str {
        "lua"
    }

    fn method_in_file(
        &mut self,
        _runner: &AstGrepRunner,
        native_name: &str,
        file: &Path,
    ) -> Result<bool, ValidatorError> {
        if !self.names_by_file.contains_key(file) {
            let names: HashSet<String> = self.extract_function_names(file)?;
            self.names_by_file.insert(file.to_path_buf(), names);
        }
        Ok(self
            .names_by_file
            .get(file)
            .map(|names| names.contains(native_name))
            .unwrap_or(false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    use crate::ast_grep::NamingConvention;
    use crate::languages::test_support::{golden_methods, repo_path, runner};
    use crate::languages::{ValidationResult, validate_language};

    fn create_temp_lua_file(content: &str) -> Result<NamedTempFile, Box<dyn core::error::Error>> {
        let mut file: NamedTempFile = NamedTempFile::with_suffix(".lua")?;
        file.write_all(content.as_bytes())?;
        file.flush()?;
        Ok(file)
    }

    fn validate_file(
        methods: &[String],
        file: &Path,
    ) -> Result<ValidationResult, Box<dyn core::error::Error>> {
        let mut validator: LuaValidator = LuaValidator::new()?;
        let result: ValidationResult = validate_language(
            &mut validator,
            &runner(),
            NamingConvention::Snake,
            "StringView",
            methods,
            &[file.to_path_buf()],
        )?;
        Ok(result)
    }

    #[test]
    fn test_detects_global_function() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_lua_file(
            r#"
function to_str(sv)
    return ""
end
"#,
        )?;
        let result: ValidationResult = validate_file(&["to_str".to_string()], file.path())?;
        assert!(result.found_methods.contains(&"to_str".to_string()));
        Ok(())
    }

    #[test]
    fn test_detects_table_method_dot() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_lua_file(
            r#"
local M = {}
function M.to_str(sv)
    return ""
end
return M
"#,
        )?;
        let result: ValidationResult = validate_file(&["to_str".to_string()], file.path())?;
        assert!(result.found_methods.contains(&"to_str".to_string()));
        Ok(())
    }

    #[test]
    fn test_detects_table_method_colon() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_lua_file(
            r#"
local M = {}
function M:to_str()
    return ""
end
return M
"#,
        )?;
        let result: ValidationResult = validate_file(&["to_str".to_string()], file.path())?;
        assert!(result.found_methods.contains(&"to_str".to_string()));
        Ok(())
    }

    #[test]
    fn test_detects_local_function() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_lua_file(
            r#"
local function to_str(sv)
    return ""
end
"#,
        )?;
        let result: ValidationResult = validate_file(&["to_str".to_string()], file.path())?;
        assert!(result.found_methods.contains(&"to_str".to_string()));
        Ok(())
    }

    #[test]
    fn test_detects_nested_table_method() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_lua_file(
            r#"
local helpers = {}
function helpers.string.to_str(sv)
    return ""
end
"#,
        )?;
        let result: ValidationResult = validate_file(&["to_str".to_string()], file.path())?;
        assert!(result.found_methods.contains(&"to_str".to_string()));
        Ok(())
    }

    #[test]
    fn test_detects_assignment_form() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_lua_file(
            r#"
local M = {}
M.to_str = function(sv)
    return ""
end
starts_with = function(sv, prefix)
    return true
end
local ends_with = function(sv, suffix)
    return false
end
return M
"#,
        )?;
        let result: ValidationResult = validate_file(
            &[
                "to_str".to_string(),
                "starts_with".to_string(),
                "ends_with".to_string(),
            ],
            file.path(),
        )?;
        assert!(result.found_methods.contains(&"to_str".to_string()));
        assert!(result.found_methods.contains(&"starts_with".to_string()));
        assert!(result.found_methods.contains(&"ends_with".to_string()));
        Ok(())
    }

    #[test]
    fn test_assignment_of_non_function_does_not_match() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_lua_file(
            r#"
local M = {}
M.to_str = "not a function"
return M
"#,
        )?;
        let result: ValidationResult = validate_file(&["to_str".to_string()], file.path())?;
        assert!(result.found_methods.is_empty());
        Ok(())
    }

    #[test]
    fn test_renamed_definition_does_not_match() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_lua_file(
            r#"
local M = {}
function M.to_str2(sv)
    return ""
end
return M
"#,
        )?;
        let result: ValidationResult = validate_file(&["to_str".to_string()], file.path())?;
        assert!(result.found_methods.is_empty());
        Ok(())
    }

    #[test]
    fn test_call_site_only_does_not_match() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_lua_file(
            r#"
local M = {}
function M.other(sv)
    local s = to_str(sv)
    return s
end
return M
"#,
        )?;
        let result: ValidationResult = validate_file(&["to_str".to_string()], file.path())?;
        assert!(result.found_methods.is_empty());
        Ok(())
    }

    #[test]
    fn test_comment_only_does_not_match() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_lua_file(
            r#"
-- to_str(sv) converts a StringView
-- function M.to_str(sv) end
local M = {}
return M
"#,
        )?;
        let result: ValidationResult = validate_file(&["to_str".to_string()], file.path())?;
        assert!(result.found_methods.is_empty());
        Ok(())
    }

    #[test]
    fn test_unreadable_file_is_error() -> Result<(), Box<dyn core::error::Error>> {
        let mut validator: LuaValidator = LuaValidator::new()?;
        // Bypass validate_language's existence check to exercise FileRead.
        let result: Result<bool, ValidatorError> =
            validator.method_in_file(&runner(), "to_str", Path::new("/nonexistent/file.lua"));
        assert!(matches!(result, Err(ValidatorError::FileRead { .. })));
        Ok(())
    }

    #[test]
    fn test_real_sdk_has_all_golden_methods() -> Result<(), Box<dyn core::error::Error>> {
        let sdk_path: PathBuf = repo_path("sdks/lua/abi/abi.lua");
        let result: ValidationResult = validate_file(&golden_methods(), &sdk_path)?;
        assert!(
            result.is_complete(),
            "lua SDK missing methods: {:?}",
            result.missing_methods
        );
        assert_eq!(result.found_methods.len(), 5);
        Ok(())
    }
}

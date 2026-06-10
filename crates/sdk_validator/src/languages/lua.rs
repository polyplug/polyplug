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

/// Collect `(name, value)` pairs from a table constructor assigned to a
/// variable whose final name segment is `enum_name`
/// (`M.AbiErrorCode = { Ok = 0, ... }`).
///
/// Pairs the i-th variable with the i-th value (mirroring
/// [`collect_assigned_functions`]) and walks the matching table's `field`
/// nodes. Comments inside the table are separate nodes and never counted.
fn collect_enum_table_variants(
    node: &Node<'_>,
    source: &str,
    enum_name: &str,
    variants: &mut Vec<(String, Option<i64>)>,
) {
    if node.kind() == "assignment_statement" {
        let mut assigned_variables: Vec<Node<'_>> = Vec::new();
        let mut assigned_values: Vec<Node<'_>> = Vec::new();

        let mut cursor: tree_sitter::TreeCursor<'_> = node.walk();
        for child in node.named_children(&mut cursor) {
            match child.kind() {
                "variable_list" => {
                    let mut list_cursor: tree_sitter::TreeCursor<'_> = child.walk();
                    assigned_variables
                        .extend(child.children_by_field_name("name", &mut list_cursor));
                }
                "expression_list" => {
                    let mut list_cursor: tree_sitter::TreeCursor<'_> = child.walk();
                    assigned_values.extend(child.children_by_field_name("value", &mut list_cursor));
                }
                _ => {}
            }
        }

        for (variable, value) in assigned_variables.iter().zip(assigned_values.iter()) {
            if value.kind() != "table_constructor" {
                continue;
            }
            let variable_text: &str = variable.utf8_text(source.as_bytes()).unwrap_or("");
            let final_segment: &str = variable_text
                .rsplit(['.', ':'])
                .next()
                .unwrap_or(variable_text);
            if final_segment == enum_name {
                collect_table_fields(value, source, variants);
            }
        }
    }

    let mut cursor: tree_sitter::TreeCursor<'_> = node.walk();
    for child in node.children(&mut cursor) {
        collect_enum_table_variants(&child, source, enum_name, variants);
    }
}

/// Record every named `field` of a table constructor as a variant entry.
fn collect_table_fields(table: &Node<'_>, source: &str, variants: &mut Vec<(String, Option<i64>)>) {
    let mut cursor: tree_sitter::TreeCursor<'_> = table.walk();
    for field in table.named_children(&mut cursor) {
        if field.kind() != "field" {
            continue;
        }
        let Some(name_node) = field.child_by_field_name("name") else {
            continue;
        };
        let name: &str = name_node.utf8_text(source.as_bytes()).unwrap_or("");
        if name.is_empty() {
            continue;
        }
        let value: Option<i64> = field
            .child_by_field_name("value")
            .and_then(|value_node| value_node.utf8_text(source.as_bytes()).ok())
            .and_then(|text| text.trim().parse::<i64>().ok());
        variants.push((name.to_string(), value));
    }
}

/// Collect `EnumName_Variant = value,` lines from `ffi.cdef[[...]]` C enum
/// text. tree-sitter sees the cdef body as one string literal, so this is a
/// deliberate text-level parse — acceptable because the cdef file is
/// generated (defense-in-depth target).
///
/// Only lines that BEGIN (after whitespace) with the `EnumName_` prefix
/// count, so commented-out variants (`// EnumName_X = 1,`) and prose
/// mentioning the enum never match.
fn collect_cdef_variants(source: &str, enum_name: &str, variants: &mut Vec<(String, Option<i64>)>) {
    let prefix: String = format!("{enum_name}_");
    for line in source.lines() {
        let trimmed: &str = line.trim();
        let Some(rest) = trimmed.strip_prefix(prefix.as_str()) else {
            continue;
        };
        let name_len: usize = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .count();
        if name_len == 0 {
            continue;
        }
        let name: &str = &rest[..name_len];
        let after_name: &str = rest[name_len..].trim_start();
        let Some(value_text) = after_name.strip_prefix('=') else {
            continue;
        };
        let value: Option<i64> = value_text
            .trim()
            .trim_end_matches(',')
            .trim()
            .parse::<i64>()
            .ok();
        variants.push((name.to_string(), value));
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

    fn enum_variants_in_file(
        &mut self,
        _runner: &AstGrepRunner,
        enum_name: &str,
        file: &Path,
    ) -> Result<Vec<(String, Option<i64>)>, ValidatorError> {
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

        let mut variants: Vec<(String, Option<i64>)> = Vec::new();
        collect_enum_table_variants(&tree.root_node(), &source, enum_name, &mut variants);
        if variants.is_empty() {
            collect_cdef_variants(&source, enum_name, &mut variants);
        }
        Ok(variants)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    use crate::ast_grep::NamingConvention;
    use crate::languages::test_support::{golden_enum, golden_methods, repo_path, runner};
    use crate::languages::{
        EnumValidationResult, ValidationResult, VariantCheck, VariantOutcome, validate_language,
        validate_language_enum,
    };

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

    fn validate_enum_file(
        enum_name: &str,
        file: &Path,
    ) -> Result<EnumValidationResult, Box<dyn core::error::Error>> {
        let mut validator: LuaValidator = LuaValidator::new()?;
        let result: EnumValidationResult = validate_language_enum(
            &mut validator,
            &runner(),
            enum_name,
            &golden_enum(enum_name),
            &[file.to_path_buf()],
        )?;
        Ok(result)
    }

    #[test]
    fn test_enum_table_exact_match_passes() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_lua_file(
            r#"
local M = {}
M.DispatchType = {
    Native = 0,
    VirtualMachine = 1,
}
return M
"#,
        )?;
        let result: EnumValidationResult = validate_enum_file("DispatchType", file.path())?;
        assert!(result.is_complete(), "unexpected drift: {result:?}");
        Ok(())
    }

    #[test]
    fn test_enum_table_wrong_value_fails_with_expected_vs_found()
    -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_lua_file(
            r#"
local M = {}
M.DispatchType = {
    Native = 0,
    VirtualMachine = 6,
}
return M
"#,
        )?;
        let result: EnumValidationResult = validate_enum_file("DispatchType", file.path())?;
        let check: &VariantCheck = result
            .checks
            .iter()
            .find(|c| c.variant == "VirtualMachine")
            .ok_or("missing VirtualMachine check")?;
        assert_eq!(check.expected, 1);
        assert_eq!(check.outcome, VariantOutcome::WrongValue { found: 6 });
        Ok(())
    }

    #[test]
    fn test_enum_table_missing_and_extra_variants_fail() -> Result<(), Box<dyn core::error::Error>>
    {
        let file: NamedTempFile = create_temp_lua_file(
            r#"
local M = {}
M.DispatchType = {
    Native = 0,
    Stale = 2,
}
return M
"#,
        )?;
        let result: EnumValidationResult = validate_enum_file("DispatchType", file.path())?;
        let check: &VariantCheck = result
            .checks
            .iter()
            .find(|c| c.variant == "VirtualMachine")
            .ok_or("missing VirtualMachine check")?;
        assert_eq!(check.outcome, VariantOutcome::Missing);
        assert_eq!(result.extra_variants.len(), 1);
        assert_eq!(result.extra_variants[0].variant, "Stale");
        Ok(())
    }

    #[test]
    fn test_enum_table_commented_out_variant_does_not_count()
    -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_lua_file(
            r#"
-- DispatchType has VirtualMachine = 1 per the ABI.
local M = {}
local doc = "VirtualMachine = 1"
M.DispatchType = {
    Native = 0,
    -- VirtualMachine = 1,
}
return M
"#,
        )?;
        let result: EnumValidationResult = validate_enum_file("DispatchType", file.path())?;
        let check: &VariantCheck = result
            .checks
            .iter()
            .find(|c| c.variant == "VirtualMachine")
            .ok_or("missing VirtualMachine check")?;
        assert_eq!(check.outcome, VariantOutcome::Missing);
        assert!(result.extra_variants.is_empty());
        Ok(())
    }

    #[test]
    fn test_enum_table_other_table_is_not_confused() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_lua_file(
            r#"
local M = {}
M.Other = {
    Native = 7,
    VirtualMachine = 8,
}
M.DispatchType = {
    Native = 0,
    VirtualMachine = 1,
}
return M
"#,
        )?;
        let result: EnumValidationResult = validate_enum_file("DispatchType", file.path())?;
        assert!(result.is_complete(), "unexpected drift: {result:?}");
        Ok(())
    }

    #[test]
    fn test_enum_cdef_exact_match_passes() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_lua_file(
            r#"
local ffi = require("ffi")
ffi.cdef[[
    typedef enum DispatchType {
        DispatchType_Native = 0,
        DispatchType_VirtualMachine = 1,
    } DispatchType;
]]
"#,
        )?;
        let result: EnumValidationResult = validate_enum_file("DispatchType", file.path())?;
        assert!(result.is_complete(), "unexpected drift: {result:?}");
        Ok(())
    }

    #[test]
    fn test_enum_cdef_wrong_value_fails_with_expected_vs_found()
    -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_lua_file(
            r#"
local ffi = require("ffi")
ffi.cdef[[
    typedef enum DispatchType {
        DispatchType_Native = 0,
        DispatchType_VirtualMachine = 9,
    } DispatchType;
]]
"#,
        )?;
        let result: EnumValidationResult = validate_enum_file("DispatchType", file.path())?;
        let check: &VariantCheck = result
            .checks
            .iter()
            .find(|c| c.variant == "VirtualMachine")
            .ok_or("missing VirtualMachine check")?;
        assert_eq!(check.expected, 1);
        assert_eq!(check.outcome, VariantOutcome::WrongValue { found: 9 });
        Ok(())
    }

    #[test]
    fn test_enum_cdef_missing_and_extra_variants_fail() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_lua_file(
            r#"
local ffi = require("ffi")
ffi.cdef[[
    typedef enum DispatchType {
        DispatchType_Native = 0,
        DispatchType_Stale = 2,
    } DispatchType;
]]
"#,
        )?;
        let result: EnumValidationResult = validate_enum_file("DispatchType", file.path())?;
        let check: &VariantCheck = result
            .checks
            .iter()
            .find(|c| c.variant == "VirtualMachine")
            .ok_or("missing VirtualMachine check")?;
        assert_eq!(check.outcome, VariantOutcome::Missing);
        assert_eq!(result.extra_variants.len(), 1);
        assert_eq!(result.extra_variants[0].variant, "Stale");
        Ok(())
    }

    #[test]
    fn test_enum_cdef_commented_out_variant_does_not_count()
    -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_lua_file(
            r#"
local ffi = require("ffi")
ffi.cdef[[
    //  DispatchType_VirtualMachine = 1 is documented here only.
    typedef enum DispatchType {
        DispatchType_Native = 0,
        // DispatchType_VirtualMachine = 1,
    } DispatchType;
]]
"#,
        )?;
        let result: EnumValidationResult = validate_enum_file("DispatchType", file.path())?;
        let check: &VariantCheck = result
            .checks
            .iter()
            .find(|c| c.variant == "VirtualMachine")
            .ok_or("missing VirtualMachine check")?;
        assert_eq!(check.outcome, VariantOutcome::Missing);
        assert!(result.extra_variants.is_empty());
        Ok(())
    }

    #[test]
    fn test_real_abi_cdef_matches_golden_enums() -> Result<(), Box<dyn core::error::Error>> {
        let path: PathBuf = repo_path("sdks/lua/abi/abi.lua");
        for enum_name in [
            "AbiErrorCode",
            "LogLevel",
            "DispatchType",
            "ReloadPhaseType",
        ] {
            let result: EnumValidationResult = validate_enum_file(enum_name, &path)?;
            assert!(result.is_complete(), "{enum_name} drift: {result:?}");
        }
        Ok(())
    }

    #[test]
    fn test_real_guest_tables_match_golden_enums() -> Result<(), Box<dyn core::error::Error>> {
        let path: PathBuf = repo_path("sdks/lua/guest/polyplug_guest.lua");
        for enum_name in ["AbiErrorCode", "DispatchType", "LogLevel"] {
            let result: EnumValidationResult = validate_enum_file(enum_name, &path)?;
            assert!(result.is_complete(), "{enum_name} drift: {result:?}");
        }
        Ok(())
    }
}

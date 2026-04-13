//! Result aggregation for SDK validation.
//!
//! This module provides functionality to aggregate validation results from
//! all language validators into a comprehensive report.

use std::collections::HashMap;

use serde::Serialize;

use crate::ast_grep::AstGrepRunner;
use crate::config::Config;
use crate::languages::{
    CSharpValidator, CppValidator, JsValidator, LanguageValidator, LuaValidator, PythonValidator,
    RustValidator, ValidationResult,
};

/// Status of a method across all language SDKs.
///
/// Tracks which language SDKs have implemented the method and which are missing it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MethodStatus {
    /// Languages where the method was found.
    pub found_in: Vec<String>,
    /// Languages where the method is missing.
    pub missing_in: Vec<String>,
}

impl MethodStatus {
    /// Create a new empty method status.
    pub fn new() -> Self {
        Self {
            found_in: Vec::new(),
            missing_in: Vec::new(),
        }
    }

    /// Check if the method is implemented in all languages.
    pub fn is_complete(&self) -> bool {
        self.missing_in.is_empty()
    }

    /// Get the completion percentage (0-100).
    #[allow(dead_code)]
    pub fn completion_percentage(&self) -> u8 {
        let total: usize = self.found_in.len() + self.missing_in.len();
        if total == 0 {
            return 100;
        }
        let found: usize = self.found_in.len();
        ((found * 100) / total) as u8
    }
}

impl Default for MethodStatus {
    fn default() -> Self {
        Self::new()
    }
}

/// Report for a single struct across all language SDKs.
///
/// Contains the status of each method and the overall completion percentage.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StructReport {
    /// Method name -> status across languages.
    pub methods: HashMap<String, MethodStatus>,
    /// Overall completion percentage (0-100).
    pub completion_percentage: f64,
}

impl StructReport {
    /// Create a new empty struct report.
    pub fn new() -> Self {
        Self {
            methods: HashMap::new(),
            completion_percentage: 100.0,
        }
    }

    /// Calculate the completion percentage from the method statuses.
    pub fn calculate_completion(&mut self) {
        if self.methods.is_empty() {
            self.completion_percentage = 100.0;
            return;
        }

        let total_found: usize = self
            .methods
            .values()
            .map(|status| status.found_in.len())
            .sum();

        let total_missing: usize = self
            .methods
            .values()
            .map(|status| status.missing_in.len())
            .sum();

        let total: usize = total_found + total_missing;
        if total == 0 {
            self.completion_percentage = 100.0;
        } else {
            self.completion_percentage = (total_found as f64 / total as f64) * 100.0;
        }
    }
}

impl Default for StructReport {
    fn default() -> Self {
        Self::new()
    }
}

/// Report for a single language SDK across all structs.
///
/// Contains the validation results for each struct and the overall completion percentage.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LanguageReport {
    /// Struct name -> validation result.
    pub structs: HashMap<String, ValidationResult>,
    /// Overall completion percentage (0-100).
    pub completion_percentage: f64,
}

impl LanguageReport {
    /// Create a new empty language report.
    pub fn new() -> Self {
        Self {
            structs: HashMap::new(),
            completion_percentage: 100.0,
        }
    }

    /// Calculate the completion percentage from the struct results.
    pub fn calculate_completion(&mut self) {
        if self.structs.is_empty() {
            self.completion_percentage = 100.0;
            return;
        }

        let total_found: usize = self
            .structs
            .values()
            .map(|result| result.found_methods.len())
            .sum();

        let total_missing: usize = self
            .structs
            .values()
            .map(|result| result.missing_methods.len())
            .sum();

        let total: usize = total_found + total_missing;
        if total == 0 {
            self.completion_percentage = 100.0;
        } else {
            self.completion_percentage = (total_found as f64 / total as f64) * 100.0;
        }
    }
}

impl Default for LanguageReport {
    fn default() -> Self {
        Self::new()
    }
}

/// Aggregated validation report across all language SDKs.
///
/// Contains overall status, per-struct reports, and per-language reports.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ValidationReport {
    /// Whether all methods are implemented in all languages.
    pub is_complete: bool,
    /// Total number of unique methods across all structs.
    pub total_methods: usize,
    /// Total number of method implementations found across all languages.
    pub found_methods: usize,
    /// Per-struct reports: struct name -> report.
    pub per_struct: HashMap<String, StructReport>,
    /// Per-language reports: language name -> report.
    pub per_language: HashMap<String, LanguageReport>,
}

impl ValidationReport {
    /// Create a new empty validation report.
    pub fn new() -> Self {
        Self {
            is_complete: true,
            total_methods: 0,
            found_methods: 0,
            per_struct: HashMap::new(),
            per_language: HashMap::new(),
        }
    }
}

impl Default for ValidationReport {
    fn default() -> Self {
        Self::new()
    }
}

/// Aggregate validation results from all language validators.
///
/// This function runs all 6 language validators (Rust, Python, C#, C++, JS, Lua)
/// and aggregates their results into a comprehensive `ValidationReport`.
///
/// # Arguments
///
/// * `config` - The configuration containing methods, naming conventions, and target files.
/// * `runner` - The ast-grep runner to use for validation.
///
/// # Returns
///
/// A `ValidationReport` containing:
/// - Overall completion status
/// - Per-struct reports with method statuses
/// - Per-language reports with struct results
pub fn aggregate_results(config: &Config, runner: &AstGrepRunner) -> ValidationReport {
    let mut report: ValidationReport = ValidationReport::new();

    // Initialize validators
    let rust_validator: RustValidator = RustValidator::new();
    let python_validator: PythonValidator = PythonValidator::new();
    let csharp_validator: CSharpValidator = CSharpValidator::new();
    let cpp_validator: CppValidator = CppValidator::new();
    let js_validator: JsValidator = JsValidator::new();

    // Lua validator requires initialization, handle gracefully
    let mut lua_validator: Option<LuaValidator> = LuaValidator::new().ok();

    // Collect all language names for tracking
    let language_names: Vec<&str> = vec!["rust", "python", "csharp", "cpp", "js", "lua"];

    // Initialize per-language reports
    for lang in &language_names {
        report
            .per_language
            .insert((*lang).to_string(), LanguageReport::new());
    }

    // Process each struct from the config
    for (struct_name, methods) in &config.methods {
        let mut struct_report: StructReport = StructReport::new();

        // Count total methods
        report.total_methods += methods.len();

        // Initialize method statuses
        for method_name in methods {
            struct_report
                .methods
                .insert(method_name.clone(), MethodStatus::new());
        }

        // Validate with Rust
        let rust_targets: Vec<String> = config.targets.get("rust").cloned().unwrap_or_default();

        let rust_result: ValidationResult =
            rust_validator.validate(runner, struct_name, methods, &rust_targets);

        update_struct_report(&mut struct_report, &rust_result, "rust");
        update_language_report(&mut report.per_language, struct_name, rust_result, "rust");

        // Validate with Python
        let python_targets: Vec<String> = config.targets.get("python").cloned().unwrap_or_default();

        let python_result: ValidationResult =
            python_validator.validate(runner, struct_name, methods, &python_targets);

        update_struct_report(&mut struct_report, &python_result, "python");
        update_language_report(
            &mut report.per_language,
            struct_name,
            python_result,
            "python",
        );

        // Validate with C#
        let csharp_targets: Vec<String> = config.targets.get("csharp").cloned().unwrap_or_default();

        let csharp_result: ValidationResult =
            csharp_validator.validate(runner, struct_name, methods, &csharp_targets);

        update_struct_report(&mut struct_report, &csharp_result, "csharp");
        update_language_report(
            &mut report.per_language,
            struct_name,
            csharp_result,
            "csharp",
        );

        // Validate with C++
        let cpp_targets: Vec<String> = config.targets.get("cpp").cloned().unwrap_or_default();

        let cpp_result: ValidationResult =
            cpp_validator.validate(runner, struct_name, methods, &cpp_targets);

        update_struct_report(&mut struct_report, &cpp_result, "cpp");
        update_language_report(&mut report.per_language, struct_name, cpp_result, "cpp");

        // Validate with JS/TypeScript
        let js_targets: Vec<String> = config.targets.get("js").cloned().unwrap_or_default();

        let js_result: ValidationResult =
            js_validator.validate(runner, struct_name, methods, &js_targets);

        update_struct_report(&mut struct_report, &js_result, "js");
        update_language_report(&mut report.per_language, struct_name, js_result, "js");

        // Validate with Lua (if available)
        match &mut lua_validator {
            Some(lua_val) => {
                let lua_targets: Vec<String> =
                    config.targets.get("lua").cloned().unwrap_or_default();

                let lua_result: ValidationResult =
                    lua_val.validate(struct_name, methods, &lua_targets);

                update_struct_report(&mut struct_report, &lua_result, "lua");
                update_language_report(&mut report.per_language, struct_name, lua_result, "lua");
            }
            None => {
                // Lua validator not available, mark all methods as missing
                for method_name in methods {
                    if let Some(status) = struct_report.methods.get_mut(method_name) {
                        status.missing_in.push("lua".to_string());
                    }
                }
            }
        }

        // Calculate struct completion
        struct_report.calculate_completion();
        report.per_struct.insert(struct_name.clone(), struct_report);
    }

    // Calculate per-language completion percentages
    for lang_report in report.per_language.values_mut() {
        lang_report.calculate_completion();
    }

    // Calculate overall statistics
    calculate_overall_stats(&mut report);

    report
}

/// Update a struct report with validation results from a language.
fn update_struct_report(
    struct_report: &mut StructReport,
    result: &ValidationResult,
    language: &str,
) {
    for method_name in &result.found_methods {
        if let Some(status) = struct_report.methods.get_mut(method_name) {
            status.found_in.push(language.to_string());
        }
    }

    for method_name in &result.missing_methods {
        if let Some(status) = struct_report.methods.get_mut(method_name) {
            status.missing_in.push(language.to_string());
        }
    }
}

/// Update a language report with validation results for a struct.
fn update_language_report(
    per_language: &mut HashMap<String, LanguageReport>,
    struct_name: &str,
    result: ValidationResult,
    language: &str,
) {
    if let Some(lang_report) = per_language.get_mut(language) {
        lang_report.structs.insert(struct_name.to_string(), result);
    }
}

/// Calculate overall statistics for the validation report.
fn calculate_overall_stats(report: &mut ValidationReport) {
    let mut total_found: usize = 0;
    let mut all_complete: bool = true;

    for struct_report in report.per_struct.values() {
        for status in struct_report.methods.values() {
            total_found += status.found_in.len();
            if !status.is_complete() {
                all_complete = false;
            }
        }
    }

    report.found_methods = total_found;
    report.is_complete = all_complete;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_method_status_new() {
        let status: MethodStatus = MethodStatus::new();
        assert!(status.found_in.is_empty());
        assert!(status.missing_in.is_empty());
        assert!(status.is_complete());
        assert_eq!(status.completion_percentage(), 100);
    }

    #[test]
    fn test_method_status_with_found() {
        let mut status: MethodStatus = MethodStatus::new();
        status.found_in.push("rust".to_string());
        status.found_in.push("python".to_string());

        assert!(status.is_complete());
        assert_eq!(status.completion_percentage(), 100);
    }

    #[test]
    fn test_method_status_with_missing() {
        let mut status: MethodStatus = MethodStatus::new();
        status.found_in.push("rust".to_string());
        status.missing_in.push("python".to_string());

        assert!(!status.is_complete());
        assert_eq!(status.completion_percentage(), 50);
    }

    #[test]
    fn test_method_status_default() {
        let status: MethodStatus = MethodStatus::default();
        assert!(status.is_complete());
    }

    #[test]
    fn test_struct_report_new() {
        let report: StructReport = StructReport::new();
        assert!(report.methods.is_empty());
        assert_eq!(report.completion_percentage, 100.0);
    }

    #[test]
    fn test_struct_report_calculate_completion_empty() {
        let mut report: StructReport = StructReport::new();
        report.calculate_completion();
        assert_eq!(report.completion_percentage, 100.0);
    }

    #[test]
    fn test_struct_report_calculate_completion_with_methods() {
        let mut report: StructReport = StructReport::new();

        let mut status1: MethodStatus = MethodStatus::new();
        status1.found_in.push("rust".to_string());
        status1.found_in.push("python".to_string());
        status1.missing_in.push("csharp".to_string());

        let mut status2: MethodStatus = MethodStatus::new();
        status2.found_in.push("rust".to_string());

        report.methods.insert("to_str".to_string(), status1);
        report.methods.insert("starts_with".to_string(), status2);

        report.calculate_completion();

        // 3 found, 1 missing = 75%
        assert_eq!(report.completion_percentage, 75.0);
    }

    #[test]
    fn test_struct_report_default() {
        let report: StructReport = StructReport::default();
        assert!(report.methods.is_empty());
    }

    #[test]
    fn test_language_report_new() {
        let report: LanguageReport = LanguageReport::new();
        assert!(report.structs.is_empty());
        assert_eq!(report.completion_percentage, 100.0);
    }

    #[test]
    fn test_language_report_calculate_completion() {
        let mut report: LanguageReport = LanguageReport::new();

        let mut result1: ValidationResult =
            ValidationResult::new("StringView".to_string(), "rust".to_string());
        result1.found_methods.push("to_str".to_string());
        result1.missing_methods.push("starts_with".to_string());

        let mut result2: ValidationResult =
            ValidationResult::new("BufferView".to_string(), "rust".to_string());
        result2.found_methods.push("as_slice".to_string());
        result2.found_methods.push("as_mut_slice".to_string());

        report.structs.insert("StringView".to_string(), result1);
        report.structs.insert("BufferView".to_string(), result2);

        report.calculate_completion();

        // 3 found, 1 missing = 75%
        assert_eq!(report.completion_percentage, 75.0);
    }

    #[test]
    fn test_language_report_default() {
        let report: LanguageReport = LanguageReport::default();
        assert!(report.structs.is_empty());
    }

    #[test]
    fn test_validation_report_new() {
        let report: ValidationReport = ValidationReport::new();
        assert!(report.is_complete);
        assert_eq!(report.total_methods, 0);
        assert_eq!(report.found_methods, 0);
        assert!(report.per_struct.is_empty());
        assert!(report.per_language.is_empty());
    }

    #[test]
    fn test_validation_report_default() {
        let report: ValidationReport = ValidationReport::default();
        assert!(report.is_complete);
    }

    #[test]
    fn test_aggregate_results_empty_config() {
        let config: Config = Config {
            version: 1,
            methods: HashMap::new(),
            naming: HashMap::new(),
            targets: HashMap::new(),
        };

        let runner: AstGrepRunner = AstGrepRunner::new();
        let report: ValidationReport = aggregate_results(&config, &runner);

        assert!(report.is_complete);
        assert_eq!(report.total_methods, 0);
        assert_eq!(report.found_methods, 0);
        assert!(report.per_struct.is_empty());
        assert_eq!(report.per_language.len(), 6);
    }

    #[test]
    fn test_aggregate_results_single_struct() {
        let mut methods: HashMap<String, Vec<String>> = HashMap::new();
        methods.insert(
            "StringView".to_string(),
            vec!["to_str".to_string(), "starts_with".to_string()],
        );

        let mut targets: HashMap<String, Vec<String>> = HashMap::new();
        targets.insert("rust".to_string(), vec!["/nonexistent.rs".to_string()]);
        targets.insert("python".to_string(), vec!["/nonexistent.py".to_string()]);
        targets.insert("csharp".to_string(), vec!["/nonexistent.cs".to_string()]);
        targets.insert("cpp".to_string(), vec!["/nonexistent.hpp".to_string()]);
        targets.insert("js".to_string(), vec!["/nonexistent.ts".to_string()]);
        targets.insert("lua".to_string(), vec!["/nonexistent.lua".to_string()]);

        let config: Config = Config {
            version: 1,
            methods,
            naming: HashMap::new(),
            targets,
        };

        let runner: AstGrepRunner = AstGrepRunner::new();
        let report: ValidationReport = aggregate_results(&config, &runner);

        // All methods should be missing since files don't exist
        assert!(!report.is_complete);
        assert_eq!(report.total_methods, 2);
        assert_eq!(report.found_methods, 0);

        // Check per-struct report
        assert!(report.per_struct.contains_key("StringView"));
        let struct_report: &StructReport = report.per_struct.get("StringView").unwrap();
        assert_eq!(struct_report.methods.len(), 2);

        // Check per-language reports
        assert_eq!(report.per_language.len(), 6);
        for lang_report in report.per_language.values() {
            assert!(lang_report.structs.contains_key("StringView"));
        }
    }

    #[test]
    fn test_update_struct_report() {
        let mut struct_report: StructReport = StructReport::new();
        struct_report
            .methods
            .insert("to_str".to_string(), MethodStatus::new());
        struct_report
            .methods
            .insert("starts_with".to_string(), MethodStatus::new());

        let mut result: ValidationResult =
            ValidationResult::new("StringView".to_string(), "rust".to_string());
        result.found_methods.push("to_str".to_string());
        result.missing_methods.push("starts_with".to_string());

        update_struct_report(&mut struct_report, &result, "rust");

        let to_str_status: &MethodStatus = struct_report.methods.get("to_str").unwrap();
        assert!(to_str_status.found_in.contains(&"rust".to_string()));
        assert!(to_str_status.missing_in.is_empty());

        let starts_with_status: &MethodStatus = struct_report.methods.get("starts_with").unwrap();
        assert!(starts_with_status.found_in.is_empty());
        assert!(starts_with_status.missing_in.contains(&"rust".to_string()));
    }

    #[test]
    fn test_update_language_report() {
        let mut per_language: HashMap<String, LanguageReport> = HashMap::new();
        per_language.insert("rust".to_string(), LanguageReport::new());

        let result: ValidationResult =
            ValidationResult::new("StringView".to_string(), "rust".to_string());

        update_language_report(&mut per_language, "StringView", result, "rust");

        let rust_report: &LanguageReport = per_language.get("rust").unwrap();
        assert!(rust_report.structs.contains_key("StringView"));
    }

    #[test]
    fn test_calculate_overall_stats() {
        let mut report: ValidationReport = ValidationReport::new();

        let mut struct_report: StructReport = StructReport::new();

        let mut status1: MethodStatus = MethodStatus::new();
        status1.found_in.push("rust".to_string());
        status1.found_in.push("python".to_string());

        let mut status2: MethodStatus = MethodStatus::new();
        status2.found_in.push("rust".to_string());
        status2.missing_in.push("python".to_string());

        struct_report.methods.insert("to_str".to_string(), status1);
        struct_report
            .methods
            .insert("starts_with".to_string(), status2);

        report
            .per_struct
            .insert("StringView".to_string(), struct_report);

        calculate_overall_stats(&mut report);

        assert_eq!(report.found_methods, 3);
        assert!(!report.is_complete);
    }

    #[test]
    fn test_calculate_overall_stats_complete() {
        let mut report: ValidationReport = ValidationReport::new();

        let mut struct_report: StructReport = StructReport::new();

        let mut status1: MethodStatus = MethodStatus::new();
        status1.found_in.push("rust".to_string());
        status1.found_in.push("python".to_string());

        let mut status2: MethodStatus = MethodStatus::new();
        status2.found_in.push("rust".to_string());
        status2.found_in.push("python".to_string());

        struct_report.methods.insert("to_str".to_string(), status1);
        struct_report
            .methods
            .insert("starts_with".to_string(), status2);

        report
            .per_struct
            .insert("StringView".to_string(), struct_report);

        calculate_overall_stats(&mut report);

        assert_eq!(report.found_methods, 4);
        assert!(report.is_complete);
    }
}

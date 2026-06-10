//! Result aggregation for SDK validation.
//!
//! Runs every language validator against the config and aggregates the
//! per-file results into a comprehensive report. Tool failures, missing
//! target files, and Lua parser init failures are fatal — they propagate as
//! errors instead of being silently counted as "missing".

use std::collections::HashMap;
use std::path::PathBuf;

use serde::Serialize;

use crate::ast_grep::{AstGrepRunner, NamingConvention};
use crate::config::Config;
use crate::error::ValidatorError;
use crate::languages::{
    CSharpValidator, CppValidator, JsValidator, LanguageValidator, LuaValidator, PythonValidator,
    RustValidator, ValidationResult, validate_language,
};

/// A language missing a method, with the target files it is missing from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MissingDetail {
    /// The language missing the method.
    pub language: String,
    /// The target files that do not implement it (empty when the language
    /// has no target files configured).
    pub files: Vec<String>,
}

/// Status of a method across all language SDKs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MethodStatus {
    /// Languages where the method was found in every target file.
    pub found_in: Vec<String>,
    /// Languages (with files) where the method is missing.
    pub missing_in: Vec<MissingDetail>,
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
}

impl Default for MethodStatus {
    fn default() -> Self {
        Self::new()
    }
}

/// Report for a single struct across all language SDKs.
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
/// Runs all 6 language validators (Rust, Python, C#, C++, JS, Lua). A
/// language with no configured target files is reported as missing every
/// method; a configured target file that does not exist is a fatal error.
///
/// # Errors
///
/// Returns a [`ValidatorError`] if:
/// - the Lua tree-sitter parser cannot be initialized
/// - a configured target file does not exist
/// - a target language has no naming convention configured
/// - ast-grep execution or output parsing fails
pub fn aggregate_results(
    config: &Config,
    runner: &AstGrepRunner,
) -> Result<ValidationReport, ValidatorError> {
    let mut report: ValidationReport = ValidationReport::new();

    let mut validators: Vec<Box<dyn LanguageValidator>> = vec![
        Box::new(RustValidator::new()),
        Box::new(PythonValidator::new()),
        Box::new(CSharpValidator::new()),
        Box::new(CppValidator::new()),
        Box::new(JsValidator::new()),
        Box::new(LuaValidator::new()?),
    ];

    for validator in &validators {
        report
            .per_language
            .insert(validator.language_name().to_string(), LanguageReport::new());
    }

    for (struct_name, methods) in &config.methods {
        let mut struct_report: StructReport = StructReport::new();
        report.total_methods += methods.len();

        for method_name in methods {
            struct_report
                .methods
                .insert(method_name.clone(), MethodStatus::new());
        }

        for validator in validators.iter_mut() {
            let language: &'static str = validator.language_name();
            let files: &[PathBuf] = config
                .targets
                .get(language)
                .map(Vec::as_slice)
                .unwrap_or(&[]);

            let naming: NamingConvention = match config.naming.get(language) {
                Some(convention) => *convention,
                // No naming needed when there is nothing to probe; the
                // language is reported as missing everything.
                None if files.is_empty() => NamingConvention::Snake,
                None => {
                    return Err(ValidatorError::MissingNamingConvention {
                        language: language.to_string(),
                    });
                }
            };

            let result: ValidationResult = validate_language(
                validator.as_mut(),
                runner,
                naming,
                struct_name,
                methods,
                files,
            )?;

            update_struct_report(&mut struct_report, &result, language);
            update_language_report(&mut report.per_language, struct_name, result, language);
        }

        struct_report.calculate_completion();
        report.per_struct.insert(struct_name.clone(), struct_report);
    }

    for lang_report in report.per_language.values_mut() {
        lang_report.calculate_completion();
    }

    calculate_overall_stats(&mut report);

    Ok(report)
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

    for missing in &result.missing_methods {
        if let Some(status) = struct_report.methods.get_mut(&missing.method) {
            status.missing_in.push(MissingDetail {
                language: language.to_string(),
                files: missing.missing_files.clone(),
            });
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
    use std::io::Write;
    use tempfile::NamedTempFile;

    use crate::languages::MissingMethod;
    use crate::languages::test_support::runner;

    fn empty_config() -> Config {
        Config {
            version: 1,
            methods: HashMap::new(),
            naming: HashMap::new(),
            targets: HashMap::new(),
        }
    }

    #[test]
    fn test_method_status_new() {
        let status: MethodStatus = MethodStatus::new();
        assert!(status.found_in.is_empty());
        assert!(status.missing_in.is_empty());
        assert!(status.is_complete());
    }

    #[test]
    fn test_struct_report_completion() {
        let mut report: StructReport = StructReport::new();

        let mut status1: MethodStatus = MethodStatus::new();
        status1.found_in.push("rust".to_string());
        status1.found_in.push("python".to_string());
        status1.missing_in.push(MissingDetail {
            language: "csharp".to_string(),
            files: vec!["a.cs".to_string()],
        });

        let mut status2: MethodStatus = MethodStatus::new();
        status2.found_in.push("rust".to_string());

        report.methods.insert("to_str".to_string(), status1);
        report.methods.insert("starts_with".to_string(), status2);
        report.calculate_completion();

        // 3 found, 1 missing = 75%
        assert_eq!(report.completion_percentage, 75.0);
    }

    #[test]
    fn test_language_report_completion() {
        let mut report: LanguageReport = LanguageReport::new();

        let mut result1: ValidationResult =
            ValidationResult::new("StringView".to_string(), "rust".to_string());
        result1.found_methods.push("to_str".to_string());
        result1.missing_methods.push(MissingMethod {
            method: "starts_with".to_string(),
            missing_files: vec!["lib.rs".to_string()],
        });

        report.structs.insert("StringView".to_string(), result1);
        report.calculate_completion();

        assert_eq!(report.completion_percentage, 50.0);
    }

    #[test]
    fn test_aggregate_results_empty_config() -> Result<(), Box<dyn core::error::Error>> {
        let config: Config = empty_config();
        let report: ValidationReport = aggregate_results(&config, &runner())?;

        assert!(report.is_complete);
        assert_eq!(report.total_methods, 0);
        assert_eq!(report.found_methods, 0);
        assert!(report.per_struct.is_empty());
        assert_eq!(report.per_language.len(), 6);
        Ok(())
    }

    #[test]
    fn test_aggregate_results_no_targets_marks_all_missing()
    -> Result<(), Box<dyn core::error::Error>> {
        let mut config: Config = empty_config();
        config.methods.insert(
            "StringView".to_string(),
            vec!["to_str".to_string(), "starts_with".to_string()],
        );

        let report: ValidationReport = aggregate_results(&config, &runner())?;

        assert!(!report.is_complete);
        assert_eq!(report.total_methods, 2);
        assert_eq!(report.found_methods, 0);

        let struct_report: &StructReport = report
            .per_struct
            .get("StringView")
            .ok_or("missing StringView struct report")?;
        let to_str_status: &MethodStatus = struct_report
            .methods
            .get("to_str")
            .ok_or("missing to_str status")?;
        assert_eq!(to_str_status.missing_in.len(), 6);
        Ok(())
    }

    #[test]
    fn test_aggregate_results_missing_target_file_is_fatal()
    -> Result<(), Box<dyn core::error::Error>> {
        let mut config: Config = empty_config();
        config
            .methods
            .insert("StringView".to_string(), vec!["to_str".to_string()]);
        config
            .naming
            .insert("rust".to_string(), crate::ast_grep::NamingConvention::Snake);
        config.targets.insert(
            "rust".to_string(),
            vec![PathBuf::from("/nonexistent/lib.rs")],
        );

        let result: Result<ValidationReport, ValidatorError> =
            aggregate_results(&config, &runner());
        assert!(matches!(
            result,
            Err(ValidatorError::TargetFileMissing { .. })
        ));
        Ok(())
    }

    #[test]
    fn test_aggregate_results_reports_missing_file_per_method()
    -> Result<(), Box<dyn core::error::Error>> {
        let mut lua_file: NamedTempFile = NamedTempFile::with_suffix(".lua")?;
        lua_file.write_all(b"function to_str(sv)\n    return \"\"\nend\n")?;
        lua_file.flush()?;

        let mut config: Config = empty_config();
        config.methods.insert(
            "StringView".to_string(),
            vec!["to_str".to_string(), "ends_with".to_string()],
        );
        config
            .naming
            .insert("lua".to_string(), crate::ast_grep::NamingConvention::Snake);
        config
            .targets
            .insert("lua".to_string(), vec![lua_file.path().to_path_buf()]);

        let report: ValidationReport = aggregate_results(&config, &runner())?;

        let struct_report: &StructReport = report
            .per_struct
            .get("StringView")
            .ok_or("missing StringView struct report")?;

        let to_str_status: &MethodStatus = struct_report
            .methods
            .get("to_str")
            .ok_or("missing to_str status")?;
        assert!(to_str_status.found_in.contains(&"lua".to_string()));

        let ends_with_status: &MethodStatus = struct_report
            .methods
            .get("ends_with")
            .ok_or("missing ends_with status")?;
        let lua_detail: &MissingDetail = ends_with_status
            .missing_in
            .iter()
            .find(|d| d.language == "lua")
            .ok_or("expected lua missing detail")?;
        assert_eq!(
            lua_detail.files,
            vec![lua_file.path().display().to_string()]
        );
        Ok(())
    }
}

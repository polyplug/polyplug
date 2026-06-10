//! Result aggregation for SDK validation.
//!
//! Runs every language validator against the config and aggregates the
//! per-file results into a comprehensive report. Tool failures, missing
//! target files, and Lua parser init failures are fatal — they propagate as
//! errors instead of being silently counted as "missing".

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use serde::Serialize;

use crate::ast_grep::{AstGrepRunner, NamingConvention};
use crate::config::Config;
use crate::error::ValidatorError;
use crate::languages::{
    CSharpValidator, CppValidator, EnumValidationResult, JsValidator, LanguageValidator,
    LuaValidator, PythonValidator, RustValidator, ValidationResult, VariantOutcome,
    validate_language, validate_language_enum,
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

/// Why a variant check failed in one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum EnumMismatchKind {
    /// Variant absent from the enum construct (or the construct is absent).
    Missing,
    /// Variant present with a different value.
    WrongValue {
        /// The value found in the file.
        found: i64,
    },
    /// Variant present without a parseable explicit value.
    MissingValue,
}

/// One failed variant check: which language/file, and how it failed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EnumMismatch {
    /// The language with the mismatch.
    pub language: String,
    /// The target file with the mismatch.
    pub file: String,
    /// How the variant check failed.
    pub kind: EnumMismatchKind,
}

/// Status of one golden variant across all language enum mirrors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EnumVariantStatus {
    /// The golden value.
    pub expected: i64,
    /// Languages where the variant is exact in every target file.
    pub found_in: Vec<String>,
    /// Per-file mismatches.
    pub mismatches: Vec<EnumMismatch>,
}

/// A stale variant found in a mirror but absent from the golden set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EnumExtraDetail {
    /// The language containing the stale variant.
    pub language: String,
    /// The target file containing it.
    pub file: String,
    /// The stale variant name.
    pub variant: String,
    /// Its value, when parseable.
    pub value: Option<i64>,
}

/// Report for a single golden enum across all language mirrors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EnumReport {
    /// Variant name -> status across languages.
    pub variants: HashMap<String, EnumVariantStatus>,
    /// Languages with at least one target file for this enum.
    pub checked_languages: Vec<String>,
    /// Stale variants found in mirrors.
    pub extra_variants: Vec<EnumExtraDetail>,
}

impl EnumReport {
    /// Create a new empty enum report.
    pub fn new() -> Self {
        Self {
            variants: HashMap::new(),
            checked_languages: Vec::new(),
            extra_variants: Vec::new(),
        }
    }

    /// Check that every variant matched in every checked language and no
    /// stale variants exist.
    pub fn is_complete(&self) -> bool {
        self.extra_variants.is_empty()
            && self
                .variants
                .values()
                .all(|status| status.mismatches.is_empty())
    }
}

impl Default for EnumReport {
    fn default() -> Self {
        Self::new()
    }
}

/// Aggregated validation report across all language SDKs.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ValidationReport {
    /// Whether all methods are implemented in all languages AND all enum
    /// mirrors match the golden enums exactly.
    pub is_complete: bool,
    /// Total number of unique methods across all structs.
    pub total_methods: usize,
    /// Total number of method implementations found across all languages.
    pub found_methods: usize,
    /// Per-struct reports: struct name -> report.
    pub per_struct: HashMap<String, StructReport>,
    /// Per-language reports: language name -> report.
    pub per_language: HashMap<String, LanguageReport>,
    /// Per-enum reports: enum name -> report.
    pub per_enum: HashMap<String, EnumReport>,
    /// Total number of (variant, language, file) enum checks performed.
    pub enum_checks_total: usize,
    /// Number of enum checks that passed exactly.
    pub enum_checks_passed: usize,
    /// Whether every enum mirror matches the golden enums exactly
    /// (no missing variants, wrong values, or stale extras).
    pub enums_complete: bool,
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
            per_enum: HashMap::new(),
            enum_checks_total: 0,
            enum_checks_passed: 0,
            enums_complete: true,
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

    for validator in validators.iter_mut() {
        let language: &'static str = validator.language_name();
        let Some(language_enum_targets) = config.enum_targets.get(language) else {
            continue;
        };

        // Deterministic iteration so reports are stable across runs.
        let mut enum_names: Vec<&String> = language_enum_targets.keys().collect();
        enum_names.sort();

        for enum_name in enum_names {
            let golden: &BTreeMap<String, i64> =
                config
                    .enums
                    .get(enum_name)
                    .ok_or_else(|| ValidatorError::UnknownEnum {
                        language: language.to_string(),
                        enum_name: enum_name.clone(),
                    })?;
            let files: &[PathBuf] = language_enum_targets
                .get(enum_name)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            if files.is_empty() {
                continue;
            }

            let result: EnumValidationResult =
                validate_language_enum(validator.as_mut(), runner, enum_name, golden, files)?;

            merge_enum_result(&mut report, enum_name, golden, &result, language);
        }
    }

    calculate_overall_stats(&mut report);

    Ok(report)
}

/// Merge one language's enum validation result into the report.
fn merge_enum_result(
    report: &mut ValidationReport,
    enum_name: &str,
    golden: &BTreeMap<String, i64>,
    result: &EnumValidationResult,
    language: &str,
) {
    let enum_report: &mut EnumReport = report.per_enum.entry(enum_name.to_string()).or_default();
    enum_report.checked_languages.push(language.to_string());

    for (variant, expected) in golden {
        enum_report
            .variants
            .entry(variant.clone())
            .or_insert_with(|| EnumVariantStatus {
                expected: *expected,
                found_in: Vec::new(),
                mismatches: Vec::new(),
            });
    }

    let mut failed_variants: Vec<&str> = Vec::new();
    for check in &result.checks {
        report.enum_checks_total += 1;
        if check.outcome == VariantOutcome::Found {
            report.enum_checks_passed += 1;
            continue;
        }
        failed_variants.push(check.variant.as_str());
        if let Some(status) = enum_report.variants.get_mut(&check.variant) {
            let kind: EnumMismatchKind = match check.outcome {
                VariantOutcome::WrongValue { found } => EnumMismatchKind::WrongValue { found },
                VariantOutcome::MissingValue => EnumMismatchKind::MissingValue,
                VariantOutcome::Missing | VariantOutcome::Found => EnumMismatchKind::Missing,
            };
            status.mismatches.push(EnumMismatch {
                language: language.to_string(),
                file: check.file.clone(),
                kind,
            });
        }
    }

    for variant in golden.keys() {
        if failed_variants.contains(&variant.as_str()) {
            continue;
        }
        if let Some(status) = enum_report.variants.get_mut(variant) {
            status.found_in.push(language.to_string());
        }
    }

    for extra in &result.extra_variants {
        enum_report.extra_variants.push(EnumExtraDetail {
            language: language.to_string(),
            file: extra.file.clone(),
            variant: extra.variant.clone(),
            value: extra.value,
        });
    }
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

    report.enums_complete = report
        .per_enum
        .values()
        .all(|enum_report| enum_report.is_complete());

    report.found_methods = total_found;
    report.is_complete = all_complete && report.enums_complete;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    use crate::languages::MissingMethod;
    use crate::languages::test_support::runner;

    use std::collections::BTreeMap;

    fn empty_config() -> Config {
        Config {
            version: 1,
            methods: HashMap::new(),
            naming: HashMap::new(),
            targets: HashMap::new(),
            enums: HashMap::new(),
            enum_targets: HashMap::new(),
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
    fn test_aggregate_results_enum_drift_fails_report() -> Result<(), Box<dyn core::error::Error>> {
        // ReentrantCall = 9 missing and a stale Bogus variant present.
        let mut lua_file: NamedTempFile = NamedTempFile::with_suffix(".lua")?;
        lua_file.write_all(
            b"local M = {}\nM.DispatchType = {\n    Native = 0,\n    Bogus = 7,\n}\nreturn M\n",
        )?;
        lua_file.flush()?;

        let mut config: Config = empty_config();
        let mut golden: BTreeMap<String, i64> = BTreeMap::new();
        golden.insert("Native".to_string(), 0);
        golden.insert("VirtualMachine".to_string(), 1);
        config.enums.insert("DispatchType".to_string(), golden);
        let mut lua_targets: HashMap<String, Vec<PathBuf>> = HashMap::new();
        lua_targets.insert(
            "DispatchType".to_string(),
            vec![lua_file.path().to_path_buf()],
        );
        config.enum_targets.insert("lua".to_string(), lua_targets);

        let report: ValidationReport = aggregate_results(&config, &runner())?;

        assert!(!report.enums_complete);
        assert!(!report.is_complete);
        assert_eq!(report.enum_checks_total, 2);
        assert_eq!(report.enum_checks_passed, 1);

        let enum_report: &EnumReport = report
            .per_enum
            .get("DispatchType")
            .ok_or("missing DispatchType enum report")?;
        assert_eq!(enum_report.checked_languages, vec!["lua".to_string()]);

        let vm_status: &EnumVariantStatus = enum_report
            .variants
            .get("VirtualMachine")
            .ok_or("missing VirtualMachine status")?;
        assert_eq!(vm_status.expected, 1);
        assert_eq!(vm_status.mismatches.len(), 1);
        assert_eq!(vm_status.mismatches[0].language, "lua");
        assert_eq!(vm_status.mismatches[0].kind, EnumMismatchKind::Missing);

        let native_status: &EnumVariantStatus = enum_report
            .variants
            .get("Native")
            .ok_or("missing Native status")?;
        assert_eq!(native_status.found_in, vec!["lua".to_string()]);

        assert_eq!(enum_report.extra_variants.len(), 1);
        assert_eq!(enum_report.extra_variants[0].variant, "Bogus");
        assert_eq!(enum_report.extra_variants[0].value, Some(7));
        Ok(())
    }

    #[test]
    fn test_aggregate_results_enum_exact_match_is_complete()
    -> Result<(), Box<dyn core::error::Error>> {
        let mut lua_file: NamedTempFile = NamedTempFile::with_suffix(".lua")?;
        lua_file.write_all(
            b"local M = {}\nM.DispatchType = {\n    Native = 0,\n    VirtualMachine = 1,\n}\nreturn M\n",
        )?;
        lua_file.flush()?;

        let mut config: Config = empty_config();
        let mut golden: BTreeMap<String, i64> = BTreeMap::new();
        golden.insert("Native".to_string(), 0);
        golden.insert("VirtualMachine".to_string(), 1);
        config.enums.insert("DispatchType".to_string(), golden);
        let mut lua_targets: HashMap<String, Vec<PathBuf>> = HashMap::new();
        lua_targets.insert(
            "DispatchType".to_string(),
            vec![lua_file.path().to_path_buf()],
        );
        config.enum_targets.insert("lua".to_string(), lua_targets);

        let report: ValidationReport = aggregate_results(&config, &runner())?;

        assert!(report.enums_complete);
        assert!(report.is_complete);
        assert_eq!(report.enum_checks_total, 2);
        assert_eq!(report.enum_checks_passed, 2);
        Ok(())
    }

    #[test]
    fn test_aggregate_results_enum_missing_target_file_is_fatal()
    -> Result<(), Box<dyn core::error::Error>> {
        let mut config: Config = empty_config();
        let mut golden: BTreeMap<String, i64> = BTreeMap::new();
        golden.insert("Native".to_string(), 0);
        config.enums.insert("DispatchType".to_string(), golden);
        let mut rust_targets: HashMap<String, Vec<PathBuf>> = HashMap::new();
        rust_targets.insert(
            "DispatchType".to_string(),
            vec![PathBuf::from("/nonexistent/enum.rs")],
        );
        config.enum_targets.insert("rust".to_string(), rust_targets);

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

//! Validation result reporting.
//!
//! This module provides functionality to generate human-readable and machine-readable
//! reports from validation results.

use crate::aggregator::{MethodStatus, StructReport, ValidationReport};

/// Reporter for generating validation reports in different formats.
///
/// Supports two output formats:
/// - **Table**: Human-readable ASCII table for terminal output
/// - **JSON**: Machine-readable JSON for CI/programmatic use
pub struct Reporter {
    /// Ordered list of languages to display in table columns.
    languages: Vec<String>,
}

impl Reporter {
    /// Create a new reporter with default language ordering.
    ///
    /// The default order is: Rust, Python, C#, C++, JS, Lua.
    pub fn new() -> Self {
        Self {
            languages: vec![
                "rust".to_string(),
                "python".to_string(),
                "csharp".to_string(),
                "cpp".to_string(),
                "js".to_string(),
                "lua".to_string(),
            ],
        }
    }

    /// Create a reporter with custom language ordering.
    ///
    /// # Arguments
    ///
    /// * `languages` - The ordered list of language names to display.
    #[allow(dead_code)]
    pub fn with_languages(languages: Vec<String>) -> Self {
        Self { languages }
    }

    /// Generate a human-readable table report.
    ///
    /// The table shows each struct with its methods and a ✓/✗ indicator
    /// for each language indicating whether the method is implemented.
    ///
    /// # Arguments
    ///
    /// * `report` - The validation report to format.
    ///
    /// # Returns
    ///
    /// A formatted string containing the ASCII table.
    pub fn generate_table(&self, report: &ValidationReport) -> String {
        let mut output: String = String::new();

        // Header
        output.push_str("SDK Validation Report\n");
        output.push_str("=====================\n\n");

        // Per-struct tables
        let mut struct_names: Vec<&String> = report.per_struct.keys().collect();
        struct_names.sort();

        for struct_name in &struct_names {
            let struct_report: &StructReport = match report.per_struct.get(*struct_name) {
                Some(sr) => sr,
                None => continue,
            };

            output.push_str(&format!("{} Methods:\n", struct_name));
            output.push_str(&self.generate_struct_table(struct_report));
            output.push('\n');
        }

        // Summary
        let completion_pct: f64 = if report.total_methods == 0 {
            100.0
        } else {
            let total_possible: usize = report.total_methods * self.languages.len();
            (report.found_methods as f64 / total_possible as f64) * 100.0
        };

        output.push_str(&format!(
            "Summary: {}/{} method implementations found ({:.1}%)\n",
            report.found_methods,
            report.total_methods * self.languages.len(),
            completion_pct
        ));

        // List methods missing in all languages
        let missing_all: Vec<String> = self.find_methods_missing_everywhere(report);
        if !missing_all.is_empty() {
            output.push_str(&format!(
                "Missing in all languages: {}\n",
                missing_all.join(", ")
            ));
        }

        output
    }

    /// Generate a table for a single struct's methods.
    fn generate_struct_table(&self, struct_report: &StructReport) -> String {
        let mut output: String = String::new();

        // Calculate column widths
        let method_width: usize = self.calculate_method_column_width(struct_report);
        let lang_widths: Vec<usize> = self.calculate_language_column_widths();

        // Header row
        output.push_str("  ");
        output.push_str(&self.pad_right("Method", method_width));
        output.push_str(" |");

        for (i, lang) in self.languages.iter().enumerate() {
            output.push_str(&format!(" {} |", self.pad_center(lang, lang_widths[i])));
        }
        output.push('\n');

        // Separator row
        output.push_str("  ");
        output.push_str(&"-".repeat(method_width));
        output.push_str("-|");
        for width in &lang_widths {
            output.push_str(&format!("-{}-|", "-".repeat(*width)));
        }
        output.push('\n');

        // Method rows (sorted alphabetically)
        let mut method_names: Vec<&String> = struct_report.methods.keys().collect();
        method_names.sort();

        for method_name in method_names {
            let status: &MethodStatus = match struct_report.methods.get(method_name) {
                Some(s) => s,
                None => continue,
            };

            output.push_str("  ");
            output.push_str(&self.pad_right(method_name, method_width));
            output.push_str(" |");

            for (i, lang) in self.languages.iter().enumerate() {
                let symbol: &str = if status.found_in.contains(lang) {
                    "✓"
                } else {
                    "✗"
                };
                output.push_str(&format!(" {} |", self.pad_center(symbol, lang_widths[i])));
            }
            output.push('\n');
        }

        output
    }

    /// Calculate the width needed for the method name column.
    fn calculate_method_column_width(&self, struct_report: &StructReport) -> usize {
        let min_width: usize = 7; // "Method" header
        let max_name_len: usize = struct_report
            .methods
            .keys()
            .map(|n| n.len())
            .max()
            .unwrap_or(min_width);
        core::cmp::max(min_width, max_name_len)
    }

    /// Calculate the widths needed for each language column.
    fn calculate_language_column_widths(&self) -> Vec<usize> {
        self.languages
            .iter()
            .map(|lang| core::cmp::max(lang.len(), 1)) // At least 1 for ✓/✗
            .collect()
    }

    /// Pad a string to the right with spaces.
    fn pad_right(&self, s: &str, width: usize) -> String {
        format!("{:width$}", s, width = width)
    }

    /// Center a string within a given width.
    fn pad_center(&self, s: &str, width: usize) -> String {
        let s_len: usize = s.chars().count();
        if s_len >= width {
            return s.to_string();
        }
        let left_pad: usize = (width - s_len) / 2;
        let right_pad: usize = width - s_len - left_pad;
        format!("{}{}{}", " ".repeat(left_pad), s, " ".repeat(right_pad))
    }

    /// Find methods that are missing in all languages.
    fn find_methods_missing_everywhere(&self, report: &ValidationReport) -> Vec<String> {
        let mut missing_all: Vec<String> = Vec::new();

        for (struct_name, struct_report) in &report.per_struct {
            for (method_name, status) in &struct_report.methods {
                if status.found_in.is_empty() {
                    missing_all.push(format!("{}.{}", struct_name, method_name));
                }
            }
        }

        missing_all.sort();
        missing_all
    }

    /// Generate a JSON report for programmatic use.
    ///
    /// The JSON output includes:
    /// - Overall completion status
    /// - Total and found method counts
    /// - Per-struct breakdown with method statuses
    /// - Per-language breakdown with struct results
    ///
    /// # Arguments
    ///
    /// * `report` - The validation report to serialize.
    ///
    /// # Returns
    ///
    /// A JSON string representation of the report.
    pub fn generate_json(&self, report: &ValidationReport) -> String {
        // Create a serializable version with completion_percentage at top level
        let completion_pct: f64 = if report.total_methods == 0 {
            100.0
        } else {
            let total_possible: usize = report.total_methods * self.languages.len();
            (report.found_methods as f64 / total_possible as f64) * 100.0
        };

        let json_output: serde_json::Value = serde_json::json!({
            "is_complete": report.is_complete,
            "total_methods": report.total_methods,
            "found_methods": report.found_methods,
            "completion_percentage": completion_pct,
            "per_struct": report.per_struct,
            "per_language": report.per_language,
        });

        serde_json::to_string_pretty(&json_output).unwrap_or_else(|_| "{}".to_string())
    }
}

impl Default for Reporter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_report() -> ValidationReport {
        let mut report: ValidationReport = ValidationReport::new();

        // Add StringView struct
        let mut string_view_report: StructReport = StructReport::new();

        let mut to_str_status: MethodStatus = MethodStatus::new();
        to_str_status.found_in = vec!["rust".to_string(), "python".to_string()];
        to_str_status.missing_in = vec![
            "csharp".to_string(),
            "cpp".to_string(),
            "js".to_string(),
            "lua".to_string(),
        ];

        let mut starts_with_status: MethodStatus = MethodStatus::new();
        starts_with_status.found_in = vec!["python".to_string(), "cpp".to_string()];
        starts_with_status.missing_in = vec![
            "rust".to_string(),
            "csharp".to_string(),
            "js".to_string(),
            "lua".to_string(),
        ];

        let mut ends_with_status: MethodStatus = MethodStatus::new();
        ends_with_status.found_in = vec![];
        ends_with_status.missing_in = vec![
            "rust".to_string(),
            "python".to_string(),
            "csharp".to_string(),
            "cpp".to_string(),
            "js".to_string(),
            "lua".to_string(),
        ];

        string_view_report
            .methods
            .insert("to_str".to_string(), to_str_status);
        string_view_report
            .methods
            .insert("starts_with".to_string(), starts_with_status);
        string_view_report
            .methods
            .insert("ends_with".to_string(), ends_with_status);
        string_view_report.calculate_completion();

        report
            .per_struct
            .insert("StringView".to_string(), string_view_report);
        report.total_methods = 3;
        report.found_methods = 4; // 2 + 2 + 0
        report.is_complete = false;

        report
    }

    #[test]
    fn test_reporter_new() {
        let reporter: Reporter = Reporter::new();
        assert_eq!(reporter.languages.len(), 6);
        assert_eq!(reporter.languages[0], "rust");
        assert_eq!(reporter.languages[5], "lua");
    }

    #[test]
    fn test_reporter_with_languages() {
        let reporter: Reporter =
            Reporter::with_languages(vec!["rust".to_string(), "python".to_string()]);
        assert_eq!(reporter.languages.len(), 2);
    }

    #[test]
    fn test_generate_table_header() {
        let reporter: Reporter = Reporter::new();
        let report: ValidationReport = ValidationReport::new();
        let output: String = reporter.generate_table(&report);

        assert!(output.contains("SDK Validation Report"));
        assert!(output.contains("====================="));
    }

    #[test]
    fn test_generate_table_with_methods() {
        let reporter: Reporter = Reporter::new();
        let report: ValidationReport = create_test_report();
        let output: String = reporter.generate_table(&report);

        // Check struct header
        assert!(output.contains("StringView Methods:"));

        // Check method names appear
        assert!(output.contains("to_str"));
        assert!(output.contains("starts_with"));
        assert!(output.contains("ends_with"));

        // Check language headers
        assert!(output.contains("rust"));
        assert!(output.contains("python"));
        assert!(output.contains("csharp"));

        // Check summary
        assert!(output.contains("Summary:"));
        assert!(output.contains("4/18 method implementations found"));
    }

    #[test]
    fn test_generate_table_missing_all() {
        let reporter: Reporter = Reporter::new();
        let report: ValidationReport = create_test_report();
        let output: String = reporter.generate_table(&report);

        // ends_with is missing in all languages
        assert!(output.contains("Missing in all languages:"));
        assert!(output.contains("StringView.ends_with"));
    }

    #[test]
    fn test_generate_json_structure() {
        let reporter: Reporter = Reporter::new();
        let report: ValidationReport = create_test_report();
        let output: String = reporter.generate_json(&report);

        // Parse the JSON to verify structure
        let parsed: serde_json::Value =
            serde_json::from_str(&output).expect("JSON should be valid");

        assert_eq!(parsed["is_complete"], false);
        assert_eq!(parsed["total_methods"], 3);
        assert_eq!(parsed["found_methods"], 4);
        assert!(parsed["completion_percentage"].is_number());
        assert!(parsed["per_struct"].is_object());
        assert!(parsed["per_struct"]["StringView"].is_object());
    }

    #[test]
    fn test_generate_json_method_status() {
        let reporter: Reporter = Reporter::new();
        let report: ValidationReport = create_test_report();
        let output: String = reporter.generate_json(&report);

        let parsed: serde_json::Value =
            serde_json::from_str(&output).expect("JSON should be valid");

        // Check to_str method status
        let to_str: &serde_json::Value = &parsed["per_struct"]["StringView"]["methods"]["to_str"];
        assert!(to_str["found_in"].is_array());
        assert!(to_str["missing_in"].is_array());

        let found_in: Vec<String> = to_str["found_in"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
        assert!(found_in.contains(&"rust".to_string()));
        assert!(found_in.contains(&"python".to_string()));
    }

    #[test]
    fn test_generate_json_empty_report() {
        let reporter: Reporter = Reporter::new();
        let report: ValidationReport = ValidationReport::new();
        let output: String = reporter.generate_json(&report);

        let parsed: serde_json::Value =
            serde_json::from_str(&output).expect("JSON should be valid");

        assert_eq!(parsed["is_complete"], true);
        assert_eq!(parsed["total_methods"], 0);
        assert_eq!(parsed["found_methods"], 0);
        assert_eq!(parsed["completion_percentage"], 100.0);
    }

    #[test]
    fn test_pad_right() {
        let reporter: Reporter = Reporter::new();
        assert_eq!(reporter.pad_right("test", 10), "test      ");
        assert_eq!(reporter.pad_right("test", 4), "test");
    }

    #[test]
    fn test_pad_center() {
        let reporter: Reporter = Reporter::new();
        assert_eq!(reporter.pad_center("a", 3), " a ");
        assert_eq!(reporter.pad_center("ab", 4), " ab ");
        assert_eq!(reporter.pad_center("abc", 3), "abc");
    }

    #[test]
    fn test_find_methods_missing_everywhere() {
        let reporter: Reporter = Reporter::new();
        let report: ValidationReport = create_test_report();
        let missing: Vec<String> = reporter.find_methods_missing_everywhere(&report);

        assert_eq!(missing.len(), 1);
        assert!(missing.contains(&"StringView.ends_with".to_string()));
    }

    #[test]
    fn test_default_reporter() {
        let reporter: Reporter = Reporter::default();
        assert_eq!(reporter.languages.len(), 6);
    }
}

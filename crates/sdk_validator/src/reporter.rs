//! Validation result reporting.
//!
//! Generates human-readable (table) and machine-readable (JSON) reports.
//! When a method is missing, both formats name the target file(s) it is
//! missing from.

use crate::aggregator::{MethodStatus, StructReport, ValidationReport};

/// Reporter for generating validation reports in different formats.
pub struct Reporter {
    /// Ordered list of languages to display in table columns.
    languages: Vec<String>,
}

impl Reporter {
    /// Create a new reporter with the default language ordering:
    /// Rust, Python, C#, C++, JS, Lua.
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

    /// Generate a human-readable table report.
    ///
    /// Shows each struct's methods with a ✓/✗ per language, followed by a
    /// per-file breakdown of every missing method.
    pub fn generate_table(&self, report: &ValidationReport) -> String {
        let mut output: String = String::new();

        output.push_str("SDK Validation Report\n");
        output.push_str("=====================\n\n");

        let mut struct_names: Vec<&String> = report.per_struct.keys().collect();
        struct_names.sort();

        for struct_name in &struct_names {
            let struct_report: &StructReport = match report.per_struct.get(*struct_name) {
                Some(sr) => sr,
                None => continue,
            };

            output.push_str(&format!("{} Methods:\n", struct_name));
            output.push_str(&self.generate_struct_table(struct_report));
            output.push_str(&self.generate_missing_details(struct_report));
            output.push('\n');
        }

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

        let method_width: usize = self.calculate_method_column_width(struct_report);
        let lang_widths: Vec<usize> = self.calculate_language_column_widths();

        output.push_str("  ");
        output.push_str(&self.pad_right("Method", method_width));
        output.push_str(" |");

        for (i, lang) in self.languages.iter().enumerate() {
            output.push_str(&format!(" {} |", self.pad_center(lang, lang_widths[i])));
        }
        output.push('\n');

        output.push_str("  ");
        output.push_str(&"-".repeat(method_width));
        output.push_str("-|");
        for width in &lang_widths {
            output.push_str(&format!("-{}-|", "-".repeat(*width)));
        }
        output.push('\n');

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

    /// Generate per-file breakdown lines for every missing method.
    fn generate_missing_details(&self, struct_report: &StructReport) -> String {
        let mut lines: Vec<String> = Vec::new();

        let mut method_names: Vec<&String> = struct_report.methods.keys().collect();
        method_names.sort();

        for method_name in method_names {
            let status: &MethodStatus = match struct_report.methods.get(method_name) {
                Some(s) => s,
                None => continue,
            };

            let mut details: Vec<&crate::aggregator::MissingDetail> =
                status.missing_in.iter().collect();
            details.sort_by(|a, b| a.language.cmp(&b.language));

            for detail in details {
                let files: String = if detail.files.is_empty() {
                    "(no target files configured)".to_string()
                } else {
                    detail.files.join(", ")
                };
                lines.push(format!(
                    "    {} [{}]: {}",
                    method_name, detail.language, files
                ));
            }
        }

        if lines.is_empty() {
            String::new()
        } else {
            format!("  Missing from:\n{}\n", lines.join("\n"))
        }
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
            .map(|lang| core::cmp::max(lang.len(), 1))
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
    /// `per_struct[*].methods[*].missing_in` is an array of
    /// `{language, files}` objects naming the file(s) each missing method is
    /// absent from.
    pub fn generate_json(&self, report: &ValidationReport) -> String {
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
    use crate::aggregator::MissingDetail;

    fn create_test_report() -> ValidationReport {
        let mut report: ValidationReport = ValidationReport::new();

        let mut string_view_report: StructReport = StructReport::new();

        let mut to_str_status: MethodStatus = MethodStatus::new();
        to_str_status.found_in = vec!["rust".to_string(), "python".to_string()];
        to_str_status.missing_in = vec![
            MissingDetail {
                language: "csharp".to_string(),
                files: vec!["sdks/csharp/abi/Abi.cs".to_string()],
            },
            MissingDetail {
                language: "cpp".to_string(),
                files: vec!["sdks/cpp/abi/polyplug/abi.hpp".to_string()],
            },
            MissingDetail {
                language: "js".to_string(),
                files: Vec::new(),
            },
            MissingDetail {
                language: "lua".to_string(),
                files: vec!["sdks/lua/abi/abi.lua".to_string()],
            },
        ];

        let mut starts_with_status: MethodStatus = MethodStatus::new();
        starts_with_status.found_in = vec![
            "rust".to_string(),
            "python".to_string(),
            "csharp".to_string(),
            "cpp".to_string(),
            "js".to_string(),
            "lua".to_string(),
        ];

        let mut ends_with_status: MethodStatus = MethodStatus::new();
        ends_with_status.missing_in = vec![
            MissingDetail {
                language: "rust".to_string(),
                files: vec!["sdks/rust/guest/src/lib.rs".to_string()],
            },
            MissingDetail {
                language: "python".to_string(),
                files: Vec::new(),
            },
            MissingDetail {
                language: "csharp".to_string(),
                files: Vec::new(),
            },
            MissingDetail {
                language: "cpp".to_string(),
                files: Vec::new(),
            },
            MissingDetail {
                language: "js".to_string(),
                files: Vec::new(),
            },
            MissingDetail {
                language: "lua".to_string(),
                files: Vec::new(),
            },
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
        report.found_methods = 8; // 2 + 6 + 0
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

        assert!(output.contains("StringView Methods:"));
        assert!(output.contains("to_str"));
        assert!(output.contains("starts_with"));
        assert!(output.contains("ends_with"));
        assert!(output.contains("rust"));
        assert!(output.contains("python"));
        assert!(output.contains("csharp"));
        assert!(output.contains("Summary:"));
        assert!(output.contains("8/18 method implementations found"));
    }

    #[test]
    fn test_generate_table_names_missing_files() {
        let reporter: Reporter = Reporter::new();
        let report: ValidationReport = create_test_report();
        let output: String = reporter.generate_table(&report);

        assert!(output.contains("Missing from:"));
        assert!(output.contains("to_str [lua]: sdks/lua/abi/abi.lua"));
        assert!(output.contains("to_str [csharp]: sdks/csharp/abi/Abi.cs"));
        assert!(output.contains("to_str [js]: (no target files configured)"));
        assert!(output.contains("ends_with [rust]: sdks/rust/guest/src/lib.rs"));
    }

    #[test]
    fn test_generate_table_missing_all() {
        let reporter: Reporter = Reporter::new();
        let report: ValidationReport = create_test_report();
        let output: String = reporter.generate_table(&report);

        assert!(output.contains("Missing in all languages:"));
        assert!(output.contains("StringView.ends_with"));
    }

    #[test]
    fn test_generate_json_structure() -> Result<(), Box<dyn core::error::Error>> {
        let reporter: Reporter = Reporter::new();
        let report: ValidationReport = create_test_report();
        let output: String = reporter.generate_json(&report);

        let parsed: serde_json::Value = serde_json::from_str(&output)?;

        assert_eq!(parsed["is_complete"], false);
        assert_eq!(parsed["total_methods"], 3);
        assert_eq!(parsed["found_methods"], 8);
        assert!(parsed["completion_percentage"].is_number());
        assert!(parsed["per_struct"]["StringView"].is_object());
        Ok(())
    }

    #[test]
    fn test_generate_json_names_missing_files() -> Result<(), Box<dyn core::error::Error>> {
        let reporter: Reporter = Reporter::new();
        let report: ValidationReport = create_test_report();
        let output: String = reporter.generate_json(&report);

        let parsed: serde_json::Value = serde_json::from_str(&output)?;
        let missing_in: &serde_json::Value =
            &parsed["per_struct"]["StringView"]["methods"]["to_str"]["missing_in"];
        assert!(missing_in.is_array());

        let lua_entry: &serde_json::Value = missing_in
            .as_array()
            .ok_or("missing_in must be an array")?
            .iter()
            .find(|e| e["language"] == "lua")
            .ok_or("expected lua entry")?;
        assert_eq!(lua_entry["files"][0], "sdks/lua/abi/abi.lua");
        Ok(())
    }

    #[test]
    fn test_generate_json_empty_report() -> Result<(), Box<dyn core::error::Error>> {
        let reporter: Reporter = Reporter::new();
        let report: ValidationReport = ValidationReport::new();
        let output: String = reporter.generate_json(&report);

        let parsed: serde_json::Value = serde_json::from_str(&output)?;

        assert_eq!(parsed["is_complete"], true);
        assert_eq!(parsed["total_methods"], 0);
        assert_eq!(parsed["found_methods"], 0);
        assert_eq!(parsed["completion_percentage"], 100.0);
        Ok(())
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
}

//! Language-specific validation modules.
//!
//! Each language has its own validator that uses the ast-grep CLI to detect
//! method/function definitions in SDK files. Lua uses tree-sitter instead
//! since ast-grep doesn't support Lua.
//!
//! Validation is per-file: every target file listed for a language must
//! independently implement every golden method.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::ast_grep::{AstGrepRunner, NamingConvention, transform_name};
use crate::error::ValidatorError;

pub mod cpp;
pub mod csharp;
pub mod js;
pub mod lua;
pub mod python;
pub mod rust;

pub use cpp::CppValidator;
pub use csharp::CSharpValidator;
pub use js::JsValidator;
pub use lua::LuaValidator;
pub use python::PythonValidator;
pub use rust::RustValidator;

/// A method missing from a language SDK, with the files it is missing from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MissingMethod {
    /// The canonical (snake_case) method name.
    pub method: String,
    /// Target files that do not implement the method. Empty when the
    /// language has no target files configured at all.
    pub missing_files: Vec<String>,
}

/// Result of validating a single struct's methods in a language SDK.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ValidationResult {
    /// Name of the struct being validated (e.g., "StringView").
    pub struct_name: String,
    /// Name of the language (e.g., "csharp").
    pub language: String,
    /// Methods found in every target file of the SDK.
    pub found_methods: Vec<String>,
    /// Methods missing from at least one target file.
    pub missing_methods: Vec<MissingMethod>,
}

impl ValidationResult {
    /// Create a new validation result.
    pub fn new(struct_name: String, language: String) -> Self {
        Self {
            struct_name,
            language,
            found_methods: Vec::new(),
            missing_methods: Vec::new(),
        }
    }

    /// Check if all required methods are present in all target files.
    pub fn is_complete(&self) -> bool {
        self.missing_methods.is_empty()
    }
}

/// Outcome of checking one golden variant in one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum VariantOutcome {
    /// Variant present with exactly the golden value.
    Found,
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

/// One golden variant checked against one target file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VariantCheck {
    /// The variant name (PascalCase, identical in all languages).
    pub variant: String,
    /// The golden value.
    pub expected: i64,
    /// The target file checked.
    pub file: String,
    /// The check outcome.
    pub outcome: VariantOutcome,
}

/// A variant found inside the enum construct that is not in the golden set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExtraVariant {
    /// The stale variant name.
    pub variant: String,
    /// Its value, when parseable.
    pub value: Option<i64>,
    /// The target file containing it.
    pub file: String,
}

/// Result of validating one golden enum against one language's target files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EnumValidationResult {
    /// Name of the enum being validated (e.g., "AbiErrorCode").
    pub enum_name: String,
    /// Name of the language (e.g., "lua").
    pub language: String,
    /// One check per (golden variant, target file).
    pub checks: Vec<VariantCheck>,
    /// Stale variants found inside the enum construct.
    pub extra_variants: Vec<ExtraVariant>,
}

impl EnumValidationResult {
    /// Check that every variant matched exactly and nothing extra was found.
    pub fn is_complete(&self) -> bool {
        self.extra_variants.is_empty()
            && self
                .checks
                .iter()
                .all(|check| check.outcome == VariantOutcome::Found)
    }
}

/// Outcome of checking one golden field in one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum FieldOutcome {
    /// Field present in the struct construct.
    Found,
    /// Field absent from the struct construct (or the construct is absent).
    Missing,
}

/// One golden field checked against one target file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FieldCheck {
    /// The canonical (snake_case) field name, for reporting.
    pub field: String,
    /// The target file checked.
    pub file: String,
    /// The check outcome.
    pub outcome: FieldOutcome,
}

/// A field found inside the struct construct that is not in the golden set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExtraField {
    /// The stale field name, in the native spelling as found in the file.
    pub field: String,
    /// The target file containing it.
    pub file: String,
}

/// Result of validating one golden struct against one language's target files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StructFieldValidationResult {
    /// Name of the struct being validated (e.g., "StringView").
    pub struct_name: String,
    /// Name of the language (e.g., "csharp").
    pub language: String,
    /// One check per (golden field, target file).
    pub checks: Vec<FieldCheck>,
    /// Stale fields found inside the struct construct.
    pub extra_fields: Vec<ExtraField>,
    /// Whether the golden fields appear in declaration order in every file.
    pub order_ok: bool,
}

impl StructFieldValidationResult {
    /// Check that every field matched, nothing extra was found, and the
    /// declaration order matches the golden order.
    pub fn is_complete(&self) -> bool {
        self.order_ok
            && self.extra_fields.is_empty()
            && self
                .checks
                .iter()
                .all(|check| check.outcome == FieldOutcome::Found)
    }
}

/// Trait for language-specific SDK validators.
///
/// Implementations answer three questions: does `native_name` have a real
/// definition (not a call site or comment) in `file`, which variants does the
/// enum construct named `enum_name` declare in `file`, and which fields does
/// the struct construct named `struct_name` declare in `file`?
pub trait LanguageValidator {
    /// Get the language name for this validator.
    fn language_name(&self) -> &'static str;

    /// Check whether a method definition exists in a single file.
    ///
    /// `native_name` is already transformed to the language's configured
    /// naming convention.
    ///
    /// # Errors
    ///
    /// Returns a [`ValidatorError`] if the detection tool fails; tool
    /// failures are never silently treated as "missing".
    fn method_in_file(
        &mut self,
        runner: &AstGrepRunner,
        native_name: &str,
        file: &Path,
    ) -> Result<bool, ValidatorError>;

    /// Extract the variants of the enum construct named `enum_name` in `file`.
    ///
    /// Returns one `(variant_name, value)` entry per declared variant, with
    /// `None` for variants lacking a parseable explicit value. An empty list
    /// means the enum construct was not found (or declares no variants) —
    /// both are reported as every golden variant missing.
    ///
    /// Variant names are PascalCase in every language (no naming transform).
    ///
    /// # Errors
    ///
    /// Returns a [`ValidatorError`] if the detection tool fails; tool
    /// failures are never silently treated as "missing".
    fn enum_variants_in_file(
        &mut self,
        runner: &AstGrepRunner,
        enum_name: &str,
        file: &Path,
    ) -> Result<Vec<(String, Option<i64>)>, ValidatorError>;

    /// Extract the field names of the struct construct named `struct_name` in
    /// `file`, in DECLARATION ORDER, in the language's NATIVE field spelling
    /// (C# PascalCase, others snake).
    ///
    /// Leading-underscore padding/marker fields (e.g. Rust's `_marker`) are
    /// included verbatim — the comparator skips them. An empty vec means the
    /// struct construct was not found (reported as every golden field
    /// missing). Comments, call sites, and forward declarations must not
    /// match — only a real field-bearing definition.
    ///
    /// # Errors
    ///
    /// Returns a [`ValidatorError`] if the detection tool fails; tool
    /// failures are never silently treated as "missing".
    fn struct_fields_in_file(
        &mut self,
        runner: &AstGrepRunner,
        struct_name: &str,
        file: &Path,
    ) -> Result<Vec<String>, ValidatorError>;
}

/// Parse a matched variant node text like `Ok = 0` or `Ok: 0`.
///
/// Splits at the first `=` or `:` (whichever comes first); a missing or
/// unparseable right-hand side yields `None` for the value.
pub(crate) fn parse_variant_text(text: &str) -> (String, Option<i64>) {
    let separator: Option<usize> = text.find(['=', ':']);
    match separator {
        Some(index) => {
            let name: &str = text[..index].trim();
            let value: Option<i64> = text[index + 1..]
                .trim()
                .trim_end_matches(',')
                .trim()
                .parse::<i64>()
                .ok();
            (name.to_string(), value)
        }
        None => (text.trim().to_string(), None),
    }
}

/// Parse a field name from a `name: type` declaration (Rust `pub items: *mut T`,
/// TypeScript `ptr: bigint`).
///
/// Takes the text before the first `:`, then the last whitespace-separated
/// token (stripping any leading visibility like `pub`).
pub(crate) fn parse_field_name_typed(text: &str) -> String {
    let before_colon: &str = text.split(':').next().unwrap_or(text).trim();
    before_colon
        .split_whitespace()
        .next_back()
        .unwrap_or(before_colon)
        .to_string()
}

/// Parse a field name from a `... name;` declaration (C# `public uint Code;`,
/// C++ `const uint8_t* ptr;`).
///
/// Drops a trailing `;` and any `= default` initializer, then takes the last
/// identifier token, stripping leading pointer/reference sigils.
pub(crate) fn parse_field_name_trailing(text: &str) -> String {
    let without_semicolon: &str = text.trim().trim_end_matches(';').trim();
    let before_init: &str = without_semicolon
        .split('=')
        .next()
        .unwrap_or(without_semicolon)
        .trim();
    before_init
        .split(|c: char| c.is_whitespace() || c == '*' || c == '&')
        .rfind(|token| !token.is_empty())
        .unwrap_or(before_init)
        .to_string()
}

/// Validate one golden enum against a language's target files, per file.
///
/// Every golden variant must be present with exactly the golden value in
/// EVERY target file, and the enum construct must not declare any variant
/// outside the golden set (a stale variant is drift).
///
/// # Errors
///
/// Returns a [`ValidatorError`] if a target file does not exist or the
/// underlying detection tool fails.
pub fn validate_language_enum(
    validator: &mut dyn LanguageValidator,
    runner: &AstGrepRunner,
    enum_name: &str,
    golden_variants: &BTreeMap<String, i64>,
    target_files: &[PathBuf],
) -> Result<EnumValidationResult, ValidatorError> {
    let language: &'static str = validator.language_name();
    let mut result = EnumValidationResult {
        enum_name: enum_name.to_string(),
        language: language.to_string(),
        checks: Vec::new(),
        extra_variants: Vec::new(),
    };

    for file in target_files {
        if !file.exists() {
            return Err(ValidatorError::TargetFileMissing {
                language: language.to_string(),
                path: file.clone(),
            });
        }
    }

    for file in target_files {
        let found: Vec<(String, Option<i64>)> =
            validator.enum_variants_in_file(runner, enum_name, file)?;
        let file_display: String = file.display().to_string();

        for (variant, expected) in golden_variants {
            let entries: Vec<&(String, Option<i64>)> =
                found.iter().filter(|(name, _)| name == variant).collect();
            let outcome: VariantOutcome = if entries.is_empty() {
                VariantOutcome::Missing
            } else if let Some(entry) = entries.iter().find(|(_, value)| *value != Some(*expected))
            {
                match entry.1 {
                    Some(found_value) => VariantOutcome::WrongValue { found: found_value },
                    None => VariantOutcome::MissingValue,
                }
            } else {
                VariantOutcome::Found
            };
            result.checks.push(VariantCheck {
                variant: variant.clone(),
                expected: *expected,
                file: file_display.clone(),
                outcome,
            });
        }

        for (name, value) in &found {
            if !golden_variants.contains_key(name) {
                result.extra_variants.push(ExtraVariant {
                    variant: name.clone(),
                    value: *value,
                    file: file_display.clone(),
                });
            }
        }
    }

    Ok(result)
}

/// Validate one golden struct against a language's target files, per file.
///
/// Every golden field must be present in EVERY target file, in declaration
/// order, and the struct construct must not declare any field outside the
/// golden set (a stale field is drift). The configured naming convention
/// transforms each canonical snake_case field into the language's native
/// spelling before probing; leading-underscore padding/marker fields are
/// skipped (neither missing nor extra).
///
/// # Errors
///
/// Returns a [`ValidatorError`] if a target file does not exist or the
/// underlying detection tool fails.
pub fn validate_language_struct(
    validator: &mut dyn LanguageValidator,
    runner: &AstGrepRunner,
    naming: NamingConvention,
    struct_name: &str,
    golden_fields: &[String],
    target_files: &[PathBuf],
) -> Result<StructFieldValidationResult, ValidatorError> {
    let language: &'static str = validator.language_name();
    let mut result = StructFieldValidationResult {
        struct_name: struct_name.to_string(),
        language: language.to_string(),
        checks: Vec::new(),
        extra_fields: Vec::new(),
        order_ok: true,
    };

    for file in target_files {
        if !file.exists() {
            return Err(ValidatorError::TargetFileMissing {
                language: language.to_string(),
                path: file.clone(),
            });
        }
    }

    let golden_native: Vec<String> = golden_fields
        .iter()
        .map(|g| transform_name(g, NamingConvention::Snake, naming))
        .collect();

    for file in target_files {
        let native_fields: Vec<String> =
            validator.struct_fields_in_file(runner, struct_name, file)?;
        let kept: Vec<String> = native_fields
            .into_iter()
            .filter(|f| !f.starts_with('_'))
            .collect();
        let file_display: String = file.display().to_string();

        for (i, g_native) in golden_native.iter().enumerate() {
            let outcome: FieldOutcome = if kept.contains(g_native) {
                FieldOutcome::Found
            } else {
                FieldOutcome::Missing
            };
            result.checks.push(FieldCheck {
                field: golden_fields[i].clone(),
                file: file_display.clone(),
                outcome,
            });
        }

        for nf in &kept {
            if !golden_native.contains(nf) {
                result.extra_fields.push(ExtraField {
                    field: nf.clone(),
                    file: file_display.clone(),
                });
            }
        }

        let present: Vec<&String> = kept.iter().filter(|f| golden_native.contains(f)).collect();
        let expected: Vec<&String> = golden_native.iter().filter(|g| kept.contains(g)).collect();
        if present != expected {
            result.order_ok = false;
        }
    }

    Ok(result)
}

/// Validate a language SDK against the golden method set, per file.
///
/// A method counts as found only if every target file implements it. The
/// configured naming convention transforms each canonical snake_case name
/// into the language's native spelling before probing.
///
/// # Errors
///
/// Returns a [`ValidatorError`] if a target file does not exist or the
/// underlying detection tool fails.
pub fn validate_language(
    validator: &mut dyn LanguageValidator,
    runner: &AstGrepRunner,
    naming: NamingConvention,
    struct_name: &str,
    required_methods: &[String],
    target_files: &[PathBuf],
) -> Result<ValidationResult, ValidatorError> {
    let language: &'static str = validator.language_name();
    let mut result: ValidationResult =
        ValidationResult::new(struct_name.to_string(), language.to_string());

    for file in target_files {
        if !file.exists() {
            return Err(ValidatorError::TargetFileMissing {
                language: language.to_string(),
                path: file.clone(),
            });
        }
    }

    for method_name in required_methods {
        let native_name: String = transform_name(method_name, NamingConvention::Snake, naming);

        if target_files.is_empty() {
            result.missing_methods.push(MissingMethod {
                method: method_name.clone(),
                missing_files: Vec::new(),
            });
            continue;
        }

        let mut missing_files: Vec<String> = Vec::new();
        for file in target_files {
            if !validator.method_in_file(runner, &native_name, file)? {
                missing_files.push(file.display().to_string());
            }
        }

        if missing_files.is_empty() {
            result.found_methods.push(method_name.clone());
        } else {
            result.missing_methods.push(MissingMethod {
                method: method_name.clone(),
                missing_files,
            });
        }
    }

    result.found_methods.sort();
    result
        .missing_methods
        .sort_by(|a, b| a.method.cmp(&b.method));

    Ok(result)
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::path::PathBuf;

    use crate::ast_grep::AstGrepRunner;

    /// Resolve an ast-grep runner or panic with the install hint.
    pub(crate) fn runner() -> AstGrepRunner {
        match AstGrepRunner::detect() {
            Ok(runner) => runner,
            Err(_) => panic!(
                "ast-grep CLI not found. Please install ast-grep: https://ast-grep.github.io/guide/introduction.html"
            ),
        }
    }

    /// Resolve a path under the repo root (two levels above this crate).
    pub(crate) fn repo_path(relative: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(relative)
    }

    /// The golden StringView method set.
    pub(crate) fn golden_methods() -> Vec<String> {
        vec![
            "to_str".to_string(),
            "starts_with".to_string(),
            "ends_with".to_string(),
            "strip_prefix".to_string(),
            "split".to_string(),
        ]
    }

    /// The golden enum sets, mirroring `sdk_validator.yaml` (which itself is
    /// kept honest by listing the Rust ABI sources as targets).
    pub(crate) fn golden_enum(name: &str) -> std::collections::BTreeMap<String, i64> {
        let entries: &[(&str, i64)] = match name {
            "AbiErrorCode" => &[
                ("Ok", 0),
                ("Generic", 1),
                ("BufferTooSmall", 2),
                ("Panic", 3),
                ("NotFound", 4),
                ("StaleHandle", 5),
                ("FunctionNotAvailable", 6),
                ("DuplicateProvider", 7),
                ("InvalidPointer", 8),
                ("ReentrantCall", 9),
                ("HostContractNotFound", 100),
                ("HostContractVersionMismatch", 101),
                ("HostContractCallFailed", 102),
            ],
            "LogLevel" => &[
                ("Error", 1),
                ("Warn", 2),
                ("Info", 3),
                ("Debug", 4),
                ("Trace", 5),
            ],
            "DispatchType" => &[("Native", 0), ("VirtualMachine", 1)],
            "ReloadPhaseType" => &[
                ("Preparing", 0),
                ("Reloaded", 1),
                ("Failed", 2),
                ("Unloading", 3),
            ],
            other => panic!("unknown golden enum: {other}"),
        };
        entries
            .iter()
            .map(|(variant, value)| (variant.to_string(), *value))
            .collect()
    }

    /// The golden struct field sets (canonical snake_case, in declaration
    /// order), mirroring `sdk_validator.yaml`.
    pub(crate) fn golden_struct(name: &str) -> Vec<String> {
        let fields: &[&str] = match name {
            "StringView" => &["ptr", "len"],
            "AbiError" => &["code", "message"],
            "Version" => &["major", "minor", "patch"],
            "Array" => &["items", "len", "align"],
            "Buffer" => &["ptr", "len", "cap"],
            "ArenaOverflowBlock" => &["next", "capacity", "used"],
            "CallArena" => &["cur", "end", "base", "host", "first_overflow"],
            "GuestContractInstance" => &["data", "contract_id"],
            "BundleInitContext" => &["bundle_id", "bundle_path"],
            other => panic!("unknown golden struct: {other}"),
        };
        fields.iter().map(|f| f.to_string()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validation_result_new() {
        let result: ValidationResult =
            ValidationResult::new("StringView".to_string(), "rust".to_string());
        assert_eq!(result.struct_name, "StringView");
        assert_eq!(result.language, "rust");
        assert!(result.found_methods.is_empty());
        assert!(result.missing_methods.is_empty());
        assert!(result.is_complete());
    }

    #[test]
    fn test_validate_language_empty_targets_marks_all_missing()
    -> Result<(), Box<dyn core::error::Error>> {
        let runner: AstGrepRunner = test_support::runner();
        let mut validator: RustValidator = RustValidator::new();

        let result: ValidationResult = validate_language(
            &mut validator,
            &runner,
            NamingConvention::Snake,
            "StringView",
            &["to_str".to_string()],
            &[],
        )?;

        assert!(result.found_methods.is_empty());
        assert_eq!(result.missing_methods.len(), 1);
        assert_eq!(result.missing_methods[0].method, "to_str");
        assert!(result.missing_methods[0].missing_files.is_empty());
        Ok(())
    }

    #[test]
    fn test_validate_language_missing_target_file_is_fatal() {
        let runner: AstGrepRunner = test_support::runner();
        let mut validator: RustValidator = RustValidator::new();

        let result: Result<ValidationResult, ValidatorError> = validate_language(
            &mut validator,
            &runner,
            NamingConvention::Snake,
            "StringView",
            &["to_str".to_string()],
            &[PathBuf::from("/nonexistent/file.rs")],
        );

        match result {
            Err(ValidatorError::TargetFileMissing { language, path }) => {
                assert_eq!(language, "rust");
                assert_eq!(path, PathBuf::from("/nonexistent/file.rs"));
            }
            other => panic!("expected TargetFileMissing error, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_variant_text() {
        assert_eq!(parse_variant_text("Ok = 0"), ("Ok".to_string(), Some(0)));
        assert_eq!(parse_variant_text("Ok: 0"), ("Ok".to_string(), Some(0)));
        assert_eq!(
            parse_variant_text("HostContractNotFound = 100,"),
            ("HostContractNotFound".to_string(), Some(100))
        );
        assert_eq!(
            parse_variant_text("Negative = -1"),
            ("Negative".to_string(), Some(-1))
        );
        assert_eq!(parse_variant_text("Bare"), ("Bare".to_string(), None));
        assert_eq!(
            parse_variant_text("Weird = foo"),
            ("Weird".to_string(), None)
        );
    }

    #[test]
    fn test_validate_language_enum_missing_target_file_is_fatal() {
        let runner: AstGrepRunner = test_support::runner();
        let mut validator: RustValidator = RustValidator::new();

        let result: Result<EnumValidationResult, ValidatorError> = validate_language_enum(
            &mut validator,
            &runner,
            "DispatchType",
            &test_support::golden_enum("DispatchType"),
            &[PathBuf::from("/nonexistent/file.rs")],
        );

        assert!(matches!(
            result,
            Err(ValidatorError::TargetFileMissing { .. })
        ));
    }

    #[test]
    fn test_parse_field_name_typed() {
        assert_eq!(parse_field_name_typed("pub items: *mut T"), "items");
        assert_eq!(parse_field_name_typed("ptr: bigint"), "ptr");
        assert_eq!(parse_field_name_typed("_marker: PhantomData<T>"), "_marker");
    }

    #[test]
    fn test_parse_field_name_trailing() {
        assert_eq!(parse_field_name_trailing("public uint Code;"), "Code");
        assert_eq!(parse_field_name_trailing("const uint8_t* ptr;"), "ptr");
        assert_eq!(
            parse_field_name_trailing("ArenaOverflowBlock* first_overflow;"),
            "first_overflow"
        );
        assert_eq!(parse_field_name_trailing("StringView message;"), "message");
    }

    /// A fake validator whose `struct_fields_in_file` returns canned native
    /// field vectors keyed by file path. The method/enum probes are unused.
    struct FakeValidator {
        fields_by_file: std::collections::HashMap<PathBuf, Vec<String>>,
    }

    impl LanguageValidator for FakeValidator {
        fn language_name(&self) -> &'static str {
            "rust"
        }

        fn method_in_file(
            &mut self,
            _runner: &AstGrepRunner,
            _native_name: &str,
            _file: &Path,
        ) -> Result<bool, ValidatorError> {
            Ok(false)
        }

        fn enum_variants_in_file(
            &mut self,
            _runner: &AstGrepRunner,
            _enum_name: &str,
            _file: &Path,
        ) -> Result<Vec<(String, Option<i64>)>, ValidatorError> {
            Ok(Vec::new())
        }

        fn struct_fields_in_file(
            &mut self,
            _runner: &AstGrepRunner,
            _struct_name: &str,
            file: &Path,
        ) -> Result<Vec<String>, ValidatorError> {
            Ok(self.fields_by_file.get(file).cloned().unwrap_or_default())
        }
    }

    fn fake_struct_result(
        naming: NamingConvention,
        golden: &[String],
        native_fields: Vec<String>,
    ) -> Result<StructFieldValidationResult, Box<dyn core::error::Error>> {
        // `validate_language_struct` rejects missing files, so back the fake
        // probe with a real (empty) temp file keyed to its own path.
        let temp: tempfile::NamedTempFile = tempfile::NamedTempFile::new()?;
        let file: PathBuf = temp.path().to_path_buf();
        let mut fields_by_file: std::collections::HashMap<PathBuf, Vec<String>> =
            std::collections::HashMap::new();
        fields_by_file.insert(file.clone(), native_fields);
        let mut validator: FakeValidator = FakeValidator { fields_by_file };
        let runner: AstGrepRunner = test_support::runner();
        let result: StructFieldValidationResult = validate_language_struct(
            &mut validator,
            &runner,
            naming,
            "StringView",
            golden,
            &[file],
        )?;
        Ok(result)
    }

    #[test]
    fn test_validate_struct_exact_match_passes() -> Result<(), Box<dyn core::error::Error>> {
        let golden: Vec<String> = vec!["ptr".to_string(), "len".to_string()];
        let result: StructFieldValidationResult = fake_struct_result(
            NamingConvention::Snake,
            &golden,
            vec!["ptr".to_string(), "len".to_string()],
        )?;
        assert!(result.is_complete(), "unexpected drift: {result:?}");
        Ok(())
    }

    #[test]
    fn test_validate_struct_missing_field() -> Result<(), Box<dyn core::error::Error>> {
        let golden: Vec<String> = vec!["ptr".to_string(), "len".to_string()];
        let result: StructFieldValidationResult =
            fake_struct_result(NamingConvention::Snake, &golden, vec!["ptr".to_string()])?;
        assert!(!result.is_complete());
        let check: &FieldCheck = result
            .checks
            .iter()
            .find(|c| c.field == "len")
            .ok_or("missing len check")?;
        assert_eq!(check.outcome, FieldOutcome::Missing);
        Ok(())
    }

    #[test]
    fn test_validate_struct_extra_field() -> Result<(), Box<dyn core::error::Error>> {
        let golden: Vec<String> = vec!["ptr".to_string(), "len".to_string()];
        let result: StructFieldValidationResult = fake_struct_result(
            NamingConvention::Snake,
            &golden,
            vec!["ptr".to_string(), "len".to_string(), "stale".to_string()],
        )?;
        assert!(!result.is_complete());
        assert_eq!(result.extra_fields.len(), 1);
        assert_eq!(result.extra_fields[0].field, "stale");
        Ok(())
    }

    #[test]
    fn test_validate_struct_out_of_order_sets_order_not_ok()
    -> Result<(), Box<dyn core::error::Error>> {
        let golden: Vec<String> = vec!["ptr".to_string(), "len".to_string()];
        let result: StructFieldValidationResult = fake_struct_result(
            NamingConvention::Snake,
            &golden,
            vec!["len".to_string(), "ptr".to_string()],
        )?;
        // Every field present, nothing extra, but order is wrong.
        assert!(result.extra_fields.is_empty());
        assert!(
            result
                .checks
                .iter()
                .all(|c| c.outcome == FieldOutcome::Found)
        );
        assert!(!result.order_ok);
        assert!(!result.is_complete());
        Ok(())
    }

    #[test]
    fn test_validate_struct_leading_underscore_field_skipped()
    -> Result<(), Box<dyn core::error::Error>> {
        let golden: Vec<String> = vec!["items".to_string(), "len".to_string(), "align".to_string()];
        let result: StructFieldValidationResult = fake_struct_result(
            NamingConvention::Snake,
            &golden,
            vec![
                "items".to_string(),
                "len".to_string(),
                "align".to_string(),
                "_marker".to_string(),
            ],
        )?;
        // The `_marker` field is neither missing nor reported extra.
        assert!(result.is_complete(), "unexpected drift: {result:?}");
        assert!(result.extra_fields.is_empty());
        Ok(())
    }

    #[test]
    fn test_validate_struct_empty_construct_all_missing() -> Result<(), Box<dyn core::error::Error>>
    {
        let golden: Vec<String> = vec!["ptr".to_string(), "len".to_string()];
        let result: StructFieldValidationResult =
            fake_struct_result(NamingConvention::Snake, &golden, Vec::new())?;
        assert!(!result.is_complete());
        assert!(
            result
                .checks
                .iter()
                .all(|c| c.outcome == FieldOutcome::Missing)
        );
        Ok(())
    }

    #[test]
    fn test_validate_struct_pascal_naming() -> Result<(), Box<dyn core::error::Error>> {
        let golden: Vec<String> = vec!["code".to_string(), "contract_id".to_string()];
        let result: StructFieldValidationResult = fake_struct_result(
            NamingConvention::Pascal,
            &golden,
            vec!["Code".to_string(), "ContractId".to_string()],
        )?;
        assert!(result.is_complete(), "unexpected drift: {result:?}");
        Ok(())
    }

    #[test]
    fn test_validate_language_struct_missing_target_file_is_fatal() {
        let runner: AstGrepRunner = test_support::runner();
        let mut validator: FakeValidator = FakeValidator {
            fields_by_file: std::collections::HashMap::new(),
        };

        let result: Result<StructFieldValidationResult, ValidatorError> = validate_language_struct(
            &mut validator,
            &runner,
            NamingConvention::Snake,
            "StringView",
            &["ptr".to_string()],
            &[PathBuf::from("/nonexistent/file.rs")],
        );

        assert!(matches!(
            result,
            Err(ValidatorError::TargetFileMissing { .. })
        ));
    }
}

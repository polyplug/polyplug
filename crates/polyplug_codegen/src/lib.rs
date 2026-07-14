pub mod context;
pub mod data;
pub mod error;
pub mod generate;
pub mod generators;
pub mod ir;
pub mod languages;
pub mod parser;
pub mod reserved;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::PathBuf;

pub use error::PolyplugcError;
pub use generate::WriteSummary;
pub use generate::generate;
pub use generate::generate_internal_cpp;
pub use generate::generate_internal_csharp;
pub use generate::generate_internal_javascript;
pub use generate::generate_internal_lua;
pub use generate::generate_internal_python;
pub use generate::generate_internal_rust;
pub use generate::parse_lang;
pub use generate::write_output;

/// Key for platform-specific file entries (os + arch).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlatformKey {
    pub os: String,
    pub arch: String,
}

/// The resolved external artifact field from bundle.toml.
#[derive(Debug, Clone)]
pub enum ResolvedBundleFile {
    /// Internal generation intentionally has no artifact path.
    Absent,
    Single(String),
    PlatformMap(HashMap<PlatformKey, String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lang {
    Rust,
    Cpp,
    CSharp,
    Python,
    Lua,
    JsQuickJs,
}

impl Lang {
    pub fn as_str(&self) -> &'static str {
        match self {
            Lang::Rust => "rust",
            Lang::Cpp => "cpp",
            Lang::CSharp => "csharp",
            Lang::Python => "python",
            Lang::Lua => "lua",
            Lang::JsQuickJs => "js-quickjs",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Host,
    Guest,
}

/// A semantic section of generated output.
///
/// Paths are deliberately not part of this model: a language generator assigns every
/// file to one partition, and the output writer decides its destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OutputPartition {
    /// Private ABI adapters, callers, entry points, manifests, and module glue.
    Bindings,
    /// Application-owned structs, enums, and flags.
    DomainTypes,
    /// Guest-facing contract declarations.
    GuestContracts,
}

/// A language adapter validated import path.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ValidatedImport {
    lang: Lang,
    value: String,
}

impl ValidatedImport {
    /// Parse an import specifier for `lang`.
    ///
    /// Every language has its own source-level grammar so an import cannot alter
    /// the generated declaration that contains it.
    pub fn parse(lang: Lang, value: impl Into<String>) -> Result<Self, PolyplugcError> {
        let value: String = value.into();
        let valid_common = !value.is_empty()
            && !value.chars().any(char::is_control)
            && !value.chars().any(char::is_whitespace)
            && !value.split(['/', '\\']).any(|segment| segment == "..");
        let valid_language = match lang {
            Lang::Rust => valid_rust_import(&value),
            Lang::Cpp => valid_cpp_include(&value),
            Lang::CSharp => valid_csharp_namespace(&value),
            Lang::Python => valid_python_module(&value),
            Lang::Lua => valid_lua_module(&value),
            Lang::JsQuickJs => valid_js_module_specifier(&value),
        };
        if valid_common && valid_language {
            Ok(Self { lang, value })
        } else {
            Err(PolyplugcError::ValidationFailed {
                message: format!("invalid {} import specifier `{value}`", lang.as_str()),
            })
        }
    }

    pub fn as_str(&self) -> &str {
        &self.value
    }

    pub fn language(&self) -> Lang {
        self.lang
    }
}

const RUST_KEYWORDS: &[&str] = &[
    "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "fn", "for",
    "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return",
    "self", "Self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use", "where",
    "while", "async", "await", "dyn",
];

const CSHARP_KEYWORDS: &[&str] = &[
    "abstract",
    "as",
    "base",
    "bool",
    "break",
    "byte",
    "case",
    "catch",
    "char",
    "checked",
    "class",
    "const",
    "continue",
    "decimal",
    "default",
    "delegate",
    "do",
    "double",
    "else",
    "enum",
    "event",
    "explicit",
    "extern",
    "false",
    "finally",
    "fixed",
    "float",
    "for",
    "foreach",
    "goto",
    "if",
    "implicit",
    "in",
    "int",
    "interface",
    "internal",
    "is",
    "lock",
    "long",
    "namespace",
    "new",
    "null",
    "object",
    "operator",
    "out",
    "override",
    "params",
    "private",
    "protected",
    "public",
    "readonly",
    "ref",
    "return",
    "sbyte",
    "sealed",
    "short",
    "sizeof",
    "stackalloc",
    "static",
    "string",
    "struct",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "uint",
    "ulong",
    "unchecked",
    "unsafe",
    "ushort",
    "using",
    "virtual",
    "void",
    "volatile",
    "while",
    "add",
    "alias",
    "ascending",
    "async",
    "await",
    "by",
    "descending",
    "dynamic",
    "equals",
    "from",
    "get",
    "global",
    "group",
    "init",
    "into",
    "join",
    "let",
    "nameof",
    "not",
    "notnull",
    "on",
    "or",
    "orderby",
    "partial",
    "remove",
    "select",
    "set",
    "unmanaged",
    "value",
    "var",
    "when",
    "where",
    "with",
    "yield",
];

const PYTHON_KEYWORDS: &[&str] = &[
    "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class", "continue",
    "def", "del", "elif", "else", "except", "finally", "for", "from", "global", "if", "import",
    "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while",
    "with", "yield",
];

fn valid_rust_import(value: &str) -> bool {
    for (index, segment) in value.split("::").enumerate() {
        if segment == "crate" || segment == "self" || segment == "super" {
            if index != 0 {
                return false;
            }
            continue;
        }
        if !valid_identifier_segment(segment, RUST_KEYWORDS, true) {
            return false;
        }
    }
    true
}

fn valid_cpp_include(value: &str) -> bool {
    !value.starts_with('/') && value.split('/').all(valid_cpp_include_segment)
}

fn valid_cpp_include_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment != "."
        && segment != ".."
        && segment.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.' | '+')
        })
}

fn valid_csharp_namespace(value: &str) -> bool {
    valid_identifier_path(value, '.', CSHARP_KEYWORDS, false)
}

fn valid_python_module(value: &str) -> bool {
    valid_identifier_path(value, '.', PYTHON_KEYWORDS, false)
}

fn valid_lua_module(value: &str) -> bool {
    valid_identifier_path(value, '.', &[], false)
}

fn valid_identifier_path(
    value: &str,
    separator: char,
    keywords: &[&str],
    reject_underscore: bool,
) -> bool {
    value
        .split(separator)
        .all(|segment| valid_identifier_segment(segment, keywords, reject_underscore))
}

fn valid_identifier_segment(segment: &str, keywords: &[&str], reject_underscore: bool) -> bool {
    let mut characters = segment.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && (!reject_underscore || segment != "_")
        && !keywords.contains(&segment)
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn valid_js_module_specifier(value: &str) -> bool {
    if let Some(path) = value.strip_prefix("file:///") {
        return valid_js_path(path);
    }
    if let Some(path) = value.strip_prefix("./") {
        return valid_js_path(path);
    }
    if let Some(scoped) = value.strip_prefix('@') {
        let Some((scope, package_path)) = scoped.split_once('/') else {
            return false;
        };
        return valid_js_package_segment(scope)
            && package_path.split('/').all(valid_js_package_segment);
    }
    value.split('/').all(valid_js_package_segment)
}

fn valid_js_path(path: &str) -> bool {
    let mut segments = path.split('/');
    let Some(first) = segments.next() else {
        return false;
    };
    (valid_js_path_segment(first) || valid_windows_drive(first))
        && segments.all(valid_js_path_segment)
}

fn valid_js_path_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment != "."
        && segment != ".."
        && segment.chars().all(valid_js_module_character)
}

fn valid_windows_drive(segment: &str) -> bool {
    matches!(segment.as_bytes(), [letter, b':'] if letter.is_ascii_alphabetic())
}

fn valid_js_package_segment(segment: &str) -> bool {
    let mut characters = segment.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    first.is_ascii_alphanumeric() && characters.all(valid_js_module_character)
}

fn valid_js_module_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.' | '~')
}

/// Where one semantic generated partition is made available.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputDestination {
    /// Keep the partition in the primary generated output root.
    Inline,
    /// Emit the partition beneath a separate root and use `import` from consumers.
    Emit {
        root: PathBuf,
        import: ValidatedImport,
    },
    /// Do not emit this partition; consumers use the supplied external import.
    ImportOnly { import: ValidatedImport },
    /// Do not emit or reference this partition.
    Omit,
}

impl OutputDestination {
    pub fn import(&self) -> Option<&ValidatedImport> {
        match self {
            Self::Emit { import, .. } | Self::ImportOnly { import } => Some(import),
            Self::Inline | Self::Omit => None,
        }
    }
}

/// The single semantic output layout shared by every language adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputLayout {
    pub bindings: OutputDestination,
    pub domain_types: OutputDestination,
    pub guest_contracts: OutputDestination,
}

impl OutputLayout {
    /// Preserve the established all-in-one generated tree.
    pub const fn unified() -> Self {
        Self {
            bindings: OutputDestination::Inline,
            domain_types: OutputDestination::Inline,
            guest_contracts: OutputDestination::Inline,
        }
    }

    pub fn destination(&self, partition: OutputPartition) -> &OutputDestination {
        match partition {
            OutputPartition::Bindings => &self.bindings,
            OutputPartition::DomainTypes => &self.domain_types,
            OutputPartition::GuestContracts => &self.guest_contracts,
        }
    }

    /// Validate every semantic reference and import against the generation language.
    pub fn validate(&self, lang: Lang, files: &[GeneratedFile]) -> Result<(), PolyplugcError> {
        for destination in [&self.bindings, &self.domain_types, &self.guest_contracts] {
            if let Some(import) = destination.import() {
                if import.language() != lang {
                    return Err(PolyplugcError::ValidationFailed {
                        message: format!(
                            "{} import cannot be used for {} generation",
                            import.language().as_str(),
                            lang.as_str()
                        ),
                    });
                }
            }
        }
        self.validate_references(files)
    }

    pub fn validate_references(&self, files: &[GeneratedFile]) -> Result<(), PolyplugcError> {
        for file in files {
            let source = self.destination(file.partition);
            if matches!(source, OutputDestination::Omit) {
                continue;
            }
            for reference in &file.references {
                let target = self.destination(*reference);
                if matches!(target, OutputDestination::Omit) {
                    return Err(PolyplugcError::ValidationFailed {
                        message: format!(
                            "generated {} file `{}` references omitted {} partition",
                            partition_name(file.partition),
                            file.path.display(),
                            partition_name(*reference),
                        ),
                    });
                }
                if !same_output_root(source, target) && target.import().is_none() {
                    return Err(PolyplugcError::ValidationFailed {
                        message: format!(
                            "generated {} file `{}` cannot resolve {} without an import",
                            partition_name(file.partition),
                            file.path.display(),
                            partition_name(*reference),
                        ),
                    });
                }
            }
        }
        Ok(())
    }
}

fn same_output_root(left: &OutputDestination, right: &OutputDestination) -> bool {
    match (left, right) {
        (OutputDestination::Inline, OutputDestination::Inline) => true,
        (
            OutputDestination::Emit { root: left, .. },
            OutputDestination::Emit { root: right, .. },
        ) => left == right,
        _ => false,
    }
}

impl Default for OutputLayout {
    fn default() -> Self {
        Self::unified()
    }
}

fn partition_name(partition: OutputPartition) -> &'static str {
    match partition {
        OutputPartition::Bindings => "bindings",
        OutputPartition::DomainTypes => "domain types",
        OutputPartition::GuestContracts => "guest contracts",
    }
}

/// Public configuration for [`generate()`].
///
/// # Source-breaking layout migration
///
/// Struct-literal callers written before output layouts must make two source
/// edits: remove the former `out_dir` field and add
/// `layout: OutputLayout::unified()` to retain the former all-in-one output
/// tree.
///
/// Before:
///
/// ```rust,ignore
/// use polyplug_codegen::{GenerateConfig, Lang, Side};
/// use std::path::PathBuf;
///
/// let config = GenerateConfig {
///     api_toml: PathBuf::from("api.toml"),
///     out_dir: PathBuf::from("generated"),
///     lang: Lang::Rust,
///     side: Side::Guest,
/// };
/// ```
///
/// After both required source edits:
///
/// ```
/// use polyplug_codegen::{GenerateConfig, Lang, OutputLayout, Side};
/// use std::path::PathBuf;
///
/// let config = GenerateConfig {
///     api_toml: PathBuf::from("api.toml"),
///     lang: Lang::Rust,
///     side: Side::Guest,
///     layout: OutputLayout::unified(),
/// };
/// ```
///
/// [`generate()`] is the supported library entry point. There is no public
/// low-level IR generation entry point.
#[derive(Debug)]
pub struct GenerateConfig {
    pub api_toml: PathBuf,
    pub lang: Lang,
    pub side: Side,
    pub layout: OutputLayout,
}

/// Configuration for the opt-in Rust internal-plugin generation profile.
///
/// `layout` is required; use [`OutputLayout::unified`] to retain the
/// established all-in-one generated tree.
#[derive(Debug)]
pub struct InternalRustGenerateConfig {
    pub bundle_toml: PathBuf,
    pub layout: OutputLayout,
}

/// Configuration for the opt-in C++ internal-plugin generation profile.
///
/// `layout` is required; use [`OutputLayout::unified`] to retain the
/// established all-in-one generated tree.
#[derive(Debug)]
pub struct InternalCppGenerateConfig {
    pub bundle_toml: PathBuf,
    pub out_dir: PathBuf,
    pub layout: OutputLayout,
}

/// Configuration for the opt-in C# internal-plugin generation profile.
///
/// `layout` is required; use [`OutputLayout::unified`] to retain the
/// established all-in-one generated tree.
#[derive(Debug)]
pub struct InternalCSharpGenerateConfig {
    pub bundle_toml: PathBuf,
    pub out_dir: PathBuf,
    pub layout: OutputLayout,
}

/// Configuration for the opt-in JavaScript internal-plugin generation profile.
///
/// `layout` is required; use [`OutputLayout::unified`] to retain the
/// established all-in-one generated tree.
#[derive(Debug)]
pub struct InternalJavaScriptGenerateConfig {
    pub bundle_toml: PathBuf,
    pub out_dir: PathBuf,
    pub layout: OutputLayout,
}

/// Configuration for the opt-in Lua internal-plugin generation profile.
///
/// `layout` is required; use [`OutputLayout::unified`] to retain the
/// established all-in-one generated tree.
#[derive(Debug)]
pub struct InternalLuaGenerateConfig {
    pub bundle_toml: PathBuf,
    pub out_dir: PathBuf,
    pub layout: OutputLayout,
}

/// Configuration for the opt-in Python internal-plugin generation profile.
///
/// `layout` is required; use [`OutputLayout::unified`] to retain the
/// established all-in-one generated tree.
#[derive(Debug)]
pub struct InternalPythonGenerateConfig {
    pub bundle_toml: PathBuf,
    pub out_dir: PathBuf,
    pub layout: OutputLayout,
}

#[derive(Debug, Clone)]
pub struct GeneratedFile {
    pub path: PathBuf,
    pub content: String,
    /// When true, the file is always (re)written even if its on-disk content is
    /// byte-identical to what would be emitted. Set for files like `manifest.toml`
    /// whose contents must always reflect the current contract ids; left false for
    /// language bindings so a no-op regeneration preserves their mtimes and does not
    /// cascade downstream rebuilds.
    pub force_regenerate: bool,
    /// Semantic output section. This is intentionally independent of `path`.
    pub partition: OutputPartition,
    /// Semantic partitions this file imports from. The writer rejects omitted
    /// destinations before creating any output directories.
    pub references: Vec<OutputPartition>,
}

pub struct GenerateOutput {
    layout: OutputLayout,
    lang: Lang,
    pub files: Vec<GeneratedFile>,
}

impl GenerateOutput {
    pub fn new(lang: Lang, layout: OutputLayout) -> Self {
        Self {
            layout,
            lang,
            files: Vec::new(),
        }
    }

    pub fn from_files(lang: Lang, layout: OutputLayout, files: Vec<GeneratedFile>) -> Self {
        Self {
            layout,
            lang,
            files,
        }
    }

    pub fn layout(&self) -> &OutputLayout {
        &self.layout
    }

    pub fn language(&self) -> Lang {
        self.lang
    }
}

impl Default for GenerateOutput {
    fn default() -> Self {
        Self::new(Lang::Rust, OutputLayout::unified())
    }
}

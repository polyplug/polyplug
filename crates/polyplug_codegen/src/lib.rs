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
pub use generate::generate_ir;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug)]
pub struct GenerateConfig {
    pub api_toml: PathBuf,
    pub lang: Lang,
    pub side: Side,
    pub out_dir: PathBuf,
}

/// Configuration for the opt-in Rust internal-plugin generation profile.
///
/// This intentionally stays separate from [`GenerateConfig`] so existing struct
/// literals and the default external generation contract remain unchanged.
#[derive(Debug)]
pub struct InternalRustGenerateConfig {
    pub bundle_toml: PathBuf,
    pub out_dir: PathBuf,
}

/// Configuration for the opt-in C++ internal-plugin generation profile.
///
/// This intentionally stays separate from [`GenerateConfig`] so existing struct
/// literals and the default external generation contract remain unchanged.
#[derive(Debug)]
pub struct InternalCppGenerateConfig {
    pub bundle_toml: PathBuf,
    pub out_dir: PathBuf,
}

/// Configuration for the opt-in C# internal-plugin generation profile.
///
/// This intentionally stays separate from [`GenerateConfig`] so existing struct
/// literals and the default external generation contract remain unchanged.
#[derive(Debug)]
pub struct InternalCSharpGenerateConfig {
    pub bundle_toml: PathBuf,
    pub out_dir: PathBuf,
}

/// Configuration for the opt-in JavaScript internal-plugin generation profile.
///
/// This intentionally stays separate from [`GenerateConfig`] so existing struct
/// literals and the default external generation contract remain unchanged.
#[derive(Debug)]
pub struct InternalJavaScriptGenerateConfig {
    pub bundle_toml: PathBuf,
    pub out_dir: PathBuf,
}

/// Configuration for the opt-in Lua internal-plugin generation profile.
///
/// This intentionally stays separate from [`GenerateConfig`] so existing struct
/// literals and the default external generation contract remain unchanged.
#[derive(Debug)]
pub struct InternalLuaGenerateConfig {
    pub bundle_toml: PathBuf,
    pub out_dir: PathBuf,
}

/// Configuration for the opt-in Python internal-plugin generation profile.
///
/// This intentionally stays separate from [`GenerateConfig`] so existing struct
/// literals and the default external generation contract remain unchanged.
#[derive(Debug)]
pub struct InternalPythonGenerateConfig {
    pub bundle_toml: PathBuf,
    pub out_dir: PathBuf,
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
}

#[derive(Debug, Clone, Default)]
pub struct GenerateOutput {
    pub files: Vec<GeneratedFile>,
}

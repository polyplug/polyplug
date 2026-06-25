pub mod context;
pub mod data;
pub mod error;
pub mod languages;
pub mod reserved;

use std::collections::HashMap;
use std::path::PathBuf;

pub use error::PolyplugcError;

/// Key for platform-specific file entries (os + arch).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlatformKey {
    pub os: String,
    pub arch: String,
}

/// The resolved file field from bundle.toml — either a single path or platform map.
#[derive(Debug, Clone)]
pub enum ResolvedBundleFile {
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

#[derive(Debug)]
pub struct GenerateOutput {
    pub files: Vec<GeneratedFile>,
}

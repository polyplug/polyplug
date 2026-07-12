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
pub use generate::generate_ir;
pub use generate::generate_ir_rust_guest;
pub use generate::generate_rust_guest;
pub use generate::parse_lang;
pub use generate::write_output;

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
/// Selects how Rust guest bindings are linked into a consumer.
///
/// [`Self::Disk`] preserves the `polyplugc` disk-bundle ABI, including the
/// loader entry point and author factory symbols. [`Self::InProcess`] emits
/// runtime-local factories and a canonical manifest registration helper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RustGuestMode {
    /// Generate the disk-loaded guest ABI used by `polyplugc`.
    Disk,
    /// Generate guest bindings registered by a Rust host at runtime.
    InProcess {
        /// Stable bundle name used by the runtime to derive its ID.
        bundle_name: String,
    },
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

#[derive(Debug, Clone, Default)]
pub struct GenerateOutput {
    pub files: Vec<GeneratedFile>,
}

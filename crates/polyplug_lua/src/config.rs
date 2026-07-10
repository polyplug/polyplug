//! Configuration types for the Lua plugin loader.

/// Lua implementation variant.
///
/// NOTE: Epic 11 supports LuaJIT only at compile time.
/// This enum is kept for future extensibility documentation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LuaVersion {
    /// LuaJIT (vendored by default; `external-luajit` links target-provided LuaJIT).
    LuaJit,
}

/// Configuration for the Lua plugin loader.
#[derive(Debug, Clone)]
pub struct LuaConfig {
    /// The Lua implementation to use. Currently only `LuaJit` is supported.
    pub version: LuaVersion,
}

impl Default for LuaConfig {
    fn default() -> Self {
        Self {
            version: LuaVersion::LuaJit,
        }
    }
}

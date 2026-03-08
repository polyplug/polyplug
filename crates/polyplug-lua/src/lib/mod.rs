//! polyplug-lua — Lua VM adapter for the polyplug runtime.
//!
//! This crate teaches `polyplug` how to load Lua plugin bundles
//! via Lua VM embedding.
//!
//! # Status
//! **Stub** — this crate is scaffolded infrastructure. The actual Lua loading
//! logic is implemented in Epic 11. All `load()` calls currently return
//! `Err(LoaderError::RuntimeNotImplemented { runtime_name: "lua" })`.
//!
//! # Usage
//! ```rust,ignore
//! use polyplug::runtime::RuntimeBuilder;
//! use polyplug_lua::LuaLoader;
//!
//! let runtime = RuntimeBuilder::new()
//!     .loader(LuaLoader::new())
//!     .build()?;
//! ```

use std::path::Path;

use polyplug::abi::PluginRegistrar;
use polyplug::error::LoaderError;
use polyplug::error::PolyplugError;
use polyplug::loader::BundleLoader;

/// Stub loader for Lua plugin bundles.
///
/// Returns `Err(LoaderError::RuntimeNotImplemented { runtime_name: "lua" })`
/// until Epic 11 implements Lua VM embedding.
pub struct LuaLoader;

impl LuaLoader {
    /// Create a new `LuaLoader`.
    pub fn new() -> LuaLoader {
        LuaLoader
    }
}

impl Default for LuaLoader {
    fn default() -> LuaLoader {
        LuaLoader::new()
    }
}

impl BundleLoader for LuaLoader {
    fn runtime_name(&self) -> &'static str {
        "lua"
    }

    fn load(&self, _path: &Path, _registrar: &mut PluginRegistrar) -> Result<(), PolyplugError> {
        Err(PolyplugError::Loader(LoaderError::RuntimeNotImplemented {
            runtime_name: "lua".to_owned(),
        }))
    }
}

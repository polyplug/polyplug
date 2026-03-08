//! polyplug-dotnet — .NET (CLR) adapter for the polyplug runtime.
//!
//! This crate teaches `polyplug` how to load standard .NET plugin bundles
//! via Microsoft's `hostfxr` API.
//!
//! # Status
//! **Stub** — this crate is scaffolded infrastructure. The actual .NET loading
//! logic is implemented in Epic 9. All `load()` calls currently return
//! `Err(LoaderError::RuntimeNotImplemented { runtime_name: "dotnet" })`.
//!
//! # Usage
//! ```rust,ignore
//! use polyplug::runtime::RuntimeBuilder;
//! use polyplug_dotnet::DotnetLoader;
//!
//! let runtime = RuntimeBuilder::new()
//!     .loader(DotnetLoader::new())
//!     .build()?;
//! ```

use std::path::Path;

use polyplug::abi::PluginRegistrar;
use polyplug::error::LoaderError;
use polyplug::error::PolyplugError;
use polyplug::loader::BundleLoader;

/// Stub loader for .NET (CLR) plugin bundles.
///
/// Returns `Err(LoaderError::RuntimeNotImplemented { runtime_name: "dotnet" })`
/// until Epic 9 implements the `hostfxr` integration.
pub struct DotnetLoader;

impl DotnetLoader {
    /// Create a new `DotnetLoader`.
    pub fn new() -> DotnetLoader {
        DotnetLoader
    }
}

impl Default for DotnetLoader {
    fn default() -> DotnetLoader {
        DotnetLoader::new()
    }
}

impl BundleLoader for DotnetLoader {
    fn runtime_name(&self) -> &'static str {
        "dotnet"
    }

    fn load(&self, _path: &Path, _registrar: &mut PluginRegistrar) -> Result<(), PolyplugError> {
        Err(PolyplugError::Loader(LoaderError::RuntimeNotImplemented {
            runtime_name: "dotnet".to_owned(),
        }))
    }
}

//! DotnetConfig — configuration for the .NET CLR loader.

use std::path::PathBuf;

/// Configuration for the .NET CLR loader.
///
/// The app developer provides this at init to declare the minimum
/// .NET framework version they support.
///
/// # Example
/// ```rust,ignore
/// use polyplug_dotnet::DotnetConfig;
/// let config = DotnetConfig::default();
/// ```
#[derive(Debug, Clone)]
pub struct DotnetConfig {
    /// Minimum acceptable .NET framework version, e.g. `"net10.0"`.
    /// Used to generate the `runtimeconfig.json` for hostfxr initialization.
    pub min_framework: String,
    /// Strategy for locating the hostfxr library.
    pub hostfxr: HostfxrLocation,
}

impl Default for DotnetConfig {
    fn default() -> DotnetConfig {
        DotnetConfig {
            min_framework: String::from("net10.0"),
            hostfxr: HostfxrLocation::Auto,
        }
    }
}

/// Location strategy for the hostfxr library.
#[derive(Debug, Clone, Default)]
pub enum HostfxrLocation {
    /// Automatically locate hostfxr using nethost.
    #[default]
    Auto,
    /// Use the hostfxr library at the given path.
    Path(PathBuf),
}

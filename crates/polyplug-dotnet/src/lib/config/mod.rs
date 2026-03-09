//! DotnetConfig — configuration for the .NET CLR loader.

/// Configuration for the .NET CLR loader.
///
/// The app developer provides this at init to declare the minimum
/// .NET framework version they support.
///
/// # Example
/// ```rust
/// use polyplug_dotnet::DotnetConfig;
/// let config = DotnetConfig { min_framework: String::from("net10.0") };
/// ```
#[derive(Debug, Clone)]
pub struct DotnetConfig {
    /// Minimum acceptable .NET framework version, e.g. `"net10.0"`.
    /// Used to generate the `runtimeconfig.json` for hostfxr initialization.
    pub min_framework: String,
}

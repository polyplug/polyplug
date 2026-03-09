//! DotnetConfig — configuration for the .NET CLR loader.

/// Configuration for the .NET CLR loader.
#[derive(Debug, Clone)]
pub struct DotnetConfig {
    /// Minimum acceptable .NET framework version, e.g. `"net10.0"`.
    pub min_framework: String,
}

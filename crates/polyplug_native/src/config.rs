//! Native loader configuration

/// Configuration for the native plugin loader.
/// Native plugins require no special configuration.
#[derive(Debug, Clone)]
pub struct NativeConfig {}

impl Default for NativeConfig {
    fn default() -> Self {
        Self {}
    }
}

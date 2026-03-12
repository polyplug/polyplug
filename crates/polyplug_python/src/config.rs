//! PythonConfig — configuration for the CPython loader.

/// Configuration for the CPython plugin loader.
///
/// # Example
/// ```rust,ignore
/// use polyplug_python::PythonConfig;
/// let config = PythonConfig::default();
/// ```
#[derive(Debug, Clone)]
pub struct PythonConfig {
    /// Minimum acceptable Python version as (major, minor).
    /// e.g. `(3, 11)` requires Python 3.11 or higher.
    pub min_version: (u32, u32),
}

impl Default for PythonConfig {
    fn default() -> PythonConfig {
        PythonConfig {
            min_version: (3, 11),
        }
    }
}

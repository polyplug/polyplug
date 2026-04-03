use crate::{error::RuntimeError, loader::manifest::ManifestData, runtime::Runtime};

/// Trait implemented by all bundle loaders (native, python, lua, js, .net).
///
/// The runtime dispatches each bundle to the loader whose `runtime_name()`
/// matches the `runtime` field in the bundle's `manifest.toml`.
pub trait BundleLoader: Send + Sync {
    /// The runtime identifier this loader handles.
    ///
    /// Must match the `runtime` field in `manifest.toml` exactly (case-sensitive).
    fn runtime_name(&self) -> &'static str;

    /// All runtime identifiers this loader handles.
    ///
    /// Defaults to a single-element vec containing `runtime_name()`.
    /// Override this method if your loader handles multiple runtime names.
    fn runtime_names(&self) -> Vec<String> {
        vec![self.runtime_name().to_owned()]
    }

    /// Load a bundle for the first time.
    ///
    /// The manifest contains all metadata needed to load the bundle:
    /// - `manifest.path` - the bundle directory
    /// - `manifest.file` - the plugin file (relative to bundle directory)
    /// - `manifest.id` - the bundle ID
    ///
    /// # Errors
    /// Returns `Err(RuntimeError::...)` on any failure.
    fn load(&self, manifest: &ManifestData, runtime: &Runtime) -> Result<(), RuntimeError>;

    /// Reload a bundle - MANDATORY for all loaders.
    ///
    /// Called when a bundle needs to be hot-reloaded (e.g., file changed).
    ///
    /// Implementation must:
    /// 1. Load/reload the bundle code (loader-specific mechanism)
    /// 2. Call init to get new vtables
    /// 3. Register new vtables with registry (vtable swap happens in registry)
    /// 4. Return Ok(()) - runtime handles callback and quiescence wait
    ///
    /// # Safety Contract
    /// After return, old resources should be cleaned up:
    /// - Native: drop old library (caller must not have cached raw pointers)
    /// - VMs: let GC handle cleanup
    ///
    /// # Errors
    /// Returns `Err(RuntimeError::...)` on any failure.
    fn reload(&self, manifest: &ManifestData, runtime: &Runtime) -> Result<(), RuntimeError>;
}

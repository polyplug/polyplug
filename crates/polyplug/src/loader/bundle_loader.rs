use crate::{
    error::RuntimeError,
    loader::{bundle_source::BundleSource, manifest::ManifestData},
    runtime::Runtime,
};

/// Trait implemented by all bundle loaders (native, python, lua, js, .net).
///
/// The runtime dispatches each bundle to the loader whose `runtime_name()`
/// matches the `runtime` field in the bundle's `manifest.toml`.
pub trait BundleLoader: Send + Sync {
    /// The runtime identifier this loader handles.
    ///
    /// Must match the `runtime` field in `manifest.toml` exactly (case-sensitive).
    fn runtime_name(&self) -> &'static str;

    /// Load a bundle for the first time.
    ///
    /// The manifest carries the bundle metadata:
    /// - `manifest.file` - the plugin file (relative to the bundle directory)
    /// - `manifest.id` - the bundle ID
    ///
    /// `source` selects where the executable artifact comes from:
    /// - [`BundleSource::Path`] - an on-disk bundle directory (path-based loading).
    ///   `manifest.path` holds the same directory and remains the resolution root.
    /// - [`BundleSource::Code`] / [`BundleSource::Bytes`] - in-memory sources with no
    ///   bundle directory. The native loader rejects these; VM loaders reject them
    ///   until they gain real in-memory support.
    ///
    /// # Errors
    /// Returns `Err(RuntimeError::...)` on any failure, including
    /// `RuntimeError::Loader(LoaderError::UnsupportedBundleSource { .. })` when the
    /// loader does not support the given source kind.
    fn load(
        &self,
        manifest: &ManifestData,
        source: &BundleSource,
        runtime: &Runtime,
    ) -> Result<(), RuntimeError>;

    /// Reload a bundle - MANDATORY for all loaders.
    ///
    /// Called when a bundle needs to be hot-reloaded (e.g., file changed).
    ///
    /// Implementation must:
    /// 1. Load/reload the bundle code (loader-specific mechanism)
    /// 2. Call init to get new interfaces
    /// 3. Register new interfaces with registry (interface swap happens in registry)
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

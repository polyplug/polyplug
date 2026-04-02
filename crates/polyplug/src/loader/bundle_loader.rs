use crate::{error::RuntimeError, loader::manifest::ManifestData, runtime::Runtime};

/// Trait implemented by all bundle loaders (native and adapter crates).
///
/// The runtime dispatches each bundle to the loader whose `runtime_name()`
/// matches the `runtime` field in the bundle's `manifest.toml`.
pub trait BundleLoader: Send + Sync {
    /// The runtime identifier this loader handles.
    ///
    /// Must match the `runtime` field in `manifest.toml` exactly (case-sensitive).
    /// The built-in native loader returns `"native"`.
    fn runtime_name(&self) -> &'static str;

    /// All runtime identifiers this loader handles.
    ///
    /// Defaults to a single-element vec containing `runtime_name()`.
    /// Override this method if your loader handles multiple runtime names.
    /// `JsLoader` does NOT need to override this — each `JsLoader` instance handles exactly one name.
    fn runtime_names(&self) -> Vec<String> {
        vec![self.runtime_name().to_owned()]
    }

    /// Load a bundle given its manifest.
    ///
    /// The manifest contains all metadata needed to load the bundle:
    /// - `manifest.path` - the bundle directory
    /// - `manifest.file` - the plugin file (relative to bundle directory)
    /// - `manifest.id` - the bundle ID
    ///
    /// # Errors
    /// Returns `Err(RuntimeError::...)` on any failure. For stub loaders,
    /// returns `Err(RuntimeError::Loader(LoaderError::JsRuntimePanic { .. }))`.
    fn load(&self, manifest: &ManifestData, runtime: &Runtime) -> Result<(), RuntimeError>;
}

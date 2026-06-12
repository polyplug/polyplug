use polyplug_utils::BundleId;

use crate::{
    error::RuntimeError,
    loader::{bundle_source::BundleSource, manifest::ManifestData},
    runtime::Runtime,
};

/// Trait implemented by all bundle loaders (native, python, lua, js, .net).
///
/// The runtime dispatches each bundle to the loader whose `loader_name()`
/// matches the `loader` field in the bundle's `manifest.toml`.
pub trait BundleLoader: Send + Sync {
    /// The loader identifier this loader handles.
    ///
    /// Must match the `loader` field in `manifest.toml` exactly (case-sensitive).
    fn loader_name(&self) -> &'static str;

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

    /// Reclaim a bundle's loader-owned resources after it has been invalidated.
    ///
    /// The runtime calls this from [`Runtime::unload_bundle`] *after*
    /// `RuntimeStore::invalidate_bundle` has removed the bundle from the registry
    /// indices and bumped its slots' generations. By that point no *new* dispatch
    /// can resolve to this bundle, so the loader only has to account for dispatches
    /// already in flight.
    ///
    /// The default implementation is a no-op: invalidate-only loaders (python,
    /// dotnet) follow the retire-not-drop model and never tear down their per-bundle
    /// state, so previously resolved raw pointers stay valid for the runtime's
    /// lifetime. They must NOT override this hook.
    ///
    /// VM loaders (lua, js) override it to free the bundle's VM at a quiescence
    /// point: if no thread is mid-dispatch on the VM the VM state is dropped (true
    /// reclaim); otherwise it is retired (kept alive) to avoid a use-after-free.
    ///
    /// The native loader overrides it to `dlclose` the dylib once it is epoch-safe
    /// to do so; reclamation is always epoch-deferred so previously resolved
    /// pointers stay valid until no in-flight dispatch can reference the bundle.
    ///
    /// # Errors
    /// Returns `Err(RuntimeError::...)` if reclamation fails.
    fn unload(&self, _bundle_id: BundleId, _runtime: &Runtime) -> Result<(), RuntimeError> {
        Ok(())
    }
}

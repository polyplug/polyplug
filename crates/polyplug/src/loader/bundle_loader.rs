use polyplug_utils::BundleId;

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
    /// The native loader overrides it to optionally `dlclose` under
    /// [`UnloadMode::Reclaim`](polyplug_abi::runtime::UnloadMode::Reclaim).
    ///
    /// # `reclaim_safe`
    /// A best-effort hint computed by the runtime from the retired interfaces'
    /// `Arc::strong_count`: `true` means no *Arc-holding* path still references the
    /// bundle's interfaces, so true reclaim is safe from that angle. It does NOT — and
    /// cannot — account for raw in-flight native calls: native dispatch is zero-overhead
    /// and the runtime keeps no native-call counter, so it is structurally blind to a
    /// thread executing inside the library. Loaders that truly free OS resources
    /// (native `dlclose`) therefore rely on the host attestation of `UnloadMode::Reclaim`
    /// for that case, using `reclaim_safe` only as an additional defer signal. VM
    /// loaders ignore it because their own `in_dispatch_threads` quiescence tracking is
    /// authoritative.
    ///
    /// # Errors
    /// Returns `Err(RuntimeError::...)` if reclamation fails.
    fn unload(
        &self,
        _bundle_id: BundleId,
        _runtime: &Runtime,
        _reclaim_safe: bool,
    ) -> Result<(), RuntimeError> {
        Ok(())
    }
}

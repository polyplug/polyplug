use polyplug_utils::BundleId;

use polyplug_common::ManifestData;

use crate::{error::LoaderError, loader::bundle_source::BundleSource, runtime::Runtime};

/// Trait implemented by all bundle loaders (native, python, lua, js, .net).
///
/// The runtime dispatches each bundle to the loader whose `loader_name()`
/// matches the `loader` field in the bundle's `manifest.toml`.
pub trait BundleLoader: Send + Sync {
    /// The loader identifier this loader handles.
    ///
    /// Must match the `loader` field in `manifest.toml` exactly (case-sensitive).
    fn loader_name(&self) -> &'static str;

    /// Whether this loader supports hot-reload.
    ///
    /// The runtime consults this *before* calling [`BundleLoader::reload`]: when it
    /// returns `false`, the runtime fails the reload with
    /// `RuntimeError::HotReloadDisabled` and never invokes `reload()`.
    fn supports_hot_reload(&self) -> bool;

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
    /// The runtime stages every `register_guest_contract` callback made during this
    /// call. It validates the complete provider and function sets, metadata, and
    /// dependencies before publishing one registry snapshot after `load` returns
    /// `Ok(())`. A loader must not call `load` directly in tests or hosts; use
    /// `Runtime::load_bundle` or `Runtime::load_bundle_from_source` so that
    /// transaction exists.
    ///
    /// `BundleInitContext` and its `bundle_path` view are borrowed for the
    /// synchronous `polyplug_init` invocation only. Loader and guest code must copy
    /// any needed bytes during init and must not retain either pointer.
    ///
    /// # Errors
    /// Returns `Err(LoaderError::...)` on any failure, including
    /// `LoaderError::UnsupportedBundleSource { .. }` when the loader does not support
    /// the given source kind.
    fn load(
        &self,
        manifest: &ManifestData,
        source: &BundleSource,
        runtime: &Runtime,
    ) -> Result<(), LoaderError>;

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
    /// The runtime gates hot-reload before calling this: it only invokes `reload()`
    /// when the config has hot-reload enabled AND [`BundleLoader::supports_hot_reload`]
    /// returns `true`. Loaders therefore do not re-check the config here.
    ///
    /// # Errors
    /// Returns `Err(LoaderError::...)` on any failure.
    fn reload(&self, manifest: &ManifestData, runtime: &Runtime) -> Result<(), LoaderError>;

    /// Reclaim a bundle's loader-owned resources after it has been invalidated.
    ///
    /// The runtime calls this from [`Runtime::unload_bundle`] *after*
    /// `RuntimeStore::invalidate_bundle` has removed the bundle from the registry
    /// indices and bumped its slots' generations. By that point no *new* dispatch
    /// can resolve to this bundle, so the loader only has to account for dispatches
    /// already in flight.
    ///
    /// The default implementation is a no-op, for a hypothetical loader with no
    /// per-bundle resources to reclaim. Every shipped loader overrides it, and each
    /// reclaim is epoch-deferred so the actual free runs only once no reader is still
    /// pinned in the epoch that preceded the unload — previously resolved pointers stay
    /// valid until no in-flight dispatch can reference the bundle:
    /// - **Native loader:** `dlclose`s the dylib (drops the `libloading::Library`),
    ///   releasing the on-disk file lock.
    /// - **VM loaders (Lua, JS):** drop the bundle's per-bundle VM.
    /// - **Python loader:** purges the bundle's re-keyed `sys.modules` entries (CPython
    ///   is single-init per process and cannot be torn down).
    /// - **.NET loader:** unloads the bundle's collectible `AssemblyLoadContext`.
    ///
    /// # Errors
    /// Returns `Err(LoaderError::...)` if reclamation fails.
    fn unload(&self, _bundle_id: BundleId, _runtime: &Runtime) -> Result<(), LoaderError> {
        Ok(())
    }
}

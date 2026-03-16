//! Native bundle loader — delegates to `polyplug::loader::load_bundle` via the global registry.

use std::path::Path;

use crate::config::NativeConfig;
use polyplug_abi::HostVTable;
use polyplug_abi::PluginRegistrar;
use polyplug::error::LoaderError;
use polyplug::error::PolyplugError;
use polyplug::loader::BundleLoader;
use polyplug::registry::Registry;
use polyplug::runtime::global_registry;
use std::sync::Arc;

pub struct NativeLoader {
    pub config: NativeConfig,
}

impl NativeLoader {
    pub fn new(config: NativeConfig) -> NativeLoader {
        NativeLoader { config }
    }
}

impl BundleLoader for NativeLoader {
    fn runtime_name(&self) -> &'static str {
        "native"
    }

    fn load(&self, path: &Path, registrar: &mut PluginRegistrar) -> Result<(), PolyplugError> {
        let registry: Arc<Registry> = global_registry().ok_or_else(|| {
            PolyplugError::Loader(LoaderError::InitFailed {
                bundle: path.to_string_lossy().into_owned(),
                error: "global registry not initialised".to_owned(),
            })
        })?;

        // SAFETY: registrar.host is set by make_registrar_context and points to a
        // leaked &'static HostVTable that lives for the runtime's lifetime.
        let host_vtable: &'static HostVTable = unsafe { &*registrar.host };

        let bundle_dir: &Path = path.parent().unwrap_or(path);
        let mut manifest: polyplug::loader::manifest::ManifestData =
            polyplug::loader::parse_manifest(bundle_dir).map_err(PolyplugError::Loader)?;
        manifest.bundle_id = polyplug_abi::bundle_id(&manifest.bundle_name);

        polyplug::loader::load_bundle(path, &manifest, &registry, host_vtable)
            .map_err(PolyplugError::Loader)
    }
}

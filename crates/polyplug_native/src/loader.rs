//! Native bundle loader — delegates to `polyplug::loader::load_bundle`.

use std::path::Path;

use crate::config::NativeConfig;
use polyplug::error::LoaderError;
use polyplug::error::PolyplugError;
use polyplug::loader::BundleLoader;
use polyplug::loader::manifest::ManifestData;
use polyplug::runtime::Runtime;

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

    fn load(&self, path: &Path, runtime: &Runtime) -> Result<(), PolyplugError> {
        let registry = runtime.registry();
        let host_vtable = runtime.host_vtable();

        let bundle_dir: &Path = path.parent().unwrap_or(path);
        let manifest: ManifestData =
            polyplug::loader::parse_manifest(bundle_dir).map_err(PolyplugError::Loader)?;

        if manifest.id == 0 {
            return Err(PolyplugError::Loader(LoaderError::InitFailed {
                bundle: path.to_string_lossy().into_owned(),
                error: "manifest.id is required but was 0 or missing".to_owned(),
            }));
        }

        polyplug::loader::load_bundle(path, &manifest, registry, host_vtable, runtime)
            .map_err(PolyplugError::Loader)
    }
}

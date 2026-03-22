//! Native bundle loader — delegates to `polyplug::loader::load_bundle`.

use std::path::PathBuf;

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

    fn load(&self, manifest: &ManifestData, runtime: &Runtime) -> Result<(), PolyplugError> {
        let registry = runtime.registry();
        let host_vtable = runtime.host_vtable();

        if manifest.id == 0 {
            return Err(PolyplugError::Loader(LoaderError::InitFailed {
                bundle: manifest.name.clone(),
                error: "manifest.id is required but was 0 or missing".to_owned(),
            }));
        }

        let bundle_path: PathBuf = if !manifest.file.is_empty() {
            manifest.path.join(&manifest.file)
        } else {
            return Err(PolyplugError::Loader(LoaderError::ManifestMissingFile {
                bundle: manifest.name.clone(),
            }));
        };

        polyplug::loader::load_bundle(&bundle_path, manifest, registry, host_vtable, runtime)
            .map_err(PolyplugError::Loader)
    }
}

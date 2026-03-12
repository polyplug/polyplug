//! polyplug-dotnet — .NET CLR loader adapter for polyplug.

pub mod config;
pub(crate) mod context;
pub mod version;
pub use config::DotnetConfig;
pub use config::HostfxrLocation;

use std::path::Path;

use netcorehost::pdcstring::PdCString;

use polyplug::abi::PluginRegistrar;
use polyplug::error::LoaderError;
use polyplug::error::PolyplugError;
use polyplug::loader::BundleLoader;

use crate::context::CLR_CONTEXT;
use crate::context::InitFn;
use crate::context::init_context;

pub struct DotnetLoader {
    config: DotnetConfig,
}

impl DotnetLoader {
    pub fn new(config: DotnetConfig) -> DotnetLoader {
        DotnetLoader { config }
    }
}

fn check_version_compatibility(tfm: &str, min_framework: &str) -> Result<(), PolyplugError> {
    if tfm.is_empty() {
        return Ok(());
    }
    let req_str: &str = min_framework.strip_prefix("net").unwrap_or(min_framework);
    let required_major: u32 = req_str
        .split('.')
        .next()
        .and_then(|s: &str| s.parse::<u32>().ok())
        .unwrap_or(10);
    let required_minor: u32 = req_str
        .split('.')
        .nth(1)
        .and_then(|s: &str| s.parse::<u32>().ok())
        .unwrap_or(0);
    let found_ver: &str = tfm.strip_prefix(".NETCoreApp,Version=v").unwrap_or(tfm);
    let found_major: u32 = found_ver
        .split('.')
        .next()
        .and_then(|s: &str| s.parse::<u32>().ok())
        .unwrap_or(0);
    let found_minor: u32 = found_ver
        .split('.')
        .nth(1)
        .and_then(|s: &str| s.parse::<u32>().ok())
        .unwrap_or(0);
    if found_major != required_major {
        return Err(PolyplugError::Loader(LoaderError::RuntimeVersionMismatch {
            required: min_framework.to_owned(),
            found: tfm.to_owned(),
        }));
    }
    if found_minor > required_minor + 2 {
        eprintln!(
            "polyplug-dotnet: warning: assembly TFM {tfm} has higher minor version than required {min_framework}"
        );
    }
    Ok(())
}

impl BundleLoader for DotnetLoader {
    fn runtime_name(&self) -> &'static str {
        "dotnet"
    }

    fn load(&self, path: &Path, registrar: &mut PluginRegistrar) -> Result<(), PolyplugError> {
        let tfm: String = crate::version::read_target_framework(path)?;
        check_version_compatibility(&tfm, &self.config.min_framework)?;

        let abs_path: std::path::PathBuf = path.canonicalize().map_err(|_| {
            PolyplugError::Loader(LoaderError::AssemblyNotFound {
                path: path.to_string_lossy().into_owned(),
            })
        })?;

        let bundle_dir: std::path::PathBuf =
            path.parent().unwrap_or(path).canonicalize().map_err(|_| {
                PolyplugError::Loader(LoaderError::AssemblyNotFound {
                    path: path.to_string_lossy().into_owned(),
                })
            })?;
        let context: std::sync::Arc<crate::context::DotnetContext> = std::sync::Arc::clone(
            CLR_CONTEXT.get_or_try_init(|| init_context(&self.config, &bundle_dir))?,
        );

        let stem: std::borrow::Cow<'_, str> =
            abs_path.file_stem().unwrap_or_default().to_string_lossy();
        let bundle_name: String = stem.into_owned();
        let type_name_str: String = format!("{bundle_name}.Plugin, {bundle_name}");

        let type_name_pdc: PdCString = PdCString::from_os_str(std::ffi::OsStr::new(&type_name_str))
            .map_err(|_| {
                PolyplugError::Loader(LoaderError::InitSymbolMissing {
                    bundle: bundle_name.clone(),
                })
            })?;
        let method_name_pdc: PdCString =
            PdCString::from_os_str(std::ffi::OsStr::new("PolyplugInit")).map_err(|_| {
                PolyplugError::Loader(LoaderError::InitSymbolMissing {
                    bundle: bundle_name.clone(),
                })
            })?;

        let managed_init: netcorehost::hostfxr::ManagedFunction<InitFn> = context.get_init_fn(
            abs_path.clone(),
            type_name_pdc.as_ref(),
            method_name_pdc.as_ref(),
        )?;

        // SAFETY: bundle_path_static outlives this call; leaked intentionally.
        let bundle_dir_str: String = bundle_dir.to_string_lossy().into_owned();
        let bundle_path_static: &'static str = Box::leak(bundle_dir_str.into_boxed_str());
        let ctx: polyplug::abi::PluginContext = polyplug::abi::PluginContext {
            bundle_path: polyplug::abi::StringView {
                ptr: bundle_path_static.as_ptr(),
                len: bundle_path_static.len(),
            },
        };
        // SAFETY: managed_init is a valid fn ptr from CLR. registrar and ctx are non-null and valid.
        let result: u32 = unsafe {
            (*managed_init)(
                registrar as *mut PluginRegistrar,
                &ctx as *const polyplug::abi::PluginContext,
            )
        };
        if result != 0 {
            return Err(PolyplugError::Loader(LoaderError::InitFailed {
                bundle: bundle_name,
                error: format!("PolyplugInit returned {result}"),
            }));
        }

        Ok(())
    }
}

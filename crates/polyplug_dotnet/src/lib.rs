//! polyplug_dotnet — .NET CLR loader adapter for polyplug.

pub mod config;
pub(crate) mod context;
pub mod ffi;
pub mod version;
pub use config::DotnetConfig;
pub use config::HostfxrLocation;

use std::path::Path;

use netcorehost::pdcstring::PdCString;

use polyplug::error::LoaderError;
use polyplug::error::PolyplugError;
use polyplug::loader::BundleLoader;
use polyplug_abi::PluginRegistrar;

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

pub(crate) fn check_version_compatibility(
    tfm: &str,
    min_framework: &str,
) -> Result<(), PolyplugError> {
    if tfm.is_empty() {
        return Ok(());
    }
    let req_str: &str = min_framework.strip_prefix("net").unwrap_or(min_framework);
    let mut req_parts = req_str.split('.');
    let req_major_str: &str = req_parts.next().ok_or_else(|| {
        PolyplugError::Loader(LoaderError::InvalidFrameworkVersion {
            tfm: min_framework.to_owned(),
            reason: "missing major version".to_owned(),
        })
    })?;
    let required_major: u32 = req_major_str.parse().map_err(|_| {
        PolyplugError::Loader(LoaderError::InvalidFrameworkVersion {
            tfm: min_framework.to_owned(),
            reason: format!("invalid major version: {req_major_str}"),
        })
    })?;
    // Lenient parsing: non-numeric minor version components are treated as 0.
    let required_minor: u32 = req_parts
        .next()
        .map(|s: &str| s.parse::<u32>().unwrap_or(0))
        .unwrap_or(0);
    let found_ver: &str = tfm.strip_prefix(".NETCoreApp,Version=v").unwrap_or(tfm);
    let mut found_parts = found_ver.split('.');
    let found_major_str: &str = found_parts.next().ok_or_else(|| {
        PolyplugError::Loader(LoaderError::InvalidFrameworkVersion {
            tfm: tfm.to_owned(),
            reason: "missing major version".to_owned(),
        })
    })?;
    let found_major: u32 = found_major_str.parse().map_err(|_| {
        PolyplugError::Loader(LoaderError::InvalidFrameworkVersion {
            tfm: tfm.to_owned(),
            reason: format!("invalid major version: {found_major_str}"),
        })
    })?;
    // Lenient parsing: non-numeric minor version components are treated as 0.
    let found_minor: u32 = found_parts
        .next()
        .map(|s: &str| s.parse::<u32>().unwrap_or(0))
        .unwrap_or(0);
    if found_major != required_major {
        return Err(PolyplugError::Loader(LoaderError::RuntimeVersionMismatch {
            required: min_framework.to_owned(),
            found: tfm.to_owned(),
        }));
    }
    if found_minor > required_minor + 2 {
        eprintln!(
            "polyplug_dotnet: warning: assembly TFM {tfm} has higher minor version than required {min_framework}"
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
        let ctx: polyplug_abi::PluginContext = polyplug_abi::PluginContext {
            bundle_path: polyplug_abi::StringView {
                ptr: bundle_path_static.as_ptr(),
                len: bundle_path_static.len(),
            },
            host_abi_version: polyplug_abi::POLYPLUG_ABI_VERSION,
        };
        // SAFETY: managed_init is a valid fn ptr from CLR. registrar and ctx are non-null and valid.
        let result: u32 = unsafe {
            (*managed_init)(
                registrar as *mut PluginRegistrar,
                &ctx as *const polyplug_abi::PluginContext,
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

#[cfg(test)]
mod tests {
    use polyplug::error::LoaderError;
    use polyplug::error::PolyplugError;

    use super::check_version_compatibility;

    // --- empty TFM (non-.NET DLL) is always compatible ---

    #[test]
    fn empty_tfm_always_ok() {
        let result: Result<(), PolyplugError> = check_version_compatibility("", "net10.0");
        assert!(result.is_ok(), "empty TFM must be unconditionally accepted");
    }

    #[test]
    fn empty_tfm_empty_min_framework_ok() {
        let result: Result<(), PolyplugError> = check_version_compatibility("", "");
        assert!(result.is_ok());
    }

    // --- same major version is compatible ---

    #[test]
    fn same_major_version_ok() {
        let result: Result<(), PolyplugError> =
            check_version_compatibility(".NETCoreApp,Version=v6.0", "net6.0");
        assert!(result.is_ok());
    }

    #[test]
    fn same_major_minor_within_window_ok() {
        // found minor (2) == required minor (0) + 2 → at the boundary → ok (not a warning trigger)
        let result: Result<(), PolyplugError> =
            check_version_compatibility(".NETCoreApp,Version=v6.2", "net6.0");
        assert!(result.is_ok());
    }

    #[test]
    fn higher_minor_beyond_window_still_ok() {
        // found minor (5) > required minor (0) + 2 → triggers eprintln warning but still Ok
        let result: Result<(), PolyplugError> =
            check_version_compatibility(".NETCoreApp,Version=v6.5", "net6.0");
        assert!(
            result.is_ok(),
            "version compat still succeeds despite minor warning"
        );
    }

    #[test]
    fn net7_assembly_against_net6_requirement_fails() {
        let result: Result<(), PolyplugError> =
            check_version_compatibility(".NETCoreApp,Version=v7.0", "net6.0");
        match result {
            Err(PolyplugError::Loader(LoaderError::RuntimeVersionMismatch { required, found })) => {
                assert_eq!(required, "net6.0");
                assert_eq!(found, ".NETCoreApp,Version=v7.0");
            }
            other => panic!("expected RuntimeVersionMismatch, got {other:?}"),
        }
    }

    #[test]
    fn net6_assembly_against_net7_requirement_fails() {
        let result: Result<(), PolyplugError> =
            check_version_compatibility(".NETCoreApp,Version=v6.0", "net7.0");
        match result {
            Err(PolyplugError::Loader(LoaderError::RuntimeVersionMismatch { .. })) => {}
            other => panic!("expected RuntimeVersionMismatch, got {other:?}"),
        }
    }

    #[test]
    fn net10_assembly_against_net10_requirement_ok() {
        let result: Result<(), PolyplugError> =
            check_version_compatibility(".NETCoreApp,Version=v10.0", "net10.0");
        assert!(result.is_ok());
    }

    #[test]
    fn tfm_without_prefix_parsed_as_raw_version() {
        // When no ".NETCoreApp,Version=v" prefix is present, the TFM is parsed directly.
        // "6.0" should parse as major=6 against net6.0 → ok.
        let result: Result<(), PolyplugError> = check_version_compatibility("6.0", "net6.0");
        assert!(result.is_ok());
    }

    #[test]
    fn tfm_without_prefix_major_mismatch_fails() {
        let result: Result<(), PolyplugError> = check_version_compatibility("7.0", "net6.0");
        match result {
            Err(PolyplugError::Loader(LoaderError::RuntimeVersionMismatch { .. })) => {}
            other => panic!("expected RuntimeVersionMismatch, got {other:?}"),
        }
    }

    // --- invalid framework strings ---

    #[test]
    fn invalid_min_framework_non_numeric_major_returns_error() {
        // min_framework with non-numeric major after stripping "net" prefix.
        let result: Result<(), PolyplugError> =
            check_version_compatibility(".NETCoreApp,Version=v6.0", "netXYZ.0");
        match result {
            Err(PolyplugError::Loader(LoaderError::InvalidFrameworkVersion { .. })) => {}
            other => panic!("expected InvalidFrameworkVersion, got {other:?}"),
        }
    }

    #[test]
    fn invalid_tfm_non_numeric_major_returns_error() {
        // TFM with non-numeric major after stripping ".NETCoreApp,Version=v" prefix.
        let result: Result<(), PolyplugError> =
            check_version_compatibility(".NETCoreApp,Version=vBAD.0", "net6.0");
        match result {
            Err(PolyplugError::Loader(LoaderError::InvalidFrameworkVersion { .. })) => {}
            other => panic!("expected InvalidFrameworkVersion, got {other:?}"),
        }
    }

    #[test]
    fn min_framework_missing_major_returns_error() {
        // Empty string after stripping "net" prefix — next() returns None → error.
        // Input: "net" alone after strip_prefix gives "", split('.').next() = Some("") which parse fails.
        let result: Result<(), PolyplugError> =
            check_version_compatibility(".NETCoreApp,Version=v6.0", "net");
        match result {
            Err(PolyplugError::Loader(LoaderError::InvalidFrameworkVersion { .. })) => {}
            other => panic!("expected InvalidFrameworkVersion, got {other:?}"),
        }
    }

    #[test]
    fn min_framework_no_minor_version_defaults_to_zero() {
        // "net6" has no minor component — minor defaults to 0.
        let result: Result<(), PolyplugError> =
            check_version_compatibility(".NETCoreApp,Version=v6.0", "net6");
        assert!(result.is_ok());
    }

    #[test]
    fn tfm_no_minor_version_defaults_to_zero() {
        // TFM with no minor component — found minor defaults to 0.
        let result: Result<(), PolyplugError> =
            check_version_compatibility(".NETCoreApp,Version=v6", "net6.0");
        assert!(result.is_ok());
    }
}

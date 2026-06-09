//! polyplug_dotnet — .NET CLR loader adapter for polyplug.

pub mod config;
pub(crate) mod context;
pub mod ffi;
pub mod version;
pub use config::DotnetConfig;
pub use config::HostfxrLocation;
use polyplug_abi::BundleInitContext;
use polyplug_abi::StringView;

use std::path::Path;
use std::sync::Arc;

use polyplug::Runtime;
use polyplug::error::LoaderError;
use polyplug::error::RuntimeError;
use polyplug::loader::BundleLoader;
use polyplug_abi::HostApi;

use crate::context::CLR_CONTEXT;
use crate::context::DotnetContext;
use crate::context::InitFn;
use crate::context::init_context;

pub struct DotnetLoader {
    config: DotnetConfig,
}

impl DotnetLoader {
    pub fn new(config: DotnetConfig) -> DotnetLoader {
        DotnetLoader { config }
    }

    /// Pre-load a dependency assembly from raw bytes into the shared CLR default load
    /// context, so a subsequently byte-loaded ([`BundleSource::Bytes`]) plugin that is not
    /// self-contained can resolve its references in memory.
    ///
    /// [`BundleSource::Bytes`] is single-file by contract, so a self-contained plugin never
    /// needs this. It exists for hosts that deliver a plugin plus its managed dependencies
    /// entirely in memory. Call it once per dependency *before* loading the dependent plugin.
    ///
    /// `config` selects/initializes the process-wide CLR (once-per-process; see the .NET
    /// runtime-isolation limitation in `CLAUDE.md`).
    ///
    /// # Memory ownership (CLAUDE.md Rule 8)
    ///
    /// `dep_bytes` is borrowed only for the duration of the managed bridge call; the bridge
    /// copies it into managed storage before returning.
    ///
    /// [`BundleSource::Bytes`]: polyplug::loader::BundleSource::Bytes
    pub fn preload_dependency_from_bytes(
        &self,
        bundle_id: u64,
        dep_bytes: &[u8],
    ) -> Result<(), RuntimeError> {
        // Dependencies have no bundle directory; the CLR probing path is irrelevant here, so
        // an empty path is passed for the (already-initialized) context lookup. The dependency
        // is loaded into `bundle_id`'s collectible ALC so it is reclaimed when that bundle is.
        let context: Arc<DotnetContext> =
            Arc::clone(CLR_CONTEXT.get_or_try_init(|| init_context(&self.config, Path::new("")))?);
        context.preload_dependency_bytes(bundle_id, dep_bytes)
    }

    /// Resolve the managed `PolyplugInit` function for an on-disk assembly
    /// ([`BundleSource::Path`]).
    ///
    /// The assembly simple name (and thus the entry type name) is derived from the file
    /// stem of the resolved assembly path, matching the historical convention.
    ///
    /// [`BundleSource::Path`]: polyplug::loader::BundleSource::Path
    fn resolve_init_from_path(
        &self,
        manifest: &polyplug::loader::ManifestData,
        bundle_dir: &Path,
    ) -> Result<(InitFn, String), RuntimeError> {
        let bundle_path: std::path::PathBuf = if !manifest.file.is_empty() {
            manifest.path.join(&manifest.file)
        } else {
            return Err(RuntimeError::Loader(LoaderError::ManifestMissingFile {
                bundle: manifest.name.clone(),
            }));
        };

        if !bundle_path.exists() {
            return Err(RuntimeError::Loader(LoaderError::InitFailed {
                bundle: manifest.name.clone(),
                error: format!(
                    "assembly not found at path '{}'",
                    bundle_path.to_string_lossy()
                ),
            }));
        }

        let tfm: String = crate::version::read_target_framework(&bundle_path)?;
        check_version_compatibility(&tfm, &self.config.min_framework)?;

        let abs_path: std::path::PathBuf = bundle_path.canonicalize().map_err(|_| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: manifest.name.clone(),
                error: format!(
                    "assembly canonicalize failed for path '{}'",
                    bundle_path.to_string_lossy()
                ),
            })
        })?;

        let context: Arc<DotnetContext> =
            Arc::clone(CLR_CONTEXT.get_or_try_init(|| init_context(&self.config, bundle_dir))?);

        let stem: std::borrow::Cow<'_, str> =
            abs_path.file_stem().unwrap_or_default().to_string_lossy();
        let bundle_name: String = stem.into_owned();
        // The bridge resolves `{name}.Plugin` directly from the Assembly object it loads into
        // the bundle's collectible ALC (no assembly-name re-binding), so a simple type name is
        // sufficient — identical to the Bytes convention.
        let simple_type_name: String = format!("{bundle_name}.Plugin");
        let init_fn: InitFn =
            context.get_init_fn_from_path(manifest.id, &abs_path, &bundle_name, &simple_type_name)?;
        Ok((init_fn, bundle_name))
    }

    /// Resolve the managed `PolyplugInit` function for an in-memory assembly
    /// ([`BundleSource::Bytes`]) via the managed byte-load bridge.
    ///
    /// # Type-resolution convention
    ///
    /// Byte-loaded assemblies have no file stem, so the assembly simple name is derived from
    /// the file stem of the manifest's declared `file` field (e.g. `"CsharpPlugin.dll"` →
    /// `"CsharpPlugin"`). The entry type is then `{name}.Plugin` — identical to the `Path`
    /// convention, but sourced from manifest data rather than the on-disk path.
    /// `manifest.file` is required for byte sources for exactly this reason.
    ///
    /// # Memory ownership (CLAUDE.md Rule 8)
    ///
    /// `bytes` is borrowed only for the duration of the managed bridge call inside
    /// [`DotnetContext::get_init_fn_from_bytes`]; the bridge copies it into managed storage
    /// during that call, so no host-allocated buffer outlives the load.
    ///
    /// [`BundleSource::Bytes`]: polyplug::loader::BundleSource::Bytes
    fn resolve_init_from_bytes(
        &self,
        manifest: &polyplug::loader::ManifestData,
        bundle_dir: &Path,
        bytes: &[u8],
    ) -> Result<(InitFn, String), RuntimeError> {
        if manifest.file.is_empty() {
            return Err(RuntimeError::Loader(LoaderError::ManifestMissingFile {
                bundle: manifest.name.clone(),
            }));
        }
        let bundle_name: String = Path::new(&manifest.file)
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        if bundle_name.is_empty() {
            return Err(RuntimeError::Loader(LoaderError::ManifestMissingFile {
                bundle: manifest.name.clone(),
            }));
        }

        let tfm: String = crate::version::target_framework_from_bytes(bytes, &bundle_name)?;
        check_version_compatibility(&tfm, &self.config.min_framework)?;

        let context: Arc<DotnetContext> =
            Arc::clone(CLR_CONTEXT.get_or_try_init(|| init_context(&self.config, bundle_dir))?);

        // The bridge resolves `{name}.Plugin` directly from the byte-loaded Assembly object.
        let simple_type_name: String = format!("{bundle_name}.Plugin");
        let init_fn: InitFn =
            context.get_init_fn_from_bytes(manifest.id, &bundle_name, bytes, &simple_type_name)?;
        Ok((init_fn, bundle_name))
    }
}

/// Test/diagnostic probe: returns whether the bundle's collectible `AssemblyLoadContext` is
/// still alive (`true`) or has been reclaimed / was never loaded (`false`). Forces a managed
/// GC inside the bridge, so it is for tests and tooling — not the hot path.
pub fn bundle_alc_alive(bundle_id: u64) -> Result<bool, RuntimeError> {
    match CLR_CONTEXT.get() {
        Some(context) => context.is_alc_alive(bundle_id),
        None => Ok(false),
    }
}

pub(crate) fn check_version_compatibility(
    tfm: &str,
    min_framework: &str,
) -> Result<(), RuntimeError> {
    if tfm.is_empty() {
        return Ok(());
    }
    let req_str: &str = min_framework.strip_prefix("net").unwrap_or(min_framework);
    let mut req_parts = req_str.split('.');
    let req_major_str: &str = req_parts.next().ok_or_else(|| {
        RuntimeError::Loader(LoaderError::InitFailed {
            bundle: min_framework.to_owned(),
            error: format!(
                "invalid framework version '{}': missing major version",
                min_framework
            ),
        })
    })?;
    let required_major: u32 = req_major_str.parse().map_err(|_| {
        RuntimeError::Loader(LoaderError::InitFailed {
            bundle: min_framework.to_owned(),
            error: format!(
                "invalid framework version '{}': invalid major version '{}'",
                min_framework, req_major_str
            ),
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
        RuntimeError::Loader(LoaderError::InitFailed {
            bundle: tfm.to_owned(),
            error: format!("invalid framework version '{}': missing major version", tfm),
        })
    })?;
    let found_major: u32 = found_major_str.parse().map_err(|_| {
        RuntimeError::Loader(LoaderError::InitFailed {
            bundle: tfm.to_owned(),
            error: format!(
                "invalid framework version '{}': invalid major version '{}'",
                tfm, found_major_str
            ),
        })
    })?;
    // Lenient parsing: non-numeric minor version components are treated as 0.
    let found_minor: u32 = found_parts
        .next()
        .map(|s: &str| s.parse::<u32>().unwrap_or(0))
        .unwrap_or(0);
    if found_major != required_major {
        return Err(RuntimeError::Loader(LoaderError::InitFailed {
            bundle: min_framework.to_owned(),
            error: format!(
                "runtime version mismatch: required={}, found={}",
                min_framework, tfm
            ),
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

    fn load(
        &self,
        manifest: &polyplug::loader::ManifestData,
        source: &polyplug::loader::BundleSource,
        runtime: &Runtime,
    ) -> Result<(), RuntimeError> {
        // `Code` is unsupported: C#/.NET is a compiled language, so there is no source-text
        // load path. Raw assembly bytes are the in-memory equivalent and are handled by the
        // `Bytes` arm below. `Path` and `Bytes` are the two real .NET load paths.
        let bundle_dir: &Path = &manifest.path;
        let bundle_id: u64 = manifest.id;

        // The init function and the assembly simple name are resolved per source kind, then
        // the shared init-invocation tail below drives the managed `PolyplugInit`.
        let (managed_init, bundle_name): (InitFn, String) = match source {
            polyplug::loader::BundleSource::Code(_) => {
                return Err(RuntimeError::Loader(LoaderError::UnsupportedBundleSource {
                    loader: "dotnet",
                    source_kind: source.kind(),
                    bundle: manifest.name.clone(),
                }));
            }
            polyplug::loader::BundleSource::Path(_) => {
                self.resolve_init_from_path(manifest, bundle_dir)?
            }
            polyplug::loader::BundleSource::Bytes(bytes) => {
                self.resolve_init_from_bytes(manifest, bundle_dir, bytes)?
            }
        };

        // BundleInitContext.bundle_path is borrowed only for the duration of the
        // managed_init call below, so a local String kept alive across that call
        // is sufficient — no leak required. `bundle_dir_str` outlives `ctx`.
        let bundle_dir_str: String = bundle_dir.to_string_lossy().into_owned();
        let ctx = BundleInitContext {
            bundle_id,
            bundle_path: StringView {
                ptr: bundle_dir_str.as_ptr(),
                len: bundle_dir_str.len(),
            },
        };

        // Get HostApi pointer from runtime (self-passing pattern).
        // This pointer is passed to the managed PolyplugInit as the first parameter.
        let host_interface: *const HostApi = runtime.as_context_ptr();

        // Push bundle_id onto the runtime's per-thread init stack for dependency
        // enforcement during init. The matching pop MUST run on every exit path
        // (success and error) so the stack never leaks an entry.
        runtime.push_init_bundle_id(bundle_id);

        // SAFETY: managed_init is a valid fn ptr from CLR. host_interface and ctx are non-null and valid.
        // InitFn signature: (host, ctx) -> u32
        // The HostApi pointer is passed directly (self-passing pattern).
        let result: u32 = unsafe { managed_init(host_interface, &ctx) };

        // Pop bundle_id from the init stack after init completes (always, including
        // the error path) so the stack does not leak an entry.
        runtime.pop_init_bundle_id();

        if result != 0 {
            return Err(RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name,
                error: format!("PolyplugInit returned {result}"),
            }));
        }

        Ok(())
    }

    fn reload(
        &self,
        _manifest: &polyplug::loader::ManifestData,
        _runtime: &Runtime,
    ) -> Result<(), RuntimeError> {
        // Collectible ALC enables true UNLOAD reclaim (see `unload`); hot-reload
        // (load-new → swap → free-old under quiescence) is a separate, larger workstream
        // and remains disabled for .NET.
        Err(RuntimeError::HotReloadDisabled)
    }

    /// Unload a .NET bundle. Under `UnloadMode::Retire` (default) the bundle's collectible
    /// `AssemblyLoadContext` is kept rooted by the managed bridge (retire-not-drop), so any
    /// raw managed function pointer already resolved into it stays valid. Under host-attested
    /// `UnloadMode::Reclaim` with `reclaim_safe`, the ALC is truly unloaded — its assemblies
    /// become eligible for GC reclamation once all references and native frames into it clear.
    ///
    /// SAFETY discipline mirrors the native `dlclose` loader: the runtime is structurally blind
    /// to in-flight native dispatch into managed code, so true reclaim is sound ONLY because the
    /// host selected `Reclaim`, attesting no thread is calling — or holds a pointer into — this
    /// bundle. `reclaim_safe` is an additional best-effort defer signal.
    fn unload(
        &self,
        bundle_id: polyplug_utils::BundleId,
        runtime: &Runtime,
        reclaim_safe: bool,
    ) -> Result<(), RuntimeError> {
        let mode: polyplug_abi::runtime::UnloadMode = runtime.config().unload_mode;
        match mode {
            polyplug_abi::runtime::UnloadMode::Retire => Ok(()),
            polyplug_abi::runtime::UnloadMode::Reclaim => {
                if !reclaim_safe {
                    // A resolver may still hold the interface; keep the ALC rooted (deferred).
                    return Ok(());
                }
                let context: Arc<DotnetContext> = match CLR_CONTEXT.get() {
                    Some(c) => Arc::clone(c),
                    None => return Ok(()),
                };
                context.unload_bundle_alc(bundle_id.id())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use polyplug::error::LoaderError;
    use polyplug::error::RuntimeError;

    use super::check_version_compatibility;

    // --- empty TFM (non-.NET DLL) is always compatible ---

    #[test]
    fn empty_tfm_always_ok() {
        let result: Result<(), RuntimeError> = check_version_compatibility("", "net10.0");
        assert!(result.is_ok(), "empty TFM must be unconditionally accepted");
    }

    #[test]
    fn empty_tfm_empty_min_framework_ok() {
        let result: Result<(), RuntimeError> = check_version_compatibility("", "");
        assert!(result.is_ok());
    }

    // --- same major version is compatible ---

    #[test]
    fn same_major_version_ok() {
        let result: Result<(), RuntimeError> =
            check_version_compatibility(".NETCoreApp,Version=v6.0", "net6.0");
        assert!(result.is_ok());
    }

    #[test]
    fn same_major_minor_within_window_ok() {
        // found minor (2) == required minor (0) + 2 → at the boundary → ok (not a warning trigger)
        let result: Result<(), RuntimeError> =
            check_version_compatibility(".NETCoreApp,Version=v6.2", "net6.0");
        assert!(result.is_ok());
    }

    #[test]
    fn higher_minor_beyond_window_still_ok() {
        // found minor (5) > required minor (0) + 2 → triggers eprintln warning but still Ok
        let result: Result<(), RuntimeError> =
            check_version_compatibility(".NETCoreApp,Version=v6.5", "net6.0");
        assert!(
            result.is_ok(),
            "version compat still succeeds despite minor warning"
        );
    }

    #[test]
    fn net7_assembly_against_net6_requirement_fails() {
        let result: Result<(), RuntimeError> =
            check_version_compatibility(".NETCoreApp,Version=v7.0", "net6.0");
        match result {
            Err(RuntimeError::Loader(LoaderError::InitFailed { bundle, error })) => {
                assert_eq!(bundle, "net6.0");
                assert!(error.contains("runtime version mismatch"));
                assert!(error.contains("net6.0"));
                assert!(error.contains(".NETCoreApp,Version=v7.0"));
            }
            other => panic!("expected InitFailed with runtime version mismatch, got {other:?}"),
        }
    }

    #[test]
    fn net6_assembly_against_net7_requirement_fails() {
        let result: Result<(), RuntimeError> =
            check_version_compatibility(".NETCoreApp,Version=v6.0", "net7.0");
        match result {
            Err(RuntimeError::Loader(LoaderError::InitFailed { .. })) => {}
            other => panic!("expected InitFailed, got {other:?}"),
        }
    }

    #[test]
    fn net10_assembly_against_net10_requirement_ok() {
        let result: Result<(), RuntimeError> =
            check_version_compatibility(".NETCoreApp,Version=v10.0", "net10.0");
        assert!(result.is_ok());
    }

    #[test]
    fn tfm_without_prefix_parsed_as_raw_version() {
        // When no ".NETCoreApp,Version=v" prefix is present, the TFM is parsed directly.
        // "6.0" should parse as major=6 against net6.0 → ok.
        let result: Result<(), RuntimeError> = check_version_compatibility("6.0", "net6.0");
        assert!(result.is_ok());
    }

    #[test]
    fn tfm_without_prefix_major_mismatch_fails() {
        let result: Result<(), RuntimeError> = check_version_compatibility("7.0", "net6.0");
        match result {
            Err(RuntimeError::Loader(LoaderError::InitFailed { .. })) => {}
            other => panic!("expected InitFailed, got {other:?}"),
        }
    }

    // --- invalid framework strings ---

    #[test]
    fn invalid_min_framework_non_numeric_major_returns_error() {
        // min_framework with non-numeric major after stripping "net" prefix.
        let result: Result<(), RuntimeError> =
            check_version_compatibility(".NETCoreApp,Version=v6.0", "netXYZ.0");
        match result {
            Err(RuntimeError::Loader(LoaderError::InitFailed { .. })) => {}
            other => panic!("expected InitFailed, got {other:?}"),
        }
    }

    #[test]
    fn invalid_tfm_non_numeric_major_returns_error() {
        // TFM with non-numeric major after stripping ".NETCoreApp,Version=v" prefix.
        let result: Result<(), RuntimeError> =
            check_version_compatibility(".NETCoreApp,Version=vBAD.0", "net6.0");
        match result {
            Err(RuntimeError::Loader(LoaderError::InitFailed { .. })) => {}
            other => panic!("expected InitFailed, got {other:?}"),
        }
    }

    #[test]
    fn min_framework_missing_major_returns_error() {
        // Empty string after stripping "net" prefix — next() returns None → error.
        // Input: "net" alone after strip_prefix gives "", split('.').next() = Some("") which parse fails.
        let result: Result<(), RuntimeError> =
            check_version_compatibility(".NETCoreApp,Version=v6.0", "net");
        match result {
            Err(RuntimeError::Loader(LoaderError::InitFailed { .. })) => {}
            other => panic!("expected InitFailed, got {other:?}"),
        }
    }

    #[test]
    fn min_framework_no_minor_version_defaults_to_zero() {
        // "net6" has no minor component — minor defaults to 0.
        let result: Result<(), RuntimeError> =
            check_version_compatibility(".NETCoreApp,Version=v6.0", "net6");
        assert!(result.is_ok());
    }

    #[test]
    fn tfm_no_minor_version_defaults_to_zero() {
        // TFM with no minor component — found minor defaults to 0.
        let result: Result<(), RuntimeError> =
            check_version_compatibility(".NETCoreApp,Version=v6", "net6.0");
        assert!(result.is_ok());
    }
}

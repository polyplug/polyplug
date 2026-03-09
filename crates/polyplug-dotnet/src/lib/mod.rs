//! polyplug-dotnet — .NET CLR loader adapter for polyplug.
//! Enables loading standard .NET C# plugins via CLR hosting (hostfxr).

pub mod config;
pub use config::DotnetConfig;
pub use config::HostfxrLocation;

use std::fs;
use std::io::Write;
use std::path::Path;

use std::sync::Arc;
use std::sync::Mutex;

use netcorehost::hostfxr::AssemblyDelegateLoader;
use netcorehost::hostfxr::HostfxrContext;
use netcorehost::hostfxr::InitializedForRuntimeConfig;
use netcorehost::hostfxr::ManagedFunction;
use netcorehost::nethost;
use netcorehost::pdcstring::PdCString;
use once_cell::sync::OnceCell;

use polyplug::abi::PluginRegistrar;
use polyplug::error::LoaderError;
use polyplug::error::PolyplugError;
use polyplug::loader::BundleLoader;

/// Global CLR context — initialized exactly once per process.
/// `HostfxrContext<InitializedForRuntimeConfig>` is `Send` but `!Sync`.
/// Wrapping in `Mutex` makes it `Sync`, allowing storage in a `static`.
static CLR_CONTEXT: OnceCell<Arc<Mutex<HostfxrContext<InitializedForRuntimeConfig>>>> =
    OnceCell::new();

/// Loader for .NET (CLR) plugin bundles.
pub struct DotnetLoader {
    config: DotnetConfig,
}

impl DotnetLoader {
    /// Create a new `DotnetLoader` with the given configuration.
    pub fn new(config: DotnetConfig) -> DotnetLoader {
        DotnetLoader { config }
    }

    /// Ensure the CLR is initialized exactly once, using the runtimeconfig.json next to `dll_path`.
    /// Returns the shared context.
    fn ensure_clr_initialized(
        &self,
        dll_path: &Path,
    ) -> Result<Arc<Mutex<HostfxrContext<InitializedForRuntimeConfig>>>, PolyplugError> {
        let context_arc: &Arc<Mutex<HostfxrContext<InitializedForRuntimeConfig>>> =
            CLR_CONTEXT.get_or_try_init(|| {
                // Locate the runtimeconfig.json next to the DLL.
                // E.g. "CsharpPlugin.dll" → "CsharpPlugin.runtimeconfig.json"
                let stem: std::borrow::Cow<'_, str> = dll_path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy();
                let runtimeconfig_name: String = format!("{stem}.runtimeconfig.json");
                let runtimeconfig_path: std::path::PathBuf = dll_path
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .join(&runtimeconfig_name);

                // If a runtimeconfig.json exists next to the DLL, use it directly.
                // Otherwise fall back to generating one from the configured TFM.
                let config_path: std::path::PathBuf = if runtimeconfig_path.exists() {
                    runtimeconfig_path
                } else {
                    // Parse version from "net10.0" → "10.0" → "10.0.0"
                    let ver_str: &str = self
                        .config
                        .min_framework
                        .strip_prefix("net")
                        .unwrap_or(&self.config.min_framework);
                    let full_version: String =
                        if ver_str.chars().filter(|c: &char| *c == '.').count() == 1 {
                            format!("{ver_str}.0")
                        } else {
                            ver_str.to_owned()
                        };

                    // Generate runtimeconfig.json content and write to a temp file.
                    let json: String = format!(
                        r#"{{"runtimeOptions":{{"tfm":"{}","framework":{{"name":"Microsoft.NETCore.App","version":"{}"}}}}}}"#,
                        self.config.min_framework, full_version
                    );
                    let mut tmp: tempfile::NamedTempFile =
                        tempfile::NamedTempFile::new().map_err(|e: std::io::Error| {
                            PolyplugError::Loader(LoaderError::ClrInitFailed {
                                path: "<tempfile>".to_owned(),
                                reason: e.to_string(),
                            })
                        })?;
                    tmp.write_all(json.as_bytes()).map_err(|e: std::io::Error| {
                        PolyplugError::Loader(LoaderError::ClrInitFailed {
                            path: "<tempfile>".to_owned(),
                            reason: e.to_string(),
                        })
                    })?;
                    // Keep the file alive until after CLR init by leaking it into a PathBuf.
                    // This is intentional: the file must exist on disk when hostfxr reads it.
                    let p: std::path::PathBuf = tmp.path().to_path_buf();
                    // SAFETY: intentional leak — temp file must remain until CLR is initialized.
                    let _keep: tempfile::NamedTempFile = tmp;
                    core::mem::forget(_keep);
                    p
                };

                // Convert path to PdCString (required by netcorehost API)
                let pdcpath: PdCString =
                    PdCString::from_os_str(config_path.as_os_str()).map_err(|_| {
                        PolyplugError::Loader(LoaderError::ClrInitFailed {
                            path: config_path.to_string_lossy().into_owned(),
                            reason: "path contains embedded nul byte".to_owned(),
                        })
                    })?;

                // Discover hostfxr
                let hostfxr: netcorehost::hostfxr::Hostfxr =
                    nethost::load_hostfxr().map_err(|e| {
                        PolyplugError::Loader(LoaderError::ClrInitFailed {
                            path: "<hostfxr>".to_owned(),
                            reason: e.to_string(),
                        })
                    })?;

                // Initialize for runtime config (CLR loaded here)
                let context: HostfxrContext<InitializedForRuntimeConfig> = hostfxr
                    .initialize_for_runtime_config(pdcpath)
                    .map_err(|e| PolyplugError::Loader(LoaderError::ClrInitFailed {
                        path: config_path.to_string_lossy().into_owned(),
                        reason: e.to_string(),
                    }))?;

                Ok::<Arc<Mutex<HostfxrContext<InitializedForRuntimeConfig>>>, PolyplugError>(Arc::new(Mutex::new(context)))
            })?;
        Ok(Arc::clone(context_arc))
    }

    /// Check that the DLL's embedded TFM is compatible with the configured minimum framework.
    fn check_framework_version(&self, dll_path: &std::path::Path) -> Result<(), PolyplugError> {
        let tfm: String = sniff_target_framework(dll_path)?;
        if tfm.is_empty() {
            // No TFM found — non-.NET dll, allow loading
            return Ok(());
        }
        // Parse required major from self.config.min_framework ("net10.0" → major=10)
        let req_str: &str = self
            .config
            .min_framework
            .strip_prefix("net")
            .unwrap_or(&self.config.min_framework);
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
        // Parse found major from TFM (".NETCoreApp,Version=v10.0" → major=10)
        // tfm looks like ".NETCoreApp,Version=v10.0"
        let found_ver: &str = tfm.strip_prefix(".NETCoreApp,Version=v").unwrap_or(&tfm);
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
                required: self.config.min_framework.clone(),
                found: tfm,
            }));
        }
        if found_minor > required_minor + 2 {
            eprintln!(
                "polyplug-dotnet: warning: assembly TFM {tfm} has higher minor version than \
                 required {}",
                self.config.min_framework
            );
        }
        Ok(())
    }
}

impl BundleLoader for DotnetLoader {
    fn runtime_name(&self) -> &'static str {
        "dotnet"
    }

    fn load(&self, path: &Path, registrar: &mut PluginRegistrar) -> Result<(), PolyplugError> {
        self.check_framework_version(path)?;
        let context_arc: Arc<Mutex<HostfxrContext<InitializedForRuntimeConfig>>> =
            self.ensure_clr_initialized(path)?;

        // Resolve absolute path for the assembly
        let abs_path: std::path::PathBuf = path.canonicalize().map_err(|_| {
            PolyplugError::Loader(LoaderError::AssemblyNotFound {
                path: path.to_string_lossy().into_owned(),
            })
        })?;

        // Convert to PdCString for netcorehost API
        let asm_path: PdCString = PdCString::from_os_str(abs_path.as_os_str()).map_err(|_| {
            PolyplugError::Loader(LoaderError::AssemblyNotFound {
                path: abs_path.to_string_lossy().into_owned(),
            })
        })?;

        // Derive type name from file stem: "CsharpPlugin.dll" → "CsharpPlugin"
        let stem: std::borrow::Cow<'_, str> =
            abs_path.file_stem().unwrap_or_default().to_string_lossy();
        let bundle_name: String = stem.into_owned();
        // Type name format: "AssemblyName.Plugin, AssemblyName"
        let type_name_str: String = format!("{bundle_name}.Plugin, {bundle_name}");

        // Get delegate loader for the assembly (this is where CLR actually loads)
        let delegate_loader: AssemblyDelegateLoader = {
            let ctx: std::sync::MutexGuard<'_, HostfxrContext<InitializedForRuntimeConfig>> =
                context_arc.lock().map_err(|_| {
                    PolyplugError::Loader(LoaderError::ClrInitFailed {
                        path: abs_path.to_string_lossy().into_owned(),
                        reason: "CLR context mutex poisoned".to_owned(),
                    })
                })?;
            ctx.get_delegate_loader_for_assembly(asm_path)
                .map_err(|e| {
                    PolyplugError::Loader(LoaderError::ClrInitFailed {
                        path: abs_path.to_string_lossy().into_owned(),
                        reason: e.to_string(),
                    })
                })?
        };

        // Convert type name and method name to PdCString
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

        // Get the [UnmanagedCallersOnly] init function pointer
        type InitFn = unsafe extern "system" fn(*mut PluginRegistrar) -> u32;
        let managed_init: ManagedFunction<InitFn> = delegate_loader
            .get_function_with_unmanaged_callers_only::<InitFn>(
                type_name_pdc.as_ref(),
                method_name_pdc.as_ref(),
            )
            .map_err(|_e| {
                PolyplugError::Loader(LoaderError::InitSymbolMissing {
                    bundle: bundle_name.clone(),
                })
            })?;

        // Call the plugin's Init function
        // SAFETY: registrar is a valid non-null mutable reference per BundleLoader contract.
        // The plugin's PolyplugInit function receives a pointer to the registrar and registers
        // its plugins. The registrar lifetime extends for the duration of this call.
        // managed_init is a valid function pointer obtained from the CLR hosting API.
        let result: u32 = unsafe { (*managed_init)(registrar as *mut PluginRegistrar) };
        if result != 0 {
            return Err(PolyplugError::Loader(LoaderError::InitFailed {
                bundle: bundle_name,
                error: format!("PolyplugInit returned {result}"),
            }));
        }

        Ok(())
    }
}

/// Scan a DLL's bytes for the TargetFrameworkAttribute blob.
/// Returns the TFM string (e.g. `".NETCoreApp,Version=v10.0"`) or empty if not found.
fn sniff_target_framework(dll_path: &std::path::Path) -> Result<String, PolyplugError> {
    let bytes: Vec<u8> = fs::read(dll_path).map_err(|_| {
        PolyplugError::Loader(LoaderError::AssemblyNotFound {
            path: dll_path.to_string_lossy().into_owned(),
        })
    })?;
    // Walk PE CLI metadata to find TargetFrameworkAttribute blob
    // The string ".NETCoreApp,Version=v" appears verbatim in the metadata blob heap
    const MARKER: &[u8] = b".NETCoreApp,Version=v";
    let pos: Option<usize> = bytes.windows(MARKER.len()).position(|w: &[u8]| w == MARKER);
    let start: usize = match pos {
        Some(p) => p,
        None => {
            // Not a .NET assembly or TFM not found — treat as compatible
            return Ok(String::new());
        }
    };
    let end: usize = bytes[start..]
        .iter()
        .position(|&b: &u8| b == 0)
        .map(|n: usize| start + n)
        .unwrap_or(bytes.len());
    let tfm_slice: &[u8] = &bytes[start..end];
    let tfm: String = String::from_utf8_lossy(tfm_slice).into_owned();
    Ok(tfm) // e.g. ".NETCoreApp,Version=v10.0"
}

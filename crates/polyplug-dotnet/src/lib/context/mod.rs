//! DotnetContext — CLR runtime context, cached across all plugin loads.

use std::collections::HashMap;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use netcorehost::hostfxr::AssemblyDelegateLoader;
use netcorehost::hostfxr::ManagedFunction;
use netcorehost::pdcstring::PdCStr;
use netcorehost::pdcstring::PdCString;

use once_cell::sync::OnceCell;

use polyplug::abi::PluginRegistrar;
use polyplug::error::LoaderError;
use polyplug::error::PolyplugError;

use crate::config::DotnetConfig;
use crate::config::HostfxrLocation;

/// InitFn: the [UnmanagedCallersOnly] entry point signature.
/// Uses `extern "system"` because `netcorehost::ManagedFunction<F>` requires `F: ManagedFnPtr`
/// which requires `<F as FnPtr>::Abi == System`. On Linux/macOS `"system"` is identical to `"C"`.
pub(crate) type InitFn = unsafe extern "system" fn(*mut PluginRegistrar) -> u32;

/// DotnetContext holds the live CLR runtime and per-assembly loader cache.
/// Created exactly once per process via CLR_CONTEXT.
pub(crate) struct DotnetContext {
    /// Held to keep CLR runtime alive. Never locked after initialization except to obtain delegate loaders.
    _context: Mutex<
        netcorehost::hostfxr::HostfxrContext<netcorehost::hostfxr::InitializedForRuntimeConfig>,
    >,
    /// Per-assembly loader cache.
    /// Each `AssemblyDelegateLoader` is wrapped in `Arc<Mutex<...>>` because
    /// `AssemblyDelegateLoader` is `!Send` (it contains raw function pointers without an
    /// explicit `Send` impl). The outer `Mutex` synchronizes access to the map itself;
    /// the inner `Arc<Mutex<AssemblyDelegateLoader>>` allows the loader to be cloned
    /// out of the map and used without holding the outer lock.
    loader_cache: Mutex<HashMap<PathBuf, Arc<Mutex<AssemblyDelegateLoader>>>>,
}

// SAFETY: DotnetContext is used only behind Arc and all its fields are protected by Mutex.
// HostfxrContext<InitializedForRuntimeConfig> has an explicit `unsafe impl Send` in netcorehost.
// AssemblyDelegateLoader contains function pointers that are safe to send across threads
// because they are obtained from the CLR hosting API and remain valid for the lifetime of the runtime.
unsafe impl Send for DotnetContext {}
// SAFETY: All mutable access to DotnetContext's fields is serialized through Mutex.
unsafe impl Sync for DotnetContext {}

/// Global CLR context — initialized exactly once per process.
pub(crate) static CLR_CONTEXT: OnceCell<Arc<DotnetContext>> = OnceCell::new();

/// Initialize a fresh `DotnetContext` from the given config.
///
/// Generates a `runtimeconfig.json` in a temp file, initializes hostfxr, and returns
/// an `Arc<DotnetContext>`. The caller is responsible for storing the result in `CLR_CONTEXT`
/// via `OnceCell::get_or_try_init`.
pub(crate) fn init_context(config: &DotnetConfig) -> Result<Arc<DotnetContext>, PolyplugError> {
    // Step 1: Parse version from "net10.0" → "10.0" → "10.0.0"
    let ver_str: &str = config
        .min_framework
        .strip_prefix("net")
        .unwrap_or(&config.min_framework);
    let full_version: String = if ver_str.chars().filter(|c: &char| *c == '.').count() == 1 {
        format!("{ver_str}.0")
    } else {
        ver_str.to_owned()
    };

    // Step 2: Generate runtimeconfig.json content and write to a temp file.
    let json: String = format!(
        r#"{{"runtimeOptions":{{"tfm":"{}","framework":{{"name":"Microsoft.NETCore.App","version":"{}"}}}}}}"#,
        config.min_framework, full_version
    );
    let mut tmp: tempfile::NamedTempFile =
        tempfile::NamedTempFile::new().map_err(|e: std::io::Error| {
            PolyplugError::Loader(LoaderError::ClrInitFailed {
                path: "<tempfile>".to_owned(),
                reason: e.to_string(),
            })
        })?;
    tmp.write_all(json.as_bytes())
        .map_err(|e: std::io::Error| {
            PolyplugError::Loader(LoaderError::ClrInitFailed {
                path: "<tempfile>".to_owned(),
                reason: e.to_string(),
            })
        })?;
    tmp.flush().map_err(|e: std::io::Error| {
        PolyplugError::Loader(LoaderError::ClrInitFailed {
            path: "<tempfile>".to_owned(),
            reason: e.to_string(),
        })
    })?;

    // Capture path before consuming `tmp`.
    let temp_path: PathBuf = tmp.path().to_path_buf();
    // Keep tmp alive so the file remains on disk until hostfxr has read it.
    // We will explicitly delete it after CLR init rather than using mem::forget.
    let _tmp_guard: tempfile::NamedTempFile = tmp;

    // Step 3: Convert path to PdCString.
    let pdcpath: PdCString = PdCString::from_os_str(temp_path.as_os_str()).map_err(|_| {
        PolyplugError::Loader(LoaderError::ClrInitFailed {
            path: temp_path.to_string_lossy().into_owned(),
            reason: "runtimeconfig path contains embedded nul byte".to_owned(),
        })
    })?;

    // Step 4: Locate and load hostfxr.
    let hostfxr: netcorehost::hostfxr::Hostfxr = match &config.hostfxr {
        HostfxrLocation::Auto => netcorehost::nethost::load_hostfxr().map_err(|e| {
            PolyplugError::Loader(LoaderError::ClrInitFailed {
                path: "<hostfxr>".to_owned(),
                reason: e.to_string(),
            })
        })?,
        HostfxrLocation::Path(p) => {
            netcorehost::hostfxr::Hostfxr::load_from_path(p).map_err(|e| {
                PolyplugError::Loader(LoaderError::ClrInitFailed {
                    path: p.to_string_lossy().into_owned(),
                    reason: e.to_string(),
                })
            })?
        }
    };

    // Step 5: Initialize for runtime config (this loads the CLR).
    let context: netcorehost::hostfxr::HostfxrContext<
        netcorehost::hostfxr::InitializedForRuntimeConfig,
    > = hostfxr
        .initialize_for_runtime_config(pdcpath)
        .map_err(|e| {
            PolyplugError::Loader(LoaderError::ClrInitFailed {
                path: temp_path.to_string_lossy().into_owned(),
                reason: e.to_string(),
            })
        })?;

    // Step 6: Explicitly delete the temp file now that hostfxr has read it synchronously.
    // Intentionally ignore the error — best-effort cleanup only.
    let _: Result<(), std::io::Error> = std::fs::remove_file(&temp_path);
    // _tmp_guard would also attempt deletion on drop, but the file is already gone — that's fine.

    Ok(Arc::new(DotnetContext {
        _context: Mutex::new(context),
        loader_cache: Mutex::new(HashMap::new()),
    }))
}

impl DotnetContext {
    /// Get the `[UnmanagedCallersOnly]` init function for the given assembly path.
    ///
    /// Uses a per-assembly loader cache — `AssemblyDelegateLoader` is obtained at most once
    /// per canonical path.
    ///
    /// # Lock Ordering (deadlock prevention)
    ///
    /// NEVER hold both `loader_cache` and `_context` locks simultaneously.
    /// Protocol:
    /// 1. Lock `loader_cache` → check for existing entry → if hit, clone Arc, drop lock, get fn.
    /// 2. On cache miss: drop `loader_cache` lock, lock `_context`, obtain new loader, drop
    ///    `_context` lock.
    /// 3. Lock `loader_cache` again → insert (or accept race winner) → get fn → drop lock.
    pub(crate) fn get_init_fn(
        &self,
        asm_path: PathBuf,
        type_name: &PdCStr,
        method_name: &PdCStr,
    ) -> Result<ManagedFunction<InitFn>, PolyplugError> {
        // Step 1: Check cache — short-circuit if the loader is already present.
        let maybe_loader: Option<Arc<Mutex<AssemblyDelegateLoader>>> = {
            let cache: std::sync::MutexGuard<
                '_,
                HashMap<PathBuf, Arc<Mutex<AssemblyDelegateLoader>>>,
            > = self.loader_cache.lock().map_err(|_| {
                PolyplugError::Loader(LoaderError::ClrInitFailed {
                    path: asm_path.to_string_lossy().into_owned(),
                    reason: "loader cache mutex poisoned".to_owned(),
                })
            })?;
            cache.get(&asm_path).map(Arc::clone)
            // loader_cache lock is dropped here.
        };

        if let Some(loader_arc) = maybe_loader {
            let loader: std::sync::MutexGuard<'_, AssemblyDelegateLoader> =
                loader_arc.lock().map_err(|_| {
                    PolyplugError::Loader(LoaderError::ClrInitFailed {
                        path: asm_path.to_string_lossy().into_owned(),
                        reason: "assembly loader mutex poisoned".to_owned(),
                    })
                })?;
            return loader
                .get_function_with_unmanaged_callers_only::<InitFn>(type_name, method_name)
                .map_err(|_| {
                    PolyplugError::Loader(LoaderError::InitSymbolMissing {
                        bundle: asm_path.to_string_lossy().into_owned(),
                    })
                });
        }

        // Step 2: Cache miss — build the PdCString path and get a new loader from _context.
        let asm_pdc: PdCString = PdCString::from_os_str(asm_path.as_os_str()).map_err(|_| {
            PolyplugError::Loader(LoaderError::AssemblyNotFound {
                path: asm_path.to_string_lossy().into_owned(),
            })
        })?;

        let new_loader: AssemblyDelegateLoader = {
            let ctx: std::sync::MutexGuard<
                '_,
                netcorehost::hostfxr::HostfxrContext<
                    netcorehost::hostfxr::InitializedForRuntimeConfig,
                >,
            > = self._context.lock().map_err(|_| {
                PolyplugError::Loader(LoaderError::ClrInitFailed {
                    path: asm_path.to_string_lossy().into_owned(),
                    reason: "CLR context mutex poisoned".to_owned(),
                })
            })?;
            ctx.get_delegate_loader_for_assembly(asm_pdc).map_err(|e| {
                PolyplugError::Loader(LoaderError::ClrInitFailed {
                    path: asm_path.to_string_lossy().into_owned(),
                    reason: e.to_string(),
                })
            })?
            // _context lock is dropped here.
        };

        // Step 3: Insert into cache and get the function pointer.
        // Accept a race: if another thread inserted while we held no lock, use its entry.
        let loader_arc: Arc<Mutex<AssemblyDelegateLoader>> = {
            let mut cache: std::sync::MutexGuard<
                '_,
                HashMap<PathBuf, Arc<Mutex<AssemblyDelegateLoader>>>,
            > = self.loader_cache.lock().map_err(|_| {
                PolyplugError::Loader(LoaderError::ClrInitFailed {
                    path: asm_path.to_string_lossy().into_owned(),
                    reason: "loader cache mutex poisoned (relock)".to_owned(),
                })
            })?;
            // `or_insert` wins the race correctly — if a concurrent thread already inserted,
            // we discard `new_loader` and reuse the existing Arc.
            Arc::clone(
                cache
                    .entry(asm_path.clone())
                    .or_insert_with(|| Arc::new(Mutex::new(new_loader))),
            )
            // loader_cache lock is dropped here.
        };

        let loader: std::sync::MutexGuard<'_, AssemblyDelegateLoader> =
            loader_arc.lock().map_err(|_| {
                PolyplugError::Loader(LoaderError::ClrInitFailed {
                    path: asm_path.to_string_lossy().into_owned(),
                    reason: "assembly loader mutex poisoned (new)".to_owned(),
                })
            })?;
        loader
            .get_function_with_unmanaged_callers_only::<InitFn>(type_name, method_name)
            .map_err(|_| {
                PolyplugError::Loader(LoaderError::InitSymbolMissing {
                    bundle: asm_path.to_string_lossy().into_owned(),
                })
            })
    }
}

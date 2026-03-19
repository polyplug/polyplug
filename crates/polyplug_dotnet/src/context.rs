//! DotnetContext — CLR runtime context, cached across all plugin loads.

use std::collections::HashMap;
use std::fs;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use netcorehost::hostfxr::AssemblyDelegateLoader;
use netcorehost::hostfxr::HostfxrContext;
use netcorehost::hostfxr::InitializedForRuntimeConfig;
use netcorehost::hostfxr::ManagedFunction;
use netcorehost::pdcstring::PdCStr;
use netcorehost::pdcstring::PdCString;

use once_cell::sync::OnceCell;

use polyplug::error::LoaderError;
use polyplug::error::PolyplugError;
use polyplug_abi::PluginRegistrar;

use crate::config::DotnetConfig;
use crate::config::HostfxrLocation;

/// InitFn: the [UnmanagedCallersOnly] entry point signature.
/// Uses `extern "system"` because `netcorehost::ManagedFunction<F>` requires `F: ManagedFnPtr`
/// which requires `<F as FnPtr>::Abi == System`. On Linux/macOS `"system"` is identical to `"C"`.
pub(crate) type InitFn =
    unsafe extern "system" fn(*mut PluginRegistrar, *const polyplug_abi::PluginContext) -> u32;

/// DotnetContext holds the live CLR runtime and per-assembly loader cache.
/// Created exactly once per process via CLR_CONTEXT.
pub(crate) struct DotnetContext {
    /// Held to keep CLR runtime alive. Never locked after initialization except to obtain delegate loaders.
    _context: Mutex<HostfxrContext<InitializedForRuntimeConfig>>,
    /// Per-assembly loader cache.
    /// Each `AssemblyDelegateLoader` is wrapped in `Arc<Mutex<...>>` because
    /// `AssemblyDelegateLoader` is `!Send` (it contains raw function pointers without an
    /// explicit `Send` impl). The outer `Mutex` synchronizes access to the map itself;
    /// the inner `Arc<Mutex<AssemblyDelegateLoader>>` allows the loader to be cloned
    /// out of the map and used without holding the outer lock.
    loader_cache: Mutex<HashMap<PathBuf, Arc<Mutex<AssemblyDelegateLoader>>>>,
}

/// Global CLR context — initialized exactly once per process.
pub(crate) static CLR_CONTEXT: OnceCell<Arc<DotnetContext>> = OnceCell::new();

/// Initialize a fresh `DotnetContext` from the given config.
///
/// Generates a `runtimeconfig.json` in a temp file, initializes hostfxr, and returns
/// an `Arc<DotnetContext>`. The caller is responsible for storing the result in `CLR_CONTEXT`
/// via `OnceCell::get_or_try_init`.
pub(crate) fn init_context(
    config: &DotnetConfig,
    bundle_dir: &std::path::Path,
) -> Result<Arc<DotnetContext>, PolyplugError> {
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
    // NOTE: additionalProbingPaths set from first-loaded bundle dir only (CLR_CONTEXT is OnceCell).
    let json: String = format!(
        "{{\"runtimeOptions\":{{\"tfm\":\"{}\",\"framework\":{{\"name\":\"Microsoft.NETCore.App\",\"version\":\"{}\"}},\"additionalProbingPaths\":[\"{}\"]}}}}",
        config.min_framework,
        full_version,
        bundle_dir.to_string_lossy()
    );
    // hostfxr requires the runtimeconfig file to have a ".json" extension —
    // it uses the filename to detect the file type. tempfile::NamedTempFile::new()
    // creates files with no extension, causing hostfxr to reject the config.
    let mut tmp: tempfile::NamedTempFile = tempfile::Builder::new()
        .suffix(".json")
        .tempfile()
        .map_err(|e: std::io::Error| {
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

    // Step 4: Locate and load hostfxr directly by scanning well-known paths.
    // For HostfxrLocation::Auto we do NOT use netcorehost::nethost::load_hostfxr().
    // That function calls into the bundled libnethost.a (statically linked by nethost-sys).
    // The bundled binary may be a different patch version than the installed .NET runtime,
    // causing hostpolicy to reject the context init with "Arguments to hostpolicy are invalid".
    // Instead we scan for libhostfxr.so ourselves — identical to what the C++ reference
    // implementation does — and call Hostfxr::load_from_path() directly.
    let hostfxr: netcorehost::hostfxr::Hostfxr = match &config.hostfxr {
        HostfxrLocation::Auto => {
            let fxr_path: PathBuf = find_hostfxr_auto().ok_or_else(|| {
                PolyplugError::Loader(LoaderError::ClrInitFailed {
                    path: "<hostfxr>".to_owned(),
                    reason: "hostfxr not found; install .NET or set DOTNET_ROOT".to_owned(),
                })
            })?;
            netcorehost::hostfxr::Hostfxr::load_from_path(&fxr_path).map_err(|e| {
                PolyplugError::Loader(LoaderError::ClrInitFailed {
                    path: fxr_path.to_string_lossy().into_owned(),
                    reason: e.to_string(),
                })
            })?
        }
        HostfxrLocation::Path(p) => {
            netcorehost::hostfxr::Hostfxr::load_from_path(p).map_err(|e| {
                PolyplugError::Loader(LoaderError::ClrInitFailed {
                    path: p.to_string_lossy().into_owned(),
                    reason: e.to_string(),
                })
            })?
        }
    };

    // Step 5: Initialize for runtime config.
    // hostfxr locates shared frameworks relative to the runtimeconfig file's directory.
    // The .json extension on the temp file is required — hostfxr uses the filename to
    // detect file type (tempfile::Builder::suffix(".json") above ensures this).
    let context: HostfxrContext<InitializedForRuntimeConfig> = hostfxr
        .initialize_for_runtime_config(&pdcpath)
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
                .map_err(|e| {
                    PolyplugError::Loader(LoaderError::InitSymbolMissing {
                        bundle: format!("{}: error={}", asm_path.to_string_lossy().into_owned(), e),
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
            let ctx: std::sync::MutexGuard<'_, HostfxrContext<InitializedForRuntimeConfig>> =
                self._context.lock().map_err(|_| {
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
            .map_err(|e| {
                PolyplugError::Loader(LoaderError::InitSymbolMissing {
                    bundle: format!("{}: error={}", asm_path.to_string_lossy().into_owned(), e),
                })
            })
    }
}

/// Scan well-known locations to find `libhostfxr.so` / `hostfxr.dll` without using
/// the bundled `libnethost.a` from nethost-sys.
///
/// Search order (matches the C++ reference implementation and Epic 9 PRD spec):
/// 1. `DOTNET_ROOT` environment variable
/// 2. `PATH` entries that contain a `dotnet` binary
/// 3. Well-known system paths: `/usr/share/dotnet`, `/usr/lib/dotnet`, `~/.dotnet`
///
/// Within each candidate dotnet root, picks the highest-version `host/fxr/<ver>/libhostfxr.so`.
fn find_hostfxr_auto() -> Option<PathBuf> {
    // Build the list of candidate dotnet roots to search.
    let mut roots: Vec<PathBuf> = Vec::new();

    // 1. DOTNET_ROOT env var.
    if let Some(val) = std::env::var_os("DOTNET_ROOT") {
        roots.push(PathBuf::from(val));
    }

    // 2. Directories on PATH that contain a `dotnet` binary.
    if let Some(path_val) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_val) {
            let candidate: PathBuf = dir.join("dotnet");
            if candidate.exists() {
                roots.push(dir);
            }
        }
    }

    // 3. Well-known system paths.
    roots.push(PathBuf::from("/usr/share/dotnet"));
    roots.push(PathBuf::from("/usr/lib/dotnet"));
    if let Some(home) = std::env::var_os("HOME") {
        roots.push(PathBuf::from(home).join(".dotnet"));
    }

    // For each root, look for host/fxr/<version>/libhostfxr.so and pick the
    // highest version found.
    for root in &roots {
        if let Some(fxr_path) = highest_version_hostfxr(root) {
            return Some(fxr_path);
        }
    }

    None
}

/// Within `<dotnet_root>/host/fxr/`, enumerate version subdirectories and return
/// the path to `libhostfxr.so` (or `hostfxr.dll` on Windows) inside the highest
/// version found. Returns `None` if the directory does not exist or is empty.
fn highest_version_hostfxr(dotnet_root: &std::path::Path) -> Option<PathBuf> {
    let fxr_dir: PathBuf = dotnet_root.join("host").join("fxr");
    if !fxr_dir.is_dir() {
        return None;
    }

    // Collect all subdirectory names that look like version strings.
    let mut versions: Vec<(Vec<u64>, PathBuf)> = Vec::new();
    let entries: fs::ReadDir = fs::read_dir(&fxr_dir).ok()?;
    for entry in entries.flatten() {
        let path: PathBuf = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name: String = path.file_name()?.to_string_lossy().into_owned();
        // Parse "major.minor.patch" into comparable numeric tuple.
        let parts: Vec<u64> = name
            .split('.')
            .map(|s: &str| s.parse::<u64>().unwrap_or(0))
            .collect();
        if parts.is_empty() {
            continue;
        }
        versions.push((parts, path));
    }

    if versions.is_empty() {
        return None;
    }

    // Sort descending so index 0 is the highest version.
    versions.sort_by(|a: &(Vec<u64>, PathBuf), b: &(Vec<u64>, PathBuf)| b.0.cmp(&a.0));

    // Platform-specific hostfxr filename.
    #[cfg(target_os = "windows")]
    let lib_name: &str = "hostfxr.dll";
    #[cfg(target_os = "macos")]
    let lib_name: &str = "libhostfxr.dylib";
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let lib_name: &str = "libhostfxr.so";

    let best_path: PathBuf = versions[0].1.join(lib_name);
    if best_path.exists() {
        Some(best_path)
    } else {
        None
    }
}

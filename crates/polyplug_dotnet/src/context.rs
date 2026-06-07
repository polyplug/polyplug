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
use polyplug::error::RuntimeError;

use crate::config::DotnetConfig;
use crate::config::HostfxrLocation;

/// InitFn: the [UnmanagedCallersOnly] entry point signature.
/// Uses `extern "system"` because `netcorehost::ManagedFunction<F>` requires `F: ManagedFnPtr`
/// which requires `<F as FnPtr>::Abi == System`. On Linux/macOS `"system"` is identical to `"C"`.
///
/// New ABI signature: (host, ctx) -> u32
/// - host: HostApi pointer (self-passing pattern)
/// - ctx: BundleInitContext with bundle_path, bundle_id
pub(crate) type InitFn = unsafe extern "system" fn(
    *const polyplug_abi::HostApi,
    *const polyplug_abi::BundleInitContext,
) -> u32;

/// Embedded bytes of the managed byte-load bridge assembly, staged by `build.rs` into
/// `OUT_DIR/byte_bridge.dll`. When the .NET SDK is unavailable at build time the staged file
/// is empty and [`BYTE_BRIDGE_AVAILABLE`] is `false`, so `Bytes` loads are rejected cleanly.
pub(crate) const BYTE_BRIDGE_DLL: &[u8] = include_bytes!(env!("POLYPLUG_DOTNET_BYTE_BRIDGE_DLL"));

/// Whether [`BYTE_BRIDGE_DLL`] holds a real assembly (`"1"`) or an empty placeholder (`"0"`).
pub(crate) const BYTE_BRIDGE_AVAILABLE: bool = matches!(
    env!("POLYPLUG_DOTNET_BYTE_BRIDGE_AVAILABLE").as_bytes(),
    b"1"
);

/// Signature of the bridge's `PolyplugByteBridgeLoadAndGetInit` entry point:
/// `(asm_ptr, asm_len, type_name_ptr, type_name_len) -> init_fn_ptr (null on failure)`.
pub(crate) type BridgeLoadAndGetInitFn =
    unsafe extern "system" fn(*const u8, i32, *const u8, i32) -> *const core::ffi::c_void;

/// Signature of the bridge's `PolyplugByteBridgePreloadDependency` entry point:
/// `(asm_ptr, asm_len) -> u32 (0 on success)`.
pub(crate) type BridgePreloadDependencyFn = unsafe extern "system" fn(*const u8, i32) -> u32;

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
    /// Lazily-resolved managed byte-load bridge entry points. The embedded bridge assembly
    /// is staged to a temp file and loaded once; its `[UnmanagedCallersOnly]` entry points
    /// are resolved through the path-based delegate loader (which works for on-disk
    /// assemblies) and cached here for the process lifetime.
    bridge: OnceCell<ByteBridge>,
}

/// Resolved entry points of the managed byte-load bridge, plus the temp file the bridge
/// assembly was staged to (kept alive so the CLR's on-disk copy is never removed while the
/// process runs — the retire-not-drop discipline the rest of the loader follows).
struct ByteBridge {
    load_and_get_init: BridgeLoadAndGetInitFn,
    preload_dependency: BridgePreloadDependencyFn,
    /// Owns the directory holding the staged bridge dll; kept alive for the process lifetime
    /// so the CLR's loaded image is never removed (retire-not-drop). Never read after init.
    _staged_dir: tempfile::TempDir,
}

// SAFETY: `ByteBridge` holds two `extern "system"` function pointers into the CLR (which is
// process-global and lives for the whole run) and a `NamedTempFile` (already Send + Sync).
// Raw fn pointers are `Send`/`Sync`; the pointed-to CLR code is callable from any thread.
unsafe impl Send for ByteBridge {}
// SAFETY: see the `Send` impl above — the contained pointers are immutable after init.
unsafe impl Sync for ByteBridge {}

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
) -> Result<Arc<DotnetContext>, RuntimeError> {
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
    // Build via serde_json so paths with backslashes (Windows) or quotes are escaped correctly.
    let json: String = serde_json::json!({
        "runtimeOptions": {
            "tfm": config.min_framework,
            "framework": {
                "name": "Microsoft.NETCore.App",
                "version": full_version
            },
            "additionalProbingPaths": [bundle_dir.to_string_lossy()]
        }
    })
    .to_string();
    // hostfxr requires the runtimeconfig file to have a ".json" extension —
    // it uses the filename to detect the file type. tempfile::NamedTempFile::new()
    // creates files with no extension, causing hostfxr to reject the config.
    let mut tmp: tempfile::NamedTempFile = tempfile::Builder::new()
        .suffix(".json")
        .tempfile()
        .map_err(|e: std::io::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: "<tempfile>".to_owned(),
                error: format!("CLR init failed: {}", e),
            })
        })?;
    tmp.write_all(json.as_bytes())
        .map_err(|e: std::io::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: "<tempfile>".to_owned(),
                error: format!("CLR init failed: {}", e),
            })
        })?;
    tmp.flush().map_err(|e: std::io::Error| {
        RuntimeError::Loader(LoaderError::InitFailed {
            bundle: "<tempfile>".to_owned(),
            error: format!("CLR init failed: {}", e),
        })
    })?;

    // Close the write handle before hostfxr reads the file, but keep the file on disk.
    // On Windows, hostfxr opens the runtimeconfig with CreateFileW(GENERIC_READ,
    // FILE_SHARE_READ); a still-open write handle is not covered by that share mode, so
    // CreateFileW returns ERROR_SHARING_VIOLATION (32), which hostfxr reports as an
    // invalid runtimeconfig.json. into_temp_path() closes the handle while retaining the
    // file (TempPath deletes it on drop/close).
    let tmp_path_guard: tempfile::TempPath = tmp.into_temp_path();
    let temp_path: PathBuf = tmp_path_guard.to_path_buf();

    // Step 3: Convert path to PdCString.
    let pdcpath: PdCString = PdCString::from_os_str(temp_path.as_os_str()).map_err(|_| {
        RuntimeError::Loader(LoaderError::InitFailed {
            bundle: temp_path.to_string_lossy().into_owned(),
            error: "CLR init failed: runtimeconfig path contains embedded nul byte".to_owned(),
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
                RuntimeError::Loader(LoaderError::InitFailed {
                    bundle: "<hostfxr>".to_owned(),
                    error: "CLR init failed: hostfxr not found; install .NET or set DOTNET_ROOT"
                        .to_owned(),
                })
            })?;
            netcorehost::hostfxr::Hostfxr::load_from_path(&fxr_path).map_err(|e| {
                RuntimeError::Loader(LoaderError::InitFailed {
                    bundle: fxr_path.to_string_lossy().into_owned(),
                    error: format!("CLR init failed: {}", e),
                })
            })?
        }
        HostfxrLocation::Path(p) => {
            netcorehost::hostfxr::Hostfxr::load_from_path(p).map_err(|e| {
                RuntimeError::Loader(LoaderError::InitFailed {
                    bundle: p.to_string_lossy().into_owned(),
                    error: format!("CLR init failed: {}", e),
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
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: temp_path.to_string_lossy().into_owned(),
                error: format!("CLR init failed: {}", e),
            })
        })?;

    // Step 6: Explicitly delete the temp file now that hostfxr has read it synchronously.
    // Intentionally ignore the error — best-effort cleanup only.
    let _: Result<(), std::io::Error> = tmp_path_guard.close();

    Ok(Arc::new(DotnetContext {
        _context: Mutex::new(context),
        loader_cache: Mutex::new(HashMap::new()),
        bridge: OnceCell::new(),
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
    ) -> Result<ManagedFunction<InitFn>, RuntimeError> {
        // Step 1: Check cache — short-circuit if the loader is already present.
        let maybe_loader: Option<Arc<Mutex<AssemblyDelegateLoader>>> = {
            let cache: std::sync::MutexGuard<
                '_,
                HashMap<PathBuf, Arc<Mutex<AssemblyDelegateLoader>>>,
            > = self.loader_cache.lock().map_err(|_| {
                RuntimeError::Loader(LoaderError::InitFailed {
                    bundle: asm_path.to_string_lossy().into_owned(),
                    error: "CLR init failed: loader cache mutex poisoned".to_owned(),
                })
            })?;
            cache.get(&asm_path).map(Arc::clone)
            // loader_cache lock is dropped here.
        };

        if let Some(loader_arc) = maybe_loader {
            let loader: std::sync::MutexGuard<'_, AssemblyDelegateLoader> =
                loader_arc.lock().map_err(|_| {
                    RuntimeError::Loader(LoaderError::InitFailed {
                        bundle: asm_path.to_string_lossy().into_owned(),
                        error: "CLR init failed: assembly loader mutex poisoned".to_owned(),
                    })
                })?;
            return loader
                .get_function_with_unmanaged_callers_only::<InitFn>(type_name, method_name)
                .map_err(|e| {
                    RuntimeError::Loader(LoaderError::InitSymbolMissing {
                        bundle: format!("{}: error={}", asm_path.to_string_lossy().into_owned(), e),
                    })
                });
        }

        // Step 2: Cache miss — build the PdCString path and get a new loader from _context.
        let asm_pdc: PdCString = PdCString::from_os_str(asm_path.as_os_str()).map_err(|_| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: asm_path.to_string_lossy().into_owned(),
                error: "CLR init failed: assembly path contains embedded nul byte".to_owned(),
            })
        })?;

        let new_loader: AssemblyDelegateLoader = {
            let ctx: std::sync::MutexGuard<'_, HostfxrContext<InitializedForRuntimeConfig>> =
                self._context.lock().map_err(|_| {
                    RuntimeError::Loader(LoaderError::InitFailed {
                        bundle: asm_path.to_string_lossy().into_owned(),
                        error: "CLR init failed: CLR context mutex poisoned".to_owned(),
                    })
                })?;
            ctx.get_delegate_loader_for_assembly(asm_pdc).map_err(|e| {
                RuntimeError::Loader(LoaderError::InitFailed {
                    bundle: asm_path.to_string_lossy().into_owned(),
                    error: format!("CLR init failed: {}", e),
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
                RuntimeError::Loader(LoaderError::InitFailed {
                    bundle: asm_path.to_string_lossy().into_owned(),
                    error: "CLR init failed: loader cache mutex poisoned (relock)".to_owned(),
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
                RuntimeError::Loader(LoaderError::InitFailed {
                    bundle: asm_path.to_string_lossy().into_owned(),
                    error: "CLR init failed: assembly loader mutex poisoned (new)".to_owned(),
                })
            })?;
        loader
            .get_function_with_unmanaged_callers_only::<InitFn>(type_name, method_name)
            .map_err(|e| {
                RuntimeError::Loader(LoaderError::InitSymbolMissing {
                    bundle: format!("{}: error={}", asm_path.to_string_lossy().into_owned(), e),
                })
            })
    }

    /// Resolve (once) and return the managed byte-load bridge entry points.
    ///
    /// The embedded bridge assembly ([`BYTE_BRIDGE_DLL`]) is staged to a temp file and
    /// loaded through the path-based delegate loader — which, unlike the path-less delegate,
    /// resolves `[UnmanagedCallersOnly]` methods from on-disk assemblies reliably. The
    /// bridge is self-contained (no dependencies), so it loads from any temp path.
    fn bridge(&self) -> Result<&ByteBridge, RuntimeError> {
        if !BYTE_BRIDGE_AVAILABLE || BYTE_BRIDGE_DLL.is_empty() {
            return Err(RuntimeError::Loader(LoaderError::InitFailed {
                bundle: "<byte-bridge>".to_owned(),
                error: "byte-load bridge unavailable: rebuild polyplug_dotnet with the .NET SDK installed".to_owned(),
            }));
        }
        self.bridge.get_or_try_init(|| self.init_bridge())
    }

    /// Stage the embedded bridge assembly to a temp file and resolve its entry points.
    fn init_bridge(&self) -> Result<ByteBridge, RuntimeError> {
        // hostfxr keys delegate loaders by assembly filename, and the bridge resolves its own
        // type by the assembly simple name, so the staged file must be named after the bridge
        // assembly: `Polyplug.Loaders.DotnetByteBridge.dll`.
        let dir: tempfile::TempDir = tempfile::Builder::new()
            .prefix("polyplug-dotnet-bridge")
            .tempdir()
            .map_err(|e: std::io::Error| {
                RuntimeError::Loader(LoaderError::InitFailed {
                    bundle: "<byte-bridge>".to_owned(),
                    error: format!("byte-bridge stage failed: {e}"),
                })
            })?;
        let dll_path: PathBuf = dir.path().join("Polyplug.Loaders.DotnetByteBridge.dll");
        std::fs::write(&dll_path, BYTE_BRIDGE_DLL).map_err(|e: std::io::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: "<byte-bridge>".to_owned(),
                error: format!("byte-bridge write failed: {e}"),
            })
        })?;

        let asm_pdc: PdCString = PdCString::from_os_str(dll_path.as_os_str()).map_err(|_| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: "<byte-bridge>".to_owned(),
                error: "byte-bridge path contains embedded nul byte".to_owned(),
            })
        })?;

        let loader: AssemblyDelegateLoader = {
            let ctx: std::sync::MutexGuard<'_, HostfxrContext<InitializedForRuntimeConfig>> =
                self._context.lock().map_err(|_| {
                    RuntimeError::Loader(LoaderError::InitFailed {
                        bundle: "<byte-bridge>".to_owned(),
                        error: "CLR context mutex poisoned (bridge)".to_owned(),
                    })
                })?;
            ctx.get_delegate_loader_for_assembly(asm_pdc).map_err(|e| {
                RuntimeError::Loader(LoaderError::InitFailed {
                    bundle: "<byte-bridge>".to_owned(),
                    error: format!("byte-bridge loader init failed: {e}"),
                })
            })?
            // _context lock dropped here.
        };

        // hostfxr's get_function_with_unmanaged_callers_only resolves by the managed method
        // name, not the `[UnmanagedCallersOnly(EntryPoint = ...)]` export name.
        let type_pdc: PdCString = bridge_pdc(
            "Polyplug.Loaders.DotnetByteBridge.ByteBridge, Polyplug.Loaders.DotnetByteBridge",
        )?;
        let load_method_pdc: PdCString = bridge_pdc("LoadAndGetInit")?;
        let preload_method_pdc: PdCString = bridge_pdc("PreloadDependency")?;

        let load_and_get_init: BridgeLoadAndGetInitFn = {
            let f: ManagedFunction<BridgeLoadAndGetInitFn> = loader
                .get_function_with_unmanaged_callers_only::<BridgeLoadAndGetInitFn>(
                    type_pdc.as_ref(),
                    load_method_pdc.as_ref(),
                )
                .map_err(|e| {
                    RuntimeError::Loader(LoaderError::InitSymbolMissing {
                        bundle: format!("<byte-bridge>: LoadAndGetInit error={e}"),
                    })
                })?;
            *f
        };
        let preload_dependency: BridgePreloadDependencyFn = {
            let f: ManagedFunction<BridgePreloadDependencyFn> = loader
                .get_function_with_unmanaged_callers_only::<BridgePreloadDependencyFn>(
                    type_pdc.as_ref(),
                    preload_method_pdc.as_ref(),
                )
                .map_err(|e| {
                    RuntimeError::Loader(LoaderError::InitSymbolMissing {
                        bundle: format!("<byte-bridge>: PreloadDependency error={e}"),
                    })
                })?;
            *f
        };

        Ok(ByteBridge {
            load_and_get_init,
            preload_dependency,
            _staged_dir: dir,
        })
    }

    /// Pre-load a dependency assembly from raw bytes into the default load context via the
    /// managed bridge, so a non-self-contained byte-loaded plugin can resolve its references.
    ///
    /// # Memory ownership (CLAUDE.md Rule 8)
    ///
    /// `dep_bytes` is borrowed only for the duration of the managed call; the bridge copies
    /// the buffer into managed storage before returning.
    pub(crate) fn preload_dependency_bytes(&self, dep_bytes: &[u8]) -> Result<(), RuntimeError> {
        let bridge: &ByteBridge = self.bridge()?;
        // SAFETY: `preload_dependency` is a valid CLR fn ptr resolved in `init_bridge`.
        // `dep_bytes` is a valid, correctly-sized buffer; the bridge copies it during the call.
        let code: u32 =
            unsafe { (bridge.preload_dependency)(dep_bytes.as_ptr(), dep_bytes.len() as i32) };
        if code != 0 {
            return Err(RuntimeError::Loader(LoaderError::InitFailed {
                bundle: "<byte-bridge-dependency>".to_owned(),
                error: format!("bridge PreloadDependency returned {code}"),
            }));
        }
        Ok(())
    }

    /// Get the `[UnmanagedCallersOnly]` `PolyplugInit` function for a plugin assembly
    /// supplied as raw bytes, via the managed byte-load bridge.
    ///
    /// The bridge loads `asm_bytes` into the default `AssemblyLoadContext` with
    /// `LoadFromStream` and resolves `simple_type_name` (e.g. `"CsharpPlugin.Plugin"`)
    /// directly from the returned `Assembly` object — avoiding hostfxr's name-rebinding path
    /// that cannot find a stream-loaded assembly. `asm_name` is used only in diagnostics.
    ///
    /// # Memory ownership (CLAUDE.md Rule 8)
    ///
    /// `asm_bytes` is borrowed only for the duration of the managed call; the bridge copies
    /// the buffer into managed storage before returning, so the caller may drop it after.
    pub(crate) fn get_init_fn_from_bytes(
        &self,
        asm_name: &str,
        asm_bytes: &[u8],
        simple_type_name: &str,
    ) -> Result<InitFn, RuntimeError> {
        let bridge: &ByteBridge = self.bridge()?;
        let type_name_bytes: &[u8] = simple_type_name.as_bytes();
        // SAFETY: `load_and_get_init` is a valid CLR fn ptr resolved in `init_bridge`.
        // Both buffers are valid and correctly sized; the bridge copies them during the call
        // and returns either a valid native fn ptr or null.
        let raw: *const core::ffi::c_void = unsafe {
            (bridge.load_and_get_init)(
                asm_bytes.as_ptr(),
                asm_bytes.len() as i32,
                type_name_bytes.as_ptr(),
                type_name_bytes.len() as i32,
            )
        };
        if raw.is_null() {
            return Err(RuntimeError::Loader(LoaderError::InitSymbolMissing {
                bundle: format!(
                    "{asm_name}: byte-bridge could not resolve {simple_type_name}.PolyplugInit"
                ),
            }));
        }
        // SAFETY: a non-null return is the native entry of the guest's `[UnmanagedCallersOnly]`
        // PolyplugInit, whose ABI matches `InitFn` (host, ctx) -> u32. Transmuting a raw fn
        // pointer of identical ABI is sound; the CLR keeps the method alive for the run.
        let init_fn: InitFn =
            unsafe { core::mem::transmute::<*const core::ffi::c_void, InitFn>(raw) };
        Ok(init_fn)
    }
}

/// Build a `PdCString` for a bridge type/method name, mapping nul-byte errors to a
/// structured loader error.
fn bridge_pdc(s: &str) -> Result<PdCString, RuntimeError> {
    PdCString::from_os_str(std::ffi::OsStr::new(s)).map_err(|_| {
        RuntimeError::Loader(LoaderError::InitFailed {
            bundle: "<byte-bridge>".to_owned(),
            error: format!("bridge name contains embedded nul byte: {s}"),
        })
    })
}

/// Scan well-known locations to find `libhostfxr.so` / `hostfxr.dll` without using
/// the bundled `libnethost.a` from nethost-sys.
///
/// Search order:
/// 1. `DOTNET_ROOT` environment variable
/// 2. `PATH` entries that contain a `dotnet` binary (`dotnet.exe` on Windows)
/// 3. Well-known system paths (per OS: `C:\Program Files\dotnet` on Windows,
///    `/usr/share/dotnet` / `/usr/lib/dotnet` / `~/.dotnet` on Unix)
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
    let dotnet_bin: &str = if cfg!(target_os = "windows") {
        "dotnet.exe"
    } else {
        "dotnet"
    };
    if let Some(path_val) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_val) {
            let candidate: PathBuf = dir.join(dotnet_bin);
            if candidate.exists() {
                roots.push(dir);
            }
        }
    }

    // 3. Well-known system paths.
    #[cfg(target_os = "windows")]
    {
        if let Some(program_files) = std::env::var_os("ProgramFiles") {
            roots.push(PathBuf::from(program_files).join("dotnet"));
        }
        if let Some(program_files_x86) = std::env::var_os("ProgramFiles(x86)") {
            roots.push(PathBuf::from(program_files_x86).join("dotnet"));
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        roots.push(PathBuf::from("/usr/share/dotnet"));
        roots.push(PathBuf::from("/usr/lib/dotnet"));
        if let Some(home) = std::env::var_os("HOME") {
            roots.push(PathBuf::from(home).join(".dotnet"));
        }
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
    let entries: fs::ReadDir = fs::read_dir(&fxr_dir)
        .map_err(|e| {
            eprintln!(
                "[polyplug_dotnet] highest_version_hostfxr: failed to read dir '{}': {}",
                fxr_dir.display(),
                e
            );
            e
        })
        .ok()?;
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

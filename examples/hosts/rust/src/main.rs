//! examples/hosts/rust/src/main.rs
//! Rust host example — discovers plugins via scanner, calls their functions.
//!
//! Real-world workflow:
//!   1. Read POLYPLUG_PLUGIN_PATH env var (or default to examples/plugins/)
//!   2. Use polyplug scanner to discover all bundles
//!   3. Load each discovered bundle
//!   4. Resolve and call plugin functions

use std::path::Path;
use std::path::PathBuf;

use polyplug::abi::AbiError;
use polyplug::abi::PluginHandle;
use polyplug::abi::PluginVTable;
use polyplug::abi::StringView;
use polyplug::abi::ABI_OK;
use polyplug::loader::manifest::ManifestData;
use polyplug::loader::scanner;
use polyplug::registry::PluginVTableGuard;
use polyplug::runtime::Runtime;
use polyplug_dotnet::DotnetConfig;
use polyplug_dotnet::DotnetLoader;
use polyplug_js::JsConfig;
use polyplug_js::JsLoader;
use polyplug_js_deno::JsDenoConfig;
use polyplug_js_deno::JsDenoLoader;
use polyplug_lua::LuaConfig;
use polyplug_lua::LuaLoader;
use polyplug_native::NativeConfig;
use polyplug_native::NativeLoader;
use polyplug_python::PythonConfig;
use polyplug_python::PythonLoader;

// ─── Contract IDs ────────────────────────────────────────────────────────────
// FNV-1a 64-bit of "data.Transformer@1" and "data.Reporter@1"
const TRANSFORMER_CONTRACT_ID: u64 = 0x3D53C682F3F5A9EF_u64;
const REPORTER_CONTRACT_ID: u64 = 0x81D41D43E511D297_u64;

// ─── ABI types ───────────────────────────────────────────────────────────────
type AbiFn = unsafe extern "C" fn(*const (), *mut ()) -> AbiError;

// ─── Read a StringView as &str ───────────────────────────────────────────────
fn string_view_to_str(sv: &StringView) -> &str {
    if sv.ptr.is_null() || sv.len == 0 {
        return "";
    }
    // SAFETY: sv.ptr is valid UTF-8 for sv.len bytes — guaranteed by guest ABI.
    unsafe {
        core::str::from_utf8(core::slice::from_raw_parts(sv.ptr, sv.len))
            .unwrap_or("(invalid utf-8)")
    }
}

// ─── Resolve plugin path from env or default ─────────────────────────────────
fn resolve_plugin_path() -> PathBuf {
    if let Ok(path) = std::env::var("POLYPLUG_PLUGIN_PATH") {
        return PathBuf::from(path);
    }

    let candidates: [PathBuf; 2] = [
        PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    ];

    for seed in &candidates {
        let mut dir: PathBuf = seed.clone();
        for _ in 0..8_u32 {
            let plugins_path: PathBuf = dir.join("examples").join("plugins");
            if plugins_path.is_dir() {
                return plugins_path;
            }
            match dir.parent() {
                Some(p) => dir = p.to_path_buf(),
                None => break,
            }
        }
    }

    PathBuf::from("examples/plugins")
}

fn main() {
    match run() {
        Ok(()) => {}
        Err(message) => {
            eprintln!("error: {message}");
            std::process::exit(1_i32);
        }
    }
}

fn run() -> Result<(), String> {
    let plugin_path: PathBuf = resolve_plugin_path();
    eprintln!("plugin directory: {}", plugin_path.display());

    // ─── Build runtime with all loaders ──────────────────────────────────────
    let runtime: Runtime = Runtime::builder()
        .loader(NativeLoader::new(NativeConfig {}))
        .loader(DotnetLoader::new(DotnetConfig::default()))
        .loader(PythonLoader::new(PythonConfig::default()))
        .loader(LuaLoader::new(LuaConfig::default()))
        .loader(JsLoader::new(JsConfig {}))
        .loader(JsDenoLoader::new(JsDenoConfig::default()))
        .build()
        .map_err(|e| format!("runtime build failed: {e}"))?;

    // ─── Scan for plugins ────────────────────────────────────────────────────
    let bundles: Vec<(PathBuf, ManifestData)> = scanner::scan_dir(&plugin_path);

    if bundles.is_empty() {
        return Err(format!(
            "no plugins found in {}. Run examples/build_all.sh first.",
            plugin_path.display()
        ));
    }

    eprintln!("discovered {} bundles", bundles.len());

    // ─── Load all discovered bundles ─────────────────────────────────────────
    for (bundle_path, manifest) in &bundles {
        runtime
            .load_bundle(Path::new(bundle_path))
            .map_err(|e| format!("failed to load {}: {e}", manifest.bundle_name))?;
        eprintln!("  loaded: {}", manifest.bundle_name);
    }

    // ─── Call each loaded plugin ─────────────────────────────────────────────
    for (_bundle_path, manifest) in &bundles {
        let contract_id: u64 = if manifest.provides.iter().any(|c| c == "data.Transformer") {
            TRANSFORMER_CONTRACT_ID
        } else if manifest.provides.iter().any(|c| c == "data.Reporter") {
            REPORTER_CONTRACT_ID
        } else {
            continue;
        };

        let fn_name: &str = if contract_id == TRANSFORMER_CONTRACT_ID {
            "transform"
        } else {
            "report"
        };

        let bid: u64 = polyplug::abi::bundle_id(&manifest.bundle_name);
        let handle: PluginHandle = runtime
            .find_by_bundle(bid, contract_id, 0_u32)
            .map_err(|e| format!("find_by_bundle({}): {e}", manifest.bundle_name))?;

        if handle.is_null() {
            return Err(format!(
                "plugin not found for bundle: {}",
                manifest.bundle_name
            ));
        }

        let guard: PluginVTableGuard = runtime
            .registry()
            .resolve_guard(handle)
            .map_err(|e| format!("resolve_guard({}): {e}", manifest.bundle_name))?;

        let vtable: *const PluginVTable = guard.vtable();
        if vtable.is_null() {
            return Err(format!("null vtable for bundle: {}", manifest.bundle_name));
        }

        let input_str: &str = "hello";
        let input_sv: StringView = StringView {
            ptr: input_str.as_ptr(),
            len: input_str.len(),
        };
        let mut output_sv: StringView = StringView::null();

        // SAFETY: vtable is non-null and valid for the lifetime of _guard.
        unsafe {
            let vt: &PluginVTable = &*vtable;
            if vt.function_count == 0_u32 || vt.functions.is_null() {
                return Err(format!(
                    "no functions in vtable for {}",
                    manifest.bundle_name
                ));
            }

            // SAFETY: functions[0] is valid per vtable contract.
            let fn_ptr_raw: *const () = *vt.functions.add(0_usize);
            if fn_ptr_raw.is_null() {
                return Err(format!(
                    "null function pointer for {}",
                    manifest.bundle_name
                ));
            }

            // SAFETY: fn_ptr_raw conforms to ABI signature.
            let func: AbiFn = core::mem::transmute(fn_ptr_raw);
            // SAFETY: input_sv and output_sv are valid stack-allocated structs.
            let err: AbiError = func(
                core::ptr::addr_of!(input_sv).cast::<()>(),
                core::ptr::addr_of_mut!(output_sv).cast::<()>(),
            );

            if err.code != ABI_OK {
                let msg: &str = if err.message.ptr.is_null() || err.message.len == 0 {
                    "unknown error"
                } else {
                    // SAFETY: error message is valid UTF-8 for message.len bytes.
                    core::str::from_utf8(core::slice::from_raw_parts(
                        err.message.ptr,
                        err.message.len,
                    ))
                    .unwrap_or("(invalid utf-8)")
                };
                return Err(format!(
                    "{} failed: {} (code {})",
                    manifest.bundle_name, msg, err.code
                ));
            }
        }

        let result: &str = string_view_to_str(&output_sv);
        let label: String = format!("[{}]", manifest.bundle_name);
        println!(
            "{:<30} {}(\"{}\") = \"{}\"",
            label, fn_name, input_str, result
        );
    }

    Ok(())
}

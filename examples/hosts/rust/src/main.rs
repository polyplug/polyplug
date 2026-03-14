//! examples/hosts/rust/src/main.rs
//! Rust host example — loads all 14 guest plugins and calls their functions.
//!
//! Demonstrates explicit loader registration for all 6 language runtimes,
//! then loads guests and invokes transform/report via vtable dispatch.

use std::path::Path;
use std::path::PathBuf;

use polyplug::abi::AbiError;
use polyplug::abi::PluginHandle;
use polyplug::abi::PluginVTable;
use polyplug::abi::StringView;
use polyplug::abi::ABI_OK;
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

// ─── FNV-1a 64-bit hash ─────────────────────────────────────────────────────
fn fnv1a_64(data: &[u8]) -> u64 {
    const FNV_OFFSET: u64 = 0xCBF29CE484222325_u64;
    const FNV_PRIME: u64 = 0x00000100000001B3_u64;
    let mut hash: u64 = FNV_OFFSET;
    for &byte in data {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

fn bundle_id(name: &str) -> u64 {
    fnv1a_64(name.as_bytes())
}

// ─── ABI types ───────────────────────────────────────────────────────────────
type AbiFn = unsafe extern "C" fn(*const (), *mut ()) -> AbiError;

struct PluginEntry {
    label: &'static str,
    _guard: PluginVTableGuard,
    vtable: *const PluginVTable,
}

// ─── Guest descriptor ────────────────────────────────────────────────────────
struct GuestSpec {
    dir: &'static str,
    bundle_name: &'static str,
    contract_id: u64,
    fn_name: &'static str,
}

const GUESTS: [GuestSpec; 14] = [
    GuestSpec {
        dir: "rust/decoder",
        bundle_name: "rust_transformer",
        contract_id: TRANSFORMER_CONTRACT_ID,
        fn_name: "transform",
    },
    GuestSpec {
        dir: "rust/reporter",
        bundle_name: "rust_reporter",
        contract_id: REPORTER_CONTRACT_ID,
        fn_name: "report",
    },
    GuestSpec {
        dir: "cpp/transformer",
        bundle_name: "cpp_transformer",
        contract_id: TRANSFORMER_CONTRACT_ID,
        fn_name: "transform",
    },
    GuestSpec {
        dir: "cpp/reporter",
        bundle_name: "cpp_reporter",
        contract_id: REPORTER_CONTRACT_ID,
        fn_name: "report",
    },
    GuestSpec {
        dir: "csharp/encoder",
        bundle_name: "csharp_transformer",
        contract_id: TRANSFORMER_CONTRACT_ID,
        fn_name: "transform",
    },
    GuestSpec {
        dir: "csharp/reporter",
        bundle_name: "csharp_reporter",
        contract_id: REPORTER_CONTRACT_ID,
        fn_name: "report",
    },
    GuestSpec {
        dir: "python/decoder",
        bundle_name: "python_transformer",
        contract_id: TRANSFORMER_CONTRACT_ID,
        fn_name: "transform",
    },
    GuestSpec {
        dir: "python/reporter",
        bundle_name: "python_reporter",
        contract_id: REPORTER_CONTRACT_ID,
        fn_name: "report",
    },
    GuestSpec {
        dir: "lua/transformer",
        bundle_name: "lua_transformer",
        contract_id: TRANSFORMER_CONTRACT_ID,
        fn_name: "transform",
    },
    GuestSpec {
        dir: "lua/reporter",
        bundle_name: "lua_reporter",
        contract_id: REPORTER_CONTRACT_ID,
        fn_name: "report",
    },
    GuestSpec {
        dir: "js_quickjs/transformer",
        bundle_name: "js_quickjs_transformer",
        contract_id: TRANSFORMER_CONTRACT_ID,
        fn_name: "transform",
    },
    GuestSpec {
        dir: "js_quickjs/reporter",
        bundle_name: "js_quickjs_reporter",
        contract_id: REPORTER_CONTRACT_ID,
        fn_name: "report",
    },
    GuestSpec {
        dir: "js_deno/transformer",
        bundle_name: "js_deno_transformer",
        contract_id: TRANSFORMER_CONTRACT_ID,
        fn_name: "transform",
    },
    GuestSpec {
        dir: "js_deno/reporter",
        bundle_name: "js_deno_reporter",
        contract_id: REPORTER_CONTRACT_ID,
        fn_name: "report",
    },
];

// ─── Find repo root ──────────────────────────────────────────────────────────
fn find_repo_root() -> PathBuf {
    let candidates: [PathBuf; 2] = [
        PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    ];

    for seed in &candidates {
        let mut dir: PathBuf = seed.clone();
        for _ in 0..8 {
            let examples_path: PathBuf = dir.join("examples").join("guests");
            if examples_path.is_dir() {
                return dir;
            }
            match dir.parent() {
                Some(p) => dir = p.to_path_buf(),
                None => break,
            }
        }
    }

    PathBuf::from(".")
}

// ─── Resolve a plugin by bundle name ─────────────────────────────────────────
fn resolve_plugin(runtime: &Runtime, guest: &GuestSpec) -> Result<PluginEntry, String> {
    let bid: u64 = bundle_id(guest.bundle_name);
    let handle: PluginHandle = runtime
        .find_by_bundle(bid, guest.contract_id, 0_u32)
        .map_err(|e| format!("find_by_bundle({}): {e}", guest.bundle_name))?;

    if handle.is_null() {
        return Err(format!(
            "plugin not found for bundle: {}",
            guest.bundle_name
        ));
    }

    let guard: PluginVTableGuard = runtime
        .registry()
        .resolve_guard(handle)
        .map_err(|e| format!("resolve_guard({}): {e}", guest.bundle_name))?;

    let vtable: *const PluginVTable = guard.vtable();
    if vtable.is_null() {
        return Err(format!("null vtable for bundle: {}", guest.bundle_name));
    }

    Ok(PluginEntry {
        label: guest.dir,
        _guard: guard,
        vtable,
    })
}

// ─── Call a vtable function ──────────────────────────────────────────────────
/// # Safety
/// `entry.vtable` must be non-null and valid. `args` and `out` must be valid.
unsafe fn call_fn(entry: &PluginEntry, args: *const (), out: *mut ()) -> Result<(), String> {
    // SAFETY: vtable is non-null and valid for the lifetime of _guard.
    let vt: &PluginVTable = unsafe { &*entry.vtable };

    if vt.function_count == 0_u32 || vt.functions.is_null() {
        return Err(format!("no functions in vtable for {}", entry.label));
    }

    // SAFETY: functions[0] is valid per vtable contract.
    let fn_ptr_raw: *const () = unsafe { *vt.functions.add(0_usize) };
    if fn_ptr_raw.is_null() {
        return Err(format!("null function pointer for {}", entry.label));
    }

    // SAFETY: fn_ptr_raw conforms to ABI signature.
    let func: AbiFn = unsafe { core::mem::transmute(fn_ptr_raw) };
    // SAFETY: args and out are valid pointers.
    let err: AbiError = unsafe { func(args, out) };

    if err.code != ABI_OK {
        let msg: &str = if err.message.ptr.is_null() || err.message.len == 0 {
            "unknown error"
        } else {
            // SAFETY: error message is valid UTF-8 for message.len bytes.
            unsafe {
                core::str::from_utf8(core::slice::from_raw_parts(
                    err.message.ptr,
                    err.message.len,
                ))
                .unwrap_or("(invalid utf-8)")
            }
        };
        return Err(format!(
            "{} failed: {} (code {})",
            entry.label, msg, err.code
        ));
    }

    Ok(())
}

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
    let repo_root: PathBuf = find_repo_root();

    // ─── Build runtime with all 6 loaders ────────────────────────────────────
    // Order: native → dotnet → python → lua → js → js_deno
    let runtime: Runtime = Runtime::builder()
        .loader(NativeLoader::new(NativeConfig {}))
        .loader(DotnetLoader::new(DotnetConfig::default()))
        .loader(PythonLoader::new(PythonConfig::default()))
        .loader(LuaLoader::new(LuaConfig::default()))
        .loader(JsLoader::new(JsConfig {}))
        .loader(JsDenoLoader::new(JsDenoConfig {}))
        .build()
        .map_err(|e| format!("runtime build failed: {e}"))?;

    // ─── Load all 14 guests ──────────────────────────────────────────────────
    for guest in &GUESTS {
        let path: PathBuf = repo_root.join("examples").join("guests").join(guest.dir);
        runtime
            .load_bundle(Path::new(&path))
            .map_err(|e| format!("failed to load {}: {e}", guest.dir))?;
    }

    // ─── Resolve and call each guest ─────────────────────────────────────────
    for guest in &GUESTS {
        let entry: PluginEntry = resolve_plugin(&runtime, guest)?;

        let input_str: &str = "hello";
        let input_sv: StringView = StringView {
            ptr: input_str.as_ptr(),
            len: input_str.len(),
        };
        let mut output_sv: StringView = StringView::null();

        // SAFETY: input_sv and output_sv are valid stack-allocated structs.
        unsafe {
            call_fn(
                &entry,
                core::ptr::addr_of!(input_sv).cast::<()>(),
                core::ptr::addr_of_mut!(output_sv).cast::<()>(),
            )?;
        }

        let result: &str = string_view_to_str(&output_sv);
        let padded_dir: String = format!("[{}]", guest.dir);
        println!(
            "{:<30} {}(\"{}\") = \"{}\"",
            padded_dir, guest.fn_name, input_str, result
        );
    }

    Ok(())
}

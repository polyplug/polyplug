use std::env;
use std::path::PathBuf;
use polyplug::loader::manifest::ManifestData;
use polyplug::loader::scanner;
use polyplug::runtime::Runtime;
use polyplug_native::{NativeConfig, NativeLoader};
use polyplug_abi::{StringView, PluginVTable, AbiError, polyplug_host_free};

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let plugin_path = env::var("POLYPLUG_PLUGIN_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("examples/plugins"));

    eprintln!("loading plugins from: {}", plugin_path.display());

    let runtime = Box::leak(Box::new(
        Runtime::builder()
            .loader(NativeLoader::new(NativeConfig {}))
            .build()
            .map_err(|e| e.to_string())?,
    ));

    let bundles: Vec<(PathBuf, ManifestData)> = scanner::scan_dir(&plugin_path);
    if bundles.is_empty() {
        return Err("no plugins found".into());
    }

    eprintln!("discovered {} bundles", bundles.len());

    for (path, manifest) in &bundles {
        runtime.load_bundle(path).map_err(|e| format!("load failed: {e}"))?;
        eprintln!("  loaded: {}", manifest.bundle_name);
    }

    println!("\n=== Pipeline Host (Rust) ===\n");

    let input = "name,value,42";
    println!("Input: \"{input}\"\n");

    for (_, manifest) in &bundles {
        let bid = polyplug_abi::bundle_id(&manifest.bundle_name);

        if manifest.provides.iter().any(|c| c.starts_with("pipeline.Decoder@1")) {
            let cid = polyplug_abi::contract_id("pipeline.Decoder", 1);
            let handle = runtime.find_by_bundle(bid, cid, 0)
                .map_err(|e| format!("find failed: {e}"))?;
            if !handle.is_null() {
                let guard = runtime.registry().resolve_guard(handle)
                    .map_err(|e| format!("resolve failed: {e}"))?;
                let vtable = guard.vtable();
                let result = call_string_fn(vtable, 0, input);
                println!("[{}] decode(\"{}\") = \"{}\"", manifest.bundle_name, input, result);
            }
        }

        if manifest.provides.iter().any(|c| c.starts_with("data.Transformer@1")) {
            let cid = polyplug_abi::contract_id("data.Transformer", 1);
            let handle = runtime.find_by_bundle(bid, cid, 0)
                .map_err(|e| format!("find failed: {e}"))?;
            if !handle.is_null() {
                let guard = runtime.registry().resolve_guard(handle)
                    .map_err(|e| format!("resolve failed: {e}"))?;
                let vtable = guard.vtable();
                let decoded = format!("DECODED:{}", input.replace(',', "|"));
                let result = call_string_fn(vtable, 0, &decoded);
                println!("[{}] transform(\"{}\") = \"{}\"", manifest.bundle_name, decoded, result);
            }
        }

        if manifest.provides.iter().any(|c| c.starts_with("pipeline.Encoder@1")) {
            let cid = polyplug_abi::contract_id("pipeline.Encoder", 1);
            let handle = runtime.find_by_bundle(bid, cid, 0)
                .map_err(|e| format!("find failed: {e}"))?;
            if !handle.is_null() {
                let guard = runtime.registry().resolve_guard(handle)
                    .map_err(|e| format!("resolve failed: {e}"))?;
                let vtable = guard.vtable();
                let transformed = "TRANSFORMED:NAME|value (transformed)|43";
                let result = call_string_fn(vtable, 0, transformed);
                println!("[{}] encode(\"{}\") = \"{}\"", manifest.bundle_name, transformed, result);
            }
        }

        if manifest.provides.iter().any(|c| c.starts_with("data.Reporter@1")) {
            let cid = polyplug_abi::contract_id("data.Reporter", 1);
            let handle = runtime.find_by_bundle(bid, cid, 0)
                .map_err(|e| format!("find failed: {e}"))?;
            if !handle.is_null() {
                let guard = runtime.registry().resolve_guard(handle)
                    .map_err(|e| format!("resolve failed: {e}"))?;
                let vtable = guard.vtable();
                let transformed = "TRANSFORMED:NAME|value (transformed)|43";
                let result = call_string_fn(vtable, 0, transformed);
                println!("[{}] report(\"{}\") = \"{}\"", manifest.bundle_name, transformed, result);
            }
        }

        if manifest.provides.iter().any(|c| c.starts_with("pipeline.Validator@1")) {
            let cid = polyplug_abi::contract_id("pipeline.Validator", 1);
            let handle = runtime.find_by_bundle(bid, cid, 0)
                .map_err(|e| format!("find failed: {e}"))?;
            if !handle.is_null() {
                let guard = runtime.registry().resolve_guard(handle)
                    .map_err(|e| format!("resolve failed: {e}"))?;
                let vtable = guard.vtable();
                let decoded = format!("DECODED:{}", input.replace(',', "|"));
                let result = call_string_fn(vtable, 0, &decoded);
                println!("[{}] validate(\"{}\") = \"{}\"", manifest.bundle_name, decoded, result);
            }
        }
    }

    println!("\ndone.");
    Ok(())
}

fn call_string_fn(vtable: *const PluginVTable, func_idx: usize, input: &str) -> String {
    let vtable = unsafe { &*vtable };
    let funcs: &[*const ()] = unsafe { std::slice::from_raw_parts(vtable.functions.cast(), vtable.function_count as usize) };
    let func_ptr = funcs[func_idx];
    let func: extern "C" fn(*const (), *mut ()) -> AbiError = unsafe { std::mem::transmute(func_ptr) };

    let input_sv = StringView { ptr: input.as_ptr(), len: input.len() };
    let mut output_sv = StringView { ptr: std::ptr::null(), len: 0 };

    let err = func(&input_sv as *const _ as *const (), &mut output_sv as *mut _ as *mut ());
    
    if err.code == 0 && !output_sv.ptr.is_null() && output_sv.len > 0 {
        let result = unsafe {
            std::str::from_utf8(std::slice::from_raw_parts(output_sv.ptr, output_sv.len))
                .unwrap_or("(invalid utf-8)")
                .to_string()
        };
        unsafe { polyplug_host_free(output_sv.ptr as *mut _, output_sv.len, 1) };
        result
    } else {
        format!("(error code={})", err.code)
    }
}

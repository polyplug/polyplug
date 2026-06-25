//! Benchmark for .NET dispatch overhead.
//!
//! Measures the performance characteristics of the .NET dispatch path:
//! 1. Real CLR dispatch through [UnmanagedCallersOnly] function pointers
//! 2. Native baseline for comparison
//!
//! This benchmark initializes the CLR and loads a real .NET assembly to measure
//! actual dispatch overhead through the CLR.

#![allow(clippy::expect_used)]

use core::ffi::c_void;
use core::hint::black_box;
use core::mem;
use core::ptr;
use criterion::{Criterion, criterion_group, criterion_main};
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::fs::ReadDir;
use std::io::Write as _;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::MutexGuard;

use netcorehost::hostfxr::Hostfxr;
use netcorehost::hostfxr::HostfxrContext;
use netcorehost::hostfxr::InitializedForRuntimeConfig;
use netcorehost::hostfxr::ManagedFunction;
use netcorehost::pdcstring::PdCString;
use tempfile::Builder as TempfileBuilder;
use tempfile::NamedTempFile;

type InitFn = unsafe extern "system" fn(*mut c_void, *const c_void, *const c_void) -> u32;

static CLR_INIT_FN: Mutex<Option<InitFn>> = Mutex::new(None);

fn find_hostfxr() -> Option<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();

    if let Some(val) = env::var_os("DOTNET_ROOT") {
        roots.push(PathBuf::from(val));
    }

    if let Some(path_val) = env::var_os("PATH") {
        for dir in env::split_paths(&path_val) {
            let candidate: PathBuf = dir.join("dotnet");
            if candidate.exists() {
                roots.push(dir);
            }
        }
    }

    roots.push(PathBuf::from("/usr/share/dotnet"));
    roots.push(PathBuf::from("/usr/lib/dotnet"));
    if let Some(home) = env::var_os("HOME") {
        roots.push(PathBuf::from(home).join(".dotnet"));
    }

    for root in &roots {
        if let Some(fxr_path) = highest_version_hostfxr(root) {
            return Some(fxr_path);
        }
    }

    None
}

fn highest_version_hostfxr(dotnet_root: &Path) -> Option<PathBuf> {
    let fxr_dir: PathBuf = dotnet_root.join("host").join("fxr");
    if !fxr_dir.is_dir() {
        return None;
    }

    let mut versions: Vec<(Vec<u64>, PathBuf)> = Vec::new();
    let entries: ReadDir = fs::read_dir(&fxr_dir).ok()?;
    for entry in entries.flatten() {
        let path: PathBuf = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name: String = path.file_name()?.to_string_lossy().into_owned();
        let parts: Vec<u64> = name
            .split('.')
            .map(|s| s.parse::<u64>().unwrap_or(0))
            .collect();
        if parts.is_empty() {
            continue;
        }
        versions.push((parts, path));
    }

    if versions.is_empty() {
        return None;
    }

    versions.sort_by(|a, b| b.0.cmp(&a.0));

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

fn init_clr() -> Option<InitFn> {
    let dll_path: PathBuf = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(|root| {
            root.join("tests")
                .join("fixtures")
                .join("csharp_plugin")
                .join("bin")
                .join("Debug")
                .join("net10.0")
                .join("CsharpPlugin.dll")
        })?;

    if !dll_path.exists() {
        return None;
    }

    let fxr_path: PathBuf = find_hostfxr()?;
    let hostfxr: Hostfxr = Hostfxr::load_from_path(&fxr_path).ok()?;

    let json: String = r#"{"runtimeOptions":{"tfm":"net10.0","framework":{"name":"Microsoft.NETCore.App","version":"10.0.0"}}}"#.to_owned();
    let mut tmp: NamedTempFile = TempfileBuilder::new().suffix(".json").tempfile().ok()?;
    tmp.write_all(json.as_bytes()).ok()?;
    tmp.flush().ok()?;
    let temp_path: PathBuf = tmp.path().to_path_buf();

    let pdcpath: PdCString = PdCString::from_os_str(temp_path.as_os_str()).ok()?;
    let context: HostfxrContext<InitializedForRuntimeConfig> =
        hostfxr.initialize_for_runtime_config(&pdcpath).ok()?;

    let asm_pdc: PdCString = PdCString::from_os_str(dll_path.as_os_str()).ok()?;
    let loader = context.get_delegate_loader_for_assembly(asm_pdc).ok()?;

    let type_name: PdCString =
        PdCString::from_os_str(OsStr::new("CsharpPlugin.Plugin, CsharpPlugin")).ok()?;
    let method_name: PdCString = PdCString::from_os_str(OsStr::new("PolyplugInit")).ok()?;

    let init_fn: ManagedFunction<InitFn> = loader
        .get_function_with_unmanaged_callers_only::<InitFn>(&type_name, &method_name)
        .ok()?;

    mem::forget(context);

    Some(*init_fn)
}

fn get_init_fn() -> Option<InitFn> {
    let mut guard: MutexGuard<'_, Option<InitFn>> = match CLR_INIT_FN.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    if guard.is_none() {
        *guard = init_clr();
    }
    *guard
}

fn bench_clr_dispatch(c: &mut Criterion) {
    let init_fn = match get_init_fn() {
        Some(f) => f,
        None => {
            eprintln!("CLR fixture not available, skipping CLR dispatch benchmark");
            return;
        }
    };

    let mut group = c.benchmark_group("clr_dispatch");

    group.bench_function("clr_init_call", |b| {
        b.iter(|| {
            // SAFETY: init_fn is the CsharpPlugin.Plugin::PolyplugInit entry point loaded
            // from the CLR via get_function_with_unmanaged_callers_only. Its three pointer
            // parameters are nullable by ABI contract; the managed side treats null as a
            // no-op and returns an AbiError code, so null pointers are sound here.
            let result: u32 = unsafe { init_fn(ptr::null_mut(), ptr::null(), ptr::null()) };
            black_box(result)
        })
    });

    group.bench_function("clr_init_10_calls", |b| {
        b.iter(|| {
            for _ in 0..10 {
                // SAFETY: init_fn is the CsharpPlugin.Plugin::PolyplugInit entry point loaded
                // from the CLR via get_function_with_unmanaged_callers_only. Its three pointer
                // parameters are nullable by ABI contract; the managed side treats null as a
                // no-op and returns an AbiError code, so null pointers are sound here.
                let result: u32 = unsafe { init_fn(ptr::null_mut(), ptr::null(), ptr::null()) };
                black_box(result);
            }
            black_box(())
        })
    });

    group.finish();
}

fn bench_native_baseline(c: &mut Criterion) {
    let mut group = c.benchmark_group("native_baseline");

    fn native_add(a: i32, b: i32) -> i32 {
        a + b
    }

    group.bench_function("native_function_call", |b| {
        b.iter(|| black_box(native_add(black_box(1), black_box(2))))
    });

    type NativeFn = extern "C" fn(i32, i32) -> i32;

    extern "C" fn native_add_extern(a: i32, b: i32) -> i32 {
        a + b
    }

    let func_ptr: NativeFn = native_add_extern;

    group.bench_function("native_function_pointer_call", |b| {
        b.iter(|| black_box(func_ptr(black_box(1), black_box(2))))
    });

    group.finish();
}

fn bench_dispatch_signature(c: &mut Criterion) {
    let mut group = c.benchmark_group("dispatch_signature");

    type InitFn = unsafe extern "system" fn(*mut c_void, *const c_void, *const c_void) -> u32;

    unsafe extern "system" fn noop_init(
        _rt_ctx: *mut c_void,
        _host_vtable: *const c_void,
        _ctx: *const c_void,
    ) -> u32 {
        0
    }

    let init_fn: InitFn = noop_init;

    group.bench_function("dispatch_with_null_pointers", |b| {
        b.iter(|| {
            // SAFETY: init_fn is the local noop_init, which ignores all three pointer
            // arguments and returns 0. Passing null pointers is therefore sound.
            let result: u32 = unsafe { init_fn(ptr::null_mut(), ptr::null(), ptr::null()) };
            black_box(result)
        })
    });

    group.bench_function("dispatch_with_stack_context", |b| {
        b.iter(|| {
            let mut rt_ctx: u64 = 0;
            let host_vtable: u64 = 0;
            let ctx: u64 = 0;
            // SAFETY: init_fn is the local noop_init, which ignores its arguments. The three
            // pointers reference live stack locals (rt_ctx, host_vtable, ctx) that outlive the
            // call, so they are valid and properly aligned for the duration of the dispatch.
            let result: u32 = unsafe {
                init_fn(
                    &mut rt_ctx as *mut u64 as *mut c_void,
                    &host_vtable as *const u64 as *const c_void,
                    &ctx as *const u64 as *const c_void,
                )
            };
            black_box(result)
        })
    });

    group.finish();
}

fn bench_computation_dispatch(c: &mut Criterion) {
    let mut group = c.benchmark_group("computation_dispatch");

    type ComputeFn = unsafe extern "system" fn(i64, i64) -> i64;

    unsafe extern "system" fn compute_sum(_args: i64, _out: i64) -> i64 {
        let mut sum: i64 = 0;
        for i in 0..100 {
            sum += i;
        }
        sum
    }

    let compute_fn: ComputeFn = compute_sum;

    group.bench_function("computation_100_iterations", |b| {
        b.iter(|| {
            // SAFETY: compute_fn is the local compute_sum, which only performs integer
            // arithmetic on its scalar arguments and dereferences no pointers. Calling it
            // with the value arguments (0, 0) is sound.
            let result: i64 = unsafe { compute_fn(0, 0) };
            black_box(result)
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_clr_dispatch,
    bench_native_baseline,
    bench_dispatch_signature,
    bench_computation_dispatch
);
criterion_main!(benches);

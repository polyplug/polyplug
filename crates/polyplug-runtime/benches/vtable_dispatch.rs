// THIS IS A BENCHMARK FILE — do not add #[test] functions here
// Run with: cargo bench -p polyplug-runtime --bench vtable_dispatch

#![allow(clippy::expect_used)]
use criterion::BenchmarkId;
use criterion::Criterion;
use criterion::Throughput;
use criterion::criterion_group;
use criterion::criterion_main;
use polyplug_runtime::abi::ABI_OK;
use polyplug_runtime::abi::AbiError;
use polyplug_runtime::abi::Buffer;
use polyplug_runtime::abi::HostVTable;
use polyplug_runtime::abi::PluginDescriptor;
use polyplug_runtime::abi::PluginHandle;
use polyplug_runtime::abi::PluginRegistrar;
use polyplug_runtime::abi::PluginVTable;
use polyplug_runtime::abi::StringView;
use polyplug_runtime::allocator::polyplug_host_alloc;
use polyplug_runtime::allocator::polyplug_host_free;
use polyplug_runtime::registry::Registry;
use std::cell::RefCell;
use std::hint::black_box;

// ─── Plugin paths from build.rs ──────────────────────────────────────────────

const TEST_PLUGIN_SO: &str = env!("TEST_PLUGIN_SO");
const MEMORY_PLUGIN_SO: &str = env!("MEMORY_PLUGIN_SO");
#[allow(dead_code)]
const ERROR_PLUGIN_SO: &str = env!("ERROR_PLUGIN_SO");

// ─── Shared argument structs ─────────────────────────────────────────────────

#[repr(C)]
struct AddArgs {
    a: u32,
    b: u32,
}

#[repr(C)]
struct FillArgs {
    buf: Buffer,
    fill_byte: u8,
}

// ─── Thread-local registry and captured vtable state ─────────────────────────

thread_local! {
    static BENCH_REGISTRY: RefCell<Registry> = RefCell::new(Registry::new());
    static LAST_VTABLE: std::cell::Cell<*const PluginVTable> = const { std::cell::Cell::new(core::ptr::null()) };
    static LAST_CONTRACT_ID: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Registration callback used by all benchmarks.
///
/// # Safety
/// `registrar`, `descriptor`, and `vtable` must be valid pointers for the call duration.
unsafe extern "C" fn bench_register_callback(
    _registrar: *mut PluginRegistrar,
    descriptor: *const PluginDescriptor,
    vtable: *const PluginVTable,
) -> AbiError {
    if descriptor.is_null() || vtable.is_null() {
        return AbiError {
            code: 1,
            message: StringView::null(),
        };
    }

    // SAFETY: descriptor and vtable are valid for this call per ABI contract.
    let desc: &PluginDescriptor = unsafe { &*descriptor };
    let vt: &PluginVTable = unsafe { &*vtable };

    // SAFETY: desc.contract_name.ptr is valid UTF-8 for desc.contract_name.len bytes.
    let contract_name: &str = unsafe {
        let bytes: &[u8] =
            core::slice::from_raw_parts(desc.contract_name.ptr, desc.contract_name.len);
        core::str::from_utf8_unchecked(bytes)
    };

    let result: Result<PluginHandle, _> = BENCH_REGISTRY.with(|cell| {
        cell.borrow().register(
            *desc,
            vtable as *const PluginVTable,
            contract_name.to_owned(),
            vt.contract_id,
        )
    });

    match result {
        Ok(_) => {
            // Capture the vtable pointer and contract_id for easy retrieval.
            LAST_VTABLE.with(|cell| cell.set(vtable));
            LAST_CONTRACT_ID.with(|cell| cell.set(vt.contract_id));
            AbiError {
                code: ABI_OK,
                message: StringView::null(),
            }
        }
        Err(_) => AbiError {
            code: 1,
            message: StringView::null(),
        },
    }
}

// ─── Stub HostVTable functions for cross-plugin dispatch ──────────────────────

/// Finds a plugin by contract_id in the thread-local BENCH_REGISTRY.
///
/// # Safety
/// Must only be called from a bench thread where BENCH_REGISTRY is initialised.
unsafe extern "C" fn bench_find_plugin(contract_id: u64, min_version: u32) -> PluginHandle {
    BENCH_REGISTRY.with(|cell| {
        cell.borrow()
            .find(contract_id, min_version)
            .unwrap_or_else(|_| PluginHandle::null())
    })
}

/// Dispatches a call through the thread-local BENCH_REGISTRY.
///
/// # Safety
/// `args` and `out` must match the signature expected by `fn_id` in the plugin's vtable.
unsafe extern "C" fn bench_call_plugin(
    plugin: PluginHandle,
    fn_id: u32,
    args: *const (),
    out: *mut (),
) -> AbiError {
    BENCH_REGISTRY.with(|cell| {
        let registry: std::cell::Ref<'_, Registry> = cell.borrow();
        let vtable_ptr: *const PluginVTable = match registry.resolve(plugin) {
            Ok(p) => p,
            Err(_) => {
                return AbiError {
                    code: 4,
                    message: StringView::null(),
                }
            }
        };
        // SAFETY: vtable_ptr is valid and 'static — the library is never dropped (mem::forget).
        let vtable: &PluginVTable = unsafe { &*vtable_ptr };
        if fn_id >= vtable.function_count {
            return AbiError {
                code: 6,
                message: StringView::null(),
            };
        }
        // SAFETY: vtable.functions is a static array of at least function_count entries.
        let fn_ptr: *const () = unsafe { *vtable.functions.add(fn_id as usize) };
        // SAFETY: fn_ptr is a valid function pointer for the given fn_id.
        // The caller guarantees args and out match the function's expected types.
        let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
            unsafe { core::mem::transmute(fn_ptr) };
        // SAFETY: args and out are forwarded as-is; caller is responsible for type correctness.
        unsafe { dispatch_fn(args, out) }
    })
}

/// Returns a null extension pointer (extensions not used in benchmarks).
///
/// # Safety
/// Always safe to call; always returns null.
unsafe extern "C" fn bench_get_extension(_: u32) -> *const () {
    core::ptr::null()
}

// ─── Setup helpers ────────────────────────────────────────────────────────────

/// Load a plugin cdylib and call `polyplug_init`, registering into BENCH_REGISTRY.
/// After this call, LAST_VTABLE holds the vtable pointer of the plugin just loaded.
fn load_and_init_plugin(path: &str) -> libloading::Library {
    // SAFETY: path is a valid compiled cdylib built by build.rs.
    let library: libloading::Library =
        unsafe { libloading::Library::new(path).expect("failed to load plugin") };

    // SAFETY: polyplug_init matches the expected ABI (unsafe extern "C" fn(*mut PluginRegistrar) -> AbiError).
    let init_fn: libloading::Symbol<'_, unsafe extern "C" fn(*mut PluginRegistrar) -> AbiError> = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init not found")
    };

    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: bench_register_callback,
        host: core::ptr::null(),
    };

    // SAFETY: init_fn is a valid function; registrar lives for the call duration.
    let result: AbiError = unsafe { init_fn(&mut registrar as *mut PluginRegistrar) };

    assert_eq!(result.code, ABI_OK, "polyplug_init failed for {}", path);
    library
}

/// Retrieve the dispatch function for `fn_id` from the last registered vtable (LAST_VTABLE).
fn get_vtable_fn(fn_id: usize) -> unsafe extern "C" fn(*const (), *mut ()) -> AbiError {
    let vtable_ptr: *const PluginVTable = LAST_VTABLE.with(|cell| cell.get());
    assert!(
        !vtable_ptr.is_null(),
        "vtable not captured — was load_and_init_plugin called?"
    );
    // SAFETY: vtable_ptr was captured from the polyplug_init callback.
    // The library is kept alive via mem::forget in each benchmark function.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    assert!(
        fn_id < vtable.function_count as usize,
        "fn_id {} out of range (function_count={})",
        fn_id,
        vtable.function_count
    );
    // SAFETY: vtable.functions is a static array; fn_id is within bounds (checked above).
    let fn_ptr: *const () = unsafe { *vtable.functions.add(fn_id) };
    // SAFETY: transmuting to the generic dispatch signature.
    // Arg/out types are enforced by each benchmark's setup code.
    unsafe { core::mem::transmute(fn_ptr) }
}

// ─── Benchmark 1 — noop dispatch ─────────────────────────────────────────────

/// Measures the cost of a single vtable function call with trivial (zero) args.
/// Isolates the raw dispatch overhead with no meaningful computation.
fn bench_dispatch_noop(c: &mut Criterion) {
    // Reset registry for a clean slate.
    BENCH_REGISTRY.with(|cell| {
        *cell.borrow_mut() = Registry::new();
    });

    let _library: libloading::Library = load_and_init_plugin(TEST_PLUGIN_SO);
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError = get_vtable_fn(0);

    let mut group: criterion::BenchmarkGroup<'_, criterion::measurement::WallTime> =
        c.benchmark_group("dispatch");
    group.throughput(Throughput::Elements(1));

    let args: AddArgs = AddArgs { a: 0, b: 0 };
    let mut out: u32 = 0_u32;

    group.bench_function(BenchmarkId::new("noop", "add(0,0)"), |b| {
        b.iter(|| {
            // SAFETY: args points to AddArgs, out points to u32.
            // test_plugin fn 0 (add) has signature: (a: u32, b: u32) -> u32.
            let result: AbiError = unsafe {
                dispatch_fn(
                    black_box(&args as *const AddArgs as *const ()),
                    black_box(&mut out as *mut u32 as *mut ()),
                )
            };
            black_box(result);
        });
    });

    group.finish();
    // Keep library alive for the duration; never drop (never-drop invariant).
    core::mem::forget(_library);
}

// ─── Benchmark 2 — buffer arg dispatch ───────────────────────────────────────

/// Measures vtable dispatch with a Buffer argument (pre-allocated 4096-byte buffer).
/// The buffer is allocated ONCE before the loop — only dispatch overhead is measured.
fn bench_dispatch_buffer_arg(c: &mut Criterion) {
    // Reset registry for a clean slate.
    BENCH_REGISTRY.with(|cell| {
        *cell.borrow_mut() = Registry::new();
    });

    let _library: libloading::Library = load_and_init_plugin(MEMORY_PLUGIN_SO);
    // fn 0 = memory_fill_preallocated_buffer(args: FillArgs) -> u32
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError = get_vtable_fn(0);

    // Allocate 4096 bytes ONCE outside the benchmark loop.
    let buf_ptr: *mut u8 = polyplug_host_alloc(4096, 1);
    assert!(!buf_ptr.is_null(), "bench alloc failed");

    let mut group: criterion::BenchmarkGroup<'_, criterion::measurement::WallTime> =
        c.benchmark_group("dispatch");
    group.throughput(Throughput::Elements(1));

    let args: FillArgs = FillArgs {
        buf: Buffer {
            ptr: buf_ptr,
            len: 0,
            cap: 4096,
        },
        fill_byte: 0xBB_u8,
    };
    let mut out: u32 = 0_u32;

    group.bench_function(BenchmarkId::new("buffer_arg", "fill_4096"), |b| {
        b.iter(|| {
            // SAFETY: args points to FillArgs; out points to u32.
            // memory_plugin fn 0 (fill_preallocated_buffer): writes to buf.ptr[0..cap].
            // buf_ptr is a valid 4096-byte allocation from polyplug_host_alloc.
            let result: AbiError = unsafe {
                dispatch_fn(
                    black_box(&args as *const FillArgs as *const ()),
                    black_box(&mut out as *mut u32 as *mut ()),
                )
            };
            black_box(result);
        });
    });

    group.finish();

    // SAFETY: buf_ptr was allocated with polyplug_host_alloc(4096, 1).
    // Freeing here is safe — the benchmark loop is complete.
    unsafe { polyplug_host_free(buf_ptr, 4096, 1) };

    core::mem::forget(_library);
}

// ─── Benchmark 3 — struct arg and return ─────────────────────────────────────

/// Measures vtable dispatch with non-trivial AddArgs (a=42, b=57) and a u32 result.
/// Same dispatch path as bench 1 but with meaningful input values to prevent
/// dead-code elimination of the computation inside the plugin.
fn bench_dispatch_struct_arg_and_return(c: &mut Criterion) {
    // Reset registry for a clean slate.
    BENCH_REGISTRY.with(|cell| {
        *cell.borrow_mut() = Registry::new();
    });

    let _library: libloading::Library = load_and_init_plugin(TEST_PLUGIN_SO);
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError = get_vtable_fn(0);

    let mut group: criterion::BenchmarkGroup<'_, criterion::measurement::WallTime> =
        c.benchmark_group("dispatch");
    group.throughput(Throughput::Elements(1));

    let args: AddArgs = AddArgs {
        a: 42_u32,
        b: 57_u32,
    };
    let mut out: u32 = 0_u32;

    group.bench_function(
        BenchmarkId::new("struct_arg_and_return", "add(42,57)"),
        |b| {
            b.iter(|| {
                // SAFETY: args points to AddArgs, out points to u32.
                // test_plugin fn 0 (add) expects (a: u32, b: u32) -> u32.
                let result: AbiError = unsafe {
                    dispatch_fn(
                        black_box(&args as *const AddArgs as *const ()),
                        black_box(&mut out as *mut u32 as *mut ()),
                    )
                };
                black_box(out);
                black_box(result);
            });
        },
    );

    group.finish();
    core::mem::forget(_library);
}

// ─── Benchmark 4 — cross-plugin dispatch ─────────────────────────────────────

/// Measures the full cross-plugin dispatch path:
///   find_plugin (Registry lookup) + call_plugin (vtable dispatch).
/// Uses memory_plugin fn 2 (echo_string_view) as the target — no allocation.
fn bench_dispatch_cross_plugin(c: &mut Criterion) {
    // Reset registry for a clean slate.
    BENCH_REGISTRY.with(|cell| {
        *cell.borrow_mut() = Registry::new();
    });

    // Load memory_plugin into BENCH_REGISTRY so find_plugin can locate it.
    let _memory_lib: libloading::Library = load_and_init_plugin(MEMORY_PLUGIN_SO);

    // Capture the memory.test contract_id (set by bench_register_callback above).
    let memory_contract_id: u64 = LAST_CONTRACT_ID.with(|cell| cell.get());
    assert_ne!(
        memory_contract_id, 0,
        "memory_plugin contract_id was not captured"
    );

    // Build a HostVTable backed by the thread-local BENCH_REGISTRY.
    let host_vtable: HostVTable = HostVTable {
        alloc: polyplug_host_alloc,
        // polyplug_host_free has the same signature as HostVTable.free.
        free: polyplug_host_free,
        find_plugin: bench_find_plugin,
        call_plugin: bench_call_plugin,
        get_extension: bench_get_extension,
    };

    // Input StringView pointing to a static byte string — no allocation needed.
    let sv: StringView = StringView {
        ptr: b"hello".as_ptr(),
        len: 5,
    };
    let mut sv_out: StringView = StringView::null();

    let mut group: criterion::BenchmarkGroup<'_, criterion::measurement::WallTime> =
        c.benchmark_group("dispatch");
    group.throughput(Throughput::Elements(1));

    group.bench_function(BenchmarkId::new("cross_plugin", "find+call"), |b| {
        b.iter(|| {
            // Simulate the full cross-plugin dispatch: find_plugin + call_plugin.
            // This measures the complete path a plugin takes when calling into another plugin.

            // SAFETY: bench_find_plugin is a valid extern C fn backed by BENCH_REGISTRY.
            let handle: PluginHandle =
                unsafe { black_box((host_vtable.find_plugin)(memory_contract_id, 0)) };

            // SAFETY: bench_call_plugin dispatches through BENCH_REGISTRY.
            // fn 2 = memory_echo_string_view(args: *const StringView, out: *mut StringView).
            // sv is a valid StringView; sv_out is a valid StringView location.
            let result: AbiError = unsafe {
                black_box((host_vtable.call_plugin)(
                    handle,
                    2_u32,
                    black_box(&sv as *const StringView as *const ()),
                    black_box(&mut sv_out as *mut StringView as *mut ()),
                ))
            };
            black_box(result);
            black_box(sv_out);
        });
    });

    group.finish();
    core::mem::forget(_memory_lib);
}

// ─── criterion_group / criterion_main ────────────────────────────────────────

criterion_group!(
    benches,
    bench_dispatch_noop,
    bench_dispatch_buffer_arg,
    bench_dispatch_struct_arg_and_return,
    bench_dispatch_cross_plugin,
);
criterion_main!(benches);

#![allow(clippy::expect_used)]

// THIS IS A BENCHMARK FILE — do not add #[test] functions here
// Run with: cargo bench -p polyplug --bench contract_dispatch

use core::cell::RefCell;
use core::hint::black_box;

use criterion::BenchmarkId;
use criterion::Criterion;
use criterion::Throughput;
use criterion::criterion_group;
use criterion::criterion_main;

use polyplug::runtime_store::RuntimeStore;
use polyplug_abi::AbiError;
use polyplug_abi::AbiErrorCode;
use polyplug_abi::Array;
use polyplug_abi::Buffer;
use polyplug_abi::DispatchType;
use polyplug_abi::GuestContractHandle;
use polyplug_abi::GuestContractInterface;
use polyplug_abi::HostApi;
use polyplug_abi::PluginDescriptor;
use polyplug_abi::StringView;
use polyplug_abi::ffi::polyplug_host_alloc;
use polyplug_abi::ffi::polyplug_host_free;
use polyplug_utils::BundleId;

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

// ─── Thread-local registry and captured interface state ────────────────────────

thread_local! {
    static BENCH_REGISTRY: RefCell<Option<RuntimeStore>> = RefCell::new(Some(RuntimeStore::new()));
    static LAST_INTERFACE: core::cell::Cell<*const GuestContractInterface> = const { core::cell::Cell::new(core::ptr::null()) };
    static LAST_CONTRACT_ID: core::cell::Cell<u64> = const { core::cell::Cell::new(0) };
}

/// Registration callback used by all benchmarks.
///
/// # Safety
/// `descriptor` and `interface` must be valid pointers for the call duration.
unsafe extern "C" fn bench_register_callback(
    _this: *const HostApi,
    descriptor: *const PluginDescriptor,
    interface: *const GuestContractInterface,
) -> AbiError {
    if descriptor.is_null() || interface.is_null() {
        return AbiError {
            code: AbiErrorCode::Generic as u32,
            message: StringView::null(),
        };
    }

    // SAFETY: descriptor and interface are valid for this call per ABI contract.
    let desc: &PluginDescriptor = unsafe { &*descriptor };
    // SAFETY: interface is valid for this call per ABI contract.
    let iface: &GuestContractInterface = unsafe { &*interface };

    // SAFETY: desc.contract_name is set from a &'static str in the benchmark fixture.
    // The bytes are valid UTF-8 by construction.
    let contract_name: &str = unsafe {
        let bytes: &[u8] =
            core::slice::from_raw_parts(desc.contract_name.ptr, desc.contract_name.len);
        core::str::from_utf8_unchecked(bytes) // SAFETY: see comment above
    };

    let result: Result<GuestContractHandle, _> =
        BENCH_REGISTRY.with(|cell: &core::cell::RefCell<Option<RuntimeStore>>| {
            // SAFETY: interface pointer is 'static — extracted from a loaded library that outlives registry.
            let borrowed = cell.borrow();
            let registry = borrowed.as_ref().expect("registry not initialized");
            // SAFETY: `interface` is a 'static pointer captured from a loaded plugin library
            // that outlives the registry, and `desc` is valid for the duration of this call
            // per the register_guest_contract ABI contract, so register_guest_contract's
            // preconditions are met.
            unsafe {
                registry.register_guest_contract(
                    *desc,
                    interface,
                    contract_name.to_owned(),
                    BundleId::from_u64(iface.contract_id.id()),
                )
            }
        });

    match result {
        Ok(_) => {
            // Capture the interface pointer and contract_id for easy retrieval.
            LAST_INTERFACE.with(|cell| cell.set(interface));
            LAST_CONTRACT_ID.with(|cell| cell.set(iface.contract_id.id()));
            AbiError::ok()
        }
        Err(_) => AbiError {
            code: AbiErrorCode::Generic as u32,
            message: StringView::null(),
        },
    }
}

// ─── Stub HostApi functions for cross-plugin dispatch ──────────────────────

/// Finds a plugin by contract_id in the thread-local BENCH_REGISTRY.
///
/// # Safety
/// Must only be called from a bench thread where BENCH_REGISTRY is initialised.
unsafe extern "C" fn bench_find_guest_contract(
    _this: *const HostApi,
    contract_id: u64,
    min_version: u32,
) -> GuestContractHandle {
    BENCH_REGISTRY.with(|cell: &core::cell::RefCell<Option<RuntimeStore>>| {
        let registry = cell.borrow();
        let reg = registry.as_ref().expect("registry not initialized");
        reg.find(
            polyplug_utils::GuestContractId::from_u64(contract_id),
            min_version,
        )
        .unwrap_or_else(|_| GuestContractHandle::null())
    })
}

/// find_all_by_contract stub — returns empty array (not used in benches).
///
/// # Safety
/// Always safe to call; returns empty array.
unsafe extern "C" fn bench_find_all_guest_contracts(
    _this: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> Array<GuestContractHandle> {
    Array::empty()
}

/// Resolves a plugin handle to an interface pointer via the thread-local BENCH_REGISTRY.
///
/// # Safety
/// The returned pointer is valid and 'static — the library is kept alive via mem::forget.
unsafe extern "C" fn bench_resolve_guest_contract(
    _this: *const HostApi,
    handle: GuestContractHandle,
) -> *const GuestContractInterface {
    BENCH_REGISTRY.with(|cell: &core::cell::RefCell<Option<RuntimeStore>>| {
        cell.borrow()
            .as_ref()
            .expect("registry not initialized")
            .resolve_guest_contract(handle)
            .unwrap_or(core::ptr::null())
    })
}

/// Returns a null host contract instance.
unsafe extern "C" fn bench_get_host_contract(
    _this: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> polyplug_abi::HostContractInstance {
    polyplug_abi::HostContractInstance::null()
}

/// Returns null pointer for host contract interface.
unsafe extern "C" fn bench_resolve_host_contract_interface(
    _this: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> *const polyplug_abi::HostContractInterface {
    core::ptr::null()
}

/// Returns empty array of bundle IDs.
unsafe extern "C" fn bench_list_bundles(_this: *const HostApi) -> Array<BundleId> {
    Array::empty()
}

/// Returns empty array of dependencies.
unsafe extern "C" fn bench_get_dependencies(
    _this: *const HostApi,
) -> Array<polyplug_abi::DependencyInfo> {
    Array::empty()
}

/// load_bundle stub — returns error (not used in benches).
unsafe extern "C" fn bench_load_bundle(
    _this: *const HostApi,
    _path: *const u8,
    _path_len: usize,
) -> AbiError {
    AbiError {
        code: AbiErrorCode::Generic as u32,
        message: StringView::null(),
    }
}

/// reload_bundle stub — returns error (not used in benches).
unsafe extern "C" fn bench_reload_bundle(
    _this: *const HostApi,
    _path: *const u8,
    _path_len: usize,
) -> AbiError {
    AbiError {
        code: AbiErrorCode::Generic as u32,
        message: StringView::null(),
    }
}

/// register_host_contract stub — returns error (not used in benches).
unsafe extern "C" fn bench_register_host_contract(
    _this: *const HostApi,
    _interface: *const polyplug_abi::HostContractInterface,
) -> AbiError {
    AbiError {
        code: AbiErrorCode::Generic as u32,
        message: StringView::null(),
    }
}

/// register_loader stub — returns error (not used in benches).
unsafe extern "C" fn bench_register_loader(
    _this: *const HostApi,
    _runtime_name: StringView,
    _loader_ptr: *mut core::ffi::c_void,
) -> AbiError {
    AbiError {
        code: AbiErrorCode::Generic as u32,
        message: StringView::null(),
    }
}

/// get_last_error stub — returns 0 (not used in benches).
unsafe extern "C" fn bench_get_last_error(
    _this: *const HostApi,
    _buf: *mut u8,
    _buf_len: usize,
) -> usize {
    0
}

/// get_error_len stub — returns 0 (not used in benches).
unsafe extern "C" fn bench_get_error_len(_this: *const HostApi) -> usize {
    0
}

/// call_guest_method stub — returns error (not used in benches).
unsafe extern "C" fn bench_call_guest_method(
    _this: *const HostApi,
    _instance: polyplug_abi::GuestContractInstance,
    _fn_id: u32,
    _args: *const core::ffi::c_void,
    _out: *mut core::ffi::c_void,
    _arena: *mut polyplug_abi::CallArena,
) -> AbiError {
    AbiError {
        code: AbiErrorCode::Generic as u32,
        message: StringView::null(),
    }
}

/// unload_bundle stub — returns ok (not used in benches).
unsafe extern "C" fn bench_unload_bundle(_this: *const HostApi, _bundle_id: BundleId) -> AbiError {
    AbiError::ok()
}

/// Alloc wrapper that ignores this (uses global allocator).
///
/// # Safety
/// Delegates to polyplug_host_alloc which is safe for any size/align.
unsafe extern "C" fn bench_alloc(_this: *const HostApi, size: usize, align: usize) -> *mut u8 {
    polyplug_host_alloc(size, align)
}

/// Free wrapper that ignores this (uses global allocator).
///
/// # Safety
/// Delegates to polyplug_host_free which requires ptr was allocated by polyplug_host_alloc.
unsafe extern "C" fn bench_free(_this: *const HostApi, ptr: *mut u8, size: usize, align: usize) {
    // SAFETY: ptr was allocated by polyplug_host_alloc (caller's responsibility).
    unsafe { polyplug_host_free(ptr, size, align) };
}

// ─── Setup helpers ────────────────────────────────────────────────────────────

/// Build a `HostApi` whose function pointers are all backed by the thread-local
/// `BENCH_REGISTRY` stubs above. Every benchmark that needs to hand a plugin a
/// host table (to register, resolve, or allocate) uses this single definition.
fn bench_host_api() -> HostApi {
    HostApi {
        runtime: core::ptr::null_mut(),
        register_guest_contract: bench_register_callback,
        alloc: bench_alloc,
        free: bench_free,
        find_guest_contract: bench_find_guest_contract,
        find_all_guest_contracts: bench_find_all_guest_contracts,
        resolve_guest_contract: bench_resolve_guest_contract,
        get_host_contract: bench_get_host_contract,
        resolve_host_contract_interface: bench_resolve_host_contract_interface,
        list_bundles: bench_list_bundles,
        get_dependencies: bench_get_dependencies,
        load_bundle: bench_load_bundle,
        reload_bundle: bench_reload_bundle,
        register_host_contract: bench_register_host_contract,
        register_loader: bench_register_loader,
        get_last_error: bench_get_last_error,
        get_error_len: bench_get_error_len,
        call_guest_method: bench_call_guest_method,
        reserved: core::ptr::null(),
        unload_bundle: bench_unload_bundle,
    }
}

/// Load a plugin cdylib and call `polyplug_init`, registering into BENCH_REGISTRY.
/// After this call, LAST_INTERFACE holds the interface pointer of the plugin just loaded.
fn load_and_init_plugin(path: &str) -> libloading::Library {
    // SAFETY: path is a valid compiled cdylib built by build.rs.
    let library: libloading::Library =
        unsafe { libloading::Library::new(path).expect("failed to load plugin") };

    // SAFETY: polyplug_init matches the expected 2-arg ABI:
    // unsafe extern "C" fn(host: *const HostApi, ctx: *const BundleInitContext) -> AbiError
    let init_fn: libloading::Symbol<
        '_,
        unsafe extern "C" fn(*const HostApi, *const polyplug_abi::BundleInitContext) -> AbiError,
    > = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init not found")
    };

    let host_interface: HostApi = bench_host_api();

    let plugin_ctx: polyplug_abi::BundleInitContext = polyplug_abi::BundleInitContext {
        bundle_path: StringView::null(),
        bundle_id: 0,
    };

    // SAFETY: init_fn is a valid function; host_interface and plugin_ctx live for the call duration.
    let result: AbiError = unsafe {
        init_fn(
            &host_interface as *const HostApi,
            &plugin_ctx as *const polyplug_abi::BundleInitContext,
        )
    };

    assert!(result.is_ok(), "polyplug_init failed for {}", path);
    library
}

/// Retrieve the dispatch function for `fn_id` from the last registered interface (LAST_INTERFACE).
fn get_interface_fn(fn_id: usize) -> unsafe extern "C" fn(*const (), *mut ()) -> AbiError {
    let interface_ptr: *const GuestContractInterface = LAST_INTERFACE.with(|cell| cell.get());
    assert!(
        !interface_ptr.is_null(),
        "interface not captured — was load_and_init_plugin called?"
    );
    // SAFETY: interface_ptr was captured from the polyplug_init callback.
    // The library is kept alive via mem::forget in each benchmark function.
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };
    assert!(
        interface.dispatch_type == DispatchType::Native,
        "expected Native dispatch type, got {:?}",
        interface.dispatch_type
    );
    // SAFETY: dispatch.native.functions is a static array; fn_id is within bounds.
    // We verified dispatch_type == Native above, so accessing .native is safe.
    let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(fn_id) };
    // SAFETY: transmuting to the generic dispatch signature.
    // Arg/out types are enforced by each benchmark's setup code.
    unsafe { core::mem::transmute(fn_ptr) }
}

// ─── Benchmark 1 — noop dispatch ─────────────────────────────────────────────

/// Measures the cost of a single contract interface function call with trivial (zero) args.
/// Isolates the raw dispatch overhead with no meaningful computation.
fn bench_dispatch_noop(c: &mut Criterion) {
    // Reset registry for a clean slate.
    BENCH_REGISTRY.with(|cell: &core::cell::RefCell<Option<RuntimeStore>>| {
        *cell.borrow_mut() = Some(RuntimeStore::new());
    });

    let _library: libloading::Library = load_and_init_plugin(TEST_PLUGIN_SO);
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError = get_interface_fn(0);

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

/// Measures contract interface dispatch with a Buffer argument (pre-allocated 4096-byte buffer).
/// The buffer is allocated ONCE before the loop — only dispatch overhead is measured.
fn bench_dispatch_buffer_arg(c: &mut Criterion) {
    // Reset registry for a clean slate.
    BENCH_REGISTRY.with(|cell: &core::cell::RefCell<Option<RuntimeStore>>| {
        *cell.borrow_mut() = Some(RuntimeStore::new());
    });

    let _library: libloading::Library = load_and_init_plugin(MEMORY_PLUGIN_SO);
    // fn 0 = memory_fill_preallocated_buffer(args: FillArgs) -> u32
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError = get_interface_fn(0);

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

/// Measures contract interface dispatch with non-trivial AddArgs (a=42, b=57) and a u32 result.
/// Same dispatch path as bench 1 but with meaningful input values to prevent
/// dead-code elimination of the computation inside the plugin.
fn bench_dispatch_struct_arg_and_return(c: &mut Criterion) {
    // Reset registry for a clean slate.
    BENCH_REGISTRY.with(|cell: &core::cell::RefCell<Option<RuntimeStore>>| {
        *cell.borrow_mut() = Some(RuntimeStore::new());
    });

    let _library: libloading::Library = load_and_init_plugin(TEST_PLUGIN_SO);
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError = get_interface_fn(0);

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
///   find_guest_contract (Registry lookup) + resolve_guest_contract (interface pointer) + direct dispatch.
/// Uses memory_plugin fn 2 (echo_string_view) as the target — no allocation.
fn bench_dispatch_cross_plugin(c: &mut Criterion) {
    // Reset registry for a clean slate.
    BENCH_REGISTRY.with(|cell: &core::cell::RefCell<Option<RuntimeStore>>| {
        *cell.borrow_mut() = Some(RuntimeStore::new());
    });

    // Load memory_plugin into BENCH_REGISTRY so find_guest_contract can locate it.
    let _memory_lib: libloading::Library = load_and_init_plugin(MEMORY_PLUGIN_SO);

    // Capture the memory.test contract_id (set by bench_register_callback above).
    let memory_contract_id: u64 = LAST_CONTRACT_ID.with(|cell| cell.get());
    assert_ne!(
        memory_contract_id, 0,
        "memory_plugin contract_id was not captured"
    );

    // Build a HostApi backed by the thread-local BENCH_REGISTRY.
    let host_interface: HostApi = bench_host_api();

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
            // SAFETY: bench_find_guest_contract is a valid extern C fn backed by BENCH_REGISTRY.
            let handle: GuestContractHandle = unsafe {
                black_box((host_interface.find_guest_contract)(
                    &host_interface as *const HostApi,
                    memory_contract_id,
                    0,
                ))
            };

            // SAFETY: bench_resolve_guest_contract returns a 'static GuestContractInterface pointer.
            let interface_ptr: *const GuestContractInterface = unsafe {
                black_box((host_interface.resolve_guest_contract)(
                    &host_interface as *const HostApi,
                    handle,
                ))
            };

            // SAFETY: interface_ptr is non-null (plugin is registered), fn 2 is in range.
            // fn 2 = memory_echo_string_view(args: *const StringView, out: *mut StringView).
            // sv is a valid StringView; sv_out is a valid StringView location.
            let result: AbiError = if interface_ptr.is_null() {
                AbiError {
                    code: AbiErrorCode::NotFound as u32,
                    message: StringView::null(),
                }
            } else {
                // SAFETY: interface_ptr is non-null (checked above) and 'static.
                let interface: &GuestContractInterface = unsafe { &*interface_ptr };
                // SAFETY: dispatch.native.functions is a valid static array; index 2 is within function_count.
                // We assume dispatch_type == Native for benchmark plugins.
                let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(2) };
                // SAFETY: fn_ptr is a valid extern C fn for the given function id.
                let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
                    unsafe { core::mem::transmute(fn_ptr) };
                // SAFETY: sv and sv_out are valid locations matching the fn signature.
                unsafe {
                    black_box(dispatch_fn(
                        black_box(&sv as *const StringView as *const ()),
                        black_box(&mut sv_out as *mut StringView as *mut ()),
                    ))
                }
            };
            black_box(result);
            black_box(sv_out);
        });
    });

    group.finish();
    core::mem::forget(_memory_lib);
}

// ─── Benchmark 5 — marshalling: borrowed view vs owned copy by size ───────────

/// Arguments to `memory_return_owned` (memory_plugin fn 5).
#[repr(C)]
struct CopyArgs {
    host: *const HostApi,
    sv: StringView,
}

/// Measures the cost of returning data across the ABI two ways, across payload
/// sizes:
///   - **borrowed** (fn 4): the plugin returns a `StringView` that aliases the
///     caller's bytes — zero allocation, zero copy, size-independent.
///   - **owned** (fn 5): the plugin host-allocates `len` bytes and copies the
///     input in — an allocation plus a `memcpy` that scales with the payload.
///
/// This is the cost behind borrowed-view returns (`&str` / `string_view` /
/// `ReadOnlySpan` / `memoryview`) versus owned native `String` / `bytes`.
fn bench_marshalling(c: &mut Criterion) {
    // Reset registry for a clean slate.
    BENCH_REGISTRY.with(|cell: &core::cell::RefCell<Option<RuntimeStore>>| {
        *cell.borrow_mut() = Some(RuntimeStore::new());
    });

    let _library: libloading::Library = load_and_init_plugin(MEMORY_PLUGIN_SO);
    // fn 4 = memory_return_borrowed(StringView) -> StringView
    // fn 5 = memory_return_owned(CopyArgs) -> Buffer
    let borrowed_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError = get_interface_fn(4);
    let owned_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError = get_interface_fn(5);

    // Host table so the owned path can allocate through the host allocator.
    let host_interface: HostApi = bench_host_api();

    // Backing payload of valid bytes; each size borrows a prefix of it.
    let payload: Vec<u8> = vec![b'a'; 16384];
    let sizes: [usize; 4] = [16, 256, 4096, 16384];

    let mut group: criterion::BenchmarkGroup<'_, criterion::measurement::WallTime> =
        c.benchmark_group("marshalling");
    group.throughput(Throughput::Elements(1));

    for &size in &sizes {
        let sv: StringView = StringView {
            ptr: payload.as_ptr(),
            len: size,
        };

        group.bench_with_input(BenchmarkId::new("borrowed", size), &sv, |b, sv| {
            let mut out: StringView = StringView::null();
            b.iter(|| {
                // SAFETY: sv points to a valid StringView; out points to a valid StringView.
                // memory_plugin fn 4 echoes the view back without touching the bytes.
                let result: AbiError = unsafe {
                    borrowed_fn(
                        black_box(sv as *const StringView as *const ()),
                        black_box(&mut out as *mut StringView as *mut ()),
                    )
                };
                black_box(result);
                black_box(out);
            });
        });

        let copy_args: CopyArgs = CopyArgs {
            host: &host_interface as *const HostApi,
            sv,
        };
        group.bench_with_input(BenchmarkId::new("owned", size), &copy_args, |b, args| {
            let mut out: Buffer = Buffer {
                ptr: core::ptr::null_mut(),
                len: 0,
                cap: 0,
            };
            b.iter(|| {
                // SAFETY: args points to a valid CopyArgs (valid host + sv); out points to a
                // valid Buffer. memory_plugin fn 5 host-allocs `sv.len` bytes and copies in.
                let result: AbiError = unsafe {
                    owned_fn(
                        black_box(args as *const CopyArgs as *const ()),
                        black_box(&mut out as *mut Buffer as *mut ()),
                    )
                };
                black_box(result);
                // Free the owned buffer each iteration so the loop does not leak/grow.
                if !out.ptr.is_null() {
                    // SAFETY: out.ptr was allocated by the plugin via bench_alloc
                    // (polyplug_host_alloc) with align 1 and `out.len` bytes.
                    unsafe { polyplug_host_free(out.ptr, out.len, 1) };
                }
            });
        });
    }

    group.finish();
    core::mem::forget(_library);
}

// ─── criterion_group / criterion_main ────────────────────────────────────────

criterion_group!(
    benches,
    bench_dispatch_noop,
    bench_dispatch_buffer_arg,
    bench_dispatch_struct_arg_and_return,
    bench_dispatch_cross_plugin,
    bench_marshalling,
);
criterion_main!(benches);

#![allow(clippy::expect_used)]

// THIS IS A BENCHMARK FILE — do not add #[test] functions here
// Run with: cargo bench -p polyplug --bench ffi_find_all
//
// Benchmark: HostApi.find_all_guest_contracts path
// Measures: Time to count, allocate, and populate an Array<GuestContractHandle>

use core::cell::Cell;
use core::cell::RefCell;
use core::ffi::c_void;
use core::hint::black_box;
use core::mem;
use core::ptr;
use core::slice::from_raw_parts;
use core::slice::from_raw_parts_mut;
use core::str::from_utf8_unchecked;

use criterion::BenchmarkId;
use criterion::Criterion;
use criterion::Throughput;
use criterion::criterion_group;
use criterion::criterion_main;

use libloading::{Library, Symbol};
use polyplug::runtime_store::RuntimeStore;
use polyplug_abi::AbiError;
use polyplug_abi::AbiErrorCode;
use polyplug_abi::Array;
use polyplug_abi::BundleInitContext;
use polyplug_abi::DependencyInfo;
use polyplug_abi::DispatchMechanisms;
use polyplug_abi::DispatchType;
use polyplug_abi::GuestContractHandle;
use polyplug_abi::GuestContractInstance;
use polyplug_abi::GuestContractInterface;
use polyplug_abi::HostApi;
use polyplug_abi::HostContractInstance;
use polyplug_abi::HostContractInterface;
use polyplug_abi::NativeDispatch;
use polyplug_abi::PluginDescriptor;
use polyplug_abi::StringView;
use polyplug_abi::dispatch::VmLoaderData;
use polyplug_abi::ffi::polyplug_host_alloc;
use polyplug_abi::ffi::polyplug_host_free;
use polyplug_abi::in_process::reject_in_process_bundle;
use polyplug_abi::types::Version;
use polyplug_utils::BundleId;
use polyplug_utils::GuestContractId;

use criterion::BenchmarkGroup;
use criterion::measurement::WallTime;

// ─── Plugin paths from build.rs ──────────────────────────────────────────────

const TEST_PLUGIN_SO: &str = env!("TEST_PLUGIN_SO");

// ─── Thread-local registry and captured interface state ────────────────────────

thread_local! {
    static BENCH_REGISTRY: RefCell<Option<RuntimeStore>> = RefCell::new(Some(RuntimeStore::new()));
    static LAST_CONTRACT_ID: Cell<u64> = const { Cell::new(0) };
}

/// Registration callback — registers the guest contract into BENCH_REGISTRY.
///
/// # Safety
/// `descriptor` and `interface` must be valid pointers for the call duration.
/// `out_err` must be non-null and writable.
unsafe extern "C" fn bench_register_callback(
    _this: *const HostApi,
    descriptor: *const PluginDescriptor,
    interface: *const GuestContractInterface,
    out_err: *mut AbiError,
) {
    if descriptor.is_null() || interface.is_null() {
        if !out_err.is_null() {
            // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
            unsafe {
                out_err.write(AbiError {
                    code: AbiErrorCode::Generic as u32,
                    message: StringView::null(),
                })
            };
        }
        return;
    }

    // SAFETY: descriptor is valid for this call per ABI contract.
    let desc: &PluginDescriptor = unsafe { &*descriptor };
    // SAFETY: interface is valid for this call per ABI contract.
    let iface: &GuestContractInterface = unsafe { &*interface };

    // SAFETY: desc.contract_name is set from a &'static str in the benchmark fixture.
    // The bytes are valid UTF-8 by construction.
    let contract_name: &str = unsafe {
        let bytes: &[u8] = from_raw_parts(desc.contract_name.ptr, desc.contract_name.len);
        from_utf8_unchecked(bytes) // SAFETY: see comment above
    };

    let result: Result<GuestContractHandle, _> =
        BENCH_REGISTRY.with(|cell: &RefCell<Option<RuntimeStore>>| {
            let borrowed = cell.borrow();
            let registry = borrowed.as_ref().expect("registry not initialized");
            // SAFETY: interface pointer is 'static — extracted from a loaded library that outlives registry.
            unsafe {
                registry.register_guest_contract(
                    *desc,
                    interface,
                    contract_name.to_owned(),
                    BundleId::from_u64(iface.contract_id.id()),
                )
            }
        });

    let err: AbiError = match result {
        Ok(_) => {
            LAST_CONTRACT_ID.with(|cell| cell.set(iface.contract_id.id()));
            AbiError::ok()
        }
        Err(_) => AbiError {
            code: AbiErrorCode::Generic as u32,
            message: StringView::null(),
        },
    };
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(err) };
    }
}

// ─── Stub HostApi functions ─────────────────────────────────────────────

/// Finds a guest contract by contract_id in the thread-local BENCH_REGISTRY.
///
/// # Safety
/// Must only be called from a bench thread where BENCH_REGISTRY is initialised.
unsafe extern "C" fn bench_find_guest_contract(
    _this: *const HostApi,
    contract_id: u64,
    min_version: u32,
) -> GuestContractHandle {
    BENCH_REGISTRY.with(|cell: &RefCell<Option<RuntimeStore>>| {
        let registry = cell.borrow();
        let reg = registry.as_ref().expect("registry not initialized");
        reg.find(GuestContractId::from_u64(contract_id), min_version)
            .unwrap_or_else(|_| GuestContractHandle::null())
    })
}

/// find_all callback — mirrors the real runtime: count, host-allocate, populate.
///
/// # Safety
/// `this` must be a valid HostApi backed by BENCH_REGISTRY.
unsafe extern "C" fn bench_find_all_guest_contracts(
    this: *const HostApi,
    contract_id: u64,
    min_version: u32,
) -> Array<GuestContractHandle> {
    BENCH_REGISTRY.with(|cell: &RefCell<Option<RuntimeStore>>| {
        let borrowed = cell.borrow();
        let registry = borrowed.as_ref().expect("registry not initialized");

        let count: usize =
            registry.count_guest_contracts(GuestContractId::from_u64(contract_id), min_version);
        if count == 0 {
            return Array::empty();
        }

        let size: usize = count * mem::size_of::<GuestContractHandle>();
        let align: usize = mem::align_of::<GuestContractHandle>();
        // SAFETY: this is a valid HostApi; alloc is safe for any size/align.
        let ptr: *mut GuestContractHandle =
            unsafe { ((*this).alloc)(this, size, align) as *mut GuestContractHandle };
        if ptr.is_null() {
            return Array::empty();
        }

        // SAFETY: ptr was allocated for `count` GuestContractHandle elements above.
        let slice: &mut [GuestContractHandle] = unsafe { from_raw_parts_mut(ptr, count) };
        let actual: usize = registry.find_all_guest_contracts_into(
            GuestContractId::from_u64(contract_id),
            min_version,
            slice,
        );

        Array::new(ptr, actual)
    })
}

/// Resolves a guest contract handle to an interface pointer via BENCH_REGISTRY.
///
/// # Safety
/// The returned pointer is valid and 'static — the library is kept alive via mem::forget.
unsafe extern "C" fn bench_resolve_guest_contract(
    _this: *const HostApi,
    handle: GuestContractHandle,
) -> *const GuestContractInterface {
    BENCH_REGISTRY.with(|cell: &RefCell<Option<RuntimeStore>>| {
        cell.borrow()
            .as_ref()
            .expect("registry not initialized")
            .resolve_guest_contract(handle)
            .unwrap_or(ptr::null())
    })
}

unsafe extern "C" fn bench_get_host_contract(
    _this: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> HostContractInstance {
    HostContractInstance::null()
}

unsafe extern "C" fn bench_resolve_host_contract_interface(
    _this: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> *const HostContractInterface {
    ptr::null()
}

unsafe extern "C" fn bench_list_bundles(_this: *const HostApi) -> Array<BundleId> {
    Array::empty()
}

unsafe extern "C" fn bench_get_dependencies(_this: *const HostApi) -> Array<DependencyInfo> {
    Array::empty()
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

unsafe extern "C" fn bench_load_bundle(
    _this: *const HostApi,
    _path: *const u8,
    _path_len: usize,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe {
            out_err.write(AbiError {
                code: AbiErrorCode::Generic as u32,
                message: StringView::null(),
            })
        };
    }
}

unsafe extern "C" fn bench_reload_bundle(
    _this: *const HostApi,
    _path: *const u8,
    _path_len: usize,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe {
            out_err.write(AbiError {
                code: AbiErrorCode::Generic as u32,
                message: StringView::null(),
            })
        };
    }
}

unsafe extern "C" fn bench_register_host_contract(
    _this: *const HostApi,
    _interface: *const HostContractInterface,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe {
            out_err.write(AbiError {
                code: AbiErrorCode::Generic as u32,
                message: StringView::null(),
            })
        };
    }
}

unsafe extern "C" fn bench_register_loader(
    _this: *const HostApi,
    _loader_ptr: *mut c_void,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe {
            out_err.write(AbiError {
                code: AbiErrorCode::Generic as u32,
                message: StringView::null(),
            })
        };
    }
}

unsafe extern "C" fn bench_get_last_error(
    _this: *const HostApi,
    _buf: *mut u8,
    _buf_len: usize,
) -> usize {
    0
}

unsafe extern "C" fn bench_get_error_len(_this: *const HostApi) -> usize {
    0
}

unsafe extern "C" fn bench_unload_bundle(
    _this: *const HostApi,
    _bundle_id: BundleId,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
    }
}

// ─── Setup helper ────────────────────────────────────────────────────────────

/// Build a HostApi backed by the thread-local BENCH_REGISTRY.
fn build_host_interface() -> HostApi {
    HostApi {
        runtime: ptr::null_mut(),
        register_guest_contract: bench_register_callback,
        register_in_process_bundle: reject_in_process_bundle,
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
        unload_bundle: bench_unload_bundle,
        log: stub_host_log,
        create_guest_instance: stub_create_guest_instance,
        destroy_guest_instance: stub_destroy_guest_instance,
        registry_revision: stub_registry_revision,
        reserved: ptr::null(),
    }
}

/// Load the test plugin cdylib, call `polyplug_init`, and register into BENCH_REGISTRY.
/// Returns the loaded library (kept alive via never-drop invariant) and the
/// registered contract_id.
fn load_and_init_plugin(host_interface: &HostApi) -> (Library, u64) {
    // SAFETY: path is a valid compiled cdylib built by build.rs.
    let library: Library = unsafe { Library::new(TEST_PLUGIN_SO).expect("failed to load plugin") };

    // SAFETY: polyplug_init matches the expected 2-arg ABI.
    let init_fn: Symbol<
        '_,
        unsafe extern "C" fn(*const HostApi, *const BundleInitContext) -> AbiError,
    > = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init not found")
    };

    let plugin_ctx: BundleInitContext = BundleInitContext {
        bundle_path: StringView::null(),
        bundle_id: 0,
    };

    // SAFETY: init_fn is a valid function; host_interface and plugin_ctx live for the call duration.
    let result: AbiError = unsafe {
        init_fn(
            host_interface as *const HostApi,
            &plugin_ctx as *const BundleInitContext,
        )
    };
    assert!(
        result.is_ok(),
        "polyplug_init failed for {}",
        TEST_PLUGIN_SO
    );

    let contract_id: u64 = LAST_CONTRACT_ID.with(|cell| cell.get());
    assert_ne!(contract_id, 0, "plugin contract_id was not captured");

    (library, contract_id)
}

// ─── Benchmark: find_all with a single matching contract ──────────────────────

fn bench_ffi_find_all_single_match(c: &mut Criterion) {
    BENCH_REGISTRY.with(|cell: &RefCell<Option<RuntimeStore>>| {
        *cell.borrow_mut() = Some(RuntimeStore::new());
    });

    let host_interface: HostApi = build_host_interface();
    let (library, contract_id): (Library, u64) = load_and_init_plugin(&host_interface);

    let mut group: BenchmarkGroup<'_, WallTime> = c.benchmark_group("ffi");
    group.throughput(Throughput::Elements(1));

    group.bench_function(
        BenchmarkId::new("find_all_by_contract", "single_match"),
        |b| {
            b.iter(|| {
                // SAFETY: bench_find_all_guest_contracts is backed by BENCH_REGISTRY.
                let arr: Array<GuestContractHandle> = unsafe {
                    (host_interface.find_all_guest_contracts)(
                        black_box(&host_interface as *const HostApi),
                        black_box(contract_id),
                        black_box(0_u32),
                    )
                };
                // Caller owns the host-allocated array; free it via host->free.
                if !arr.items.is_null() {
                    let size: usize = arr.len * mem::size_of::<GuestContractHandle>();
                    // SAFETY: arr.items was allocated by bench_alloc with this size/align.
                    unsafe {
                        (host_interface.free)(
                            &host_interface as *const HostApi,
                            arr.items as *mut u8,
                            size,
                            arr.align,
                        );
                    }
                }
                black_box(arr.len);
            });
        },
    );

    group.finish();
    // Keep library alive for the process lifetime (never-drop invariant).
    mem::forget(library);
}

// ─── Benchmark: find_all with no matching contract ────────────────────────────

fn bench_ffi_find_all_empty_result(c: &mut Criterion) {
    BENCH_REGISTRY.with(|cell: &RefCell<Option<RuntimeStore>>| {
        *cell.borrow_mut() = Some(RuntimeStore::new());
    });

    let host_interface: HostApi = build_host_interface();
    let (library, _contract_id): (Library, u64) = load_and_init_plugin(&host_interface);

    let nonexistent_contract_id: u64 = 0xDEAD_BEEF_CAFE_0000_u64;

    let mut group: BenchmarkGroup<'_, WallTime> = c.benchmark_group("ffi");
    group.throughput(Throughput::Elements(1));

    group.bench_function(
        BenchmarkId::new("find_all_by_contract", "empty_result"),
        |b| {
            b.iter(|| {
                // SAFETY: bench_find_all_guest_contracts is backed by BENCH_REGISTRY.
                let arr: Array<GuestContractHandle> = unsafe {
                    (host_interface.find_all_guest_contracts)(
                        black_box(&host_interface as *const HostApi),
                        black_box(nonexistent_contract_id),
                        black_box(0_u32),
                    )
                };
                black_box(arr.len);
            });
        },
    );

    group.finish();
    mem::forget(library);
}

// ─── Synthetic-interface helpers for the registry scale sweep ─────────────────

/// Stub create_instance for the synthetic sweep interfaces.
unsafe extern "C" fn sweep_create_instance(
    _adapter_context: *mut c_void,
    _loader_data: VmLoaderData,
    _host: *const HostApi,
    _args: *const (),
    out_instance: *mut GuestContractInstance,
) {
    if !out_instance.is_null() {
        // SAFETY: out_instance is non-null (just checked) and writable per the ABI contract.
        unsafe { out_instance.write(GuestContractInstance::null()) };
    }
}

/// Stub destroy_instance for the synthetic sweep interfaces.
unsafe extern "C" fn sweep_destroy_instance(
    _adapter_context: *mut c_void,
    _loader_data: VmLoaderData,
    _host: *const HostApi,
    _instance: GuestContractInstance,
) {
}

/// Build a leaked `'static` native interface for `contract_id` (no functions —
/// the sweep only counts + collects handles, it never dispatches).
fn leak_sweep_interface(contract_id: u64) -> &'static GuestContractInterface {
    Box::leak(Box::new(GuestContractInterface {
        contract_id: GuestContractId::from_u64(contract_id),
        contract_version: Version {
            major: 1,
            minor: 0,
            patch: 0,
        },
        dispatch_type: DispatchType::Native,
        adapter_context: ptr::null_mut(),
        create_instance: sweep_create_instance,
        destroy_instance: sweep_destroy_instance,
        dispatch: DispatchMechanisms {
            native: NativeDispatch {
                function_count: 0,
                functions: ptr::null(),
            },
        },
    }))
}

// ─── Benchmark: find_all scaling across registry sizes (10 / 100 / 1000) ──────

/// Registers `size` distinct contracts into BENCH_REGISTRY, then times the FFI
/// `find_all_guest_contracts` of one target contract that has a single matching
/// provider. find_all looks the contract up by id (a HashMap probe) and then
/// counts + collects only that contract's providers, so the per-call cost is
/// dominated by the single-match path, not the total registry size — this sweep
/// is the evidence for that. The result Array is host-allocated and freed each
/// iteration so the loop does not grow.
fn bench_ffi_find_all_registry_sweep(c: &mut Criterion) {
    let host_interface: HostApi = build_host_interface();
    let sizes: [u64; 3] = [10, 100, 1000];

    let mut group: BenchmarkGroup<'_, WallTime> = c.benchmark_group("ffi");
    group.throughput(Throughput::Elements(1));

    for &size in &sizes {
        // Fresh registry holding `size` distinct contracts; the LAST id is the
        // single-match target the bench looks up.
        BENCH_REGISTRY.with(|cell: &RefCell<Option<RuntimeStore>>| {
            *cell.borrow_mut() = Some(RuntimeStore::new());
        });

        let base_id: u64 = 0x6000_0000_0000_0000_u64;
        for i in 0..size {
            let interface: &'static GuestContractInterface = leak_sweep_interface(base_id + i);
            let descriptor: PluginDescriptor = PluginDescriptor {
                name: StringView::from_static(b"sweep_plugin"),
                contract_name: StringView::from_static(b"sweep.contract"),
                version: Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
            };
            BENCH_REGISTRY.with(|cell: &RefCell<Option<RuntimeStore>>| {
                let borrowed = cell.borrow();
                let registry: &RuntimeStore = borrowed.as_ref().expect("registry not initialized");
                // SAFETY: interface is leaked ('static), valid for the registry lifetime.
                unsafe {
                    registry
                        .register_guest_contract(
                            descriptor,
                            interface,
                            format!("sweep.contract.{}", i),
                            BundleId::from_u64(i),
                        )
                        .expect("registration should succeed");
                }
            });
        }

        let target_id: u64 = base_id + size - 1;

        group.bench_with_input(
            BenchmarkId::new("find_all_by_contract", format!("registry_{}", size)),
            &target_id,
            |b, &target_id| {
                b.iter(|| {
                    // SAFETY: bench_find_all_guest_contracts is backed by BENCH_REGISTRY.
                    let arr: Array<GuestContractHandle> = unsafe {
                        (host_interface.find_all_guest_contracts)(
                            black_box(&host_interface as *const HostApi),
                            black_box(target_id),
                            black_box(0_u32),
                        )
                    };
                    if !arr.items.is_null() {
                        let size_bytes: usize = arr.len * mem::size_of::<GuestContractHandle>();
                        // SAFETY: arr.items was host-allocated by bench_alloc with this size/align.
                        unsafe {
                            (host_interface.free)(
                                &host_interface as *const HostApi,
                                arr.items as *mut u8,
                                size_bytes,
                                arr.align,
                            );
                        }
                    }
                    black_box(arr.len);
                });
            },
        );
    }

    group.finish();
}

// ─── criterion_group / criterion_main ────────────────────────────────────────

criterion_group!(
    benches,
    bench_ffi_find_all_single_match,
    bench_ffi_find_all_empty_result,
    bench_ffi_find_all_registry_sweep,
);
criterion_main!(benches);

/// `HostApi.log` stub for test hosts — drops the record.
unsafe extern "C" fn stub_host_log(
    _this: *const HostApi,
    _level: u32,
    _scope: StringView,
    _message: StringView,
) {
}

unsafe extern "C" fn stub_create_guest_instance(
    _this: *const HostApi,
    _interface: *const GuestContractInterface,
    _args: *const c_void,
    out_instance: *mut GuestContractInstance,
) {
    if !out_instance.is_null() {
        // SAFETY: out_instance is non-null (just checked) and writable per the ABI contract.
        unsafe { out_instance.write(GuestContractInstance::null()) };
    }
}

unsafe extern "C" fn stub_destroy_guest_instance(
    _this: *const HostApi,
    _interface: *const GuestContractInterface,
    _instance: GuestContractInstance,
) {
}

unsafe extern "C" fn stub_registry_revision(_this: *const HostApi) -> u64 {
    0
}

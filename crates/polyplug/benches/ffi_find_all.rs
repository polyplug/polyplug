#![allow(clippy::expect_used)]

// THIS IS A BENCHMARK FILE — do not add #[test] functions here
// Run with: cargo bench -p polyplug --bench ffi_find_all
//
// Benchmark: HostApi.find_all_guest_contracts path
// Measures: Time to count, allocate, and populate an Array<GuestContractHandle>

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
use polyplug_abi::GuestContractHandle;
use polyplug_abi::GuestContractInterface;
use polyplug_abi::HostApi;
use polyplug_abi::PluginDescriptor;
use polyplug_abi::StringView;
use polyplug_abi::ffi::polyplug_host_alloc;
use polyplug_abi::ffi::polyplug_host_free;
use polyplug_utils::BundleId;
use polyplug_utils::GuestContractId;

// ─── Plugin paths from build.rs ──────────────────────────────────────────────

const TEST_PLUGIN_SO: &str = env!("TEST_PLUGIN_SO");

// ─── Thread-local registry and captured interface state ────────────────────────

thread_local! {
    static BENCH_REGISTRY: RefCell<Option<RuntimeStore>> = RefCell::new(Some(RuntimeStore::new()));
    static LAST_CONTRACT_ID: core::cell::Cell<u64> = const { core::cell::Cell::new(0) };
}

/// Registration callback — registers the guest contract into BENCH_REGISTRY.
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

    // SAFETY: descriptor is valid for this call per ABI contract.
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

    match result {
        Ok(_) => {
            LAST_CONTRACT_ID.with(|cell| cell.set(iface.contract_id.id()));
            AbiError::ok()
        }
        Err(_) => AbiError {
            code: AbiErrorCode::Generic as u32,
            message: StringView::null(),
        },
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
    BENCH_REGISTRY.with(|cell: &core::cell::RefCell<Option<RuntimeStore>>| {
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
    BENCH_REGISTRY.with(|cell: &core::cell::RefCell<Option<RuntimeStore>>| {
        let borrowed = cell.borrow();
        let registry = borrowed.as_ref().expect("registry not initialized");

        let count: usize =
            registry.count_guest_contracts(GuestContractId::from_u64(contract_id), min_version);
        if count == 0 {
            return Array::empty();
        }

        let size: usize = count * core::mem::size_of::<GuestContractHandle>();
        let align: usize = core::mem::align_of::<GuestContractHandle>();
        // SAFETY: this is a valid HostApi; alloc is safe for any size/align.
        let ptr: *mut GuestContractHandle =
            unsafe { ((*this).alloc)(this, size, align) as *mut GuestContractHandle };
        if ptr.is_null() {
            return Array::empty();
        }

        // SAFETY: ptr was allocated for `count` GuestContractHandle elements above.
        let slice: &mut [GuestContractHandle] =
            unsafe { core::slice::from_raw_parts_mut(ptr, count) };
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
    BENCH_REGISTRY.with(|cell: &core::cell::RefCell<Option<RuntimeStore>>| {
        cell.borrow()
            .as_ref()
            .expect("registry not initialized")
            .resolve_guest_contract(handle)
            .unwrap_or(core::ptr::null())
    })
}

unsafe extern "C" fn bench_get_host_contract(
    _this: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> polyplug_abi::HostContractInstance {
    polyplug_abi::HostContractInstance::null()
}

unsafe extern "C" fn bench_resolve_host_contract_interface(
    _this: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> *const polyplug_abi::HostContractInterface {
    core::ptr::null()
}

unsafe extern "C" fn bench_list_bundles(_this: *const HostApi) -> Array<BundleId> {
    Array::empty()
}

unsafe extern "C" fn bench_get_dependencies(
    _this: *const HostApi,
) -> Array<polyplug_abi::DependencyInfo> {
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
) -> AbiError {
    AbiError {
        code: AbiErrorCode::Generic as u32,
        message: StringView::null(),
    }
}

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

unsafe extern "C" fn bench_register_host_contract(
    _this: *const HostApi,
    _interface: *const polyplug_abi::HostContractInterface,
) -> AbiError {
    AbiError {
        code: AbiErrorCode::Generic as u32,
        message: StringView::null(),
    }
}

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

unsafe extern "C" fn bench_unload_bundle(_this: *const HostApi, _bundle_id: BundleId) -> AbiError {
    AbiError::ok()
}

// ─── Setup helper ────────────────────────────────────────────────────────────

/// Build a HostApi backed by the thread-local BENCH_REGISTRY.
fn build_host_interface() -> HostApi {
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
        unload_bundle: bench_unload_bundle,
        log: stub_host_log,
        reserved: core::ptr::null(),
    }
}

/// Load the test plugin cdylib, call `polyplug_init`, and register into BENCH_REGISTRY.
/// Returns the loaded library (kept alive via never-drop invariant) and the
/// registered contract_id.
fn load_and_init_plugin(host_interface: &HostApi) -> (libloading::Library, u64) {
    // SAFETY: path is a valid compiled cdylib built by build.rs.
    let library: libloading::Library =
        unsafe { libloading::Library::new(TEST_PLUGIN_SO).expect("failed to load plugin") };

    // SAFETY: polyplug_init matches the expected 2-arg ABI.
    let init_fn: libloading::Symbol<
        '_,
        unsafe extern "C" fn(*const HostApi, *const polyplug_abi::BundleInitContext) -> AbiError,
    > = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init not found")
    };

    let plugin_ctx: polyplug_abi::BundleInitContext = polyplug_abi::BundleInitContext {
        bundle_path: StringView::null(),
        bundle_id: 0,
    };

    // SAFETY: init_fn is a valid function; host_interface and plugin_ctx live for the call duration.
    let result: AbiError = unsafe {
        init_fn(
            host_interface as *const HostApi,
            &plugin_ctx as *const polyplug_abi::BundleInitContext,
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
    BENCH_REGISTRY.with(|cell: &core::cell::RefCell<Option<RuntimeStore>>| {
        *cell.borrow_mut() = Some(RuntimeStore::new());
    });

    let host_interface: HostApi = build_host_interface();
    let (library, contract_id): (libloading::Library, u64) = load_and_init_plugin(&host_interface);

    let mut group: criterion::BenchmarkGroup<'_, criterion::measurement::WallTime> =
        c.benchmark_group("ffi");
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
                    let size: usize = arr.len * core::mem::size_of::<GuestContractHandle>();
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
    core::mem::forget(library);
}

// ─── Benchmark: find_all with no matching contract ────────────────────────────

fn bench_ffi_find_all_empty_result(c: &mut Criterion) {
    BENCH_REGISTRY.with(|cell: &core::cell::RefCell<Option<RuntimeStore>>| {
        *cell.borrow_mut() = Some(RuntimeStore::new());
    });

    let host_interface: HostApi = build_host_interface();
    let (library, _contract_id): (libloading::Library, u64) = load_and_init_plugin(&host_interface);

    let nonexistent_contract_id: u64 = 0xDEAD_BEEF_CAFE_0000_u64;

    let mut group: criterion::BenchmarkGroup<'_, criterion::measurement::WallTime> =
        c.benchmark_group("ffi");
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
    core::mem::forget(library);
}

// ─── criterion_group / criterion_main ────────────────────────────────────────

criterion_group!(
    benches,
    bench_ffi_find_all_single_match,
    bench_ffi_find_all_empty_result
);
criterion_main!(benches);

/// `HostApi.log` stub for test hosts — drops the record.
unsafe extern "C" fn stub_host_log(
    _this: *const polyplug_abi::HostApi,
    _level: u32,
    _scope: polyplug_abi::StringView,
    _message: polyplug_abi::StringView,
) {
}

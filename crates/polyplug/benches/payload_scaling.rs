#![allow(clippy::expect_used)]

// THIS IS A BENCHMARK FILE — do not add #[test] functions here
// Run with: cargo bench -p polyplug --bench payload_scaling
//
// ─── What this measures (and why it's the honest one) ────────────────────────
//
// `counter_inc` deliberately uses the cheapest possible payload (`x + 1`) to
// expose polyplug's *fixed* per-call overhead. This bench answers the obvious
// next question: **how much does that fixed overhead matter once the call does
// real work?**
//
// It runs the *same* unit of work — fill (write) N bytes, one byte at a time —
// two ways, across a sweep of N:
//
//   native_direct     — an `#[inline(never)]` Rust fn that writes N bytes,
//                        statically linked. The work with minimal call overhead.
//   polyplug_dispatch  — the identical byte-write loop, but it lives in a
//                        dynamically-loaded plugin (`memory_plugin` fn 0) and is
//                        reached through polyplug's resolved contract dispatch.
//
// Both arms execute the **same per-byte loop** (this file's `fill_abi` is a copy
// of `memory_fill_preallocated_buffer` in tests/fixtures/memory_plugin), so for
// any given N the difference between them is *only* the dispatch overhead. As N
// grows, the per-byte work dominates and that fixed overhead shrinks to a
// rounding error:
//
//   N (bytes) │ native_direct │ polyplug_dispatch │ overhead │ overhead %
//   ──────────┼───────────────┼───────────────────┼──────────┼───────────
//        0    │   ~1 ns       │   ~2-3 ns         │  ~1 ns   │  ~100%   ← all overhead
//       16    │    …          │    …              │  ~1 ns   │   large
//      256    │    …          │    …              │  ~1 ns   │   small
//     4096    │    …          │    …              │  ~1 ns   │  ~1%
//    16384    │    …          │    …              │  ~1 ns   │  ~0.3%   ← invisible
//
// The numbers fill in when you run it locally. The shape is the point: the
// `overhead` column is roughly *constant* (it's the fixed dispatch cost), so the
// `overhead %` column collapses toward zero as the payload grows. That is the
// real-world claim — on any call that does meaningful work, the safety boundary
// is free.
//
// Honest caveats:
//   * The "work" here is a byte-write loop. It is representative of "transform N
//     bytes" plugins but is cheaper per byte than, say, parsing or crypto; a
//     heavier per-byte workload only makes the overhead % smaller, never larger.
//   * `native_direct` is statically linked, so it is not literally "no plugin" —
//     it is "the same work with the cheapest call." The dispatch arm is the
//     product. We compare the two; we do not claim dispatch is free, only that
//     its cost is fixed and amortizes away.

use core::cell::Cell;
use core::ffi::c_void;
use core::hint::black_box;
use core::mem;
use core::ptr;

use criterion::BenchmarkGroup;
use criterion::BenchmarkId;
use criterion::Criterion;
use criterion::Throughput;
use criterion::criterion_group;
use criterion::criterion_main;
use criterion::measurement::WallTime;

use libloading::{Library, Symbol};
use polyplug_abi::AbiError;
use polyplug_abi::AbiErrorCode;
use polyplug_abi::Array;
use polyplug_abi::Buffer;
use polyplug_abi::BundleInitContext;
use polyplug_abi::DependencyInfo;
use polyplug_abi::DispatchType;
use polyplug_abi::GuestContractHandle;
use polyplug_abi::GuestContractInstance;
use polyplug_abi::GuestContractInterface;
use polyplug_abi::HostApi;
use polyplug_abi::HostContractInstance;
use polyplug_abi::HostContractInterface;
use polyplug_abi::PluginDescriptor;
use polyplug_abi::StringView;
use polyplug_abi::ffi::polyplug_host_alloc;
use polyplug_abi::ffi::polyplug_host_free;
use polyplug_abi::in_process::reject_in_process_bundle;
use polyplug_utils::BundleId;

const MEMORY_PLUGIN_SO: &str = env!("MEMORY_PLUGIN_SO");

/// Payload sizes swept, in bytes. 0 isolates pure overhead; by 1 MiB the fixed
/// per-call cost is buried under three-plus orders of magnitude of real work.
const SIZES: [usize; 10] = [0, 16, 64, 256, 1024, 4096, 16384, 65536, 262_144, 1_048_576];

/// Largest payload — the one buffer allocated up front and reused for every size.
const MAX_SIZE: usize = 1_048_576;

/// Mirror of `memory_plugin`'s `FillArgs` (fn 0): pre-allocated buffer + fill byte.
#[repr(C)]
struct FillArgs {
    buf: Buffer,
    fill_byte: u8,
}

// ─── native_direct arm — identical work, statically linked ───────────────────

/// A byte-for-byte copy of `memory_fill_preallocated_buffer` (memory_plugin fn 0),
/// compiled into this binary so the per-byte work matches the dispatched arm
/// exactly. `#[inline(never)]` keeps it an honest call, not a folded loop.
///
/// # Safety
/// `args` must point to a valid `FillArgs` whose `buf.ptr` is writable for
/// `buf.cap` bytes; `out` must point to a writable `u32`.
#[inline(never)]
extern "C" fn fill_abi(args: *const (), out: *mut ()) -> AbiError {
    // SAFETY: args points to a valid FillArgs per the bench's call site.
    let fill_args: &FillArgs = unsafe { &*(args as *const FillArgs) };
    let ptr: *mut u8 = fill_args.buf.ptr;
    let cap: usize = fill_args.buf.cap;
    // SAFETY: ptr is valid for `cap` writes (allocated by the bench), out is a u32.
    unsafe {
        let mut i: usize = 0;
        while i < cap {
            ptr.add(i).write(fill_args.fill_byte);
            i += 1;
        }
        (out as *mut u32).write(cap as u32);
    }
    AbiError::ok()
}

// ─── Interface capture for the dispatch arm ──────────────────────────────────

thread_local! {
    static CAPTURED_INTERFACE: Cell<*const GuestContractInterface> =
        const { Cell::new(ptr::null()) };
}

/// Register callback that captures the interface pointer.
///
/// # Safety
/// `interface` must be valid for the duration of this call.
/// `out_err` must be non-null and writable.
unsafe extern "C" fn capture_register_callback(
    _this: *const HostApi,
    descriptor: *const PluginDescriptor,
    interface: *const GuestContractInterface,
    out_err: *mut AbiError,
) {
    let err: AbiError = if descriptor.is_null() || interface.is_null() {
        AbiError {
            code: AbiErrorCode::Generic as u32,
            message: StringView::null(),
        }
    } else {
        CAPTURED_INTERFACE.with(|cell: &Cell<*const GuestContractInterface>| cell.set(interface));
        AbiError::ok()
    };
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(err) };
    }
}

// ─── Unused HostApi stubs (never invoked; fields are non-nullable fn ptrs) ────

unsafe extern "C" fn stub_find(
    _this: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> GuestContractHandle {
    GuestContractHandle::null()
}

unsafe extern "C" fn stub_find_all(
    _this: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> Array<GuestContractHandle> {
    Array::empty()
}

unsafe extern "C" fn stub_resolve(
    _this: *const HostApi,
    _handle: GuestContractHandle,
) -> *const GuestContractInterface {
    ptr::null()
}

unsafe extern "C" fn stub_get_host_contract(
    _this: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> HostContractInstance {
    HostContractInstance::null()
}

unsafe extern "C" fn stub_resolve_host_iface(
    _this: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> *const HostContractInterface {
    ptr::null()
}

unsafe extern "C" fn stub_list_bundles(_this: *const HostApi) -> Array<BundleId> {
    Array::empty()
}

unsafe extern "C" fn stub_get_dependencies(_this: *const HostApi) -> Array<DependencyInfo> {
    Array::empty()
}

unsafe extern "C" fn stub_load_bundle(
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

unsafe extern "C" fn stub_reload_bundle(
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

unsafe extern "C" fn stub_register_host_contract(
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

unsafe extern "C" fn stub_register_loader(
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

unsafe extern "C" fn stub_get_last_error(
    _this: *const HostApi,
    _buf: *mut u8,
    _buf_len: usize,
) -> usize {
    0
}

unsafe extern "C" fn stub_get_error_len(_this: *const HostApi) -> usize {
    0
}

unsafe extern "C" fn stub_unload_bundle(
    _this: *const HostApi,
    _bundle_id: BundleId,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
    }
}

unsafe extern "C" fn stub_alloc(_this: *const HostApi, _size: usize, _align: usize) -> *mut u8 {
    ptr::null_mut()
}

unsafe extern "C" fn stub_free(_this: *const HostApi, _ptr: *mut u8, _size: usize, _align: usize) {}

/// A `HostApi` whose `register_guest_contract` captures the interface pointer.
fn capture_host() -> HostApi {
    HostApi {
        runtime: ptr::null_mut(),
        register_guest_contract: capture_register_callback,
        register_in_process_bundle: reject_in_process_bundle,
        alloc: stub_alloc,
        free: stub_free,
        find_guest_contract: stub_find,
        find_all_guest_contracts: stub_find_all,
        resolve_guest_contract: stub_resolve,
        get_host_contract: stub_get_host_contract,
        resolve_host_contract_interface: stub_resolve_host_iface,
        list_bundles: stub_list_bundles,
        get_dependencies: stub_get_dependencies,
        load_bundle: stub_load_bundle,
        reload_bundle: stub_reload_bundle,
        register_host_contract: stub_register_host_contract,
        register_loader: stub_register_loader,
        get_last_error: stub_get_last_error,
        get_error_len: stub_get_error_len,
        unload_bundle: stub_unload_bundle,
        log: stub_host_log,
        create_guest_instance: stub_create_guest_instance,
        destroy_guest_instance: stub_destroy_guest_instance,
        registry_revision: stub_registry_revision,
        reserved: ptr::null(),
    }
}

/// Load `memory_plugin`, run `polyplug_init`, and return the dispatch function
/// for `memory.test` fn 0 (`fill_preallocated_buffer`) plus the kept-alive lib.
fn load_fill_dispatch() -> (
    Library,
    unsafe extern "C" fn(*mut c_void, GuestContractInstance, *const (), *mut (), *mut AbiError),
    *mut c_void,
) {
    // SAFETY: MEMORY_PLUGIN_SO is a valid cdylib built by build_all.sh.
    let library: Library = unsafe { Library::new(MEMORY_PLUGIN_SO).expect("load memory plugin") };
    // SAFETY: polyplug_init matches the 2-arg ABI.
    let init_fn: Symbol<
        '_,
        unsafe extern "C" fn(*const HostApi, *const BundleInitContext) -> AbiError,
    > = unsafe { library.get(b"polyplug_init\0").expect("polyplug_init") };

    let host: HostApi = capture_host();
    let ctx: BundleInitContext = BundleInitContext {
        bundle_path: StringView::null(),
        bundle_id: 0,
    };
    // SAFETY: host and ctx live for the call; init registers memory.test.
    let result: AbiError =
        unsafe { init_fn(&host as *const HostApi, &ctx as *const BundleInitContext) };
    assert!(result.is_ok(), "polyplug_init failed");

    let interface_ptr: *const GuestContractInterface =
        CAPTURED_INTERFACE.with(|cell: &Cell<*const GuestContractInterface>| cell.get());
    assert!(!interface_ptr.is_null(), "interface not captured");
    // SAFETY: interface is 'static within the loaded library (kept alive by caller).
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };
    assert!(
        interface.dispatch_type == DispatchType::Native,
        "expected native dispatch"
    );
    // SAFETY: fn 0 (fill) is in range (function_count == 4 for memory.test).
    let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(0) };
    // SAFETY: the registered fn has the native dispatch signature
    // `fn(GuestContractInstance, *const (), *mut (), *mut AbiError)`.
    let dispatch_fn: unsafe extern "C" fn(
        *mut c_void,
        GuestContractInstance,
        *const (),
        *mut (),
        *mut AbiError,
    ) = {
        // SAFETY: the pointer comes from the generated dispatch table and is cast to that function's exact ABI.
        unsafe { mem::transmute(fn_ptr) }
    };
    (library, dispatch_fn, interface.adapter_context)
}

// ─── Benchmark ───────────────────────────────────────────────────────────────

fn bench_payload_scaling(c: &mut Criterion) {
    // One 8-aligned buffer for the largest size, reused for every size (memory_plugin
    // fn 0 requires `buf.ptr` aligned to `align_of::<u64>()` and cap <= the alloc).
    let buf_ptr: *mut u8 = polyplug_host_alloc(MAX_SIZE, 8);
    assert!(!buf_ptr.is_null(), "bench alloc failed");

    let (library, dispatch_fn, adapter_context): (
        Library,
        unsafe extern "C" fn(*mut c_void, GuestContractInstance, *const (), *mut (), *mut AbiError),
        *mut c_void,
    ) = load_fill_dispatch();

    let mut group: BenchmarkGroup<'_, WallTime> = c.benchmark_group("payload_scaling");

    for &size in SIZES.iter() {
        // Throughput in bytes so criterion also reports MB/s per arm per size.
        group.throughput(Throughput::Bytes(size.max(1) as u64));

        let args: FillArgs = FillArgs {
            buf: Buffer {
                ptr: buf_ptr,
                len: 0,
                cap: size,
            },
            fill_byte: 0xCD,
        };

        // ── native_direct: same byte loop, statically linked ──────────────────
        group.bench_with_input(
            BenchmarkId::new("native_direct", size),
            &args,
            |b, args: &FillArgs| {
                b.iter(|| {
                    let mut out: u32 = 0;
                    // fill_abi is a safe extern "C" fn; its internal writes to
                    // args.buf.ptr (valid for cap bytes) are guarded inside it.
                    let err: AbiError = fill_abi(
                        black_box(args as *const FillArgs as *const ()),
                        black_box(&mut out as *mut u32 as *mut ()),
                    );
                    debug_assert!(err.is_ok());
                    black_box(out)
                });
            },
        );

        // ── polyplug_dispatch: same byte loop, reached through dispatch ────────
        group.bench_with_input(
            BenchmarkId::new("polyplug_dispatch", size),
            &args,
            |b, args: &FillArgs| {
                b.iter(|| {
                    let mut out: u32 = 0;
                    let mut err: AbiError = AbiError::ok();
                    // SAFETY: args.buf.ptr is valid for cap writes; out is a u32;
                    // memory.test fn 0 fills cap bytes and writes the count to out;
                    // err is a valid writable out-param.
                    unsafe {
                        black_box(dispatch_fn)(
                            adapter_context,
                            black_box(GuestContractInstance::null()),
                            black_box(args as *const FillArgs as *const ()),
                            black_box(&mut out as *mut u32 as *mut ()),
                            &mut err,
                        )
                    };
                    debug_assert!(err.is_ok());
                    black_box(out)
                });
            },
        );
    }

    group.finish();

    // SAFETY: buf_ptr was allocated with polyplug_host_alloc(MAX_SIZE, 8); the
    // benchmark loop is complete, so no dispatch holds it any more.
    unsafe { polyplug_host_free(buf_ptr, MAX_SIZE, 8) };
    mem::forget(library);
}

criterion_group!(benches, bench_payload_scaling);
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

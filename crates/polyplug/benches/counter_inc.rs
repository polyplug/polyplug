#![allow(clippy::expect_used)]

// THIS IS A BENCHMARK FILE — do not add #[test] functions here
// Run with: cargo bench -p polyplug --bench counter_inc
//
// ─── What this measures ──────────────────────────────────────────────────────
//
// The "count to 1,000,000" stress test. Each arm runs the *same* logical loop —
// `for _ in 0..1_000_000 { counter = inc(counter) }` — but reaches `inc` through
// a different mechanism, so the per-iteration delta is the cost of that
// mechanism and nothing else (the loop, the counter, and the black_box fences
// are identical across arms).
//
//   1. native/inline_never  — `inc(u32) -> u32`, a plain Rust call the optimizer
//                              is forbidden to inline (#[inline(never)]). This is
//                              the theoretical floor: a direct, statically-linked
//                              function call with no ABI boundary.
//   2. native/abi_marshalled — an `extern "C" fn(GuestContractInstance, *const (),
//                              *mut ())` called through a function pointer, still
//                              statically linked (no dynamic library). Mirrors
//                              the current native dispatch convention (leading
//                              instance handle), isolating the cost of polyplug's
//                              instance + pointer-in / pointer-out marshalling
//                              convention from the cost of crossing a .so border.
//   3. ffi/by_value         — `inc(u32) -> u32` resolved by symbol name via
//                              `dlsym` from a dynamically-loaded cdylib and called
//                              by value. This is the unsafe, hand-rolled FFI a user
//                              would write *without* polyplug: raw, no validation,
//                              no lifecycle, no safety.
//   4. polyplug/dispatch    — the product. The same cdylib is loaded through the
//                              runtime, the `test.add` contract is resolved once,
//                              and its function is dispatched 1,000,000 times. This
//                              is the realistic hot path: resolve a contract once,
//                              then call it in a loop.
//   5. polyplug/dispatch_cpp — identical to arm 4, but the loaded plugin was
//                              authored in C++ (`libtest_plugin_cpp.so`) instead
//                              of Rust. Same `test.add` contract, same native
//                              dispatch path — proving native dispatch is
//                              compiler/language-agnostic. This is the native
//                              row that anchors the cross-language dispatch matrix
//                              in README.md (VM rows live in the per-loader
//                              `dispatch_benchmark.rs` benches).
//
// Arms 3 and 4 load the *identical* `libtest_plugin.so`. `polyplug_bench_inc`
// (arm 3) and the registered `add` (arm 4) both compute `x + 1`, so the only
// difference between "raw FFI" and "polyplug" is the safety machinery — which is
// exactly the comparison we want the numbers to make. Arm 5 swaps the Rust
// plugin for a C++ one to show the dispatch cost does not depend on the plugin's
// source language.
//
// Honesty notes:
//   * A direct call (arm 1) is genuinely cheaper than any FFI/dynamic-dispatch
//     call — it has no ABI boundary to cross. We do not claim parity with it;
//     it is the floor reference.
//   * The fair, like-for-like comparison is arm 4 vs arm 3: polyplug's safe
//     dispatch vs the raw FFI a user would otherwise hand-write. polyplug's
//     per-call overhead over raw FFI is the price of the safety it provides.
//   * The cost of *finding + resolving* a contract on every call (the pessimal
//     "never cache the handle" pattern) is measured separately in
//     `contract_dispatch.rs::bench_dispatch_cross_plugin`; nobody re-resolves
//     inside a tight loop, so this bench resolves once, as a real host would.

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
use polyplug_abi::in_process::reject_in_process_bundle;
use polyplug_utils::BundleId;

const TEST_PLUGIN_SO: &str = env!("TEST_PLUGIN_SO");
const TEST_PLUGIN_CPP_SO: &str = env!("TEST_PLUGIN_CPP_SO");

/// Number of increments per measured sample — "count to one million".
const COUNT: u64 = 1_000_000;

/// Arguments to the registered `add` function (test.add fn 0).
#[repr(C)]
struct AddArgs {
    a: u32,
    b: u32,
}

// ─── Arm 1 — plain Rust call, no ABI boundary ────────────────────────────────

/// The floor: a direct call the optimizer may not inline.
#[inline(never)]
fn inc_native(x: u32) -> u32 {
    x.wrapping_add(1)
}

// ─── Arm 2 — polyplug's ABI shape, statically linked ─────────────────────────

/// Same instance + pointer-in / pointer-out convention as a dispatched contract
/// function, but a normal statically-linked Rust fn — no dynamic library, no
/// registry. The leading `GuestContractInstance` mirrors the native dispatch ABI
/// (`fn(GuestContractInstance, *const (), *mut (), *mut AbiError)`); this stateless
/// arm ignores it, exactly as the stateless fixture does.
///
/// # Safety
/// `args` must point to a valid `u32`; `out` must point to a writable `u32`;
/// `out_err` must be non-null and writable.
#[inline(never)]
extern "C" fn inc_abi(
    _adapter_context: *mut c_void,
    _instance: GuestContractInstance,
    args: *const (),
    out: *mut (),
    out_err: *mut AbiError,
) {
    // SAFETY: the bench passes `&u32` as `args`; it is non-null and aligned.
    let x: u32 = unsafe { *(args as *const u32) };
    // SAFETY: the bench passes `&mut u32` as `out`; it is non-null and aligned.
    unsafe { ptr::write(out as *mut u32, x.wrapping_add(1)) };
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
    }
}

// ─── Interface capture for the polyplug-dispatch arm ─────────────────────────

thread_local! {
    /// The interface pointer captured from `polyplug_init`'s register callback.
    static CAPTURED_INTERFACE: Cell<*const GuestContractInterface> =
        const { Cell::new(ptr::null()) };
}

/// Register callback that captures the interface pointer. The bench only ever
/// resolves and dispatches the captured interface; the registry is not needed.
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

// ─── Unused HostApi stubs ────────────────────────────────────────────────────
//
// `polyplug_init` only calls `register_guest_contract`, but `HostApi`'s fields
// are non-nullable `extern "C"` function pointers, so every slot needs a valid
// callee. These are never invoked by this bench.

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

/// Load the cdylib at `so_path`, run `polyplug_init`, and return the dispatch
/// function for `test.add` fn 0 (`add(a, b) -> u32`) plus the kept-alive library.
///
/// Both the Rust (`libtest_plugin.so`) and C++ (`libtest_plugin_cpp.so`) fixtures
/// register the identical `test.add` contract with the same native-dispatch
/// signature, so the same loader works for either.
fn load_dispatch_fn(
    so_path: &str,
) -> (
    Library,
    unsafe extern "C" fn(*mut c_void, GuestContractInstance, *const (), *mut (), *mut AbiError),
    *mut c_void,
) {
    // SAFETY: so_path is a valid cdylib built by tests/fixtures/build_all.sh.
    let library: Library = unsafe { Library::new(so_path).expect("load test plugin") };

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
    // SAFETY: host and ctx live for the call; init_fn registers test.add, which
    // capture_register_callback records into CAPTURED_INTERFACE.
    let result: AbiError =
        unsafe { init_fn(&host as *const HostApi, &ctx as *const BundleInitContext) };
    assert!(result.is_ok(), "polyplug_init failed");

    let interface_ptr: *const GuestContractInterface =
        CAPTURED_INTERFACE.with(|cell: &Cell<*const GuestContractInterface>| cell.get());
    assert!(!interface_ptr.is_null(), "interface not captured");
    // SAFETY: the interface is 'static within the loaded library, kept alive by
    // returning `library` to the caller.
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };
    assert!(
        interface.dispatch_type == DispatchType::Native,
        "expected native dispatch"
    );
    // SAFETY: fn 0 is in range (function_count == 1 for test.add).
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

// ─── Benchmarks ──────────────────────────────────────────────────────────────

fn bench_counter_inc(c: &mut Criterion) {
    let mut group: BenchmarkGroup<'_, WallTime> = c.benchmark_group("counter_inc_1m");
    // Report throughput per individual increment (elements/sec), not per 1M loop.
    group.throughput(Throughput::Elements(COUNT));

    // ── Arm 1: direct, inline-never Rust call (floor) ──────────────────────────
    group.bench_function(BenchmarkId::new("native", "inline_never"), |b| {
        b.iter(|| {
            let mut counter: u32 = 0;
            for _ in 0..COUNT {
                counter = inc_native(black_box(counter));
            }
            black_box(counter)
        });
    });

    // ── Arm 2: ABI-shaped call (ptr-in/ptr-out), statically linked ─────────────
    let abi_fn: unsafe extern "C" fn(
        *mut c_void,
        GuestContractInstance,
        *const (),
        *mut (),
        *mut AbiError,
    ) = inc_abi;
    group.bench_function(BenchmarkId::new("native", "abi_marshalled"), |b| {
        b.iter(|| {
            let mut counter: u32 = 0;
            for _ in 0..COUNT {
                let mut out: u32 = 0;
                let mut err: AbiError = AbiError::ok();
                // SAFETY: &counter is a valid u32 in, &mut out is a valid u32 out;
                // the instance handle is ignored by this stateless arm; err is writable.
                unsafe {
                    black_box(abi_fn)(
                        ptr::null_mut(),
                        black_box(GuestContractInstance::null()),
                        black_box(&counter as *const u32 as *const ()),
                        black_box(&mut out as *mut u32 as *mut ()),
                        &mut err,
                    )
                };
                debug_assert!(err.is_ok());
                counter = out;
            }
            black_box(counter)
        });
    });

    // ── Arm 3: raw FFI by value (dlsym), dynamically loaded .so ────────────────
    {
        // SAFETY: TEST_PLUGIN_SO is a valid cdylib.
        let library: Library =
            unsafe { Library::new(TEST_PLUGIN_SO).expect("load test plugin (ffi)") };
        // SAFETY: polyplug_bench_inc is exported as `extern "C" fn(u32) -> u32`.
        let inc_ffi: Symbol<'_, unsafe extern "C" fn(u32) -> u32> = unsafe {
            library
                .get(b"polyplug_bench_inc\0")
                .expect("polyplug_bench_inc")
        };
        let inc_ffi: unsafe extern "C" fn(u32) -> u32 = *inc_ffi;

        group.bench_function(BenchmarkId::new("ffi", "by_value"), |b| {
            b.iter(|| {
                let mut counter: u32 = 0;
                for _ in 0..COUNT {
                    // SAFETY: inc_ffi is a valid extern "C" fn(u32) -> u32.
                    counter = unsafe { black_box(inc_ffi)(black_box(counter)) };
                }
                black_box(counter)
            });
        });

        mem::forget(library);
    }

    // ── Arm 4: polyplug resolved dispatch, dynamically loaded Rust .so ─────────
    {
        let (library, dispatch_fn, adapter_context): (
            Library,
            unsafe extern "C" fn(
                *mut c_void,
                GuestContractInstance,
                *const (),
                *mut (),
                *mut AbiError,
            ),
            *mut c_void,
        ) = load_dispatch_fn(TEST_PLUGIN_SO);

        group.bench_function(BenchmarkId::new("polyplug", "dispatch"), |b| {
            b.iter(|| {
                let mut counter: u32 = 0;
                for _ in 0..COUNT {
                    let args: AddArgs = AddArgs { a: counter, b: 1 };
                    let mut out: u32 = 0;
                    let mut err: AbiError = AbiError::ok();
                    // SAFETY: args is a valid AddArgs, out is a valid u32; the
                    // registered `add` reads (a, b) and writes the u32 sum. The
                    // stateless contract ignores the null instance handle; err is writable.
                    unsafe {
                        black_box(dispatch_fn)(
                            adapter_context,
                            black_box(GuestContractInstance::null()),
                            black_box(&args as *const AddArgs as *const ()),
                            black_box(&mut out as *mut u32 as *mut ()),
                            &mut err,
                        )
                    };
                    debug_assert!(err.is_ok());
                    counter = out;
                }
                black_box(counter)
            });
        });

        mem::forget(library);
    }

    // ── Arm 5: polyplug resolved dispatch, dynamically loaded C++ .so ──────────
    // Same contract, same dispatch path — only the plugin's source language
    // differs. Anchors the native rows of the cross-language dispatch matrix.
    {
        let (library, dispatch_fn, adapter_context): (
            Library,
            unsafe extern "C" fn(
                *mut c_void,
                GuestContractInstance,
                *const (),
                *mut (),
                *mut AbiError,
            ),
            *mut c_void,
        ) = load_dispatch_fn(TEST_PLUGIN_CPP_SO);

        group.bench_function(BenchmarkId::new("polyplug", "dispatch_cpp"), |b| {
            b.iter(|| {
                let mut counter: u32 = 0;
                for _ in 0..COUNT {
                    let args: AddArgs = AddArgs { a: counter, b: 1 };
                    let mut out: u32 = 0;
                    let mut err: AbiError = AbiError::ok();
                    // SAFETY: args is a valid AddArgs, out is a valid u32; the
                    // C++ `test.add` reads (a, b) and writes the u32 sum. The
                    // stateless contract ignores the null instance handle; err is writable.
                    unsafe {
                        black_box(dispatch_fn)(
                            adapter_context,
                            black_box(GuestContractInstance::null()),
                            black_box(&args as *const AddArgs as *const ()),
                            black_box(&mut out as *mut u32 as *mut ()),
                            &mut err,
                        )
                    };
                    debug_assert!(err.is_ok());
                    counter = out;
                }
                black_box(counter)
            });
        });

        mem::forget(library);
    }

    group.finish();
}

criterion_group!(benches, bench_counter_inc);
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

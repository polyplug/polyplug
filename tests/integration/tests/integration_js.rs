//! Integration tests for JsLoader (js-quickjs).

#![allow(clippy::expect_used)]
#![allow(clippy::undocumented_unsafe_blocks)]

use core::sync::atomic::AtomicU64;
use core::sync::atomic::Ordering;
use polyplug::error::LoaderError;
use polyplug::error::RuntimeError;
use polyplug::loader::BundleLoader;
use polyplug::runtime::Runtime;
use polyplug_abi::AbiError;
use polyplug_abi::AbiErrorCode;
use polyplug_abi::Array;
use polyplug_abi::CallArena;
use polyplug_abi::DependencyInfo;
use polyplug_abi::DispatchType;
use polyplug_abi::GuestContractHandle;
use polyplug_abi::GuestContractInstance;
use polyplug_abi::GuestContractInterface;
use polyplug_abi::HostApi;
use polyplug_abi::HostContractInstance;
use polyplug_abi::HostContractInterface;
use polyplug_abi::PluginDescriptor;
use polyplug_js::JsConfig;
use polyplug_js::JsLoader;
use polyplug_utils::BundleId;
use polyplug_utils::guest_contract_id;
use std::sync::Arc;

#[test]
fn js_quickjs_loader_loader_name() {
    let loader: JsLoader = JsLoader::new(JsConfig {});
    assert_eq!(loader.loader_name(), "js-quickjs");
}

#[test]
fn js_quickjs_registered_in_runtime_builder() {
    let result: Result<Arc<polyplug::runtime::Runtime>, RuntimeError> =
        polyplug::runtime::Runtime::builder()
            .loader(JsLoader::new(JsConfig {}))
            .build();
    assert!(
        result.is_ok(),
        "RuntimeBuilder with JsLoader must succeed: {:?}",
        result.err()
    );
}

#[test]
fn js_quickjs_duplicate_loader_name_is_rejected() {
    let result: Result<Arc<polyplug::runtime::Runtime>, RuntimeError> =
        polyplug::runtime::Runtime::builder()
            .loader(JsLoader::new(JsConfig {}))
            .loader(JsLoader::new(JsConfig {}))
            .build();
    assert!(
        matches!(
            result,
            Err(RuntimeError::Loader(LoaderError::DuplicateLoader { .. }))
        ),
        "Duplicate js-quickjs registration must return DuplicateLoader"
    );
}

const JS_PLUGIN: &str = env!("TEST_JS_PLUGIN");

#[repr(C)]
struct AddArgs {
    a: u32,
    b: u32,
}

#[test]
fn js_quickjs_load_bundle_and_call() {
    let rt: Arc<Runtime> = Runtime::builder()
        .loader(JsLoader::new(JsConfig {}))
        .build()
        .expect("failed to build runtime");
    let result: Result<(), RuntimeError> = rt.load_bundle(std::path::Path::new(JS_PLUGIN));
    assert!(
        result.is_ok(),
        "JsLoader::load() failed: {:?}",
        result.err()
    );

    let contract_id: u64 = guest_contract_id("test.add", 1);
    let handle: GuestContractHandle = rt
        .find_guest_contract(contract_id, 0)
        .expect("test.add must be registered after load");
    let vtable_ptr: *const GuestContractInterface = rt
        .resolve_guest_contract(handle)
        .expect("handle must be valid");
    let vtable: &GuestContractInterface = unsafe { &*vtable_ptr };
    assert_eq!(
        vtable.dispatch_type,
        DispatchType::VirtualMachine,
        "JS loader must use VM dispatch"
    );

    let args: AddArgs = AddArgs { a: 3, b: 5 };
    let mut out: u32 = 0_u32;
    let mut result: AbiError = AbiError::ok();
    unsafe {
        (vtable.dispatch.vm.call)(
            vtable.dispatch.vm.loader_data,
            GuestContractInstance::null(),
            0,
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
            core::ptr::null_mut(),
            &mut result as *mut AbiError,
        )
    };
    assert_eq!(
        result.code,
        AbiErrorCode::Ok as u32,
        "add must return AbiErrorCode::Ok"
    );
    assert_eq!(out, 8_u32, "add(3, 5) must equal 8");
}

// ─── Call-arena end-to-end (string-returning JS guest) ────────────────────────

static ARENA_HOST_ALLOC_CALLS: AtomicU64 = AtomicU64::new(0);

unsafe extern "C" fn counting_alloc(_this: *const HostApi, size: usize, align: usize) -> *mut u8 {
    ARENA_HOST_ALLOC_CALLS.fetch_add(1, Ordering::SeqCst);
    let layout: core::alloc::Layout =
        core::alloc::Layout::from_size_align(size, align).expect("valid layout");
    unsafe { std::alloc::alloc(layout) }
}

unsafe extern "C" fn counting_free(_this: *const HostApi, ptr: *mut u8, size: usize, align: usize) {
    let layout: core::alloc::Layout =
        core::alloc::Layout::from_size_align(size, align).expect("valid layout");
    unsafe { std::alloc::dealloc(ptr, layout) }
}

unsafe extern "C" fn arena_stub_register_guest(
    _this: *const HostApi,
    _descriptor: *const PluginDescriptor,
    _interface: *const GuestContractInterface,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
    }
}

unsafe extern "C" fn arena_stub_find(
    _this: *const HostApi,
    _id: u64,
    _ver: u32,
) -> GuestContractHandle {
    GuestContractHandle::null()
}

unsafe extern "C" fn arena_stub_find_all(
    _this: *const HostApi,
    _id: u64,
    _ver: u32,
) -> Array<GuestContractHandle> {
    Array::empty()
}

unsafe extern "C" fn arena_stub_resolve_guest(
    _this: *const HostApi,
    _handle: GuestContractHandle,
) -> *const GuestContractInterface {
    core::ptr::null()
}

unsafe extern "C" fn arena_stub_get_host_contract(
    _this: *const HostApi,
    _id: u64,
    _ver: u32,
) -> HostContractInstance {
    HostContractInstance::null()
}

unsafe extern "C" fn arena_stub_resolve_host_interface(
    _this: *const HostApi,
    _id: u64,
    _ver: u32,
) -> *const HostContractInterface {
    core::ptr::null()
}

unsafe extern "C" fn arena_stub_list_bundles(_this: *const HostApi) -> Array<BundleId> {
    Array::empty()
}

unsafe extern "C" fn arena_stub_get_deps(_this: *const HostApi) -> Array<DependencyInfo> {
    Array::empty()
}

unsafe extern "C" fn arena_stub_load(
    _this: *const HostApi,
    _p: *const u8,
    _l: usize,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
    }
}

unsafe extern "C" fn arena_stub_register_host(
    _this: *const HostApi,
    _interface: *const HostContractInterface,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
    }
}

unsafe extern "C" fn arena_stub_register_loader(
    _this: *const HostApi,
    _loader: *mut core::ffi::c_void,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
    }
}

unsafe extern "C" fn arena_stub_get_last_error(
    _this: *const HostApi,
    _buf: *mut u8,
    _len: usize,
) -> usize {
    0
}

unsafe extern "C" fn arena_stub_get_len(_this: *const HostApi) -> usize {
    0
}

unsafe extern "C" fn arena_stub_call_guest_method(
    _this: *const HostApi,
    _instance: GuestContractInstance,
    _fn_id: u32,
    _args: *const core::ffi::c_void,
    _out: *mut core::ffi::c_void,
    _arena: *mut CallArena,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
    }
}

unsafe extern "C" fn arena_stub_unload_bundle(
    _this: *const HostApi,
    _bundle_id: BundleId,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
    }
}

/// Build a HostApi whose allocator counts calls — only `alloc`/`free` are ever
/// exercised by the arena; the remaining fields are non-null stubs so the struct
/// is a valid HostApi.
fn counting_host() -> HostApi {
    HostApi {
        runtime: core::ptr::null_mut(),
        register_guest_contract: arena_stub_register_guest,
        alloc: counting_alloc,
        free: counting_free,
        find_guest_contract: arena_stub_find,
        find_all_guest_contracts: arena_stub_find_all,
        resolve_guest_contract: arena_stub_resolve_guest,
        get_host_contract: arena_stub_get_host_contract,
        resolve_host_contract_interface: arena_stub_resolve_host_interface,
        list_bundles: arena_stub_list_bundles,
        get_dependencies: arena_stub_get_deps,
        load_bundle: arena_stub_load,
        reload_bundle: arena_stub_load,
        register_host_contract: arena_stub_register_host,
        register_loader: arena_stub_register_loader,
        get_last_error: arena_stub_get_last_error,
        get_error_len: arena_stub_get_len,
        call_guest_method: arena_stub_call_guest_method,
        unload_bundle: arena_stub_unload_bundle,
        log: stub_host_log,
        create_guest_instance: stub_create_guest_instance,
        destroy_guest_instance: stub_destroy_guest_instance,
        reserved: core::ptr::null(),
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct AbiStringView {
    ptr_lo: u32,
    ptr_hi: u32,
    len: u32,
}

impl AbiStringView {
    fn addr(&self) -> usize {
        ((self.ptr_hi as u64) << 32 | self.ptr_lo as u64) as usize
    }

    /// Read the bytes the view points at into an owned String.
    fn read_string(self) -> String {
        if self.len == 0 {
            return String::new();
        }
        let ptr: *const u8 = self.addr() as *const u8;
        // SAFETY: the view points into the call arena, valid until the next reset.
        let slice: &[u8] = unsafe { core::slice::from_raw_parts(ptr, self.len as usize) };
        String::from_utf8_lossy(slice).into_owned()
    }
}

/// Repeatedly call the string-returning `echo` JS function through a real
/// CallArena and assert: values are correct, a view from call N stays readable
/// until call N+1, and the host allocator is hit ~0 times per call after warmup
/// (arenaAlloc serves small strings from the bump region).
#[test]
fn js_quickjs_echo_uses_call_arena() {
    let rt: Arc<Runtime> = Runtime::builder()
        .loader(JsLoader::new(JsConfig {}))
        .build()
        .expect("failed to build runtime");
    rt.load_bundle(std::path::Path::new(JS_PLUGIN))
        .expect("JsLoader::load() failed");

    let contract_id: u64 = guest_contract_id("test.add", 1);
    let handle: GuestContractHandle = rt
        .find_guest_contract(contract_id, 0)
        .expect("test.add must be registered after load");
    let vtable_ptr: *const GuestContractInterface = rt
        .resolve_guest_contract(handle)
        .expect("handle must be valid");
    let vtable: &GuestContractInterface = unsafe { &*vtable_ptr };

    // The host buffer the guest reads its input from must outlive each call.
    let input: &[u8] = b"hello arena";
    let input_view: AbiStringView = AbiStringView {
        ptr_lo: (input.as_ptr() as usize as u32),
        ptr_hi: ((input.as_ptr() as usize >> 32) as u32),
        len: input.len() as u32,
    };

    let host: HostApi = counting_host();
    let mut arena_buf: [u8; 512] = [0; 512];
    let mut arena: CallArena = CallArena::new(&mut arena_buf, &host);

    const ECHO_FN_ID: u32 = 4;
    const ITERATIONS: usize = 10_000;
    const WARMUP: usize = 8;

    let mut prev_view: Option<AbiStringView> = None;

    for i in 0..ITERATIONS {
        // Reset at call start: the previous call's view stays valid right up to
        // here, so verify it before reusing the arena region.
        if let Some(prev) = prev_view.take() {
            assert_eq!(
                prev.read_string(),
                "hello arena",
                "view from the previous call must read correctly until reset (iter {i})"
            );
        }
        arena.reset();

        let calls_before: u64 = ARENA_HOST_ALLOC_CALLS.load(Ordering::SeqCst);

        let mut out: AbiStringView = AbiStringView {
            ptr_lo: 0,
            ptr_hi: 0,
            len: 0,
        };
        let mut err: AbiError = AbiError::ok();
        unsafe {
            (vtable.dispatch.vm.call)(
                vtable.dispatch.vm.loader_data,
                GuestContractInstance::null(),
                ECHO_FN_ID,
                &input_view as *const AbiStringView as *const (),
                &mut out as *mut AbiStringView as *mut (),
                &mut arena as *mut CallArena,
                &mut err as *mut AbiError,
            )
        };
        assert_eq!(
            err.code,
            AbiErrorCode::Ok as u32,
            "echo must return Ok (iter {i})"
        );
        assert_eq!(out.read_string(), "hello arena", "echo value (iter {i})");

        let calls_after: u64 = ARENA_HOST_ALLOC_CALLS.load(Ordering::SeqCst);
        if i >= WARMUP {
            assert_eq!(
                calls_after, calls_before,
                "after warmup, a small-string echo must not hit the host allocator (iter {i})"
            );
        }

        prev_view = Some(out);
    }

    // Free everything the arena owns before it drops.
    arena.reset();

    assert_eq!(
        ARENA_HOST_ALLOC_CALLS.load(Ordering::SeqCst),
        0,
        "the 512-byte arena region serves every 11-byte echo with zero host allocations"
    );
}

/// `HostApi.log` stub for test hosts — drops the record.
unsafe extern "C" fn stub_host_log(
    _this: *const polyplug_abi::HostApi,
    _level: u32,
    _scope: polyplug_abi::StringView,
    _message: polyplug_abi::StringView,
) {
}

unsafe extern "C" fn stub_create_guest_instance(
    _this: *const polyplug_abi::HostApi,
    _interface: *const polyplug_abi::GuestContractInterface,
    _args: *const core::ffi::c_void,
    out_instance: *mut polyplug_abi::GuestContractInstance,
) {
    if !out_instance.is_null() {
        // SAFETY: out_instance is non-null (just checked) and writable per the ABI contract.
        unsafe { out_instance.write(polyplug_abi::GuestContractInstance::null()) };
    }
}

unsafe extern "C" fn stub_destroy_guest_instance(
    _this: *const polyplug_abi::HostApi,
    _interface: *const polyplug_abi::GuestContractInterface,
    _instance: polyplug_abi::GuestContractInstance,
) {
}

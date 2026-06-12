#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use core::sync::atomic::AtomicU64;
use core::sync::atomic::Ordering;

use polyplug::runtime::Runtime;
use polyplug_abi::AbiError;
use polyplug_abi::AbiErrorCode;
use polyplug_abi::Array;
use polyplug_abi::CallArena;
use polyplug_abi::DependencyInfo;
use polyplug_abi::GuestContractHandle;
use polyplug_abi::GuestContractInstance;
use polyplug_abi::GuestContractInterface;
use polyplug_abi::HostApi;
use polyplug_abi::HostContractInstance;
use polyplug_abi::HostContractInterface;
use polyplug_abi::PluginDescriptor;
use polyplug_abi::StringView;
use polyplug_abi::VmLoaderData;
use polyplug_native::NativeLoader;
use polyplug_utils::BundleId;

fn test_add_contract_id() -> u64 {
    polyplug_utils::guest_contract_id("test.add", 1)
}

#[repr(C)]
pub struct AddArgs {
    pub a: u32,
    pub b: u32,
}

#[derive(Debug)]
pub struct ContractError {
    pub code: AbiErrorCode,
    pub message: String,
}

impl ContractError {
    pub fn new(code: AbiErrorCode) -> Self {
        Self {
            code,
            message: String::new(),
        }
    }
}

pub struct TestAddContract {
    vtable: *const GuestContractInterface,
}

impl TestAddContract {
    pub fn create(runtime: &'static Runtime, min_version: u32) -> Option<Self> {
        let handle: GuestContractHandle = runtime
            .find_guest_contract(test_add_contract_id(), min_version)
            .ok()?;
        // `runtime` is `&'static`, so the resolved vtable pointer stays valid for its lifetime.
        let vtable: *const GuestContractInterface = runtime.resolve_guest_contract(handle).ok()?;
        Some(Self { vtable })
    }

    pub fn is_valid(&self) -> bool {
        true
    }

    pub fn reset(&mut self) {}

    #[allow(clippy::absurd_extreme_comparisons)]
    pub fn add(&self, a: u32, b: u32) -> Result<u32, ContractError> {
        let args: AddArgs = AddArgs { a, b };
        // SAFETY: u32 is a primitive type with no invalid bit patterns, so zeroed() is safe.
        let mut out_val: u32 = unsafe { core::mem::zeroed() };
        // SAFETY: args_ptr points to a valid AddArgs and out_ptr to a valid u32.
        let args_ptr: *const () = &args as *const AddArgs as *const ();
        let out_ptr: *mut () = &mut out_val as *mut u32 as *mut ();
        let vtable_ptr: *const GuestContractInterface = self.vtable;
        // SAFETY: vtable_ptr is valid for the duration of the call.
        let err: AbiError = unsafe {
            let vtable: &GuestContractInterface = &*vtable_ptr;
            if 0_u32 >= vtable.dispatch.native.function_count {
                AbiError {
                    code: AbiErrorCode::FunctionNotAvailable as u32,
                    message: polyplug_abi::StringView::null(),
                }
            } else if vtable.dispatch_type != polyplug_abi::DispatchType::Native {
                AbiError {
                    code: polyplug_abi::AbiErrorCode::Generic as u32,
                    message: polyplug_abi::StringView::null(),
                }
            } else {
                let fn_ptr: *const () = *vtable.dispatch.native.functions.add(0_usize);
                let dispatch_fn: unsafe extern "C" fn(
                    GuestContractInstance,
                    *const (),
                    *mut (),
                ) -> AbiError = core::mem::transmute(fn_ptr);
                dispatch_fn(GuestContractInstance::null(), args_ptr, out_ptr)
            }
        };
        if err.code != AbiErrorCode::Ok as u32 {
            return Err(ContractError {
                code: AbiErrorCode::from_u32(err.code),
                message: String::new(),
            });
        }
        Ok(out_val)
    }
}

fn create_static_runtime() -> &'static Runtime {
    let rt: std::sync::Arc<Runtime> = Runtime::builder()
        .loader(NativeLoader::new(polyplug_native::NativeConfig::default()))
        .build()
        .expect("build runtime");
    Box::leak(Box::new(rt))
}

#[test]
fn test_host_caller_factory_method_returns_some_when_plugin_exists() {
    let rt: &'static Runtime = create_static_runtime();

    let plugin_dir: &str = env!("TEST_PLUGIN_DIR");
    rt.load_bundle(std::path::Path::new(plugin_dir))
        .expect("load plugin");

    let caller: Option<TestAddContract> = TestAddContract::create(rt, 0);
    assert!(
        caller.is_some(),
        "create() should return Some when plugin exists"
    );
}

#[test]
fn test_host_caller_factory_method_returns_none_when_plugin_not_found() {
    let rt: &'static Runtime = create_static_runtime();

    let caller: Option<TestAddContract> = TestAddContract::create(rt, 0);
    assert!(
        caller.is_none(),
        "create() should return None when no plugin loaded"
    );
}

#[test]
fn test_host_caller_is_valid_returns_true() {
    let rt: &'static Runtime = create_static_runtime();

    let plugin_dir: &str = env!("TEST_PLUGIN_DIR");
    rt.load_bundle(std::path::Path::new(plugin_dir))
        .expect("load plugin");

    let caller: TestAddContract = TestAddContract::create(rt, 0).expect("caller should exist");

    assert!(caller.is_valid(), "is_valid() should return true");
}

#[test]
fn test_host_caller_method_call_works() {
    let rt: &'static Runtime = create_static_runtime();

    let plugin_dir: &str = env!("TEST_PLUGIN_DIR");
    rt.load_bundle(std::path::Path::new(plugin_dir))
        .expect("load plugin");

    let caller: TestAddContract = TestAddContract::create(rt, 0).expect("caller should exist");

    let result: u32 = caller.add(10, 32).expect("add should succeed");
    assert_eq!(result, 42_u32, "add(10, 32) should return 42");
}

#[test]
fn test_host_caller_reset_is_noop() {
    let rt: &'static Runtime = create_static_runtime();

    let plugin_dir: &str = env!("TEST_PLUGIN_DIR");
    rt.load_bundle(std::path::Path::new(plugin_dir))
        .expect("load plugin");

    let mut caller: TestAddContract = TestAddContract::create(rt, 0).expect("caller should exist");

    caller.reset();
    assert!(
        caller.is_valid(),
        "is_valid() should still return true after reset"
    );

    let result: u32 = caller.add(5, 7).expect("add should work after reset");
    assert_eq!(result, 12_u32, "add(5, 7) should return 12");
}

// ─── Per-caller arena reuse (VM dispatch, &mut self caller) ────────────────────
//
// Mirrors the generated host caller for a `StringView`-returning VM contract: a
// caller owns a boxed 512-byte buffer plus a `CallArena`, resets the arena at
// every call, and passes it to the VM `call`. This proves the generated `&mut
// self` design reuses its buffer — small returns never hit the host allocator
// after warmup, and a view stays valid until the next arena-backed call.

static CALLER_ARENA_ALLOC_CALLS: AtomicU64 = AtomicU64::new(0);

unsafe extern "C" fn caller_counting_alloc(
    _this: *const HostApi,
    size: usize,
    align: usize,
) -> *mut u8 {
    CALLER_ARENA_ALLOC_CALLS.fetch_add(1, Ordering::SeqCst);
    let layout: core::alloc::Layout =
        core::alloc::Layout::from_size_align(size, align).expect("valid layout");
    // SAFETY: every arena overflow request has non-zero size, so the layout is non-zero.
    unsafe { std::alloc::alloc(layout) }
}

unsafe extern "C" fn caller_counting_free(
    _this: *const HostApi,
    ptr: *mut u8,
    size: usize,
    align: usize,
) {
    let layout: core::alloc::Layout =
        core::alloc::Layout::from_size_align(size, align).expect("valid layout");
    // SAFETY: ptr/size/align match the allocation made by caller_counting_alloc.
    unsafe { std::alloc::dealloc(ptr, layout) }
}

unsafe extern "C" fn caller_stub_register_guest(
    _this: *const HostApi,
    _descriptor: *const PluginDescriptor,
    _interface: *const GuestContractInterface,
) -> AbiError {
    AbiError::ok()
}

unsafe extern "C" fn caller_stub_find(
    _this: *const HostApi,
    _id: u64,
    _ver: u32,
) -> GuestContractHandle {
    GuestContractHandle::null()
}

unsafe extern "C" fn caller_stub_find_all(
    _this: *const HostApi,
    _id: u64,
    _ver: u32,
) -> Array<GuestContractHandle> {
    Array::empty()
}

unsafe extern "C" fn caller_stub_resolve_guest(
    _this: *const HostApi,
    _handle: GuestContractHandle,
) -> *const GuestContractInterface {
    core::ptr::null()
}

unsafe extern "C" fn caller_stub_get_host_contract(
    _this: *const HostApi,
    _id: u64,
    _ver: u32,
) -> HostContractInstance {
    HostContractInstance::null()
}

unsafe extern "C" fn caller_stub_resolve_host_interface(
    _this: *const HostApi,
    _id: u64,
    _ver: u32,
) -> *const HostContractInterface {
    core::ptr::null()
}

unsafe extern "C" fn caller_stub_list_bundles(_this: *const HostApi) -> Array<BundleId> {
    Array::empty()
}

unsafe extern "C" fn caller_stub_get_deps(_this: *const HostApi) -> Array<DependencyInfo> {
    Array::empty()
}

unsafe extern "C" fn caller_stub_load(_this: *const HostApi, _p: *const u8, _l: usize) -> AbiError {
    AbiError::ok()
}

unsafe extern "C" fn caller_stub_register_host(
    _this: *const HostApi,
    _interface: *const HostContractInterface,
) -> AbiError {
    AbiError::ok()
}

unsafe extern "C" fn caller_stub_register_loader(
    _this: *const HostApi,
    _loader: *mut core::ffi::c_void,
) -> AbiError {
    AbiError::ok()
}

unsafe extern "C" fn caller_stub_get_last_error(
    _this: *const HostApi,
    _buf: *mut u8,
    _len: usize,
) -> usize {
    0
}

unsafe extern "C" fn caller_stub_get_len(_this: *const HostApi) -> usize {
    0
}

unsafe extern "C" fn caller_stub_call_guest_method(
    _this: *const HostApi,
    _instance: GuestContractInstance,
    _fn_id: u32,
    _args: *const core::ffi::c_void,
    _out: *mut core::ffi::c_void,
    _arena: *mut CallArena,
) -> AbiError {
    AbiError::ok()
}

unsafe extern "C" fn caller_stub_unload_bundle(
    _this: *const HostApi,
    _bundle_id: BundleId,
) -> AbiError {
    AbiError::ok()
}

fn counting_host() -> HostApi {
    HostApi {
        runtime: core::ptr::null_mut(),
        register_guest_contract: caller_stub_register_guest,
        alloc: caller_counting_alloc,
        free: caller_counting_free,
        find_guest_contract: caller_stub_find,
        find_all_guest_contracts: caller_stub_find_all,
        resolve_guest_contract: caller_stub_resolve_guest,
        get_host_contract: caller_stub_get_host_contract,
        resolve_host_contract_interface: caller_stub_resolve_host_interface,
        list_bundles: caller_stub_list_bundles,
        get_dependencies: caller_stub_get_deps,
        load_bundle: caller_stub_load,
        reload_bundle: caller_stub_load,
        register_host_contract: caller_stub_register_host,
        register_loader: caller_stub_register_loader,
        get_last_error: caller_stub_get_last_error,
        get_error_len: caller_stub_get_len,
        call_guest_method: caller_stub_call_guest_method,
        unload_bundle: caller_stub_unload_bundle,
        log: stub_host_log,
        create_guest_instance: stub_create_guest_instance,
        destroy_guest_instance: stub_destroy_guest_instance,
        reserved: core::ptr::null(),
    }
}

/// Fake VM `call`: echoes the 11-byte input string into arena memory and writes
/// the resulting `StringView` to `out`. A null arena would force a host alloc.
unsafe extern "C" fn echo_vm_call(
    _loader_data: VmLoaderData,
    _instance: GuestContractInstance,
    _fn_id: u32,
    args: *const (),
    out: *mut (),
    arena: *mut CallArena,
) -> AbiError {
    if arena.is_null() || args.is_null() || out.is_null() {
        return AbiError {
            code: AbiErrorCode::InvalidPointer as u32,
            message: StringView::null(),
        };
    }
    // SAFETY: args points to a valid StringView per the caller contract.
    let input: &StringView = unsafe { &*(args as *const StringView) };
    // SAFETY: arena is the per-caller CallArena, reset by the caller for this call.
    let dst: *mut u8 = unsafe { (*arena).alloc(input.len, 1) };
    if dst.is_null() {
        return AbiError {
            code: AbiErrorCode::Generic as u32,
            message: StringView::null(),
        };
    }
    // SAFETY: dst owns input.len bytes from the arena; input.ptr is valid for input.len.
    unsafe { core::ptr::copy_nonoverlapping(input.ptr, dst, input.len) };
    // SAFETY: out points to a valid StringView slot per the caller contract.
    unsafe {
        core::ptr::write(
            out as *mut StringView,
            StringView {
                ptr: dst,
                len: input.len,
            },
        );
    }
    AbiError::ok()
}

const ARENA_BUF_LEN: usize = 512;

/// Caller mirroring the generated `&mut self` arena caller for a VM contract.
struct EchoVmCaller {
    vtable: *const GuestContractInterface,
    _arena_buf: Box<[u8; ARENA_BUF_LEN]>,
    arena: CallArena,
}

impl EchoVmCaller {
    fn new(vtable: *const GuestContractInterface, host: *const HostApi) -> Self {
        let mut arena_buf: Box<[u8; ARENA_BUF_LEN]> = Box::new([0u8; ARENA_BUF_LEN]);
        let arena: CallArena = CallArena::new(arena_buf.as_mut_slice(), host);
        Self {
            vtable,
            _arena_buf: arena_buf,
            arena,
        }
    }

    fn echo(&mut self, input: StringView) -> StringView {
        self.arena.reset();
        let mut out: StringView = StringView::null();
        // SAFETY: vtable is valid; args/out point to valid StringView slots; arena is per-caller.
        let err: AbiError = unsafe {
            let vtable: &GuestContractInterface = &*self.vtable;
            (vtable.dispatch.vm.call)(
                vtable.dispatch.vm.loader_data,
                GuestContractInstance::null(),
                0_u32,
                &input as *const StringView as *const (),
                &mut out as *mut StringView as *mut (),
                &mut self.arena as *mut CallArena,
            )
        };
        assert_eq!(err.code, AbiErrorCode::Ok as u32, "echo must return Ok");
        out
    }
}

impl Drop for EchoVmCaller {
    fn drop(&mut self) {
        self.arena.reset();
    }
}

#[test]
fn test_host_caller_arena_reuses_buffer_across_calls() {
    use polyplug_abi::DispatchMechanisms;
    use polyplug_abi::DispatchType;
    use polyplug_abi::Version;
    use polyplug_abi::VmDispatch;

    CALLER_ARENA_ALLOC_CALLS.store(0, Ordering::SeqCst);

    let interface: GuestContractInterface = GuestContractInterface {
        contract_id: polyplug_utils::GuestContractId::from_u64(0),
        contract_version: Version {
            major: 1,
            minor: 0,
            patch: 0,
        },
        dispatch_type: DispatchType::VirtualMachine,
        create_instance: vm_noop_create,
        destroy_instance: vm_noop_destroy,
        dispatch: DispatchMechanisms {
            vm: VmDispatch {
                call: echo_vm_call,
                loader_data: VmLoaderData {
                    data: core::ptr::null_mut(),
                },
            },
        },
    };

    let host: HostApi = counting_host();
    let mut caller: EchoVmCaller = EchoVmCaller::new(
        &interface as *const GuestContractInterface,
        &host as *const HostApi,
    );

    let input_bytes: &[u8] = b"hello arena";
    let input: StringView = StringView {
        ptr: input_bytes.as_ptr(),
        len: input_bytes.len(),
    };

    const ITERATIONS: usize = 10_000;
    const WARMUP: usize = 4;

    let mut prev: Option<StringView> = None;
    for i in 0..ITERATIONS {
        // The previous call's view stays valid until this call resets the arena.
        if let Some(p) = prev.take() {
            // SAFETY: p borrows arena memory valid until the next echo() reset.
            let s: &[u8] = unsafe { core::slice::from_raw_parts(p.ptr, p.len) };
            assert_eq!(
                s, input_bytes,
                "previous view must read correctly (iter {i})"
            );
        }

        let before: u64 = CALLER_ARENA_ALLOC_CALLS.load(Ordering::SeqCst);
        let out: StringView = caller.echo(input);
        let after: u64 = CALLER_ARENA_ALLOC_CALLS.load(Ordering::SeqCst);

        // SAFETY: out borrows arena memory valid until the next echo() reset.
        let out_slice: &[u8] = unsafe { core::slice::from_raw_parts(out.ptr, out.len) };
        assert_eq!(out_slice, input_bytes, "echo value (iter {i})");

        if i >= WARMUP {
            assert_eq!(
                before, after,
                "after warmup, a small echo must not hit the host allocator (iter {i})"
            );
        }
        prev = Some(out);
    }

    drop(caller);
    assert_eq!(
        CALLER_ARENA_ALLOC_CALLS.load(Ordering::SeqCst),
        0,
        "the 512-byte arena serves every 11-byte echo with zero host allocations"
    );
}

unsafe extern "C" fn vm_noop_create(
    _host: *const HostApi,
    _args: *const (),
) -> GuestContractInstance {
    GuestContractInstance::null()
}

unsafe extern "C" fn vm_noop_destroy(_host: *const HostApi, _instance: GuestContractInstance) {}

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
) -> polyplug_abi::GuestContractInstance {
    polyplug_abi::GuestContractInstance::null()
}

unsafe extern "C" fn stub_destroy_guest_instance(
    _this: *const polyplug_abi::HostApi,
    _interface: *const polyplug_abi::GuestContractInterface,
    _instance: polyplug_abi::GuestContractInstance,
) {
}

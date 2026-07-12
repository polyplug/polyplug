#![allow(clippy::expect_used)]

use core::cell::Ref;
use core::cell::RefCell;
use core::ffi::c_void;
use core::mem;
use core::ptr;
use core::slice;
use core::str;
use std::sync::Arc;
use std::thread;

use libloading::{Library, Symbol};
use polyplug::runtime_store::RuntimeStore;
use polyplug_abi::ffi::polyplug_host_alloc;
use polyplug_abi::ffi::polyplug_host_free;

use polyplug_abi::{
    AbiError, AbiErrorCode, Array, Buffer, BundleInitContext, DependencyInfo, GuestContractHandle,
    GuestContractInstance, GuestContractInterface, HostApi, HostContractInstance,
    HostContractInterface, PluginDescriptor, StringView,
};
use polyplug_utils::{BundleId, GuestContractId};

const MEMORY_PLUGIN_SO: &str = env!("MEMORY_PLUGIN_SO");

thread_local! {
    static FFI_REGISTRY: RefCell<RuntimeStore> = RefCell::new(RuntimeStore::new());
}

#[repr(C)]
struct FillArgs {
    buf: Buffer,
    fill_byte: u8,
}

unsafe extern "C" fn registry_register_callback(
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
                    code: AbiErrorCode::InvalidPointer as u32,
                    message: StringView::null(),
                })
            };
        }
        return;
    }

    // SAFETY: descriptor and interface are valid for this call (ABI contract).
    let desc: &PluginDescriptor = unsafe { &*descriptor };
    // SAFETY: interface is valid for this call (ABI contract).
    let iface: &GuestContractInterface = unsafe { &*interface };

    // SAFETY: desc.contract_name originates from a test plugin with static UTF-8.
    let contract_name: &str = unsafe {
        let bytes: &[u8] = slice::from_raw_parts(desc.contract_name.ptr, desc.contract_name.len);
        str::from_utf8_unchecked(bytes)
    };

    let result: Result<GuestContractHandle, _> = FFI_REGISTRY.with(|reg_cell| {
        let registry: Ref<'_, RuntimeStore> = reg_cell.borrow();
        // SAFETY: interface pointer is 'static -- extracted from a loaded library that outlives registry.
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
        Ok(_) => AbiError {
            code: AbiErrorCode::Ok as u32,
            message: StringView::null(),
        },
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

/// No-op alloc callback.
unsafe extern "C" fn noop_alloc(_this: *const HostApi, size: usize, align: usize) -> *mut u8 {
    polyplug_host_alloc(size, align)
}

/// No-op free callback.
unsafe extern "C" fn noop_free(_this: *const HostApi, ptr: *mut u8, size: usize, align: usize) {
    // SAFETY: polyplug_host_free is a safe wrapper around the system allocator.
    unsafe { polyplug_host_free(ptr, size, align) }
}

/// No-op find_guest_contract callback.
unsafe extern "C" fn noop_find_guest_contract(
    _this: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> GuestContractHandle {
    GuestContractHandle::null()
}

/// No-op find_all_by_contract callback.
unsafe extern "C" fn noop_find_all_guest_contracts(
    _this: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> Array<GuestContractHandle> {
    Array::empty()
}

/// No-op resolve_guest_contract callback.
unsafe extern "C" fn noop_resolve_guest_contract(
    _this: *const HostApi,
    _handle: GuestContractHandle,
) -> *const GuestContractInterface {
    ptr::null()
}

/// No-op get_host_contract callback.
unsafe extern "C" fn noop_get_host_contract(
    _this: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> HostContractInstance {
    HostContractInstance::null()
}

/// No-op list_bundles callback.
unsafe extern "C" fn noop_list_bundles(_this: *const HostApi) -> Array<BundleId> {
    Array::empty()
}

/// No-op get_dependencies callback.
unsafe extern "C" fn noop_get_dependencies(_this: *const HostApi) -> Array<DependencyInfo> {
    Array::empty()
}

/// No-op resolve_host_contract_interface callback.
unsafe extern "C" fn noop_resolve_host_contract_interface(
    _this: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> *const HostContractInterface {
    ptr::null()
}

/// No-op load_bundle callback.
unsafe extern "C" fn noop_load_bundle(
    _this: *const HostApi,
    _path: *const u8,
    _path_len: usize,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
    }
}

/// No-op reload_bundle callback.
unsafe extern "C" fn noop_reload_bundle(
    _this: *const HostApi,
    _path: *const u8,
    _path_len: usize,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
    }
}

/// No-op register_host_contract callback.
unsafe extern "C" fn noop_register_host_contract(
    _this: *const HostApi,
    _interface: *const HostContractInterface,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
    }
}

/// No-op register_loader callback.
unsafe extern "C" fn noop_register_loader(
    _this: *const HostApi,
    _loader_ptr: *mut c_void,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
    }
}

/// No-op get_last_error callback.
unsafe extern "C" fn noop_get_last_error(
    _this: *const HostApi,
    _buf: *mut u8,
    _buf_len: usize,
) -> usize {
    0
}

/// No-op get_error_len callback.
unsafe extern "C" fn noop_get_error_len(_this: *const HostApi) -> usize {
    0
}

unsafe extern "C" fn noop_unload_bundle(
    _this: *const HostApi,
    _bundle_id: BundleId,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
    }
}

fn load_memory_plugin() -> Library {
    // SAFETY: MEMORY_PLUGIN_SO is a compiled cdylib built by build.rs.
    unsafe { Library::new(MEMORY_PLUGIN_SO).expect("failed to load memory_plugin .so") }
}

fn init_memory_plugin_interface(library: &Library) -> *const GuestContractInterface {
    FFI_REGISTRY.with(|cell| {
        *cell.borrow_mut() = RuntimeStore::new();
    });

    // SAFETY: polyplug_init matches the expected ABI signature (2-arg).
    let init_fn: Symbol<
        '_,
        unsafe extern "C" fn(*const HostApi, *const BundleInitContext) -> AbiError,
    > = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found")
    };

    let host_interface: HostApi = HostApi {
        runtime: ptr::null_mut(),
        register_guest_contract: registry_register_callback,
        alloc: noop_alloc,
        free: noop_free,
        find_guest_contract: noop_find_guest_contract,
        find_all_guest_contracts: noop_find_all_guest_contracts,
        resolve_guest_contract: noop_resolve_guest_contract,
        get_host_contract: noop_get_host_contract,
        resolve_host_contract_interface: noop_resolve_host_contract_interface,
        list_bundles: noop_list_bundles,
        get_dependencies: noop_get_dependencies,
        load_bundle: noop_load_bundle,
        reload_bundle: noop_reload_bundle,
        register_host_contract: noop_register_host_contract,
        register_loader: noop_register_loader,
        get_last_error: noop_get_last_error,
        get_error_len: noop_get_error_len,
        unload_bundle: noop_unload_bundle,
        log: stub_host_log,
        create_guest_instance: stub_create_guest_instance,
        destroy_guest_instance: stub_destroy_guest_instance,
        registry_revision: stub_registry_revision,
        reserved: ptr::null(),
    };

    let ctx: BundleInitContext = BundleInitContext {
        bundle_path: StringView::null(),
        bundle_id: 0,
    };

    // SAFETY: init_fn is valid; host_interface and ctx live for the duration of this call.
    let init_result: AbiError = unsafe {
        init_fn(
            &host_interface as *const HostApi,
            &ctx as *const BundleInitContext,
        )
    };
    assert_eq!(
        init_result.code,
        AbiErrorCode::Ok as u32,
        "polyplug_init must succeed"
    );

    let contract_id: GuestContractId = GuestContractId::new("memory.test", 1);
    let handle: GuestContractHandle = FFI_REGISTRY.with(|cell| {
        cell.borrow()
            .find(contract_id, 0)
            .expect("memory.test must be registered")
    });

    FFI_REGISTRY.with(|cell| {
        cell.borrow()
            .resolve_guest_contract(handle)
            .expect("interface must be resolvable")
    })
}

#[test]
fn test_misaligned_buffer_fill() {
    let library: Library = load_memory_plugin();
    let interface_ptr: *const GuestContractInterface = init_memory_plugin_interface(&library);

    // SAFETY: interface_ptr is valid (plugin is loaded, library not yet dropped).
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };

    const BUFFER_SIZE: usize = 64;
    let base_ptr: *mut u8 = polyplug_host_alloc(BUFFER_SIZE, 8);
    assert!(!base_ptr.is_null(), "host_alloc must succeed");

    // SAFETY: base_ptr is valid for BUFFER_SIZE bytes; zero-initialize for deterministic checks.
    unsafe { ptr::write_bytes(base_ptr, 0_u8, BUFFER_SIZE) };

    // SAFETY: base_ptr is valid for BUFFER_SIZE bytes; offset by 1 keeps within allocation.
    let misaligned_ptr: *mut u8 = unsafe { base_ptr.add(1) };
    let cap: usize = BUFFER_SIZE - 1;

    let args: FillArgs = FillArgs {
        buf: Buffer {
            ptr: misaligned_ptr,
            len: 0,
            cap,
        },
        fill_byte: 0x5A_u8,
    };
    let mut out: u32 = 0_u32;

    // SAFETY: fn_ptr is function 0 in the interface (memory_fill_preallocated_buffer).
    let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(0) };
    let dispatch_fn: unsafe extern "C" fn(
        *mut c_void,
        GuestContractInstance,
        *const (),
        *mut (),
        *mut AbiError,
    ) = {
        // SAFETY: `fn_ptr` names function 0 and therefore has this exact generated ABI.
        unsafe { mem::transmute(fn_ptr) }
    };

    let mut call_result: AbiError = AbiError::ok();
    // SAFETY: args is a valid FillArgs, out is a valid u32 location; &mut call_result is valid.
    unsafe {
        dispatch_fn(
            interface.adapter_context,
            GuestContractInstance::null(),
            &args as *const FillArgs as *const (),
            &mut out as *mut u32 as *mut (),
            &mut call_result as *mut AbiError,
        )
    };

    assert_ne!(
        call_result.code,
        AbiErrorCode::Ok as u32,
        "misaligned buffer must error"
    );
    assert_eq!(out, 0_u32, "out must remain zero on error");

    // SAFETY: base_ptr was allocated by polyplug_host_alloc with matching size and alignment.
    unsafe { polyplug_host_free(base_ptr, BUFFER_SIZE, 8) };
    mem::forget(library);
}

#[test]
fn test_stringview_cross_thread_echo() {
    let library: Library = load_memory_plugin();
    let interface_ptr: *const GuestContractInterface = init_memory_plugin_interface(&library);

    // SAFETY: interface_ptr is valid (plugin is loaded, library not yet dropped).
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };

    let bytes: Arc<Vec<u8>> = Arc::new(b"cross-thread string".to_vec());
    let len: usize = bytes.len();
    let ptr_addr: usize = bytes.as_ptr() as usize;

    thread::scope(|scope| {
        let bytes_in_thread: Arc<Vec<u8>> = Arc::clone(&bytes);
        scope.spawn(move || {
            let ptr: *const u8 = ptr_addr as *const u8;
            let input_sv: StringView = StringView { ptr, len };
            let mut out_sv: StringView = StringView::null();

            // SAFETY: fn_ptr is function 2 in the interface (memory_echo_string_view).
            let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(2) };
            let dispatch_fn: unsafe extern "C" fn(
                *mut c_void,
                GuestContractInstance,
                *const (),
                *mut (),
                *mut AbiError,
            ) = {
                // SAFETY: `fn_ptr` names function 2 and therefore has this exact generated ABI.
                unsafe { mem::transmute(fn_ptr) }
            };

            let mut call_result: AbiError = AbiError::ok();
            // SAFETY: input_sv is valid for the call; out_sv is a valid output location; &mut is valid.
            unsafe {
                dispatch_fn(
                    interface.adapter_context,
                    GuestContractInstance::null(),
                    &input_sv as *const StringView as *const (),
                    &mut out_sv as *mut StringView as *mut (),
                    &mut call_result as *mut AbiError,
                )
            };

            assert_eq!(
                call_result.code,
                AbiErrorCode::Ok as u32,
                "echo must return Ok"
            );
            assert_eq!(out_sv.ptr, input_sv.ptr, "ptr must round-trip");
            assert_eq!(out_sv.len, input_sv.len, "len must round-trip");

            // SAFETY: out_sv.ptr is valid for out_sv.len bytes from bytes_in_thread.
            let echoed: &[u8] = unsafe { slice::from_raw_parts(out_sv.ptr, out_sv.len) };
            assert_eq!(
                echoed,
                bytes_in_thread.as_slice(),
                "echoed bytes must match input"
            );
        });
    });

    mem::forget(library);
}

#[test]
fn test_buffer_cap_less_than_len() {
    let library: Library = load_memory_plugin();
    let interface_ptr: *const GuestContractInterface = init_memory_plugin_interface(&library);

    // SAFETY: interface_ptr is valid (plugin is loaded, library not yet dropped).
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };

    const BUFFER_SIZE: usize = 32;
    let ptr: *mut u8 = polyplug_host_alloc(BUFFER_SIZE, 8);
    assert!(!ptr.is_null(), "host_alloc must succeed");

    // SAFETY: ptr is valid for BUFFER_SIZE bytes.
    unsafe { ptr::write_bytes(ptr, 0x11_u8, BUFFER_SIZE) };

    let cap: usize = 16;
    let len: usize = 24;
    let args: FillArgs = FillArgs {
        buf: Buffer { ptr, len, cap },
        fill_byte: 0xAA_u8,
    };
    let mut out: u32 = 0_u32;

    // SAFETY: fn_ptr is function 0 in the interface (memory_fill_preallocated_buffer).
    let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(0) };
    let dispatch_fn: unsafe extern "C" fn(
        *mut c_void,
        GuestContractInstance,
        *const (),
        *mut (),
        *mut AbiError,
    ) = {
        // SAFETY: `fn_ptr` names function 0 and therefore has this exact generated ABI.
        unsafe { mem::transmute(fn_ptr) }
    };

    let mut call_result: AbiError = AbiError::ok();
    // SAFETY: args is a valid FillArgs, out is a valid u32 location; &mut call_result is valid.
    unsafe {
        dispatch_fn(
            interface.adapter_context,
            GuestContractInstance::null(),
            &args as *const FillArgs as *const (),
            &mut out as *mut u32 as *mut (),
            &mut call_result as *mut AbiError,
        )
    };

    assert_ne!(
        call_result.code,
        AbiErrorCode::Ok as u32,
        "cap < len must error"
    );
    assert_eq!(out, 0_u32, "out must remain zero on error");

    // SAFETY: ptr is valid for BUFFER_SIZE bytes.
    let filled: &[u8] = unsafe { slice::from_raw_parts(ptr, BUFFER_SIZE) };
    assert!(
        filled.iter().all(|&b| b == 0x11_u8),
        "buffer must remain sentinel on error"
    );

    // SAFETY: ptr was allocated by polyplug_host_alloc with matching size and alignment.
    unsafe { polyplug_host_free(ptr, BUFFER_SIZE, 8) };
    mem::forget(library);
}

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

#![allow(clippy::expect_used)]

use core::cell::RefCell;
use std::sync::Arc;

use polyplug::registry::plugin_registry::PluginRegistry;
use polyplug_abi::{
    AbiErrorCode, AbiError, Buffer, HostInterface, GuestContractInterface, GuestContractInstance,
    PluginContext, PluginDescriptor, PluginHandle, StringView, Version, DispatchMechanisms,
    DispatchType, NativeDispatch,
};
use polyplug_abi::ffi::polyplug_host_alloc;
use polyplug_abi::ffi::polyplug_host_free;
use polyplug_utils::{guest_contract_id, bundle_id, GuestContractId, BundleId};

const MEMORY_PLUGIN_SO: &str = env!("MEMORY_PLUGIN_SO");

std::thread_local! {
    static FFI_REGISTRY: RefCell<PluginRegistry> = RefCell::new(PluginRegistry::new());
}

#[repr(C)]
struct FillArgs {
    buf: Buffer,
    fill_byte: u8,
}

unsafe extern "C" fn registry_register_callback(
    _this: *const HostInterface,
    descriptor: *const PluginDescriptor,
    interface: *const GuestContractInterface,
) -> AbiError {
    if descriptor.is_null() || interface.is_null() {
        return AbiError {
            code: AbiErrorCode::InvalidPointer,
            message: StringView::null(),
        };
    }

    // SAFETY: descriptor and interface are valid for this call (ABI contract).
    let desc: &PluginDescriptor = unsafe { &*descriptor };
    // SAFETY: interface is valid for this call (ABI contract).
    let iface: &GuestContractInterface = unsafe { &*interface };

    // SAFETY: desc.contract_name originates from a test plugin with static UTF-8.
    let contract_name: &str = unsafe {
        let bytes: &[u8] =
            core::slice::from_raw_parts(desc.contract_name.ptr, desc.contract_name.len);
        core::str::from_utf8_unchecked(bytes)
    };

    let result: Result<PluginHandle, _> = FFI_REGISTRY.with(|reg_cell| {
        let registry: core::cell::Ref<'_, PluginRegistry> = reg_cell.borrow();
        // SAFETY: interface pointer is 'static -- extracted from a loaded library that outlives registry.
        unsafe { registry.register(*desc, interface, contract_name.to_owned(), BundleId::from_u64(iface.contract_id.id())) }
    });

    match result {
        Ok(_) => AbiError {
            code: AbiErrorCode::Ok,
            message: StringView::null(),
        },
        Err(_) => AbiError {
            code: AbiErrorCode::Generic,
            message: StringView::null(),
        },
    }
}

/// No-op alloc callback.
unsafe extern "C" fn noop_alloc(
    _this: *const HostInterface,
    size: usize,
    align: usize,
) -> *mut u8 {
    polyplug_host_alloc(size, align)
}

/// No-op free callback.
unsafe extern "C" fn noop_free(
    _this: *const HostInterface,
    ptr: *mut u8,
    size: usize,
    align: usize,
) {
    // SAFETY: polyplug_host_free is a safe wrapper around the system allocator.
    unsafe { polyplug_host_free(ptr, size, align) }
}

/// No-op find_by_contract callback.
unsafe extern "C" fn noop_find_by_contract(
    _this: *const HostInterface,
    _contract_id: u64,
    _min_version: u32,
) -> PluginHandle {
    PluginHandle::null()
}

/// No-op find_all_by_contract callback.
unsafe extern "C" fn noop_find_all_by_contract(
    _this: *const HostInterface,
    _contract_id: u64,
    _min_version: u32,
) -> polyplug_abi::Array<PluginHandle> {
    polyplug_abi::Array::empty()
}

/// No-op resolve_contract callback.
unsafe extern "C" fn noop_resolve_contract(
    _this: *const HostInterface,
    _handle: PluginHandle,
) -> *const GuestContractInterface {
    core::ptr::null()
}

/// No-op call_guest_method callback.
unsafe extern "C" fn noop_call_guest_method(
    _this: *const HostInterface,
    _instance: GuestContractInstance,
    _method_id: u32,
    _args: *const (),
    _out: *mut (),
) -> AbiError {
    AbiError {
        code: AbiErrorCode::Ok,
        message: StringView::null(),
    }
}

/// No-op get_host_contract callback.
unsafe extern "C" fn noop_get_host_contract(
    _this: *const HostInterface,
    _contract_id: u64,
    _min_version: u32,
) -> polyplug_abi::HostContractInstance {
    polyplug_abi::HostContractInstance::null()
}

/// No-op list_bundles callback.
unsafe extern "C" fn noop_list_bundles(
    _this: *const HostInterface,
) -> polyplug_abi::Array<polyplug_utils::BundleId> {
    polyplug_abi::Array::empty()
}

/// No-op get_dependencies callback.
unsafe extern "C" fn noop_get_dependencies(
    _this: *const HostInterface,
) -> polyplug_abi::Array<polyplug_abi::DependencyInfo> {
    polyplug_abi::Array::empty()
}

/// No-op resolve_host_contract_interface callback.
unsafe extern "C" fn noop_resolve_host_contract_interface(
    _this: *const HostInterface,
    _contract_id: u64,
    _min_version: u32,
) -> *const polyplug_abi::HostContractInterface {
    core::ptr::null()
}

fn load_memory_plugin() -> libloading::Library {
    // SAFETY: MEMORY_PLUGIN_SO is a compiled cdylib built by build.rs.
    unsafe { libloading::Library::new(MEMORY_PLUGIN_SO).expect("failed to load memory_plugin .so") }
}

fn init_memory_plugin_interface(library: &libloading::Library) -> *const GuestContractInterface {
    FFI_REGISTRY.with(|cell| {
        *cell.borrow_mut() = PluginRegistry::new();
    });

    // SAFETY: polyplug_init matches the expected ABI signature (2-arg).
    let init_fn: libloading::Symbol<
        '_,
        unsafe extern "C" fn(
            *const HostInterface,
            *const PluginContext,
        ) -> AbiError,
    > = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found")
    };

    let host_interface: HostInterface = HostInterface {
        runtime: core::ptr::null_mut(),
        register_contract: registry_register_callback,
        alloc: noop_alloc,
        free: noop_free,
        find_by_contract: noop_find_by_contract,
        find_all_by_contract: noop_find_all_by_contract,
        resolve_contract: noop_resolve_contract,
        call_guest_method: noop_call_guest_method,
        get_host_contract: noop_get_host_contract,
        resolve_host_contract_interface: noop_resolve_host_contract_interface,
        list_bundles: noop_list_bundles,
        get_dependencies: noop_get_dependencies,
    };

    let ctx: PluginContext = PluginContext {
        bundle_path: StringView::null(),
        bundle_id: 0,
    };

    // SAFETY: init_fn is valid; host_interface and ctx live for the duration of this call.
    let init_result: AbiError = unsafe {
        init_fn(
            &host_interface as *const HostInterface,
            &ctx as *const PluginContext,
        )
    };
    assert_eq!(init_result.code, AbiErrorCode::Ok, "polyplug_init must succeed");

    let contract_id: GuestContractId = GuestContractId::new("memory.test", 1);
    let handle: PluginHandle = FFI_REGISTRY.with(|cell| {
        cell.borrow()
            .find(contract_id, 0)
            .expect("memory.test must be registered")
    });

    FFI_REGISTRY.with(|cell| {
        cell.borrow()
            .resolve(handle)
            .expect("interface must be resolvable")
    })
}

#[test]
fn test_misaligned_buffer_fill() {
    let library: libloading::Library = load_memory_plugin();
    let interface_ptr: *const GuestContractInterface = init_memory_plugin_interface(&library);

    // SAFETY: interface_ptr is valid (plugin is loaded, library not yet dropped).
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };

    const BUFFER_SIZE: usize = 64;
    let base_ptr: *mut u8 = polyplug_host_alloc(BUFFER_SIZE, 8);
    assert!(!base_ptr.is_null(), "host_alloc must succeed");

    // SAFETY: base_ptr is valid for BUFFER_SIZE bytes; zero-initialize for deterministic checks.
    unsafe { core::ptr::write_bytes(base_ptr, 0_u8, BUFFER_SIZE) };

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
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        // SAFETY: fn_ptr is cast to the generic dispatch signature.
        unsafe { core::mem::transmute(fn_ptr) };

    // SAFETY: args is a valid FillArgs, out is a valid u32 location.
    let call_result: AbiError = unsafe {
        dispatch_fn(
            &args as *const FillArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };

    assert_ne!(call_result.code, AbiErrorCode::Ok, "misaligned buffer must error");
    assert_eq!(out, 0_u32, "out must remain zero on error");

    // SAFETY: base_ptr was allocated by polyplug_host_alloc with matching size and alignment.
    unsafe { polyplug_host_free(base_ptr, BUFFER_SIZE, 8) };
    core::mem::forget(library);
}

#[test]
fn test_stringview_cross_thread_echo() {
    let library: libloading::Library = load_memory_plugin();
    let interface_ptr: *const GuestContractInterface = init_memory_plugin_interface(&library);

    // SAFETY: interface_ptr is valid (plugin is loaded, library not yet dropped).
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };

    let bytes: Arc<Vec<u8>> = Arc::new(b"cross-thread string".to_vec());
    let len: usize = bytes.len();
    let ptr_addr: usize = bytes.as_ptr() as usize;

    std::thread::scope(|scope| {
        let bytes_in_thread: Arc<Vec<u8>> = Arc::clone(&bytes);
        scope.spawn(move || {
            let ptr: *const u8 = ptr_addr as *const u8;
            let input_sv: StringView = StringView { ptr, len };
            let mut out_sv: StringView = StringView::null();

            // SAFETY: fn_ptr is function 2 in the interface (memory_echo_string_view).
            let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(2) };
            let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
                // SAFETY: fn_ptr is cast to the generic dispatch signature.
                unsafe { core::mem::transmute(fn_ptr) };

            // SAFETY: input_sv is valid for the call; out_sv is a valid output location.
            let call_result: AbiError = unsafe {
                dispatch_fn(
                    &input_sv as *const StringView as *const (),
                    &mut out_sv as *mut StringView as *mut (),
                )
            };

            assert_eq!(call_result.code, AbiErrorCode::Ok, "echo must return ABI_OK");
            assert_eq!(out_sv.ptr, input_sv.ptr, "ptr must round-trip");
            assert_eq!(out_sv.len, input_sv.len, "len must round-trip");

            // SAFETY: out_sv.ptr is valid for out_sv.len bytes from bytes_in_thread.
            let echoed: &[u8] = unsafe { core::slice::from_raw_parts(out_sv.ptr, out_sv.len) };
            assert_eq!(
                echoed,
                bytes_in_thread.as_slice(),
                "echoed bytes must match input"
            );
        });
    });

    core::mem::forget(library);
}

#[test]
fn test_buffer_cap_less_than_len() {
    let library: libloading::Library = load_memory_plugin();
    let interface_ptr: *const GuestContractInterface = init_memory_plugin_interface(&library);

    // SAFETY: interface_ptr is valid (plugin is loaded, library not yet dropped).
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };

    const BUFFER_SIZE: usize = 32;
    let ptr: *mut u8 = polyplug_host_alloc(BUFFER_SIZE, 8);
    assert!(!ptr.is_null(), "host_alloc must succeed");

    // SAFETY: ptr is valid for BUFFER_SIZE bytes.
    unsafe { core::ptr::write_bytes(ptr, 0x11_u8, BUFFER_SIZE) };

    let cap: usize = 16;
    let len: usize = 24;
    let args: FillArgs = FillArgs {
        buf: Buffer { ptr, len, cap },
        fill_byte: 0xAA_u8,
    };
    let mut out: u32 = 0_u32;

    // SAFETY: fn_ptr is function 0 in the interface (memory_fill_preallocated_buffer).
    let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(0) };
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        // SAFETY: fn_ptr is cast to the generic dispatch signature.
        unsafe { core::mem::transmute(fn_ptr) };

    // SAFETY: args is a valid FillArgs, out is a valid u32 location.
    let call_result: AbiError = unsafe {
        dispatch_fn(
            &args as *const FillArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };

    assert_ne!(call_result.code, AbiErrorCode::Ok, "cap < len must error");
    assert_eq!(out, 0_u32, "out must remain zero on error");

    // SAFETY: ptr is valid for BUFFER_SIZE bytes.
    let filled: &[u8] = unsafe { core::slice::from_raw_parts(ptr, BUFFER_SIZE) };
    assert!(
        filled.iter().all(|&b| b == 0x11_u8),
        "buffer must remain sentinel on error"
    );

    // SAFETY: ptr was allocated by polyplug_host_alloc with matching size and alignment.
    unsafe { polyplug_host_free(ptr, BUFFER_SIZE, 8) };
    core::mem::forget(library);
}

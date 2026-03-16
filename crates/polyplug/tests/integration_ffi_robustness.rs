use core::cell::RefCell;
use polyplug_abi::ABI_OK;
use polyplug_abi::AbiError;
use polyplug_abi::Buffer;
use polyplug_abi::POLYPLUG_ABI_VERSION;
use polyplug_abi::PluginContext;
use polyplug_abi::PluginDescriptor;
use polyplug_abi::PluginHandle;
use polyplug_abi::PluginRegistrar;
use polyplug_abi::PluginVTable;
use polyplug_abi::StringView;
use polyplug::allocator::polyplug_host_alloc;
use polyplug::allocator::polyplug_host_free;
use polyplug::registry::Registry;
use std::sync::Arc;

const MEMORY_PLUGIN_SO: &str = env!("MEMORY_PLUGIN_SO");

std::thread_local! {
    static FFI_REGISTRY: RefCell<Registry> = RefCell::new(Registry::new());
}

#[repr(C)]
struct FillArgs {
    buf: Buffer,
    fill_byte: u8,
}

unsafe extern "C" fn registry_register_callback(
    _registrar: *mut PluginRegistrar,
    descriptor: *const PluginDescriptor,
    vtable: *const PluginVTable,
) -> AbiError {
    if descriptor.is_null() || vtable.is_null() {
        return AbiError {
            code: 1_u32,
            message: StringView::null(),
        };
    }

    // SAFETY: descriptor and vtable are valid for this call (ABI contract).
    let desc: &PluginDescriptor = unsafe { &*descriptor };
    // SAFETY: vtable is valid for this call (ABI contract).
    let vt: &PluginVTable = unsafe { &*vtable };

    // SAFETY: desc.contract_name originates from a test plugin with static UTF-8.
    let contract_name: &str = unsafe {
        let bytes: &[u8] =
            core::slice::from_raw_parts(desc.contract_name.ptr, desc.contract_name.len);
        core::str::from_utf8_unchecked(bytes)
    };

    let result: Result<PluginHandle, _> = FFI_REGISTRY.with(|reg_cell| {
        let registry: core::cell::Ref<'_, Registry> = reg_cell.borrow();
        // SAFETY: vtable pointer is 'static — extracted from a loaded library that outlives registry.
        unsafe { registry.register(*desc, vtable, contract_name.to_owned(), vt.contract_id) }
    });

    match result {
        Ok(_) => AbiError {
            code: ABI_OK,
            message: StringView::null(),
        },
        Err(_) => AbiError {
            code: 1_u32,
            message: StringView::null(),
        },
    }
}

fn load_memory_plugin() -> libloading::Library {
    // SAFETY: MEMORY_PLUGIN_SO is a compiled cdylib built by build.rs.
    unsafe { libloading::Library::new(MEMORY_PLUGIN_SO).expect("failed to load memory_plugin .so") }
}
fn init_memory_plugin_vtable(library: &libloading::Library) -> *const PluginVTable {
    FFI_REGISTRY.with(|cell| {
        *cell.borrow_mut() = Registry::new();
    });

    // SAFETY: polyplug_init matches the expected ABI signature.
    let init_fn: libloading::Symbol<
        '_,
        unsafe extern "C" fn(*mut PluginRegistrar, *const PluginContext) -> AbiError,
    > = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found")
    };

    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: registry_register_callback,
        host: core::ptr::null(),
    };

    let ctx: PluginContext = PluginContext {
        bundle_path: StringView::null(),
        host_abi_version: POLYPLUG_ABI_VERSION,
    };

    // SAFETY: init_fn is valid; registrar and ctx live for the duration of this call.
    let init_result: AbiError = unsafe {
        init_fn(
            &mut registrar as *mut PluginRegistrar,
            &ctx as *const PluginContext,
        )
    };
    assert_eq!(init_result.code, ABI_OK, "polyplug_init must succeed");

    let contract_id: u64 = polyplug_abi::contract_id("memory.test", 1);
    let handle: PluginHandle = FFI_REGISTRY.with(|cell| {
        cell.borrow()
            .find(contract_id, 0)
            .expect("memory.test must be registered")
    });

    FFI_REGISTRY.with(|cell| {
        cell.borrow()
            .resolve(handle)
            .expect("vtable must be resolvable")
    })
}

#[test]
fn test_misaligned_buffer_fill() {
    let library: libloading::Library = load_memory_plugin();
    let vtable_ptr: *const PluginVTable = init_memory_plugin_vtable(&library);

    // SAFETY: vtable_ptr is valid (plugin is loaded, library not yet dropped).
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };

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

    // SAFETY: fn_ptr is function 0 in the vtable (memory_fill_preallocated_buffer).
    let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
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

    assert_ne!(call_result.code, ABI_OK, "misaligned buffer must error");
    assert_eq!(out, 0_u32, "out must remain zero on error");

    // SAFETY: base_ptr was allocated by polyplug_host_alloc with matching size and alignment.
    unsafe { polyplug_host_free(base_ptr, BUFFER_SIZE, 8) };
    core::mem::forget(library);
}

#[test]
fn test_stringview_cross_thread_echo() {
    let library: libloading::Library = load_memory_plugin();
    let vtable_ptr: *const PluginVTable = init_memory_plugin_vtable(&library);

    // SAFETY: vtable_ptr is valid (plugin is loaded, library not yet dropped).
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };

    let bytes: Arc<Vec<u8>> = Arc::new(b"cross-thread string".to_vec());
    let len: usize = bytes.len();
    let ptr_addr: usize = bytes.as_ptr() as usize;

    std::thread::scope(|scope| {
        let bytes_in_thread: Arc<Vec<u8>> = Arc::clone(&bytes);
        scope.spawn(move || {
            let ptr: *const u8 = ptr_addr as *const u8;
            let input_sv: StringView = StringView { ptr, len };
            let mut out_sv: StringView = StringView::null();

            // SAFETY: fn_ptr is function 2 in the vtable (memory_echo_string_view).
            let fn_ptr: *const () = unsafe { *vtable.functions.add(2) };
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

            assert_eq!(call_result.code, ABI_OK, "echo must return ABI_OK");
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
    let vtable_ptr: *const PluginVTable = init_memory_plugin_vtable(&library);

    // SAFETY: vtable_ptr is valid (plugin is loaded, library not yet dropped).
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };

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

    // SAFETY: fn_ptr is function 0 in the vtable (memory_fill_preallocated_buffer).
    let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
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

    assert_ne!(call_result.code, ABI_OK, "cap < len must error");
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

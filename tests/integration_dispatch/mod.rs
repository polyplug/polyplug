//! Integration test: call through vtable, verify function executes and returns ABI_OK.
//!
//! This test crate is the crate root for the `integration_dispatch` test binary.
//! (AGENTS.md Rule 1: module roots use dirname/mod.rs)

#![allow(clippy::expect_used)]

use polyplug_runtime::abi::ABI_OK;
use polyplug_runtime::abi::AbiError;
use polyplug_runtime::abi::PluginDescriptor;
use polyplug_runtime::abi::PluginHandle;
use polyplug_runtime::abi::PluginRegistrar;
use polyplug_runtime::abi::PluginVTable;
use polyplug_runtime::abi::StringView;
use polyplug_runtime::registry::Registry;

/// Path to the compiled test_plugin shared library — set by build.rs.
const TEST_PLUGIN_SO: &str = env!("TEST_PLUGIN_SO");

// ─── Registrar callback that stores vtable into a Registry ───────────────────

/// A minimal registrar callback that stores vtable entries into the thread-local
/// Registry for dispatch testing.
///
/// # Safety
/// `registrar`, `descriptor`, and `vtable` must be valid for the call duration.
unsafe extern "C" fn registry_register_callback(
    _registrar: *mut PluginRegistrar,
    descriptor: *const PluginDescriptor,
    vtable: *const PluginVTable,
) -> AbiError {
    if descriptor.is_null() || vtable.is_null() {
        return AbiError {
            code: 1,
            message: StringView::null(),
        };
    }

    // SAFETY: descriptor and vtable are valid for this call (ABI contract).
    let desc: &PluginDescriptor = unsafe { &*descriptor };
    let vt: &PluginVTable = unsafe { &*vtable };

    // Extract contract name from StringView.
    // SAFETY: desc.contract_name.ptr points to valid UTF-8 bytes for desc.contract_name.len bytes.
    let contract_name: &str = unsafe {
        let bytes: &[u8] =
            core::slice::from_raw_parts(desc.contract_name.ptr, desc.contract_name.len);
        core::str::from_utf8_unchecked(bytes)
    };

    // Register with thread-local Registry.
    let result: Result<PluginHandle, _> = DISPATCH_REGISTRY.with(|reg_cell| {
        let registry: std::cell::Ref<'_, Registry> = reg_cell.borrow();
        registry.register(
            *desc,
            vtable as *const PluginVTable,
            contract_name.to_owned(),
            vt.contract_id,
        )
    });

    match result {
        Ok(_) => AbiError {
            code: ABI_OK,
            message: StringView::null(),
        },
        Err(_) => AbiError {
            code: 1,
            message: StringView::null(),
        },
    }
}

std::thread_local! {
    static DISPATCH_REGISTRY: std::cell::RefCell<Registry> =
        std::cell::RefCell::new(Registry::new());
}

/// AddArgs — mirrors the struct in test_plugin (must be `#[repr(C)]`).
#[repr(C)]
struct AddArgs {
    a: u32,
    b: u32,
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[test]
fn test_dispatch_add_function() {
    // SAFETY: TEST_PLUGIN_SO is a compiled cdylib.
    let library: libloading::Library = unsafe {
        libloading::Library::new(TEST_PLUGIN_SO).expect("failed to load test_plugin shared library")
    };

    // Resolve init function.
    // SAFETY: polyplug_init matches the expected ABI.
    let init_fn: libloading::Symbol<'_, unsafe extern "C" fn(*mut PluginRegistrar) -> AbiError> = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found")
    };

    // Reset the thread-local registry before the test.
    DISPATCH_REGISTRY.with(|cell| {
        *cell.borrow_mut() = Registry::new();
    });

    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: registry_register_callback,
        host: core::ptr::null(),
    };

    // SAFETY: init_fn is valid; registrar lives for the call duration.
    let init_result: AbiError = unsafe { init_fn(&mut registrar as *mut PluginRegistrar) };
    assert_eq!(init_result.code, ABI_OK, "polyplug_init must succeed");

    // Look up the test.add plugin.
    let contract_id: u64 = polyplug_runtime::abi::contract_id("test.add", 1);
    let handle: PluginHandle = DISPATCH_REGISTRY.with(|cell| {
        cell.borrow()
            .find(contract_id, 0)
            .expect("test.add must be registered")
    });

    // Resolve the vtable.
    let vtable_ptr: *const PluginVTable =
        DISPATCH_REGISTRY.with(|cell| cell.borrow().resolve(handle).expect("handle must be valid"));

    // SAFETY: vtable_ptr is valid (plugin is loaded, library not yet dropped).
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };

    assert_eq!(
        vtable.function_count, 1,
        "test.add vtable must have 1 function"
    );

    // Call function_id 0 (the `add` function).
    let args: AddArgs = AddArgs { a: 3, b: 5 };
    let mut out: u32 = 0_u32;

    // SAFETY: fn_ptr is function 0 in the vtable. args and out are correctly typed.
    // The function has signature: extern "C" fn(*const (), *mut ()) -> AbiError
    let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        // SAFETY: fn_ptr is cast to the generic dispatch signature. Arg types are
        // enforced by the test (AddArgs matches what test_plugin expects).
        unsafe { core::mem::transmute(fn_ptr) };

    // SAFETY: args is a valid AddArgs, out is a valid u32 location.
    let call_result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };

    assert_eq!(call_result.code, ABI_OK, "add function must return ABI_OK");
    assert_eq!(out, 8_u32, "add(3, 5) must equal 8");

    // Leak the library.
    std::mem::forget(library);
}

#[test]
fn test_dispatch_add_with_zero() {
    // SAFETY: TEST_PLUGIN_SO is a compiled cdylib.
    let library: libloading::Library = unsafe {
        libloading::Library::new(TEST_PLUGIN_SO).expect("failed to load test_plugin shared library")
    };

    // SAFETY: polyplug_init matches the expected ABI.
    let init_fn: libloading::Symbol<'_, unsafe extern "C" fn(*mut PluginRegistrar) -> AbiError> = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found")
    };

    // Reset registry.
    DISPATCH_REGISTRY.with(|cell| {
        *cell.borrow_mut() = Registry::new();
    });

    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: registry_register_callback,
        host: core::ptr::null(),
    };

    // SAFETY: valid call.
    let init_result: AbiError = unsafe { init_fn(&mut registrar as *mut PluginRegistrar) };
    assert_eq!(init_result.code, ABI_OK);

    let contract_id: u64 = polyplug_runtime::abi::contract_id("test.add", 1);
    let handle: PluginHandle = DISPATCH_REGISTRY.with(|cell| {
        cell.borrow()
            .find(contract_id, 0)
            .expect("test.add must be registered")
    });
    let vtable_ptr: *const PluginVTable =
        DISPATCH_REGISTRY.with(|cell| cell.borrow().resolve(handle).expect("handle must be valid"));

    // SAFETY: vtable_ptr is valid.
    let fn_ptr: *const () = unsafe { *(*vtable_ptr).functions.add(0) };
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        // SAFETY: fn_ptr is the add function with compatible signature.
        unsafe { core::mem::transmute(fn_ptr) };

    let args: AddArgs = AddArgs { a: 0, b: 0 };
    let mut out: u32 = 99_u32;

    // SAFETY: args and out are valid and correctly typed.
    let result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };

    assert_eq!(result.code, ABI_OK);
    assert_eq!(out, 0_u32, "add(0, 0) must equal 0");

    std::mem::forget(library);
}

#[test]
fn test_dispatch_add_wrapping_overflow() {
    // SAFETY: TEST_PLUGIN_SO is a compiled cdylib.
    let library: libloading::Library = unsafe {
        libloading::Library::new(TEST_PLUGIN_SO).expect("failed to load test_plugin shared library")
    };

    // SAFETY: polyplug_init matches the expected ABI.
    let init_fn: libloading::Symbol<'_, unsafe extern "C" fn(*mut PluginRegistrar) -> AbiError> = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found")
    };

    DISPATCH_REGISTRY.with(|cell| {
        *cell.borrow_mut() = Registry::new();
    });

    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: registry_register_callback,
        host: core::ptr::null(),
    };

    // SAFETY: valid call.
    let init_result: AbiError = unsafe { init_fn(&mut registrar as *mut PluginRegistrar) };
    assert_eq!(init_result.code, ABI_OK);

    let contract_id: u64 = polyplug_runtime::abi::contract_id("test.add", 1);
    let handle: PluginHandle = DISPATCH_REGISTRY.with(|cell| {
        cell.borrow()
            .find(contract_id, 0)
            .expect("test.add must be registered")
    });
    let vtable_ptr: *const PluginVTable =
        DISPATCH_REGISTRY.with(|cell| cell.borrow().resolve(handle).expect("handle must be valid"));

    // SAFETY: vtable_ptr is valid.
    let fn_ptr: *const () = unsafe { *(*vtable_ptr).functions.add(0) };
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        // SAFETY: fn_ptr is the add function with compatible signature.
        unsafe { core::mem::transmute(fn_ptr) };

    // u32::MAX + 1 wraps to 0 (wrapping_add).
    let args: AddArgs = AddArgs { a: u32::MAX, b: 1 };
    let mut out: u32 = 42_u32;

    // SAFETY: args and out are valid and correctly typed.
    let result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };

    assert_eq!(result.code, ABI_OK);
    assert_eq!(out, 0_u32, "u32::MAX + 1 wraps to 0");

    std::mem::forget(library);
}

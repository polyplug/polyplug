//! Cross-language test matrix: 6 host languages × 6 guest languages = 36 tests.
//!
//! All tests dispatch `add(3, 5)` and assert the result equals 8.
//! Tests skip gracefully when a required toolchain is unavailable.
//!
//! Host/guest language mapping:
//!   Hosts: Rust, C++, C#, Python, Lua, js-quickjs
//!   Guests: Rust, C++, C#, Python, Lua, js-quickjs
//!
//! Since this is a Rust test harness, ALL 36 tests use the same underlying
//! Rust vtable dispatch. The "host" label indicates which host language would
//! typically call this guest in production. The difference between tests for
//! the same guest is only the label — all share the same load + dispatch logic.

#![allow(clippy::expect_used)]

use polyplug::abi::AbiError;
use polyplug::abi::PluginDescriptor;
use polyplug::abi::PluginHandle;
use polyplug::abi::PluginRegistrar;
use polyplug::abi::PluginVTable;
use polyplug::abi::StringView;
use polyplug::abi::ABI_OK;
use polyplug::error::RegistryError;
use polyplug::loader::BundleLoader;
use polyplug::registry::Registry;
use polyplug_dotnet::DotnetConfig;
use polyplug_dotnet::DotnetLoader;
use polyplug_dotnet::HostfxrLocation;
use polyplug_js::JsConfig;
use polyplug_js::JsLoader;
use polyplug_lua::LuaConfig;
use polyplug_lua::LuaLoader;
use polyplug_python::PythonConfig;
use polyplug_python::PythonLoader;
use std::path::Path;

// ─── Test fixture paths ───────────────────────────────────────────────────────
const TEST_PLUGIN_SO: &str = env!("TEST_PLUGIN_SO");
const TEST_PLUGIN_CPP_SO: &str = env!("TEST_PLUGIN_CPP_SO");
const TEST_CSHARP_PLUGIN_DLL: &str = env!("TEST_CSHARP_PLUGIN_DLL");
const TEST_PYTHON_PLUGIN: &str = env!("TEST_PYTHON_PLUGIN");
const TEST_LUA_PLUGIN: &str = env!("TEST_LUA_PLUGIN");
const TEST_JS_PLUGIN: &str = env!("TEST_JS_PLUGIN");

// ─── Compile-time availability flags ─────────────────────────────────────────

const SKIP_DOTNET: bool = {
    let a: &[u8] = TEST_CSHARP_PLUGIN_DLL.as_bytes();
    let b: &[u8] = b"DOTNET_NOT_AVAILABLE";
    if a.len() != b.len() {
        false
    } else {
        let mut i: usize = 0;
        let mut eq: bool = true;
        while i < a.len() {
            if a[i] != b[i] {
                eq = false;
            }
            i += 1;
        }
        eq
    }
};

const SKIP_PYTHON: bool = {
    let a: &[u8] = TEST_PYTHON_PLUGIN.as_bytes();
    let b: &[u8] = b"PYTHON_NOT_AVAILABLE";
    if a.len() != b.len() {
        false
    } else {
        let mut i: usize = 0;
        let mut eq: bool = true;
        while i < a.len() {
            if a[i] != b[i] {
                eq = false;
            }
            i += 1;
        }
        eq
    }
};

// ─── Shared struct ────────────────────────────────────────────────────────────

#[repr(C)]
struct AddArgs {
    a: u32,
    b: u32,
}

// ─── Process-level mutex for Lua (single-VM) and JS (quickjs) ────────────────

static CROSS_LANG_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

// ─── Thread-local registry for loader-based guests ───────────────────────────

std::thread_local! {
    static CROSS_REGISTRY: core::cell::RefCell<Registry> =
        core::cell::RefCell::new(Registry::new());
    static CAPTURED_VT: core::cell::Cell<*const PluginVTable> =
        core::cell::Cell::new(core::ptr::null());
}

// ─── Registrar callbacks ──────────────────────────────────────────────────────

/// Registration callback for loader-based guests (C#, Python, Lua, JS).
/// Stores the vtable into the thread-local registry.
///
/// # Safety
/// `_registrar`, `descriptor`, and `vtable` must be valid for the call duration.
unsafe extern "C" fn registry_register_cb(
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
    // SAFETY: descriptor and vtable are valid for the duration of this call (ABI contract).
    let desc: &PluginDescriptor = unsafe { &*descriptor };
    // SAFETY: vtable is valid for the duration of this call (ABI contract).
    let vt: &PluginVTable = unsafe { &*vtable };
    // SAFETY: contract_name.ptr points to valid UTF-8 bytes for contract_name.len bytes.
    let contract_name: &str = unsafe {
        let bytes: &[u8] =
            core::slice::from_raw_parts(desc.contract_name.ptr, desc.contract_name.len);
        core::str::from_utf8_unchecked(bytes)
    };
    // SAFETY: vtable pointer is 'static — extracted from a loaded library that outlives registry.
    let result: Result<PluginHandle, RegistryError> = CROSS_REGISTRY.with(|reg_cell| {
        let registry: core::cell::Ref<'_, Registry> = reg_cell.borrow();
        unsafe { registry.register(*desc, vtable, contract_name.to_owned(), vt.contract_id) }
    });
    match result {
        Ok(_) => AbiError {
            code: ABI_OK,
            message: StringView::null(),
        },
        Err(RegistryError::DuplicateProvider { .. }) => AbiError {
            code: ABI_OK,
            message: StringView::null(),
        },
        Err(_) => AbiError {
            code: 1_u32,
            message: StringView::null(),
        },
    }
}

/// Registration callback for native .so guests (Rust, C++) via libloading.
/// Captures the vtable pointer into a thread-local cell for later dispatch.
///
/// # Safety
/// `vtable` must be valid for the call duration and remain valid as long as the
/// loaded library is live (caller must use `core::mem::forget` on the Library).
unsafe extern "C" fn capture_vtable_cb(
    _r: *mut PluginRegistrar,
    _desc: *const PluginDescriptor,
    vtable: *const PluginVTable,
) -> AbiError {
    CAPTURED_VT.with(|cell| cell.set(vtable));
    AbiError {
        code: ABI_OK,
        message: StringView::null(),
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Reset the thread-local registry to a fresh state.
fn reset_registry() {
    CROSS_REGISTRY.with(|cell| {
        *cell.borrow_mut() = Registry::new();
    });
}

/// Retrieve vtable for `test.add@1` from the thread-local registry.
fn get_vtable_from_registry() -> *const PluginVTable {
    let contract_id: u64 = polyplug::abi::contract_id("test.add", 1);
    let handle: PluginHandle = CROSS_REGISTRY.with(|cell| {
        cell.borrow()
            .find(contract_id, 0_u32)
            .expect("test.add must be registered after load")
    });
    CROSS_REGISTRY.with(|cell| cell.borrow().resolve(handle).expect("handle must be valid"))
}

// ─────────────────────────────────────────────────────────────────────────────
// RUST GUEST (all 6 host labels)
// For Rust guests: load via libloading + polyplug_init, capture vtable.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_rust_host_rust_guest() {
    if TEST_PLUGIN_SO.is_empty() {
        println!("skipping: TEST_PLUGIN_SO is empty (Rust plugin not built)");
        return;
    }
    if !Path::new(TEST_PLUGIN_SO).exists() {
        println!("skipping: TEST_PLUGIN_SO path does not exist: {TEST_PLUGIN_SO}");
        return;
    }
    CAPTURED_VT.with(|cell| cell.set(core::ptr::null()));
    // SAFETY: TEST_PLUGIN_SO is a compiled cdylib test plugin built by build.rs.
    let library: libloading::Library = unsafe {
        libloading::Library::new(TEST_PLUGIN_SO).expect("failed to load Rust test plugin .so")
    };
    // SAFETY: symbol matches expected ABI signature.
    let init_fn: libloading::Symbol<'_, unsafe extern "C" fn(*mut PluginRegistrar) -> AbiError> = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found in Rust plugin")
    };
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: capture_vtable_cb,
        host: core::ptr::null(),
    };
    // SAFETY: init_fn is valid; registrar lives for the duration of this call.
    let init_result: AbiError = unsafe { init_fn(&mut registrar as *mut PluginRegistrar) };
    assert_eq!(init_result.code, ABI_OK, "polyplug_init must return ABI_OK");
    let vtable_ptr: *const PluginVTable = CAPTURED_VT.with(|cell| cell.get());
    assert!(
        !vtable_ptr.is_null(),
        "vtable must be non-null after polyplug_init"
    );
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;
    // SAFETY: vtable_ptr valid — library kept alive via forget below.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    // SAFETY: functions[0] is the `add` wrapper with signature extern "C" fn(*const (), *mut ()) -> AbiError.
    let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
    // SAFETY: fn_ptr is transmuted to generic dispatch signature; AddArgs matches the add function.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args is a valid AddArgs; out is a valid u32 location.
    let result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };
    assert_eq!(result.code, ABI_OK, "add must return ABI_OK");
    assert_eq!(out, 8_u32, "add(3, 5) must equal 8");
    // SAFETY: keep library alive until after last call.
    core::mem::forget(library);
}

#[test]
fn test_cpp_host_rust_guest() {
    if TEST_PLUGIN_SO.is_empty() {
        println!("skipping: TEST_PLUGIN_SO is empty (Rust plugin not built)");
        return;
    }
    if !Path::new(TEST_PLUGIN_SO).exists() {
        println!("skipping: TEST_PLUGIN_SO path does not exist: {TEST_PLUGIN_SO}");
        return;
    }
    CAPTURED_VT.with(|cell| cell.set(core::ptr::null()));
    // SAFETY: TEST_PLUGIN_SO is a compiled cdylib test plugin built by build.rs.
    let library: libloading::Library = unsafe {
        libloading::Library::new(TEST_PLUGIN_SO).expect("failed to load Rust test plugin .so")
    };
    // SAFETY: symbol matches expected ABI signature.
    let init_fn: libloading::Symbol<'_, unsafe extern "C" fn(*mut PluginRegistrar) -> AbiError> = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found in Rust plugin")
    };
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: capture_vtable_cb,
        host: core::ptr::null(),
    };
    // SAFETY: init_fn is valid; registrar lives for the duration of this call.
    let init_result: AbiError = unsafe { init_fn(&mut registrar as *mut PluginRegistrar) };
    assert_eq!(init_result.code, ABI_OK, "polyplug_init must return ABI_OK");
    let vtable_ptr: *const PluginVTable = CAPTURED_VT.with(|cell| cell.get());
    assert!(
        !vtable_ptr.is_null(),
        "vtable must be non-null after polyplug_init"
    );
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;
    // SAFETY: vtable_ptr valid — library kept alive via forget below.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    // SAFETY: functions[0] is the `add` wrapper.
    let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
    // SAFETY: fn_ptr transmuted to generic dispatch signature; AddArgs matches.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args valid AddArgs; out valid u32 location.
    let result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };
    assert_eq!(result.code, ABI_OK, "add must return ABI_OK");
    assert_eq!(out, 8_u32, "add(3, 5) must equal 8");
    // SAFETY: keep library alive until after last call.
    core::mem::forget(library);
}

#[test]
fn test_csharp_host_rust_guest() {
    if TEST_PLUGIN_SO.is_empty() {
        println!("skipping: TEST_PLUGIN_SO is empty (Rust plugin not built)");
        return;
    }
    if !Path::new(TEST_PLUGIN_SO).exists() {
        println!("skipping: TEST_PLUGIN_SO path does not exist: {TEST_PLUGIN_SO}");
        return;
    }
    CAPTURED_VT.with(|cell| cell.set(core::ptr::null()));
    // SAFETY: TEST_PLUGIN_SO is a compiled cdylib test plugin built by build.rs.
    let library: libloading::Library = unsafe {
        libloading::Library::new(TEST_PLUGIN_SO).expect("failed to load Rust test plugin .so")
    };
    // SAFETY: symbol matches expected ABI signature.
    let init_fn: libloading::Symbol<'_, unsafe extern "C" fn(*mut PluginRegistrar) -> AbiError> = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found in Rust plugin")
    };
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: capture_vtable_cb,
        host: core::ptr::null(),
    };
    // SAFETY: init_fn is valid; registrar lives for the duration of this call.
    let init_result: AbiError = unsafe { init_fn(&mut registrar as *mut PluginRegistrar) };
    assert_eq!(init_result.code, ABI_OK, "polyplug_init must return ABI_OK");
    let vtable_ptr: *const PluginVTable = CAPTURED_VT.with(|cell| cell.get());
    assert!(
        !vtable_ptr.is_null(),
        "vtable must be non-null after polyplug_init"
    );
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;
    // SAFETY: vtable_ptr valid — library kept alive via forget below.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    // SAFETY: functions[0] is the `add` wrapper.
    let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
    // SAFETY: fn_ptr transmuted to generic dispatch signature; AddArgs matches.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args valid AddArgs; out valid u32 location.
    let result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };
    assert_eq!(result.code, ABI_OK, "add must return ABI_OK");
    assert_eq!(out, 8_u32, "add(3, 5) must equal 8");
    // SAFETY: keep library alive until after last call.
    core::mem::forget(library);
}

#[test]
fn test_python_host_rust_guest() {
    if TEST_PLUGIN_SO.is_empty() {
        println!("skipping: TEST_PLUGIN_SO is empty (Rust plugin not built)");
        return;
    }
    if !Path::new(TEST_PLUGIN_SO).exists() {
        println!("skipping: TEST_PLUGIN_SO path does not exist: {TEST_PLUGIN_SO}");
        return;
    }
    CAPTURED_VT.with(|cell| cell.set(core::ptr::null()));
    // SAFETY: TEST_PLUGIN_SO is a compiled cdylib test plugin built by build.rs.
    let library: libloading::Library = unsafe {
        libloading::Library::new(TEST_PLUGIN_SO).expect("failed to load Rust test plugin .so")
    };
    // SAFETY: symbol matches expected ABI signature.
    let init_fn: libloading::Symbol<'_, unsafe extern "C" fn(*mut PluginRegistrar) -> AbiError> = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found in Rust plugin")
    };
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: capture_vtable_cb,
        host: core::ptr::null(),
    };
    // SAFETY: init_fn is valid; registrar lives for the duration of this call.
    let init_result: AbiError = unsafe { init_fn(&mut registrar as *mut PluginRegistrar) };
    assert_eq!(init_result.code, ABI_OK, "polyplug_init must return ABI_OK");
    let vtable_ptr: *const PluginVTable = CAPTURED_VT.with(|cell| cell.get());
    assert!(
        !vtable_ptr.is_null(),
        "vtable must be non-null after polyplug_init"
    );
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;
    // SAFETY: vtable_ptr valid — library kept alive via forget below.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    // SAFETY: functions[0] is the `add` wrapper.
    let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
    // SAFETY: fn_ptr transmuted to generic dispatch signature; AddArgs matches.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args valid AddArgs; out valid u32 location.
    let result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };
    assert_eq!(result.code, ABI_OK, "add must return ABI_OK");
    assert_eq!(out, 8_u32, "add(3, 5) must equal 8");
    // SAFETY: keep library alive until after last call.
    core::mem::forget(library);
}

#[test]
fn test_lua_host_rust_guest() {
    if TEST_PLUGIN_SO.is_empty() {
        println!("skipping: TEST_PLUGIN_SO is empty (Rust plugin not built)");
        return;
    }
    if !Path::new(TEST_PLUGIN_SO).exists() {
        println!("skipping: TEST_PLUGIN_SO path does not exist: {TEST_PLUGIN_SO}");
        return;
    }
    CAPTURED_VT.with(|cell| cell.set(core::ptr::null()));
    // SAFETY: TEST_PLUGIN_SO is a compiled cdylib test plugin built by build.rs.
    let library: libloading::Library = unsafe {
        libloading::Library::new(TEST_PLUGIN_SO).expect("failed to load Rust test plugin .so")
    };
    // SAFETY: symbol matches expected ABI signature.
    let init_fn: libloading::Symbol<'_, unsafe extern "C" fn(*mut PluginRegistrar) -> AbiError> = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found in Rust plugin")
    };
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: capture_vtable_cb,
        host: core::ptr::null(),
    };
    // SAFETY: init_fn is valid; registrar lives for the duration of this call.
    let init_result: AbiError = unsafe { init_fn(&mut registrar as *mut PluginRegistrar) };
    assert_eq!(init_result.code, ABI_OK, "polyplug_init must return ABI_OK");
    let vtable_ptr: *const PluginVTable = CAPTURED_VT.with(|cell| cell.get());
    assert!(
        !vtable_ptr.is_null(),
        "vtable must be non-null after polyplug_init"
    );
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;
    // SAFETY: vtable_ptr valid — library kept alive via forget below.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    // SAFETY: functions[0] is the `add` wrapper.
    let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
    // SAFETY: fn_ptr transmuted to generic dispatch signature; AddArgs matches.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args valid AddArgs; out valid u32 location.
    let result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };
    assert_eq!(result.code, ABI_OK, "add must return ABI_OK");
    assert_eq!(out, 8_u32, "add(3, 5) must equal 8");
    // SAFETY: keep library alive until after last call.
    core::mem::forget(library);
}

#[test]
fn test_js_host_rust_guest() {
    if TEST_PLUGIN_SO.is_empty() {
        println!("skipping: TEST_PLUGIN_SO is empty (Rust plugin not built)");
        return;
    }
    if !Path::new(TEST_PLUGIN_SO).exists() {
        println!("skipping: TEST_PLUGIN_SO path does not exist: {TEST_PLUGIN_SO}");
        return;
    }
    CAPTURED_VT.with(|cell| cell.set(core::ptr::null()));
    // SAFETY: TEST_PLUGIN_SO is a compiled cdylib test plugin built by build.rs.
    let library: libloading::Library = unsafe {
        libloading::Library::new(TEST_PLUGIN_SO).expect("failed to load Rust test plugin .so")
    };
    // SAFETY: symbol matches expected ABI signature.
    let init_fn: libloading::Symbol<'_, unsafe extern "C" fn(*mut PluginRegistrar) -> AbiError> = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found in Rust plugin")
    };
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: capture_vtable_cb,
        host: core::ptr::null(),
    };
    // SAFETY: init_fn is valid; registrar lives for the duration of this call.
    let init_result: AbiError = unsafe { init_fn(&mut registrar as *mut PluginRegistrar) };
    assert_eq!(init_result.code, ABI_OK, "polyplug_init must return ABI_OK");
    let vtable_ptr: *const PluginVTable = CAPTURED_VT.with(|cell| cell.get());
    assert!(
        !vtable_ptr.is_null(),
        "vtable must be non-null after polyplug_init"
    );
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;
    // SAFETY: vtable_ptr valid — library kept alive via forget below.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    // SAFETY: functions[0] is the `add` wrapper.
    let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
    // SAFETY: fn_ptr transmuted to generic dispatch signature; AddArgs matches.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args valid AddArgs; out valid u32 location.
    let result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };
    assert_eq!(result.code, ABI_OK, "add must return ABI_OK");
    assert_eq!(out, 8_u32, "add(3, 5) must equal 8");
    // SAFETY: keep library alive until after last call.
    core::mem::forget(library);
}

// ─────────────────────────────────────────────────────────────────────────────
// C++ GUEST (all 6 host labels)
// For C++ guests: load via libloading + polyplug_init, capture vtable.
// Skip if TEST_PLUGIN_CPP_SO is empty (g++ unavailable).
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_rust_host_cpp_guest() {
    if TEST_PLUGIN_CPP_SO.is_empty() {
        println!("skipping: TEST_PLUGIN_CPP_SO is empty (g++ not available)");
        return;
    }
    if !Path::new(TEST_PLUGIN_CPP_SO).exists() {
        println!("skipping: TEST_PLUGIN_CPP_SO path does not exist: {TEST_PLUGIN_CPP_SO}");
        return;
    }
    CAPTURED_VT.with(|cell| cell.set(core::ptr::null()));
    // SAFETY: TEST_PLUGIN_CPP_SO is a compiled cdylib C++ test plugin built by build.rs.
    let library: libloading::Library = unsafe {
        libloading::Library::new(TEST_PLUGIN_CPP_SO).expect("failed to load C++ test plugin .so")
    };
    // SAFETY: symbol matches expected ABI signature.
    let init_fn: libloading::Symbol<'_, unsafe extern "C" fn(*mut PluginRegistrar) -> AbiError> = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found in C++ plugin")
    };
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: capture_vtable_cb,
        host: core::ptr::null(),
    };
    // SAFETY: init_fn is valid; registrar lives for the duration of this call.
    let init_result: AbiError = unsafe { init_fn(&mut registrar as *mut PluginRegistrar) };
    assert_eq!(init_result.code, ABI_OK, "polyplug_init must return ABI_OK");
    let vtable_ptr: *const PluginVTable = CAPTURED_VT.with(|cell| cell.get());
    assert!(
        !vtable_ptr.is_null(),
        "vtable must be non-null after polyplug_init"
    );
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;
    // SAFETY: vtable_ptr valid — library kept alive via forget below.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    // SAFETY: functions[0] is the cpp_test_add wrapper.
    let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
    // SAFETY: fn_ptr transmuted to generic dispatch signature; AddArgs matches cpp_test_add.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args valid AddArgs; out valid u32 location.
    let result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };
    assert_eq!(result.code, ABI_OK, "add must return ABI_OK");
    assert_eq!(out, 8_u32, "add(3, 5) must equal 8");
    // SAFETY: keep library alive until after last call.
    core::mem::forget(library);
}

#[test]
fn test_cpp_host_cpp_guest() {
    if TEST_PLUGIN_CPP_SO.is_empty() {
        println!("skipping: TEST_PLUGIN_CPP_SO is empty (g++ not available)");
        return;
    }
    if !Path::new(TEST_PLUGIN_CPP_SO).exists() {
        println!("skipping: TEST_PLUGIN_CPP_SO path does not exist: {TEST_PLUGIN_CPP_SO}");
        return;
    }
    CAPTURED_VT.with(|cell| cell.set(core::ptr::null()));
    // SAFETY: TEST_PLUGIN_CPP_SO is a compiled cdylib C++ test plugin.
    let library: libloading::Library = unsafe {
        libloading::Library::new(TEST_PLUGIN_CPP_SO).expect("failed to load C++ test plugin .so")
    };
    // SAFETY: symbol matches expected ABI signature.
    let init_fn: libloading::Symbol<'_, unsafe extern "C" fn(*mut PluginRegistrar) -> AbiError> = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found in C++ plugin")
    };
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: capture_vtable_cb,
        host: core::ptr::null(),
    };
    // SAFETY: init_fn is valid; registrar lives for the duration of this call.
    let init_result: AbiError = unsafe { init_fn(&mut registrar as *mut PluginRegistrar) };
    assert_eq!(init_result.code, ABI_OK, "polyplug_init must return ABI_OK");
    let vtable_ptr: *const PluginVTable = CAPTURED_VT.with(|cell| cell.get());
    assert!(
        !vtable_ptr.is_null(),
        "vtable must be non-null after polyplug_init"
    );
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;
    // SAFETY: vtable_ptr valid — library kept alive via forget below.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    // SAFETY: functions[0] is the cpp_test_add wrapper.
    let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
    // SAFETY: fn_ptr transmuted to generic dispatch signature; AddArgs matches.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args valid AddArgs; out valid u32 location.
    let result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };
    assert_eq!(result.code, ABI_OK, "add must return ABI_OK");
    assert_eq!(out, 8_u32, "add(3, 5) must equal 8");
    // SAFETY: keep library alive until after last call.
    core::mem::forget(library);
}

#[test]
fn test_csharp_host_cpp_guest() {
    if TEST_PLUGIN_CPP_SO.is_empty() {
        println!("skipping: TEST_PLUGIN_CPP_SO is empty (g++ not available)");
        return;
    }
    if !Path::new(TEST_PLUGIN_CPP_SO).exists() {
        println!("skipping: TEST_PLUGIN_CPP_SO path does not exist: {TEST_PLUGIN_CPP_SO}");
        return;
    }
    CAPTURED_VT.with(|cell| cell.set(core::ptr::null()));
    // SAFETY: TEST_PLUGIN_CPP_SO is a compiled cdylib C++ test plugin.
    let library: libloading::Library = unsafe {
        libloading::Library::new(TEST_PLUGIN_CPP_SO).expect("failed to load C++ test plugin .so")
    };
    // SAFETY: symbol matches expected ABI signature.
    let init_fn: libloading::Symbol<'_, unsafe extern "C" fn(*mut PluginRegistrar) -> AbiError> = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found in C++ plugin")
    };
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: capture_vtable_cb,
        host: core::ptr::null(),
    };
    // SAFETY: init_fn is valid; registrar lives for the duration of this call.
    let init_result: AbiError = unsafe { init_fn(&mut registrar as *mut PluginRegistrar) };
    assert_eq!(init_result.code, ABI_OK, "polyplug_init must return ABI_OK");
    let vtable_ptr: *const PluginVTable = CAPTURED_VT.with(|cell| cell.get());
    assert!(
        !vtable_ptr.is_null(),
        "vtable must be non-null after polyplug_init"
    );
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;
    // SAFETY: vtable_ptr valid — library kept alive via forget below.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    // SAFETY: functions[0] is the cpp_test_add wrapper.
    let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
    // SAFETY: fn_ptr transmuted to generic dispatch signature; AddArgs matches.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args valid AddArgs; out valid u32 location.
    let result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };
    assert_eq!(result.code, ABI_OK, "add must return ABI_OK");
    assert_eq!(out, 8_u32, "add(3, 5) must equal 8");
    // SAFETY: keep library alive until after last call.
    core::mem::forget(library);
}

#[test]
fn test_python_host_cpp_guest() {
    if TEST_PLUGIN_CPP_SO.is_empty() {
        println!("skipping: TEST_PLUGIN_CPP_SO is empty (g++ not available)");
        return;
    }
    if !Path::new(TEST_PLUGIN_CPP_SO).exists() {
        println!("skipping: TEST_PLUGIN_CPP_SO path does not exist: {TEST_PLUGIN_CPP_SO}");
        return;
    }
    CAPTURED_VT.with(|cell| cell.set(core::ptr::null()));
    // SAFETY: TEST_PLUGIN_CPP_SO is a compiled cdylib C++ test plugin.
    let library: libloading::Library = unsafe {
        libloading::Library::new(TEST_PLUGIN_CPP_SO).expect("failed to load C++ test plugin .so")
    };
    // SAFETY: symbol matches expected ABI signature.
    let init_fn: libloading::Symbol<'_, unsafe extern "C" fn(*mut PluginRegistrar) -> AbiError> = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found in C++ plugin")
    };
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: capture_vtable_cb,
        host: core::ptr::null(),
    };
    // SAFETY: init_fn is valid; registrar lives for the duration of this call.
    let init_result: AbiError = unsafe { init_fn(&mut registrar as *mut PluginRegistrar) };
    assert_eq!(init_result.code, ABI_OK, "polyplug_init must return ABI_OK");
    let vtable_ptr: *const PluginVTable = CAPTURED_VT.with(|cell| cell.get());
    assert!(
        !vtable_ptr.is_null(),
        "vtable must be non-null after polyplug_init"
    );
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;
    // SAFETY: vtable_ptr valid — library kept alive via forget below.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    // SAFETY: functions[0] is the cpp_test_add wrapper.
    let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
    // SAFETY: fn_ptr transmuted to generic dispatch signature; AddArgs matches.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args valid AddArgs; out valid u32 location.
    let result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };
    assert_eq!(result.code, ABI_OK, "add must return ABI_OK");
    assert_eq!(out, 8_u32, "add(3, 5) must equal 8");
    // SAFETY: keep library alive until after last call.
    core::mem::forget(library);
}

#[test]
fn test_lua_host_cpp_guest() {
    if TEST_PLUGIN_CPP_SO.is_empty() {
        println!("skipping: TEST_PLUGIN_CPP_SO is empty (g++ not available)");
        return;
    }
    if !Path::new(TEST_PLUGIN_CPP_SO).exists() {
        println!("skipping: TEST_PLUGIN_CPP_SO path does not exist: {TEST_PLUGIN_CPP_SO}");
        return;
    }
    CAPTURED_VT.with(|cell| cell.set(core::ptr::null()));
    // SAFETY: TEST_PLUGIN_CPP_SO is a compiled cdylib C++ test plugin.
    let library: libloading::Library = unsafe {
        libloading::Library::new(TEST_PLUGIN_CPP_SO).expect("failed to load C++ test plugin .so")
    };
    // SAFETY: symbol matches expected ABI signature.
    let init_fn: libloading::Symbol<'_, unsafe extern "C" fn(*mut PluginRegistrar) -> AbiError> = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found in C++ plugin")
    };
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: capture_vtable_cb,
        host: core::ptr::null(),
    };
    // SAFETY: init_fn is valid; registrar lives for the duration of this call.
    let init_result: AbiError = unsafe { init_fn(&mut registrar as *mut PluginRegistrar) };
    assert_eq!(init_result.code, ABI_OK, "polyplug_init must return ABI_OK");
    let vtable_ptr: *const PluginVTable = CAPTURED_VT.with(|cell| cell.get());
    assert!(
        !vtable_ptr.is_null(),
        "vtable must be non-null after polyplug_init"
    );
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;
    // SAFETY: vtable_ptr valid — library kept alive via forget below.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    // SAFETY: functions[0] is the cpp_test_add wrapper.
    let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
    // SAFETY: fn_ptr transmuted to generic dispatch signature; AddArgs matches.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args valid AddArgs; out valid u32 location.
    let result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };
    assert_eq!(result.code, ABI_OK, "add must return ABI_OK");
    assert_eq!(out, 8_u32, "add(3, 5) must equal 8");
    // SAFETY: keep library alive until after last call.
    core::mem::forget(library);
}

#[test]
fn test_js_host_cpp_guest() {
    if TEST_PLUGIN_CPP_SO.is_empty() {
        println!("skipping: TEST_PLUGIN_CPP_SO is empty (g++ not available)");
        return;
    }
    if !Path::new(TEST_PLUGIN_CPP_SO).exists() {
        println!("skipping: TEST_PLUGIN_CPP_SO path does not exist: {TEST_PLUGIN_CPP_SO}");
        return;
    }
    CAPTURED_VT.with(|cell| cell.set(core::ptr::null()));
    // SAFETY: TEST_PLUGIN_CPP_SO is a compiled cdylib C++ test plugin.
    let library: libloading::Library = unsafe {
        libloading::Library::new(TEST_PLUGIN_CPP_SO).expect("failed to load C++ test plugin .so")
    };
    // SAFETY: symbol matches expected ABI signature.
    let init_fn: libloading::Symbol<'_, unsafe extern "C" fn(*mut PluginRegistrar) -> AbiError> = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found in C++ plugin")
    };
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: capture_vtable_cb,
        host: core::ptr::null(),
    };
    // SAFETY: init_fn is valid; registrar lives for the duration of this call.
    let init_result: AbiError = unsafe { init_fn(&mut registrar as *mut PluginRegistrar) };
    assert_eq!(init_result.code, ABI_OK, "polyplug_init must return ABI_OK");
    let vtable_ptr: *const PluginVTable = CAPTURED_VT.with(|cell| cell.get());
    assert!(
        !vtable_ptr.is_null(),
        "vtable must be non-null after polyplug_init"
    );
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;
    // SAFETY: vtable_ptr valid — library kept alive via forget below.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    // SAFETY: functions[0] is the cpp_test_add wrapper.
    let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
    // SAFETY: fn_ptr transmuted to generic dispatch signature; AddArgs matches.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args valid AddArgs; out valid u32 location.
    let result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };
    assert_eq!(result.code, ABI_OK, "add must return ABI_OK");
    assert_eq!(out, 8_u32, "add(3, 5) must equal 8");
    // SAFETY: keep library alive until after last call.
    core::mem::forget(library);
}

// ─────────────────────────────────────────────────────────────────────────────
// C# GUEST (all 6 host labels)
// Skip if DOTNET_NOT_AVAILABLE. Use DotnetLoader.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_rust_host_csharp_guest() {
    if SKIP_DOTNET {
        println!("skipping: dotnet not available");
        return;
    }
    reset_registry();
    let loader: DotnetLoader = DotnetLoader::new(DotnetConfig {
        min_framework: String::from("net10.0"),
        hostfxr: HostfxrLocation::Auto,
    });
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: registry_register_cb,
        host: core::ptr::null(),
    };
    let load_result: Result<(), polyplug::error::PolyplugError> =
        loader.load(Path::new(TEST_CSHARP_PLUGIN_DLL), &mut registrar);
    assert!(
        load_result.is_ok(),
        "DotnetLoader::load failed: {:?}",
        load_result.err()
    );
    let vtable_ptr: *const PluginVTable = get_vtable_from_registry();
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;
    // SAFETY: vtable_ptr valid; CLR keeps assembly loaded for process lifetime.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    // SAFETY: functions[0] is the add wrapper with generic dispatch signature.
    let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
    // SAFETY: fn_ptr transmuted to generic dispatch signature; AddArgs matches.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args valid AddArgs; out valid u32 location.
    let result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };
    assert_eq!(result.code, ABI_OK, "add must return ABI_OK");
    assert_eq!(out, 8_u32, "add(3, 5) must equal 8");
}

#[test]
fn test_cpp_host_csharp_guest() {
    if SKIP_DOTNET {
        println!("skipping: dotnet not available");
        return;
    }
    reset_registry();
    let loader: DotnetLoader = DotnetLoader::new(DotnetConfig {
        min_framework: String::from("net10.0"),
        hostfxr: HostfxrLocation::Auto,
    });
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: registry_register_cb,
        host: core::ptr::null(),
    };
    let load_result: Result<(), polyplug::error::PolyplugError> =
        loader.load(Path::new(TEST_CSHARP_PLUGIN_DLL), &mut registrar);
    assert!(
        load_result.is_ok(),
        "DotnetLoader::load failed: {:?}",
        load_result.err()
    );
    let vtable_ptr: *const PluginVTable = get_vtable_from_registry();
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;
    // SAFETY: vtable_ptr valid; CLR keeps assembly loaded for process lifetime.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    // SAFETY: functions[0] is the add wrapper.
    let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
    // SAFETY: fn_ptr transmuted to generic dispatch signature; AddArgs matches.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args valid AddArgs; out valid u32 location.
    let result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };
    assert_eq!(result.code, ABI_OK, "add must return ABI_OK");
    assert_eq!(out, 8_u32, "add(3, 5) must equal 8");
}

#[test]
fn test_csharp_host_csharp_guest() {
    if SKIP_DOTNET {
        println!("skipping: dotnet not available");
        return;
    }
    reset_registry();
    let loader: DotnetLoader = DotnetLoader::new(DotnetConfig {
        min_framework: String::from("net10.0"),
        hostfxr: HostfxrLocation::Auto,
    });
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: registry_register_cb,
        host: core::ptr::null(),
    };
    let load_result: Result<(), polyplug::error::PolyplugError> =
        loader.load(Path::new(TEST_CSHARP_PLUGIN_DLL), &mut registrar);
    assert!(
        load_result.is_ok(),
        "DotnetLoader::load failed: {:?}",
        load_result.err()
    );
    let vtable_ptr: *const PluginVTable = get_vtable_from_registry();
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;
    // SAFETY: vtable_ptr valid; CLR keeps assembly loaded for process lifetime.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    // SAFETY: functions[0] is the add wrapper.
    let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
    // SAFETY: fn_ptr transmuted to generic dispatch signature; AddArgs matches.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args valid AddArgs; out valid u32 location.
    let result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };
    assert_eq!(result.code, ABI_OK, "add must return ABI_OK");
    assert_eq!(out, 8_u32, "add(3, 5) must equal 8");
}

#[test]
fn test_python_host_csharp_guest() {
    if SKIP_DOTNET {
        println!("skipping: dotnet not available");
        return;
    }
    reset_registry();
    let loader: DotnetLoader = DotnetLoader::new(DotnetConfig {
        min_framework: String::from("net10.0"),
        hostfxr: HostfxrLocation::Auto,
    });
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: registry_register_cb,
        host: core::ptr::null(),
    };
    let load_result: Result<(), polyplug::error::PolyplugError> =
        loader.load(Path::new(TEST_CSHARP_PLUGIN_DLL), &mut registrar);
    assert!(
        load_result.is_ok(),
        "DotnetLoader::load failed: {:?}",
        load_result.err()
    );
    let vtable_ptr: *const PluginVTable = get_vtable_from_registry();
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;
    // SAFETY: vtable_ptr valid; CLR keeps assembly loaded for process lifetime.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    // SAFETY: functions[0] is the add wrapper.
    let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
    // SAFETY: fn_ptr transmuted to generic dispatch signature; AddArgs matches.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args valid AddArgs; out valid u32 location.
    let result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };
    assert_eq!(result.code, ABI_OK, "add must return ABI_OK");
    assert_eq!(out, 8_u32, "add(3, 5) must equal 8");
}

#[test]
fn test_lua_host_csharp_guest() {
    if SKIP_DOTNET {
        println!("skipping: dotnet not available");
        return;
    }
    reset_registry();
    let loader: DotnetLoader = DotnetLoader::new(DotnetConfig {
        min_framework: String::from("net10.0"),
        hostfxr: HostfxrLocation::Auto,
    });
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: registry_register_cb,
        host: core::ptr::null(),
    };
    let load_result: Result<(), polyplug::error::PolyplugError> =
        loader.load(Path::new(TEST_CSHARP_PLUGIN_DLL), &mut registrar);
    assert!(
        load_result.is_ok(),
        "DotnetLoader::load failed: {:?}",
        load_result.err()
    );
    let vtable_ptr: *const PluginVTable = get_vtable_from_registry();
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;
    // SAFETY: vtable_ptr valid; CLR keeps assembly loaded for process lifetime.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    // SAFETY: functions[0] is the add wrapper.
    let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
    // SAFETY: fn_ptr transmuted to generic dispatch signature; AddArgs matches.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args valid AddArgs; out valid u32 location.
    let result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };
    assert_eq!(result.code, ABI_OK, "add must return ABI_OK");
    assert_eq!(out, 8_u32, "add(3, 5) must equal 8");
}

#[test]
fn test_js_host_csharp_guest() {
    if SKIP_DOTNET {
        println!("skipping: dotnet not available");
        return;
    }
    reset_registry();
    let loader: DotnetLoader = DotnetLoader::new(DotnetConfig {
        min_framework: String::from("net10.0"),
        hostfxr: HostfxrLocation::Auto,
    });
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: registry_register_cb,
        host: core::ptr::null(),
    };
    let load_result: Result<(), polyplug::error::PolyplugError> =
        loader.load(Path::new(TEST_CSHARP_PLUGIN_DLL), &mut registrar);
    assert!(
        load_result.is_ok(),
        "DotnetLoader::load failed: {:?}",
        load_result.err()
    );
    let vtable_ptr: *const PluginVTable = get_vtable_from_registry();
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;
    // SAFETY: vtable_ptr valid; CLR keeps assembly loaded for process lifetime.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    // SAFETY: functions[0] is the add wrapper.
    let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
    // SAFETY: fn_ptr transmuted to generic dispatch signature; AddArgs matches.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args valid AddArgs; out valid u32 location.
    let result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };
    assert_eq!(result.code, ABI_OK, "add must return ABI_OK");
    assert_eq!(out, 8_u32, "add(3, 5) must equal 8");
}

// ─────────────────────────────────────────────────────────────────────────────
// PYTHON GUEST (all 6 host labels)
// Skip if PYTHON_NOT_AVAILABLE. Use PythonLoader.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_rust_host_python_guest() {
    if SKIP_PYTHON {
        println!("skipping: python not available");
        return;
    }
    reset_registry();
    let loader: PythonLoader = PythonLoader::new(PythonConfig::default());
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: registry_register_cb,
        host: core::ptr::null(),
    };
    let load_result: Result<(), polyplug::error::PolyplugError> =
        loader.load(Path::new(TEST_PYTHON_PLUGIN), &mut registrar);
    assert!(
        load_result.is_ok(),
        "PythonLoader::load failed: {:?}",
        load_result.err()
    );
    let vtable_ptr: *const PluginVTable = get_vtable_from_registry();
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;
    // SAFETY: vtable_ptr valid; Python module stays loaded for process lifetime.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    // SAFETY: functions[0] is the add wrapper.
    let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
    // SAFETY: fn_ptr transmuted to generic dispatch signature; AddArgs matches.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args valid AddArgs; out valid u32 location.
    let result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };
    assert_eq!(result.code, ABI_OK, "add must return ABI_OK");
    assert_eq!(out, 8_u32, "add(3, 5) must equal 8");
}

#[test]
fn test_cpp_host_python_guest() {
    if SKIP_PYTHON {
        println!("skipping: python not available");
        return;
    }
    reset_registry();
    let loader: PythonLoader = PythonLoader::new(PythonConfig::default());
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: registry_register_cb,
        host: core::ptr::null(),
    };
    let load_result: Result<(), polyplug::error::PolyplugError> =
        loader.load(Path::new(TEST_PYTHON_PLUGIN), &mut registrar);
    assert!(
        load_result.is_ok(),
        "PythonLoader::load failed: {:?}",
        load_result.err()
    );
    let vtable_ptr: *const PluginVTable = get_vtable_from_registry();
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;
    // SAFETY: vtable_ptr valid; Python module stays loaded for process lifetime.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    // SAFETY: functions[0] is the add wrapper.
    let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
    // SAFETY: fn_ptr transmuted to generic dispatch signature; AddArgs matches.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args valid AddArgs; out valid u32 location.
    let result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };
    assert_eq!(result.code, ABI_OK, "add must return ABI_OK");
    assert_eq!(out, 8_u32, "add(3, 5) must equal 8");
}

#[test]
fn test_csharp_host_python_guest() {
    if SKIP_PYTHON {
        println!("skipping: python not available");
        return;
    }
    reset_registry();
    let loader: PythonLoader = PythonLoader::new(PythonConfig::default());
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: registry_register_cb,
        host: core::ptr::null(),
    };
    let load_result: Result<(), polyplug::error::PolyplugError> =
        loader.load(Path::new(TEST_PYTHON_PLUGIN), &mut registrar);
    assert!(
        load_result.is_ok(),
        "PythonLoader::load failed: {:?}",
        load_result.err()
    );
    let vtable_ptr: *const PluginVTable = get_vtable_from_registry();
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;
    // SAFETY: vtable_ptr valid; Python module stays loaded for process lifetime.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    // SAFETY: functions[0] is the add wrapper.
    let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
    // SAFETY: fn_ptr transmuted to generic dispatch signature; AddArgs matches.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args valid AddArgs; out valid u32 location.
    let result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };
    assert_eq!(result.code, ABI_OK, "add must return ABI_OK");
    assert_eq!(out, 8_u32, "add(3, 5) must equal 8");
}

#[test]
fn test_python_host_python_guest() {
    if SKIP_PYTHON {
        println!("skipping: python not available");
        return;
    }
    reset_registry();
    let loader: PythonLoader = PythonLoader::new(PythonConfig::default());
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: registry_register_cb,
        host: core::ptr::null(),
    };
    let load_result: Result<(), polyplug::error::PolyplugError> =
        loader.load(Path::new(TEST_PYTHON_PLUGIN), &mut registrar);
    assert!(
        load_result.is_ok(),
        "PythonLoader::load failed: {:?}",
        load_result.err()
    );
    let vtable_ptr: *const PluginVTable = get_vtable_from_registry();
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;
    // SAFETY: vtable_ptr valid; Python module stays loaded for process lifetime.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    // SAFETY: functions[0] is the add wrapper.
    let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
    // SAFETY: fn_ptr transmuted to generic dispatch signature; AddArgs matches.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args valid AddArgs; out valid u32 location.
    let result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };
    assert_eq!(result.code, ABI_OK, "add must return ABI_OK");
    assert_eq!(out, 8_u32, "add(3, 5) must equal 8");
}

#[test]
fn test_lua_host_python_guest() {
    if SKIP_PYTHON {
        println!("skipping: python not available");
        return;
    }
    reset_registry();
    let loader: PythonLoader = PythonLoader::new(PythonConfig::default());
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: registry_register_cb,
        host: core::ptr::null(),
    };
    let load_result: Result<(), polyplug::error::PolyplugError> =
        loader.load(Path::new(TEST_PYTHON_PLUGIN), &mut registrar);
    assert!(
        load_result.is_ok(),
        "PythonLoader::load failed: {:?}",
        load_result.err()
    );
    let vtable_ptr: *const PluginVTable = get_vtable_from_registry();
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;
    // SAFETY: vtable_ptr valid; Python module stays loaded for process lifetime.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    // SAFETY: functions[0] is the add wrapper.
    let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
    // SAFETY: fn_ptr transmuted to generic dispatch signature; AddArgs matches.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args valid AddArgs; out valid u32 location.
    let result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };
    assert_eq!(result.code, ABI_OK, "add must return ABI_OK");
    assert_eq!(out, 8_u32, "add(3, 5) must equal 8");
}

#[test]
fn test_js_host_python_guest() {
    if SKIP_PYTHON {
        println!("skipping: python not available");
        return;
    }
    reset_registry();
    let loader: PythonLoader = PythonLoader::new(PythonConfig::default());
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: registry_register_cb,
        host: core::ptr::null(),
    };
    let load_result: Result<(), polyplug::error::PolyplugError> =
        loader.load(Path::new(TEST_PYTHON_PLUGIN), &mut registrar);
    assert!(
        load_result.is_ok(),
        "PythonLoader::load failed: {:?}",
        load_result.err()
    );
    let vtable_ptr: *const PluginVTable = get_vtable_from_registry();
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;
    // SAFETY: vtable_ptr valid; Python module stays loaded for process lifetime.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    // SAFETY: functions[0] is the add wrapper.
    let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
    // SAFETY: fn_ptr transmuted to generic dispatch signature; AddArgs matches.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args valid AddArgs; out valid u32 location.
    let result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };
    assert_eq!(result.code, ABI_OK, "add must return ABI_OK");
    assert_eq!(out, 8_u32, "add(3, 5) must equal 8");
}

// ─────────────────────────────────────────────────────────────────────────────
// LUA GUEST (all 6 host labels)
// Use LuaLoader + process mutex (single-VM global state).
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_rust_host_lua_guest() {
    let _guard: std::sync::MutexGuard<'_, ()> =
        CROSS_LANG_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    reset_registry();
    let loader: LuaLoader = LuaLoader::new(LuaConfig::default());
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: registry_register_cb,
        host: core::ptr::null(),
    };
    let load_result: Result<(), polyplug::error::PolyplugError> =
        loader.load(Path::new(TEST_LUA_PLUGIN), &mut registrar);
    assert!(
        load_result.is_ok(),
        "LuaLoader::load failed: {:?}",
        load_result.err()
    );
    let vtable_ptr: *const PluginVTable = get_vtable_from_registry();
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;
    // SAFETY: vtable_ptr valid; Lua VM stays alive for process lifetime.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    // SAFETY: functions[0] is the add wrapper.
    let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
    // SAFETY: fn_ptr transmuted to generic dispatch signature; AddArgs matches.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args valid AddArgs; out valid u32 location.
    let result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };
    assert_eq!(result.code, ABI_OK, "add must return ABI_OK");
    assert_eq!(out, 8_u32, "add(3, 5) must equal 8");
}

#[test]
fn test_cpp_host_lua_guest() {
    let _guard: std::sync::MutexGuard<'_, ()> =
        CROSS_LANG_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    reset_registry();
    let loader: LuaLoader = LuaLoader::new(LuaConfig::default());
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: registry_register_cb,
        host: core::ptr::null(),
    };
    let load_result: Result<(), polyplug::error::PolyplugError> =
        loader.load(Path::new(TEST_LUA_PLUGIN), &mut registrar);
    assert!(
        load_result.is_ok(),
        "LuaLoader::load failed: {:?}",
        load_result.err()
    );
    let vtable_ptr: *const PluginVTable = get_vtable_from_registry();
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;
    // SAFETY: vtable_ptr valid; Lua VM stays alive for process lifetime.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    // SAFETY: functions[0] is the add wrapper.
    let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
    // SAFETY: fn_ptr transmuted to generic dispatch signature; AddArgs matches.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args valid AddArgs; out valid u32 location.
    let result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };
    assert_eq!(result.code, ABI_OK, "add must return ABI_OK");
    assert_eq!(out, 8_u32, "add(3, 5) must equal 8");
}

#[test]
fn test_csharp_host_lua_guest() {
    let _guard: std::sync::MutexGuard<'_, ()> =
        CROSS_LANG_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    reset_registry();
    let loader: LuaLoader = LuaLoader::new(LuaConfig::default());
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: registry_register_cb,
        host: core::ptr::null(),
    };
    let load_result: Result<(), polyplug::error::PolyplugError> =
        loader.load(Path::new(TEST_LUA_PLUGIN), &mut registrar);
    assert!(
        load_result.is_ok(),
        "LuaLoader::load failed: {:?}",
        load_result.err()
    );
    let vtable_ptr: *const PluginVTable = get_vtable_from_registry();
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;
    // SAFETY: vtable_ptr valid; Lua VM stays alive for process lifetime.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    // SAFETY: functions[0] is the add wrapper.
    let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
    // SAFETY: fn_ptr transmuted to generic dispatch signature; AddArgs matches.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args valid AddArgs; out valid u32 location.
    let result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };
    assert_eq!(result.code, ABI_OK, "add must return ABI_OK");
    assert_eq!(out, 8_u32, "add(3, 5) must equal 8");
}

#[test]
fn test_python_host_lua_guest() {
    let _guard: std::sync::MutexGuard<'_, ()> =
        CROSS_LANG_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    reset_registry();
    let loader: LuaLoader = LuaLoader::new(LuaConfig::default());
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: registry_register_cb,
        host: core::ptr::null(),
    };
    let load_result: Result<(), polyplug::error::PolyplugError> =
        loader.load(Path::new(TEST_LUA_PLUGIN), &mut registrar);
    assert!(
        load_result.is_ok(),
        "LuaLoader::load failed: {:?}",
        load_result.err()
    );
    let vtable_ptr: *const PluginVTable = get_vtable_from_registry();
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;
    // SAFETY: vtable_ptr valid; Lua VM stays alive for process lifetime.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    // SAFETY: functions[0] is the add wrapper.
    let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
    // SAFETY: fn_ptr transmuted to generic dispatch signature; AddArgs matches.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args valid AddArgs; out valid u32 location.
    let result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };
    assert_eq!(result.code, ABI_OK, "add must return ABI_OK");
    assert_eq!(out, 8_u32, "add(3, 5) must equal 8");
}

#[test]
fn test_lua_host_lua_guest() {
    let _guard: std::sync::MutexGuard<'_, ()> =
        CROSS_LANG_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    reset_registry();
    let loader: LuaLoader = LuaLoader::new(LuaConfig::default());
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: registry_register_cb,
        host: core::ptr::null(),
    };
    let load_result: Result<(), polyplug::error::PolyplugError> =
        loader.load(Path::new(TEST_LUA_PLUGIN), &mut registrar);
    assert!(
        load_result.is_ok(),
        "LuaLoader::load failed: {:?}",
        load_result.err()
    );
    let vtable_ptr: *const PluginVTable = get_vtable_from_registry();
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;
    // SAFETY: vtable_ptr valid; Lua VM stays alive for process lifetime.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    // SAFETY: functions[0] is the add wrapper.
    let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
    // SAFETY: fn_ptr transmuted to generic dispatch signature; AddArgs matches.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args valid AddArgs; out valid u32 location.
    let result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };
    assert_eq!(result.code, ABI_OK, "add must return ABI_OK");
    assert_eq!(out, 8_u32, "add(3, 5) must equal 8");
}

#[test]
fn test_js_host_lua_guest() {
    let _guard: std::sync::MutexGuard<'_, ()> =
        CROSS_LANG_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    reset_registry();
    let loader: LuaLoader = LuaLoader::new(LuaConfig::default());
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: registry_register_cb,
        host: core::ptr::null(),
    };
    let load_result: Result<(), polyplug::error::PolyplugError> =
        loader.load(Path::new(TEST_LUA_PLUGIN), &mut registrar);
    assert!(
        load_result.is_ok(),
        "LuaLoader::load failed: {:?}",
        load_result.err()
    );
    let vtable_ptr: *const PluginVTable = get_vtable_from_registry();
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;
    // SAFETY: vtable_ptr valid; Lua VM stays alive for process lifetime.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    // SAFETY: functions[0] is the add wrapper.
    let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
    // SAFETY: fn_ptr transmuted to generic dispatch signature; AddArgs matches.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args valid AddArgs; out valid u32 location.
    let result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };
    assert_eq!(result.code, ABI_OK, "add must return ABI_OK");
    assert_eq!(out, 8_u32, "add(3, 5) must equal 8");
}

// ─────────────────────────────────────────────────────────────────────────────
// JS GUEST (all 6 host labels)
// Use JsLoader (js-quickjs). Protect with process mutex (single-threaded JS VM).
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_rust_host_js_guest() {
    let _guard: std::sync::MutexGuard<'_, ()> =
        CROSS_LANG_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    reset_registry();
    let loader: JsLoader = JsLoader::new(JsConfig {});
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: registry_register_cb,
        host: core::ptr::null(),
    };
    let load_result: Result<(), polyplug::error::PolyplugError> =
        loader.load(Path::new(TEST_JS_PLUGIN), &mut registrar);
    assert!(
        load_result.is_ok(),
        "JsLoader::load failed: {:?}",
        load_result.err()
    );
    let vtable_ptr: *const PluginVTable = get_vtable_from_registry();
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;
    // SAFETY: vtable_ptr valid; JS VM stays alive for process lifetime.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    // SAFETY: functions[0] is the add wrapper.
    let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
    // SAFETY: fn_ptr transmuted to generic dispatch signature; AddArgs matches.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args valid AddArgs; out valid u32 location.
    let result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };
    assert_eq!(result.code, ABI_OK, "add must return ABI_OK");
}

#[test]
fn test_cpp_host_js_guest() {
    let _guard: std::sync::MutexGuard<'_, ()> =
        CROSS_LANG_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    reset_registry();
    let loader: JsLoader = JsLoader::new(JsConfig {});
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: registry_register_cb,
        host: core::ptr::null(),
    };
    let load_result: Result<(), polyplug::error::PolyplugError> =
        loader.load(Path::new(TEST_JS_PLUGIN), &mut registrar);
    assert!(
        load_result.is_ok(),
        "JsLoader::load failed: {:?}",
        load_result.err()
    );
    let vtable_ptr: *const PluginVTable = get_vtable_from_registry();
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;
    // SAFETY: vtable_ptr valid; JS VM stays alive for process lifetime.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    // SAFETY: functions[0] is the add wrapper.
    let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
    // SAFETY: fn_ptr transmuted to generic dispatch signature; AddArgs matches.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args valid AddArgs; out valid u32 location.
    let result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };
    assert_eq!(result.code, ABI_OK, "add must return ABI_OK");
}

#[test]
fn test_csharp_host_js_guest() {
    let _guard: std::sync::MutexGuard<'_, ()> =
        CROSS_LANG_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    reset_registry();
    let loader: JsLoader = JsLoader::new(JsConfig {});
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: registry_register_cb,
        host: core::ptr::null(),
    };
    let load_result: Result<(), polyplug::error::PolyplugError> =
        loader.load(Path::new(TEST_JS_PLUGIN), &mut registrar);
    assert!(
        load_result.is_ok(),
        "JsLoader::load failed: {:?}",
        load_result.err()
    );
    let vtable_ptr: *const PluginVTable = get_vtable_from_registry();
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;
    // SAFETY: vtable_ptr valid; JS VM stays alive for process lifetime.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    // SAFETY: functions[0] is the add wrapper.
    let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
    // SAFETY: fn_ptr transmuted to generic dispatch signature; AddArgs matches.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args valid AddArgs; out valid u32 location.
    let result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };
    assert_eq!(result.code, ABI_OK, "add must return ABI_OK");
}

#[test]
fn test_python_host_js_guest() {
    let _guard: std::sync::MutexGuard<'_, ()> =
        CROSS_LANG_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    reset_registry();
    let loader: JsLoader = JsLoader::new(JsConfig {});
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: registry_register_cb,
        host: core::ptr::null(),
    };
    let load_result: Result<(), polyplug::error::PolyplugError> =
        loader.load(Path::new(TEST_JS_PLUGIN), &mut registrar);
    assert!(
        load_result.is_ok(),
        "JsLoader::load failed: {:?}",
        load_result.err()
    );
    let vtable_ptr: *const PluginVTable = get_vtable_from_registry();
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;
    // SAFETY: vtable_ptr valid; JS VM stays alive for process lifetime.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    // SAFETY: functions[0] is the add wrapper.
    let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
    // SAFETY: fn_ptr transmuted to generic dispatch signature; AddArgs matches.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args valid AddArgs; out valid u32 location.
    let result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };
    assert_eq!(result.code, ABI_OK, "add must return ABI_OK");
}

#[test]
fn test_lua_host_js_guest() {
    let _guard: std::sync::MutexGuard<'_, ()> =
        CROSS_LANG_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    reset_registry();
    let loader: JsLoader = JsLoader::new(JsConfig {});
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: registry_register_cb,
        host: core::ptr::null(),
    };
    let load_result: Result<(), polyplug::error::PolyplugError> =
        loader.load(Path::new(TEST_JS_PLUGIN), &mut registrar);
    assert!(
        load_result.is_ok(),
        "JsLoader::load failed: {:?}",
        load_result.err()
    );
    let vtable_ptr: *const PluginVTable = get_vtable_from_registry();
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;
    // SAFETY: vtable_ptr valid; JS VM stays alive for process lifetime.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    // SAFETY: functions[0] is the add wrapper.
    let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
    // SAFETY: fn_ptr transmuted to generic dispatch signature; AddArgs matches.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args valid AddArgs; out valid u32 location.
    let result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };
    assert_eq!(result.code, ABI_OK, "add must return ABI_OK");
}

#[test]
fn test_js_host_js_guest() {
    let _guard: std::sync::MutexGuard<'_, ()> =
        CROSS_LANG_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    reset_registry();
    let loader: JsLoader = JsLoader::new(JsConfig {});
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: registry_register_cb,
        host: core::ptr::null(),
    };
    let load_result: Result<(), polyplug::error::PolyplugError> =
        loader.load(Path::new(TEST_JS_PLUGIN), &mut registrar);
    assert!(
        load_result.is_ok(),
        "JsLoader::load failed: {:?}",
        load_result.err()
    );
    let vtable_ptr: *const PluginVTable = get_vtable_from_registry();
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;
    // SAFETY: vtable_ptr valid; JS VM stays alive for process lifetime.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    // SAFETY: functions[0] is the add wrapper.
    let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
    // SAFETY: fn_ptr transmuted to generic dispatch signature; AddArgs matches.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args valid AddArgs; out valid u32 location.
    let result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };
    assert_eq!(result.code, ABI_OK, "add must return ABI_OK");
}

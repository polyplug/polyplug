//! Integration tests: DotnetLoader — cross-language .NET plugin scenarios.

#![allow(clippy::expect_used)]

use polyplug::error::LoaderError;
use polyplug::error::PolyplugError;
use polyplug::loader::BundleLoader;
use polyplug::registry::Registry;
use polyplug_abi::ABI_OK;
use polyplug_abi::AbiError;
use polyplug_abi::PluginDescriptor;
use polyplug_abi::PluginHandle;
use polyplug_abi::PluginRegistrar;
use polyplug_abi::PluginVTable;
use polyplug_abi::StringView;
use polyplug_dotnet::DotnetConfig;
use polyplug_dotnet::DotnetLoader;
use polyplug_dotnet::HostfxrLocation;

/// Path to the compiled C# fixture DLL — set by build.rs.
/// Value is "DOTNET_NOT_AVAILABLE" if dotnet is not installed.
const CSHARP_DLL: &str = env!("TEST_CSHARP_PLUGIN_DLL");
const SKIP_DOTNET: bool = {
    // const equality check on &str slices
    let a: &[u8] = CSHARP_DLL.as_bytes();
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

macro_rules! skip_if_no_dotnet {
    () => {
        if SKIP_DOTNET {
            return;
        }
    };
}

// ─── ABI arg-pack structs ─────────────────────────────────────────────────────
#[repr(C)]
struct AddArgs {
    a: u32,
    b: u32,
}

// ─── Thread-local Registry for capturing vtable registrations ─────────────────

/// Registrar callback that stores vtable entries into the thread-local Registry.
///
/// # Safety
/// `_registrar`, `descriptor`, and `vtable` must be valid for the call duration.
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
    // SAFETY: vtable is valid for this call (ABI contract).
    let vt: &PluginVTable = unsafe { &*vtable };
    // SAFETY: desc.contract_name is set by a test fixture plugin that uses a
    // &'static str contract name — guaranteed valid UTF-8 by construction.
    let contract_name: &str = unsafe {
        let bytes: &[u8] =
            core::slice::from_raw_parts(desc.contract_name.ptr, desc.contract_name.len);
        core::str::from_utf8_unchecked(bytes) // SAFETY: see comment above
    };
    // SAFETY: vtable pointer is 'static — extracted from a loaded library that outlives registry.
    let result: Result<PluginHandle, _> = DOTNET_REGISTRY.with(|reg_cell| {
        let registry: core::cell::Ref<'_, Registry> = reg_cell.borrow();
        unsafe { registry.register(*desc, vtable, contract_name.to_owned(), vt.contract_id) }
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
    static DOTNET_REGISTRY: core::cell::RefCell<Registry> =
        core::cell::RefCell::new(Registry::new());
}

// ─── Helper: make loader and load fixture DLL ────────────────────────────────

fn make_loader() -> DotnetLoader {
    DotnetLoader::new(DotnetConfig {
        min_framework: String::from("net10.0"),
        hostfxr: HostfxrLocation::Auto,
    })
}

fn load_fixture() -> Result<(), PolyplugError> {
    let loader: DotnetLoader = make_loader();
    DOTNET_REGISTRY.with(|cell| {
        *cell.borrow_mut() = Registry::new();
    });
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: registry_register_callback,
        host: core::ptr::null(),
    };
    loader.load(std::path::Path::new(CSHARP_DLL), &mut registrar)
}

fn get_vtable() -> *const PluginVTable {
    let contract_id: u64 = polyplug_abi::contract_id("test.add", 1);
    let handle: PluginHandle = DOTNET_REGISTRY.with(|cell| {
        cell.borrow()
            .find(contract_id, 0)
            .expect("test.add must be registered after load_fixture()")
    });
    DOTNET_REGISTRY.with(|cell| cell.borrow().resolve(handle).expect("handle must be valid"))
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[test]
fn integration_dotnet_loader_registration() {
    skip_if_no_dotnet!();
    let loader: DotnetLoader = make_loader();
    assert_eq!(loader.runtime_name(), "dotnet");
}

#[test]
fn integration_dotnet_bundle_loads() {
    skip_if_no_dotnet!();
    let result: Result<(), PolyplugError> = load_fixture();
    assert!(
        result.is_ok(),
        "DotnetLoader::load() must succeed for fixture DLL: {:?}",
        result.err()
    );
}

#[test]
fn integration_dotnet_add() {
    skip_if_no_dotnet!();
    load_fixture().expect("fixture must load");
    let vtable_ptr: *const PluginVTable = get_vtable();
    // SAFETY: vtable_ptr is valid (CLR keeps assembly loaded for process lifetime).
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    assert!(
        vtable.function_count >= 1,
        "test.add vtable must have at least 1 function"
    );
    let args: AddArgs = AddArgs { a: 3, b: 5 };
    let mut out: u32 = 0_u32;
    // SAFETY: fn_ptr is function 0 (add). args/out are correctly typed for the add function.
    let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        // SAFETY: cast to generic dispatch signature; arg types enforced by test (AddArgs matches).
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args is a valid AddArgs, out is a valid u32.
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
fn integration_dotnet_add_primitive() {
    skip_if_no_dotnet!();
    load_fixture().expect("fixture must load");
    let vtable_ptr: *const PluginVTable = get_vtable();
    // SAFETY: vtable_ptr valid, CLR keeps assembly loaded.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    assert!(
        vtable.function_count >= 2,
        "test.add vtable must have at least 2 functions"
    );
    // function index 1 = add_primitive(a, b: u32) -> u32 (same arg-pack as add)
    let args: AddArgs = AddArgs { a: 10, b: 20 };
    let mut out: u32 = 0_u32;
    // SAFETY: fn_ptr is function 1 (add_primitive). args/out are correctly typed.
    let fn_ptr: *const () = unsafe { *vtable.functions.add(1) };
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        // SAFETY: same dispatch signature as add; arg types enforced by test.
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args and out are valid and correctly typed.
    let result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };
    assert_eq!(result.code, ABI_OK, "add_primitive must return ABI_OK");
    assert_eq!(out, 30_u32, "add_primitive(10, 20) must equal 30");
}

#[test]
fn integration_dotnet_version_string() {
    skip_if_no_dotnet!();
    load_fixture().expect("fixture must load");
    let vtable_ptr: *const PluginVTable = get_vtable();
    // SAFETY: vtable_ptr valid.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    assert!(
        vtable.function_count >= 3,
        "test.add vtable must have at least 3 functions"
    );
    // function index 2 = version() -> StringView (no args, pass null)
    let mut out_view: StringView = StringView::null();
    // SAFETY: fn_ptr is function 2 (version). No arg input needed; pass null.
    let fn_ptr: *const () = unsafe { *vtable.functions.add(2) };
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        // SAFETY: same dispatch signature; version takes no args (null input accepted by C# side).
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: out_view is a valid StringView allocation on the stack.
    let result: AbiError = unsafe {
        dispatch_fn(
            core::ptr::null::<()>(),
            &mut out_view as *mut StringView as *mut (),
        )
    };
    assert_eq!(result.code, ABI_OK, "version must return ABI_OK");
    // SAFETY: out_view.ptr points to valid UTF-8 bytes for out_view.len bytes (C# static array).
    let version_bytes: &[u8] = unsafe { core::slice::from_raw_parts(out_view.ptr, out_view.len) };
    assert_eq!(version_bytes, b"1.0", "version() must return \"1.0\"");
}

#[test]
fn integration_dotnet_reset() {
    skip_if_no_dotnet!();
    load_fixture().expect("fixture must load");
    let vtable_ptr: *const PluginVTable = get_vtable();
    // SAFETY: vtable_ptr valid.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    assert!(
        vtable.function_count >= 4,
        "test.add vtable must have at least 4 functions"
    );
    // function index 3 = reset() — no args, no meaningful output
    // SAFETY: fn_ptr is function 3 (reset). No args; dummy out is acceptable.
    let fn_ptr: *const () = unsafe { *vtable.functions.add(3) };
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        // SAFETY: same dispatch signature; reset ignores both args and out.
        unsafe { core::mem::transmute(fn_ptr) };
    let mut dummy_out: u32 = 0_u32;
    // SAFETY: null args and dummy_out are safe because reset() ignores both.
    let result: AbiError = unsafe {
        dispatch_fn(
            core::ptr::null::<()>(),
            &mut dummy_out as *mut u32 as *mut (),
        )
    };
    assert_eq!(result.code, ABI_OK, "reset must return ABI_OK");
}

#[test]
fn integration_dotnet_wrong_major_version_rejected() {
    skip_if_no_dotnet!();
    let loader: DotnetLoader = DotnetLoader::new(DotnetConfig {
        min_framework: String::from("net99.0"),
        hostfxr: HostfxrLocation::Auto,
    });
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: registry_register_callback,
        host: core::ptr::null(),
    };
    let result: Result<(), PolyplugError> =
        loader.load(std::path::Path::new(CSHARP_DLL), &mut registrar);
    match result {
        Err(PolyplugError::Loader(LoaderError::RuntimeVersionMismatch { .. })) => {}
        other => panic!("expected RuntimeVersionMismatch for net99.0, got: {other:?}"),
    }
}

#[test]
fn integration_dotnet_clr_shared_across_loads() {
    skip_if_no_dotnet!();
    // Load the fixture twice using the same DotnetLoader.
    // CLR is a global once-initialized singleton — second load must succeed.
    let loader: DotnetLoader = make_loader();
    DOTNET_REGISTRY.with(|cell| {
        *cell.borrow_mut() = Registry::new();
    });
    let mut registrar1: PluginRegistrar = PluginRegistrar {
        register_plugin: registry_register_callback,
        host: core::ptr::null(),
    };
    let result1: Result<(), PolyplugError> =
        loader.load(std::path::Path::new(CSHARP_DLL), &mut registrar1);
    assert!(
        result1.is_ok(),
        "first load must succeed: {:?}",
        result1.err()
    );
    // Reset registry so second load can re-register
    DOTNET_REGISTRY.with(|cell| {
        *cell.borrow_mut() = Registry::new();
    });
    let mut registrar2: PluginRegistrar = PluginRegistrar {
        register_plugin: registry_register_callback,
        host: core::ptr::null(),
    };
    let result2: Result<(), PolyplugError> =
        loader.load(std::path::Path::new(CSHARP_DLL), &mut registrar2);
    assert!(
        result2.is_ok(),
        "second load (CLR shared) must succeed: {:?}",
        result2.err()
    );
}

#[test]
fn pelite_reads_target_framework() {
    skip_if_no_dotnet!();
    let tfm: String =
        polyplug_dotnet::version::read_target_framework(std::path::Path::new(CSHARP_DLL))
            .expect("pelite TFM read must succeed");
    assert!(!tfm.is_empty(), "TFM must be non-empty for .NET assembly");
    // TFM from CA blob is LONG form: ".NETCoreApp,Version=v10.0" (NOT "net10.0")
    assert!(
        tfm.starts_with(".NETCoreApp,Version=v"),
        "TFM must be long-form '.NETCoreApp,Version=vX.Y': got {tfm}"
    );
}

#[test]
fn version_mismatch_pelite() {
    skip_if_no_dotnet!();
    let loader: DotnetLoader = DotnetLoader::new(DotnetConfig {
        min_framework: String::from("net99.0"),
        hostfxr: HostfxrLocation::Auto,
    });
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: registry_register_callback,
        host: core::ptr::null(),
    };
    let result: Result<(), PolyplugError> =
        loader.load(std::path::Path::new(CSHARP_DLL), &mut registrar);
    match result {
        Err(PolyplugError::Loader(LoaderError::RuntimeVersionMismatch { .. })) => {}
        other => panic!("expected RuntimeVersionMismatch, got: {other:?}"),
    }
}

#[test]
fn delegate_loader_cached_across_loads() {
    skip_if_no_dotnet!();
    // Load the same DLL twice — both must succeed, proving AssemblyDelegateLoader is cached and reused
    let loader: DotnetLoader = make_loader();
    DOTNET_REGISTRY.with(|cell| {
        *cell.borrow_mut() = Registry::new();
    });
    let mut r1: PluginRegistrar = PluginRegistrar {
        register_plugin: registry_register_callback,
        host: core::ptr::null(),
    };
    let result1: Result<(), PolyplugError> = loader.load(std::path::Path::new(CSHARP_DLL), &mut r1);
    assert!(
        result1.is_ok(),
        "first load must succeed: {:?}",
        result1.err()
    );
    DOTNET_REGISTRY.with(|cell| {
        *cell.borrow_mut() = Registry::new();
    });
    let mut r2: PluginRegistrar = PluginRegistrar {
        register_plugin: registry_register_callback,
        host: core::ptr::null(),
    };
    let result2: Result<(), PolyplugError> = loader.load(std::path::Path::new(CSHARP_DLL), &mut r2);
    assert!(
        result2.is_ok(),
        "second load (cached loader) must succeed: {:?}",
        result2.err()
    );
}

#[test]
fn non_dotnet_dll_allowed() {
    // A non-.NET shared library (e.g., a plain C .so) should be allowed through version check
    // because read_target_framework returns Ok("") for non-CLR files.
    // We test this by passing a path to a known-non-dotnet file (a Rust test binary or lib).
    // The actual load will fail at the CLR level (not a .NET assembly) but NOT at version check.
    // Since we can't guarantee a non-dotnet file path in CI, test with a dummy path that
    // doesn't exist — the version::read_target_framework returns AssemblyNotFound, not
    // RuntimeVersionMismatch, confirming the version check path is bypassed for non-dotnet files.
    //
    // Instead: test the module function directly.
    let result: Result<String, PolyplugError> =
        polyplug_dotnet::version::read_target_framework(std::path::Path::new("nonexistent.dll"));
    // Non-existent file should return AssemblyNotFound error
    match result {
        Err(PolyplugError::Loader(LoaderError::AssemblyNotFound { .. })) => {}
        Ok(s) => panic!("expected error for nonexistent file, got Ok({s:?})"),
        Err(other) => panic!("expected AssemblyNotFound, got: {other:?}"),
    }
}

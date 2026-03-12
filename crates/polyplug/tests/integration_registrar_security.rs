use core::ptr;

use polyplug::abi::AbiError;
use polyplug::abi::HostVTable;
use polyplug::abi::PluginDescriptor;
use polyplug::abi::PluginHandle;
use polyplug::abi::PluginRegistrar;
use polyplug::abi::PluginVTable;
use polyplug::abi::StringView;
use polyplug::registry::Registry;

const EMPTY_FNS: [*const (); 0] = [];

static VTABLE_MALFORMED: PluginVTable = PluginVTable {
    contract_id: 0xA1B2_C3D4_E5F6_0001_u64,
    contract_version: 0_u32,
    function_count: 0_u32,
    functions: EMPTY_FNS.as_ptr(),
};

static VTABLE_DUPLICATE: PluginVTable = PluginVTable {
    contract_id: 0xDEAD_BEEF_0000_0001_u64,
    contract_version: 0_u32,
    function_count: 0_u32,
    functions: EMPTY_FNS.as_ptr(),
};

unsafe extern "C" fn stub_alloc(_size: usize, _align: usize) -> *mut u8 {
    ptr::null_mut()
}

unsafe extern "C" fn stub_free(_ptr: *mut u8, _size: usize, _align: usize) {}

unsafe extern "C" fn stub_find_by_contract(_contract_id: u64, _min_version: u32) -> PluginHandle {
    PluginHandle::null()
}

unsafe extern "C" fn stub_find_by_bundle(
    _bundle_id: u64,
    _contract_id: u64,
    _min_version: u32,
) -> PluginHandle {
    PluginHandle::null()
}

unsafe extern "C" fn stub_find_all_by_contract(
    _contract_id: u64,
    _min_version: u32,
    _out: *mut PluginHandle,
    _out_cap: usize,
) -> usize {
    0_usize
}

unsafe extern "C" fn stub_resolve_plugin(_handle: PluginHandle) -> *const PluginVTable {
    ptr::null()
}

unsafe extern "C" fn stub_get_extension(_extension_id: u32) -> *const () {
    ptr::null()
}

static HOST_VTABLE: HostVTable = HostVTable {
    alloc: stub_alloc,
    free: stub_free,
    find_by_contract: stub_find_by_contract,
    find_by_bundle: stub_find_by_bundle,
    find_all_by_contract: stub_find_all_by_contract,
    resolve_plugin: stub_resolve_plugin,
    get_extension: stub_get_extension,
};

#[test]
fn registrar_callback_null_registry_ptr_returns_error() {
    let registry: Registry = Registry::new();
    let bundle_id: u64 = 0xABCD_u64;
    let mut context: polyplug::loader::testing::RegistrarContext =
        polyplug::loader::testing::make_registrar_context(&registry, bundle_id, &HOST_VTABLE);
    context.drop_guard();

    let descriptor: *const PluginDescriptor = ptr::null();
    let vtable: *const PluginVTable = ptr::null();
    let registrar: &mut PluginRegistrar = context.registrar_mut();
    let result: AbiError = unsafe {
        (registrar.register_plugin)(registrar as *mut PluginRegistrar, descriptor, vtable)
    };

    assert_eq!(result.code, 1_u32);
    assert!(result.message.ptr.is_null());
    assert_eq!(result.message.len, 0_usize);
}

#[test]
fn registrar_callback_accepts_null_stringviews() {
    let registry: Registry = Registry::new();
    let bundle_id: u64 = 0x1000_u64;
    let mut context: polyplug::loader::testing::RegistrarContext =
        polyplug::loader::testing::make_registrar_context(&registry, bundle_id, &HOST_VTABLE);

    let descriptor: PluginDescriptor = PluginDescriptor {
        name: StringView::null(),
        contract_name: StringView::null(),
        version_major: 1_u32,
        version_minor: 0_u32,
        version_patch: 0_u32,
    };
    let descriptor_ptr: *const PluginDescriptor = &descriptor as *const PluginDescriptor;
    let vtable: *const PluginVTable = &VTABLE_MALFORMED as *const PluginVTable;

    let registrar: &mut PluginRegistrar = context.registrar_mut();
    let result: AbiError = unsafe {
        (registrar.register_plugin)(registrar as *mut PluginRegistrar, descriptor_ptr, vtable)
    };

    assert_eq!(result.code, 0_u32);
}

#[test]
fn registrar_callback_duplicate_provider_returns_error() {
    let registry: Registry = Registry::new();
    let bundle_id: u64 = 0xBEEF_u64;
    let mut context: polyplug::loader::testing::RegistrarContext =
        polyplug::loader::testing::make_registrar_context(&registry, bundle_id, &HOST_VTABLE);

    let descriptor: PluginDescriptor = PluginDescriptor {
        name: StringView::from_static(b"dup-provider"),
        contract_name: StringView::from_static(b"dup.contract"),
        version_major: 1_u32,
        version_minor: 0_u32,
        version_patch: 0_u32,
    };
    let descriptor_ptr: *const PluginDescriptor = &descriptor as *const PluginDescriptor;
    let vtable: *const PluginVTable = &VTABLE_DUPLICATE as *const PluginVTable;

    let registrar: &mut PluginRegistrar = context.registrar_mut();
    let first: AbiError = unsafe {
        (registrar.register_plugin)(registrar as *mut PluginRegistrar, descriptor_ptr, vtable)
    };
    assert_eq!(first.code, 0_u32);

    let second: AbiError = unsafe {
        (registrar.register_plugin)(registrar as *mut PluginRegistrar, descriptor_ptr, vtable)
    };
    assert_eq!(second.code, 1_u32);
}

use core::ffi::c_void;
use core::mem;
use core::ptr;
use core::slice;
use core::str;
use std::collections::{HashMap, HashSet};
use std::os::raw::c_char;
use std::sync::{LazyLock, Mutex, PoisonError};

use mlua::ffi;
use polyplug_abi::dispatch::dispatch_mechanisms::DispatchMechanisms;
use polyplug_abi::dispatch::vm_dispatch::VmDispatch;
use polyplug_abi::types::{StringView, Version};
use polyplug_abi::{
    AbiError, AbiErrorCode, DispatchType, GuestContractHandle, GuestContractInstance,
    GuestContractInterface, HostApi, PluginDescriptor, VmLoaderData,
};
use polyplug_utils::{BundleId, GuestContractId};
const LUA_REGISTRYINDEX: i32 = -10000;
const LUA_TFUNCTION: i32 = 6;
const LUA_TTABLE: i32 = 5;
const LUA_OK: i32 = 0;

type LuaCFunction = unsafe extern "C" fn(*mut c_void) -> i32;
type LuaGetTop = unsafe extern "C" fn(*mut c_void) -> i32;
type LuaSetTop = unsafe extern "C" fn(*mut c_void, i32);
type LuaType = unsafe extern "C" fn(*mut c_void, i32) -> i32;
type LuaPushValue = unsafe extern "C" fn(*mut c_void, i32);
type LuaPushInteger = unsafe extern "C" fn(*mut c_void, isize);
type LuaPushLightUserdata = unsafe extern "C" fn(*mut c_void, *mut c_void);
type LuaPushCClosure = unsafe extern "C" fn(*mut c_void, LuaCFunction, i32);
type LuaCreateTable = unsafe extern "C" fn(*mut c_void, i32, i32);
type LuaSetField = unsafe extern "C" fn(*mut c_void, i32, *const c_char);
type LuaRawGetI = unsafe extern "C" fn(*mut c_void, i32, i32);
type LuaPCall = unsafe extern "C" fn(*mut c_void, i32, i32, i32) -> i32;
type LuaLRef = unsafe extern "C" fn(*mut c_void, i32) -> i32;
type LuaLUnref = unsafe extern "C" fn(*mut c_void, i32, i32);
type LuaToInteger = unsafe extern "C" fn(*mut c_void, i32) -> isize;
type LuaToLString = unsafe extern "C" fn(*mut c_void, i32, *mut usize) -> *const c_char;

#[cfg(unix)]
unsafe extern "C" {
    fn pthread_self() -> usize;
}

#[cfg(windows)]
unsafe extern "system" {
    #[link_name = "GetCurrentThreadId"]
    fn get_current_thread_id() -> u32;
}

unsafe extern "C" fn linked_gettop(lua: *mut c_void) -> i32 {
    // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { ffi::lua_gettop(lua.cast()) }
}

unsafe extern "C" fn linked_settop(lua: *mut c_void, index: i32) {
    // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { ffi::lua_settop(lua.cast(), index) }
}

unsafe extern "C" fn linked_type(lua: *mut c_void, index: i32) -> i32 {
    // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { ffi::lua_type(lua.cast(), index) }
}

unsafe extern "C" fn linked_pushvalue(lua: *mut c_void, index: i32) {
    // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { ffi::lua_pushvalue(lua.cast(), index) }
}

unsafe extern "C" fn linked_pushinteger(lua: *mut c_void, value: isize) {
    // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { ffi::lua_pushinteger(lua.cast(), value as ffi::lua_Integer) }
}

unsafe extern "C" fn linked_pushlightuserdata(lua: *mut c_void, pointer: *mut c_void) {
    // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { ffi::lua_pushlightuserdata(lua.cast(), pointer) }
}

unsafe extern "C" fn linked_pushcclosure(lua: *mut c_void, function: LuaCFunction, upvalues: i32) {
    let function: ffi::lua_CFunction = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { mem::transmute(function) };
    // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { ffi::lua_pushcclosure(lua.cast(), function, upvalues) }
}

unsafe extern "C" fn linked_createtable(lua: *mut c_void, array: i32, records: i32) {
    // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { ffi::lua_createtable(lua.cast(), array, records) }
}

unsafe extern "C" fn linked_setfield(lua: *mut c_void, index: i32, key: *const c_char) {
    // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { ffi::lua_setfield(lua.cast(), index, key) }
}

unsafe extern "C" fn linked_rawgeti(lua: *mut c_void, index: i32, reference: i32) {
    // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { ffi::lua_rawgeti_(lua.cast(), index, reference) }
}

unsafe extern "C" fn linked_pcall(
    lua: *mut c_void,
    argument_count: i32,
    result_count: i32,
    error_handler: i32,
) -> i32 {
    // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { ffi::lua_pcall(lua.cast(), argument_count, result_count, error_handler) }
}

unsafe extern "C" fn linked_lref(lua: *mut c_void, table: i32) -> i32 {
    // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { ffi::luaL_ref(lua.cast(), table) }
}

unsafe extern "C" fn linked_lunref(lua: *mut c_void, table: i32, reference: i32) {
    // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { ffi::luaL_unref(lua.cast(), table, reference) }
}

unsafe extern "C" fn linked_tointeger(lua: *mut c_void, index: i32) -> isize {
    // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { ffi::lua_tointeger_(lua.cast(), index) as isize }
}

unsafe extern "C" fn linked_tostring(
    lua: *mut c_void,
    index: i32,
    length: *mut usize,
) -> *const c_char {
    // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { ffi::lua_tolstring(lua.cast(), index, length) }
}

struct Api {
    gettop: LuaGetTop,
    settop: LuaSetTop,
    type_: LuaType,
    pushvalue: LuaPushValue,
    pushinteger: LuaPushInteger,
    pushlightuserdata: LuaPushLightUserdata,
    pushcclosure: LuaPushCClosure,
    createtable: LuaCreateTable,
    setfield: LuaSetField,
    rawgeti: LuaRawGetI,
    pcall: LuaPCall,
    lref: LuaLRef,
    lunref: LuaLUnref,
    tointeger: LuaToInteger,
    tostring: LuaToLString,
}

static BRIDGE_ADDRESSES: LazyLock<Mutex<HashSet<usize>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

impl Api {
    unsafe fn resolve() -> Self {
        Self {
            gettop: linked_gettop,
            settop: linked_settop,
            type_: linked_type,
            pushvalue: linked_pushvalue,
            pushinteger: linked_pushinteger,
            pushlightuserdata: linked_pushlightuserdata,
            pushcclosure: linked_pushcclosure,
            createtable: linked_createtable,
            setfield: linked_setfield,
            rawgeti: linked_rawgeti,
            pcall: linked_pcall,
            lref: linked_lref,
            lunref: linked_lunref,
            tointeger: linked_tointeger,
            tostring: linked_tostring,
        }
    }
}
struct ContractBridge {
    resident: *mut Resident,
    factory: i32,
    dispatcher: i32,
    contract_id: u64,
    plugin_name: String,
    contract_name: String,
    descriptor: PluginDescriptor,
    interface: GuestContractInterface,
    instances: Mutex<HashMap<u64, (i32, i32)>>,
    pending_implementation: Mutex<Option<i32>>,
    next_instance: Mutex<u64>,
}

struct Resident {
    lua: *mut c_void,
    api: Api,
    owner_thread_id: u64,
    manifest: String,
    contracts: Mutex<Vec<*mut ContractBridge>>,
}

impl Resident {
    fn is_owner_thread(&self) -> bool {
        self.owner_thread_id == owner_thread_id()
    }
}

fn bridge_for_interface(interface: *const GuestContractInterface) -> Option<*const ContractBridge> {
    if interface.is_null() {
        return None;
    }
    let adapter_context = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { (*interface).adapter_context };
    if adapter_context.is_null() {
        return None;
    }
    let address = adapter_context as usize;
    if !BRIDGE_ADDRESSES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .contains(&address)
    {
        return None;
    }
    Some(adapter_context.cast())
}

unsafe fn caller_interface_is_current(
    host: *const HostApi,
    handle: GuestContractHandle,
    interface: *const GuestContractInterface,
) -> bool {
    !host.is_null()
        && !interface.is_null()
        && // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        unsafe { ((*host).resolve_guest_contract)(host, handle) == interface }
}

fn owner_thread_id() -> u64 {
    #[cfg(unix)]
    {
        // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        unsafe { pthread_self() as u64 }
    }
    #[cfg(windows)]
    {
        // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        unsafe { u64::from(get_current_thread_id()) }
    }
}

unsafe fn restore(api: &Api, lua: *mut c_void, top: i32) {
    // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { (api.settop)(lua, top) };
}

unsafe fn push_ref(api: &Api, lua: *mut c_void, reference: i32) {
    // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { (api.rawgeti)(lua, LUA_REGISTRYINDEX, reference) };
}

unsafe fn registry_ref(api: &Api, lua: *mut c_void, index: i32) -> i32 {
    // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { (api.pushvalue)(lua, index) };
    // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { (api.lref)(lua, LUA_REGISTRYINDEX) }
}

unsafe fn lua_string(api: &Api, lua: *mut c_void, index: i32) -> Option<String> {
    let mut len = 0_usize;
    let bytes = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { (api.tostring)(lua, index, &mut len) };
    if bytes.is_null() {
        return None;
    }
    let bytes = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { slice::from_raw_parts(bytes.cast::<u8>(), len) };
    str::from_utf8(bytes).ok().map(str::to_owned)
}

fn string_view(value: &str) -> StringView {
    StringView {
        ptr: value.as_ptr(),
        len: value.len(),
    }
}

unsafe fn write_error(out_error: *mut AbiError, code: AbiErrorCode, message: &'static [u8]) {
    if !out_error.is_null() {
        // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        unsafe {
            out_error.write(AbiError {
                code: code as u32,
                message: StringView::from_static(message),
            });
        }
    }
}

unsafe extern "C" fn create_resident(lua: *mut c_void) -> i32 {
    let api = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { Api::resolve() };
    let Some(manifest) = (
        // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        unsafe { lua_string(&api, lua, 1) }
    ) else {
        return 0;
    };
    let resident = Box::new(Resident {
        lua,
        api,
        owner_thread_id: owner_thread_id(),
        manifest,
        contracts: Mutex::new(Vec::new()),
    });
    // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { (resident.api.pushinteger)(lua, Box::into_raw(resident) as isize) };
    1
}

unsafe extern "C" fn add_provider(lua: *mut c_void) -> i32 {
    let api = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { Api::resolve() };
    let resident = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { (api.tointeger)(lua, 1) as usize as *mut Resident };
    if resident.is_null()
        || !// SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        unsafe { (&*resident).is_owner_thread() }
        || // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        unsafe { (api.type_)(lua, 2) } != LUA_TFUNCTION
        || // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        unsafe { (api.type_)(lua, 3) } != LUA_TFUNCTION
    {
        return 0;
    }
    let Some(plugin_name) = (
        // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        unsafe { lua_string(&api, lua, 4) }
    ) else {
        return 0;
    };
    let Some(contract_name) = (
        // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        unsafe { lua_string(&api, lua, 5) }
    ) else {
        return 0;
    };
    let contract_id = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { (api.tointeger)(lua, 6) as u32 as u64 | ((api.tointeger)(lua, 7) as u32 as u64) << 32 };
    let version = Version {
        major: // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        unsafe { (api.tointeger)(lua, 8) as u32 },
        minor: // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        unsafe { (api.tointeger)(lua, 9) as u32 },
        patch: // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        unsafe { (api.tointeger)(lua, 10) as u32 },
    };
    if contract_id == 0 {
        return 0;
    }
    let factory = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { registry_ref(&api, lua, 2) };
    let dispatcher = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { registry_ref(&api, lua, 3) };
    let mut bridge = Box::new(ContractBridge {
        resident,
        factory,
        dispatcher,
        contract_id,
        plugin_name,
        contract_name,
        descriptor: PluginDescriptor {
            name: StringView::null(),
            contract_name: StringView::null(),
            version,
        },
        interface: GuestContractInterface {
            contract_id: GuestContractId::from_u64(contract_id),
            contract_version: version,
            dispatch_type: DispatchType::VirtualMachine,
            adapter_context: ptr::null_mut(),
            create_instance: polyplug_lua_internal_plugin_create_instance,
            destroy_instance: polyplug_lua_internal_plugin_destroy_instance,
            dispatch: DispatchMechanisms {
                vm: VmDispatch {
                    call: polyplug_lua_internal_plugin_vm_dispatch,
                    loader_data: VmLoaderData::null(),
                },
            },
        },
        instances: Mutex::new(HashMap::new()),
        pending_implementation: Mutex::new(None),
        next_instance: Mutex::new(1),
    });
    bridge.descriptor.name = string_view(&bridge.plugin_name);
    bridge.descriptor.contract_name = string_view(&bridge.contract_name);
    let bridge = Box::into_raw(bridge);
    BRIDGE_ADDRESSES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(bridge as usize);
    // SAFETY: The validated bridge owns the referenced allocation for this call, and the live owner-thread Lua state keeps the C API pointers and stack valid.
    unsafe {
        (*bridge).interface.adapter_context = bridge.cast();
        (&*resident)
            .contracts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(bridge);
        (api.pushinteger)(lua, bridge as isize);
    }
    1
}

/// Creates a Lua-backed guest instance.
///
/// # Safety
/// `adapter_context` must be a live `ContractBridge` owned by this resident;
/// `out_instance` must be writable, and `host` must be valid while the factory runs.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_lua_internal_plugin_create_instance(
    adapter_context: *mut c_void,
    _loader_data: VmLoaderData,
    host: *const HostApi,
    _args: *const (),
    out_instance: *mut GuestContractInstance,
) {
    if out_instance.is_null() {
        return;
    }
    // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { out_instance.write(GuestContractInstance::null()) };
    if adapter_context.is_null() {
        return;
    }
    let bridge = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { &*adapter_context.cast::<ContractBridge>() };
    let resident = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { &*bridge.resident };
    if !resident.is_owner_thread() {
        return;
    }
    let top = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { (resident.api.gettop)(resident.lua) };
    let implementation = bridge
        .pending_implementation
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take();
    let implementation = if let Some(implementation) = implementation {
        // SAFETY: The validated bridge owns the referenced allocation for this call, and the live owner-thread Lua state keeps the C API pointers and stack valid.
        unsafe {
            push_ref(&resident.api, resident.lua, implementation);
            if (resident.api.type_)(resident.lua, -1) != LUA_TTABLE {
                (resident.api.lunref)(resident.lua, LUA_REGISTRYINDEX, implementation);
                restore(&resident.api, resident.lua, top);
                return;
            }
            restore(&resident.api, resident.lua, top);
        }
        implementation
    } else {
        // SAFETY: The validated bridge owns the referenced allocation for this call, and the live owner-thread Lua state keeps the C API pointers and stack valid.
        unsafe {
            push_ref(&resident.api, resident.lua, bridge.factory);
            (resident.api.pushlightuserdata)(resident.lua, host.cast_mut().cast());
        }
        if
        // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        unsafe { (resident.api.pcall)(resident.lua, 1, 1, 0) } != LUA_OK
            || // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
            unsafe { (resident.api.type_)(resident.lua, -1) } != LUA_TTABLE
        {
            // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
            unsafe { restore(&resident.api, resident.lua, top) };
            return;
        }
        // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        unsafe { (resident.api.lref)(resident.lua, LUA_REGISTRYINDEX) }
    };
    // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { (resident.api.createtable)(resident.lua, 0, 3) };
    let roots = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { (resident.api.lref)(resident.lua, LUA_REGISTRYINDEX) };
    let id = {
        let mut next = bridge
            .next_instance
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let id = *next;
        let Some(next_id) = id.checked_add(1) else {
            // SAFETY: The validated bridge owns the referenced allocation for this call, and the live owner-thread Lua state keeps the C API pointers and stack valid.
            unsafe {
                (resident.api.lunref)(resident.lua, LUA_REGISTRYINDEX, implementation);
                (resident.api.lunref)(resident.lua, LUA_REGISTRYINDEX, roots);
                restore(&resident.api, resident.lua, top);
            }
            return;
        };
        *next = next_id;
        id
    };
    bridge
        .instances
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(id, (implementation, roots));
    // SAFETY: The validated bridge owns the referenced allocation for this call, and the live owner-thread Lua state keeps the C API pointers and stack valid.
    unsafe {
        out_instance.write(GuestContractInstance {
            data: id as *mut c_void,
            contract_id: GuestContractId::from_u64(bridge.contract_id),
        });
        restore(&resident.api, resident.lua, top);
    }
}

/// Destroys a previously created Lua-backed guest instance.
///
/// # Safety
/// `adapter_context` must be a live `ContractBridge` for the owner-thread resident,
/// and `instance` must be either null or an instance returned by that bridge.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_lua_internal_plugin_destroy_instance(
    adapter_context: *mut c_void,
    _loader_data: VmLoaderData,
    _host: *const HostApi,
    instance: GuestContractInstance,
) {
    if adapter_context.is_null() || instance.data.is_null() {
        return;
    }
    let bridge = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { &*adapter_context.cast::<ContractBridge>() };
    let resident = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { &*bridge.resident };
    if !resident.is_owner_thread() {
        return;
    }
    if let Some((implementation, roots)) = bridge
        .instances
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(&(instance.data as usize as u64))
    {
        // SAFETY: The validated bridge owns the referenced allocation for this call, and the live owner-thread Lua state keeps the C API pointers and stack valid.
        unsafe {
            (resident.api.lunref)(resident.lua, LUA_REGISTRYINDEX, implementation);
            (resident.api.lunref)(resident.lua, LUA_REGISTRYINDEX, roots);
        }
    }
}

/// Releases a Lua resident and every bridge it owns.
///
/// # Safety
/// `resident` must be null or the unique allocation returned by `create_resident`;
/// this function must run on its recorded Lua owner thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_lua_internal_plugin_release(resident: *mut c_void) {
    if resident.is_null() {
        return;
    }
    let resident = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { Box::from_raw(resident.cast::<Resident>()) };
    if !resident.is_owner_thread() {
        let _ = Box::into_raw(resident);
        return;
    }
    let Resident {
        lua,
        api,
        owner_thread_id: _,
        manifest: _,
        contracts,
    } = *resident;
    let contracts = contracts
        .into_inner()
        .unwrap_or_else(PoisonError::into_inner);
    for bridge in contracts {
        BRIDGE_ADDRESSES
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&(bridge as usize));
        let bridge = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        unsafe { Box::from_raw(bridge) };
        for (_, (implementation, roots)) in bridge
            .instances
            .into_inner()
            .unwrap_or_else(PoisonError::into_inner)
        {
            // SAFETY: The validated bridge owns the referenced allocation for this call, and the live owner-thread Lua state keeps the C API pointers and stack valid.
            unsafe {
                (api.lunref)(lua, LUA_REGISTRYINDEX, implementation);
                (api.lunref)(lua, LUA_REGISTRYINDEX, roots);
            }
        }
        // SAFETY: The validated bridge owns the referenced allocation for this call, and the live owner-thread Lua state keeps the C API pointers and stack valid.
        unsafe {
            (api.lunref)(lua, LUA_REGISTRYINDEX, bridge.factory);
            (api.lunref)(lua, LUA_REGISTRYINDEX, bridge.dispatcher);
        }
    }
}

unsafe fn dispatch(
    bridge: &ContractBridge,
    instance: GuestContractInstance,
    fn_id: u32,
    args: *const (),
    out: *mut (),
    out_error: *mut AbiError,
) {
    let resident = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { &*bridge.resident };
    if !resident.is_owner_thread() {
        // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        unsafe {
            write_error(
                out_error,
                AbiErrorCode::Generic,
                b"Lua internal-plugin call used a non-owner thread",
            )
        };
        return;
    }
    let entry = bridge
        .instances
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&(instance.data as usize as u64))
        .copied();
    let Some((implementation, roots)) = entry else {
        // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        unsafe {
            write_error(
                out_error,
                AbiErrorCode::Generic,
                b"unknown Lua internal-plugin instance",
            )
        };
        return;
    };
    let top = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { (resident.api.gettop)(resident.lua) };
    // SAFETY: The validated bridge owns the referenced allocation for this call, and the live owner-thread Lua state keeps the C API pointers and stack valid.
    unsafe {
        push_ref(&resident.api, resident.lua, bridge.dispatcher);
        push_ref(&resident.api, resident.lua, implementation);
        (resident.api.pushinteger)(resident.lua, fn_id as isize);
        (resident.api.pushlightuserdata)(resident.lua, args.cast_mut().cast());
        (resident.api.pushlightuserdata)(resident.lua, out.cast());
        push_ref(&resident.api, resident.lua, roots);
    }
    let valid_dispatch_stack = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { (resident.api.gettop)(resident.lua) == top + 6
        && (resident.api.type_)(resident.lua, top + 1) == LUA_TFUNCTION
        && (resident.api.type_)(resident.lua, top + 2) == LUA_TTABLE
        && (resident.api.type_)(resident.lua, top + 6) == LUA_TTABLE };
    if !valid_dispatch_stack {
        // SAFETY: The validated bridge owns the referenced allocation for this call, and the live owner-thread Lua state keeps the C API pointers and stack valid.
        unsafe {
            restore(&resident.api, resident.lua, top);
            write_error(
                out_error,
                AbiErrorCode::Generic,
                b"Lua internal-plugin dispatch has invalid state",
            );
        }
        return;
    }
    let status = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { (resident.api.pcall)(resident.lua, 5, 1, 0) };
    if
    // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { (resident.api.gettop)(resident.lua) } != top + 1 {
        // SAFETY: The validated bridge owns the referenced allocation for this call, and the live owner-thread Lua state keeps the C API pointers and stack valid.
        unsafe {
            restore(&resident.api, resident.lua, top);
            write_error(
                out_error,
                AbiErrorCode::Generic,
                b"Lua internal-plugin dispatch has invalid result state",
            );
        }
        return;
    }
    let code = if status == LUA_OK {
        // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        unsafe { (resident.api.tointeger)(resident.lua, -1) as u32 }
    } else {
        AbiErrorCode::Generic as u32
    };
    // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { restore(&resident.api, resident.lua, top) };
    // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe {
        write_error(
            out_error,
            if code == 0 {
                AbiErrorCode::Ok
            } else {
                AbiErrorCode::Generic
            },
            b"Lua internal-plugin dispatch failed",
        )
    };
}
/// Dispatches an internal Lua guest contract call.
///
/// # Safety
/// `adapter_context` must identify a live owner-thread `ContractBridge`; `args`,
/// `out`, and `out_error` must meet the ABI contract for `fn_id`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_lua_internal_plugin_vm_dispatch(
    adapter_context: *mut c_void,
    _loader_data: VmLoaderData,
    instance: GuestContractInstance,
    fn_id: u32,
    args: *const (),
    out: *mut (),
    _arena: *mut polyplug_abi::CallArena,
    out_error: *mut AbiError,
) {
    if adapter_context.is_null() {
        // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        unsafe {
            write_error(
                out_error,
                AbiErrorCode::InvalidPointer,
                b"Lua internal-plugin bridge is null",
            )
        };
        return;
    }
    // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe {
        dispatch(
            &*adapter_context.cast::<ContractBridge>(),
            instance,
            fn_id,
            args,
            out,
            out_error,
        )
    };
}

unsafe fn integer_pointer(api: &Api, lua: *mut c_void, index: i32) -> *mut c_void {
    let value = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { (api.tointeger)(lua, index) };
    if value < 0 || (value as i128) >= 9_007_199_254_740_992 {
        return ptr::null_mut();
    }
    value as usize as *mut c_void
}

unsafe fn caller_replacement(
    host: *const HostApi,
    handle: GuestContractHandle,
) -> Option<(*const GuestContractInterface, GuestContractInstance, u64)> {
    if host.is_null() {
        return None;
    }
    let interface = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { ((*host).resolve_guest_contract)(host, handle) };
    if interface.is_null() {
        return None;
    }
    let mut instance = GuestContractInstance::null();
    // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { ((*host).create_guest_instance)(host, interface, ptr::null(), &mut instance) };
    let revision = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { ((*host).registry_revision)(host) };
    Some((interface, instance, revision))
}

unsafe extern "C" fn caller_resolve_from_handle(lua: *mut c_void) -> i32 {
    let api = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { Api::resolve() };
    let host = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { integer_pointer(&api, lua, 1).cast::<HostApi>().cast_const() };
    let handle = GuestContractHandle {
        index: // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        unsafe { (api.tointeger)(lua, 2) as u32 },
        generation: // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        unsafe { (api.tointeger)(lua, 3) as u32 },
    };
    if host.is_null() {
        // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        unsafe { (api.pushinteger)(lua, 0) };
        return 1;
    }
    let interface = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { ((*host).resolve_guest_contract)(host, handle) };
    if interface.is_null() {
        // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        unsafe { (api.pushinteger)(lua, 0) };
        return 1;
    }
    let Some(bridge) = bridge_for_interface(interface) else {
        // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        unsafe { (api.pushinteger)(lua, 0) };
        return 1;
    };
    let bridge = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { &*bridge };
    let resident = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { &*bridge.resident };
    if !resident.is_owner_thread() {
        // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        unsafe { (api.pushinteger)(lua, 0) };
        return 1;
    }
    let revision = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { ((*host).registry_revision)(host) };
    // SAFETY: The validated bridge owns the referenced allocation for this call, and the live owner-thread Lua state keeps the C API pointers and stack valid.
    unsafe {
        (api.pushinteger)(lua, 1);
        (api.pushinteger)(lua, handle.index as isize);
        (api.pushinteger)(lua, handle.generation as isize);
        (api.pushinteger)(lua, interface as isize);
        (api.pushinteger)(lua, revision as isize);
        push_ref(&resident.api, resident.lua, bridge.factory);
    }
    6
}

unsafe extern "C" fn caller_create_with_implementation(lua: *mut c_void) -> i32 {
    let api = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { Api::resolve() };
    let host = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { integer_pointer(&api, lua, 1).cast::<HostApi>().cast_const() };
    let interface = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { integer_pointer(&api, lua, 2)
        .cast::<GuestContractInterface>()
        .cast_const() };
    if host.is_null() || interface.is_null() || // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { (api.type_)(lua, 3) } != LUA_TTABLE
    {
        // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        unsafe { (api.pushinteger)(lua, 0) };
        return 1;
    }
    let Some(bridge) = bridge_for_interface(interface) else {
        // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        unsafe { (api.pushinteger)(lua, 0) };
        return 1;
    };
    let bridge = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { &*bridge };
    let resident = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { &*bridge.resident };
    if !resident.is_owner_thread() {
        // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        unsafe { (api.pushinteger)(lua, 0) };
        return 1;
    }
    let implementation = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { registry_ref(&api, lua, 3) };
    {
        let mut pending = bridge
            .pending_implementation
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if pending.replace(implementation).is_some() {
            // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
            unsafe { (api.lunref)(lua, LUA_REGISTRYINDEX, implementation) };
            // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
            unsafe { (api.pushinteger)(lua, 0) };
            return 1;
        }
    }
    let mut instance = GuestContractInstance::null();
    // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { ((*host).create_guest_instance)(host, interface, ptr::null(), &mut instance) };
    if let Some(unused) = bridge
        .pending_implementation
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
    {
        // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        unsafe { (api.lunref)(lua, LUA_REGISTRYINDEX, unused) };
    }
    // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { (api.pushinteger)(lua, instance.data as isize) };
    1
}

unsafe extern "C" fn caller_destroy(lua: *mut c_void) -> i32 {
    let api = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { Api::resolve() };
    let host = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { integer_pointer(&api, lua, 1).cast::<HostApi>().cast_const() };

    let interface = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { integer_pointer(&api, lua, 5)
        .cast::<GuestContractInterface>()
        .cast_const() };
    let instance = GuestContractInstance {
        data: // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        unsafe { integer_pointer(&api, lua, 6) },
        contract_id: GuestContractId::from_u64(0),
    };
    let handle = GuestContractHandle {
        index: // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        unsafe { (api.tointeger)(lua, 2) as u32 },
        generation: // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        unsafe { (api.tointeger)(lua, 3) as u32 },
    };
    if
    // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { caller_interface_is_current(host, handle, interface) } {
        // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        unsafe { ((*host).destroy_guest_instance)(host, interface, instance) };
    }
    0
}

unsafe extern "C" fn caller_reset(lua: *mut c_void) -> i32 {
    let api = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { Api::resolve() };
    let host = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { integer_pointer(&api, lua, 1).cast::<HostApi>().cast_const() };
    let handle = GuestContractHandle {
        index: // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        unsafe { (api.tointeger)(lua, 2) as u32 },
        generation: // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        unsafe { (api.tointeger)(lua, 3) as u32 },
    };
    let interface = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { integer_pointer(&api, lua, 5)
        .cast::<GuestContractInterface>()
        .cast_const() };
    let instance = GuestContractInstance {
        data: // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        unsafe { integer_pointer(&api, lua, 6) },
        contract_id: GuestContractId::from_u64(0),
    };
    if
    // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { caller_interface_is_current(host, handle, interface) } {
        // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        unsafe { ((*host).destroy_guest_instance)(host, interface, instance) };
    }
    let Some((interface, instance, revision)) = (
        // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        unsafe { caller_replacement(host, handle) }
    ) else {
        // SAFETY: The validated bridge owns the referenced allocation for this call, and the live owner-thread Lua state keeps the C API pointers and stack valid.
        unsafe {
            (api.pushinteger)(lua, 0);
            (api.pushinteger)(lua, 0);
            (api.pushinteger)(lua, 0);
            (api.pushinteger)(lua, 0);
        }
        return 4;
    };
    // SAFETY: The validated bridge owns the referenced allocation for this call, and the live owner-thread Lua state keeps the C API pointers and stack valid.
    unsafe {
        (api.pushinteger)(lua, 1);
        (api.pushinteger)(lua, interface as isize);
        (api.pushinteger)(lua, instance.data as isize);
        (api.pushinteger)(lua, revision as isize);
    }
    4
}

unsafe extern "C" fn release_resident(lua: *mut c_void) -> i32 {
    let api = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { Api::resolve() };
    let resident = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { integer_pointer(&api, lua, 1).cast::<Resident>() };
    if resident.is_null()
        || !// SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { (&*resident).is_owner_thread() }
    {
        // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        unsafe { (api.pushinteger)(lua, 0) };
        return 1;
    }
    // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { polyplug_lua_internal_plugin_release(resident.cast()) };
    // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { (api.pushinteger)(lua, 1) };
    1
}
type BeginInternalPlugin =
    unsafe extern "C" fn(*const HostApi, *const u8, usize, u32, *mut u64, *mut AbiError);
type AttachInternalPluginResident = unsafe extern "C" fn(
    *const HostApi,
    u64,
    *mut c_void,
    u64,
    unsafe extern "C" fn(*mut c_void),
    *mut AbiError,
) -> bool;
type CurrentOsThreadId = unsafe extern "C" fn() -> u64;
type CommitInternalPlugin = unsafe extern "C" fn(
    *const HostApi,
    u64,
    *mut GuestContractHandle,
    usize,
    *mut usize,
    *mut AbiError,
);
type AbortInternalPlugin = unsafe extern "C" fn(*const HostApi, u64);

fn transaction_needs_abort(began: bool, commit_attempted: bool) -> bool {
    began && !commit_attempted
}

unsafe extern "C" fn register_transaction(lua: *mut c_void) -> i32 {
    let api = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { Api::resolve() };
    let resident = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { integer_pointer(&api, lua, 1).cast::<Resident>() };
    let host = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { integer_pointer(&api, lua, 2).cast::<HostApi>().cast_const() };
    let begin_address = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { integer_pointer(&api, lua, 3) };
    let attach_address = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { integer_pointer(&api, lua, 4) };
    let current_thread_address = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { integer_pointer(&api, lua, 5) };
    let commit_address = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { integer_pointer(&api, lua, 6) };
    let abort_address = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { integer_pointer(&api, lua, 7) };
    let mut error = AbiError::ok();
    let mut attached = false;
    let mut commit_attempted = false;
    if resident.is_null()
        || host.is_null()
        || begin_address.is_null()
        || attach_address.is_null()
        || current_thread_address.is_null()
        || commit_address.is_null()
        || abort_address.is_null()
        || !// SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        unsafe { (&*resident).is_owner_thread() }
    {
        // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        unsafe {
            write_error(
                &mut error,
                AbiErrorCode::InvalidPointer,
                b"Lua registration gateway received an invalid token",
            )
        };
    } else {
        let begin: BeginInternalPlugin = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        unsafe { mem::transmute(begin_address) };
        let attach: AttachInternalPluginResident = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        unsafe { mem::transmute(attach_address) };
        let current_thread: CurrentOsThreadId = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        unsafe { mem::transmute(current_thread_address) };
        let commit: CommitInternalPlugin = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        unsafe { mem::transmute(commit_address) };
        let abort: AbortInternalPlugin = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        unsafe { mem::transmute(abort_address) };
        let resident_ref = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        unsafe { &*resident };
        let mut raw_bundle_id = 0_u64;
        // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        unsafe {
            begin(
                host,
                resident_ref.manifest.as_ptr(),
                resident_ref.manifest.len(),
                4,
                &mut raw_bundle_id,
                &mut error,
            )
        };
        let began = error.is_ok();
        let bundle_id = BundleId::from_u64(raw_bundle_id);
        if error.is_ok() {
            attached = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
            unsafe { attach(
                host,
                bundle_id.id(),
                resident.cast(),
                current_thread(),
                polyplug_lua_internal_plugin_release,
                &mut error,
            ) };
        }
        if error.is_ok() && attached {
            let contracts = resident_ref
                .contracts
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            for bridge in contracts.iter().copied() {
                let bridge = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
                unsafe { &*bridge };
                // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
                unsafe {
                    ((*host).register_guest_contract)(
                        host,
                        &bridge.descriptor,
                        &bridge.interface,
                        &mut error,
                    )
                };
                if !error.is_ok() {
                    break;
                }
            }
            if error.is_ok() {
                let mut handles = vec![GuestContractHandle::null(); contracts.len()];
                let mut handle_count = 0_usize;
                commit_attempted = true;
                // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
                unsafe {
                    commit(
                        host,
                        bundle_id.id(),
                        handles.as_mut_ptr(),
                        handles.len(),
                        &mut handle_count,
                        &mut error,
                    )
                };
                if handle_count > handles.len() {
                    // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
                    unsafe {
                        write_error(
                            &mut error,
                            AbiErrorCode::Generic,
                            b"Lua registration commit returned too many handles",
                        )
                    };
                }
                if error.is_ok() {
                    // SAFETY: The validated bridge owns the referenced allocation for this call, and the live owner-thread Lua state keeps the C API pointers and stack valid.
                    unsafe {
                        (api.pushinteger)(lua, 1);
                        (api.pushinteger)(lua, bundle_id.id() as u32 as isize);
                        (api.pushinteger)(lua, (bundle_id.id() >> 32) as u32 as isize);
                        (api.pushinteger)(lua, handle_count as isize);
                        for handle in handles.iter().take(handle_count) {
                            (api.pushinteger)(lua, handle.index as isize);
                            (api.pushinteger)(lua, handle.generation as isize);
                        }
                    }
                    return 4 + (handle_count * 2) as i32;
                }
            }
        }
        if transaction_needs_abort(began, commit_attempted) {
            // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
            unsafe { abort(host, bundle_id.id()) };
        }
        if !attached {
            // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
            unsafe { polyplug_lua_internal_plugin_release(resident.cast()) };
        }
    }
    // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { (api.pushinteger)(lua, 0) };
    1
}

/// Exports the native Lua bridge module into a Lua state.
///
/// # Safety
/// `lua` must be a live LuaJIT state on its owner thread with sufficient stack space
/// for the exported table and closures.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn luaopen_polyplug_lua_bridge(lua: *mut c_void) -> i32 {
    let api = // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
    unsafe { Api::resolve() };
    // SAFETY: The validated bridge owns the referenced allocation for this call, and the live owner-thread Lua state keeps the C API pointers and stack valid.
    unsafe {
        (api.createtable)(lua, 0, 8);
        (api.pushcclosure)(lua, create_resident, 0);
        (api.setfield)(lua, -2, c"create_resident".as_ptr());
        (api.pushcclosure)(lua, add_provider, 0);
        (api.setfield)(lua, -2, c"add_provider".as_ptr());
        (api.pushcclosure)(lua, release_resident, 0);
        (api.setfield)(lua, -2, c"release_resident".as_ptr());
        (api.pushcclosure)(lua, register_transaction, 0);
        (api.setfield)(lua, -2, c"register_transaction".as_ptr());
        (api.pushcclosure)(lua, caller_resolve_from_handle, 0);
        (api.setfield)(lua, -2, c"caller_resolve_from_handle".as_ptr());
        (api.pushcclosure)(lua, caller_create_with_implementation, 0);
        (api.setfield)(lua, -2, c"caller_create_with_implementation".as_ptr());
        (api.pushcclosure)(lua, caller_destroy, 0);
        (api.setfield)(lua, -2, c"caller_destroy".as_ptr());
        (api.pushcclosure)(lua, caller_reset, 0);
        (api.setfield)(lua, -2, c"caller_reset".as_ptr());
    }
    1
}

#[cfg(test)]
mod tests {
    use core::ffi::{c_char, c_void};
    use core::ptr;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::thread;

    use mlua::{Function, Lua, Table, ffi};

    use polyplug_abi::dispatch::dispatch_mechanisms::DispatchMechanisms;
    use polyplug_abi::dispatch::vm_dispatch::VmDispatch;
    use polyplug_abi::types::{StringView, Version};
    use polyplug_abi::{
        AbiError, AbiErrorCode, DispatchType, GuestContractInstance, GuestContractInterface,
        PluginDescriptor, VmLoaderData,
    };
    use polyplug_utils::GuestContractId;

    use super::Api;
    use super::ContractBridge;
    use super::Resident;
    use super::luaopen_polyplug_lua_bridge;
    use super::polyplug_lua_internal_plugin_create_instance;
    use super::polyplug_lua_internal_plugin_vm_dispatch;

    unsafe extern "C" fn gettop(_: *mut c_void) -> i32 {
        0
    }

    unsafe extern "C" fn settop(_: *mut c_void, _: i32) {}

    unsafe extern "C" fn type_(_: *mut c_void, _: i32) -> i32 {
        0
    }

    unsafe extern "C" fn pushvalue(_: *mut c_void, _: i32) {}

    unsafe extern "C" fn pushinteger(_: *mut c_void, _: isize) {}

    unsafe extern "C" fn pushlightuserdata(_: *mut c_void, _: *mut c_void) {}

    unsafe extern "C" fn pushcclosure(_: *mut c_void, _: super::LuaCFunction, _: i32) {}

    unsafe extern "C" fn createtable(_: *mut c_void, _: i32, _: i32) {}

    unsafe extern "C" fn setfield(_: *mut c_void, _: i32, _: *const c_char) {}

    unsafe extern "C" fn rawgeti(_: *mut c_void, _: i32, _: i32) {}

    unsafe extern "C" fn pcall(_: *mut c_void, _: i32, _: i32, _: i32) -> i32 {
        0
    }

    unsafe extern "C" fn lref(_: *mut c_void, _: i32) -> i32 {
        0
    }

    unsafe extern "C" fn lunref(_: *mut c_void, _: i32, _: i32) {}

    unsafe extern "C" fn tointeger(_: *mut c_void, _: i32) -> isize {
        0
    }

    unsafe extern "C" fn tostring(_: *mut c_void, _: i32, _: *mut usize) -> *const c_char {
        ptr::null()
    }

    fn api() -> Api {
        Api {
            gettop,
            settop,
            type_,
            pushvalue,
            pushinteger,
            pushlightuserdata,
            pushcclosure,
            createtable,
            setfield,
            rawgeti,
            pcall,
            lref,
            lunref,
            tointeger,
            tostring,
        }
    }

    fn registry_reference(lua: &Lua, function: Function) -> i32 {
        // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        match unsafe {
            lua.exec_raw(function, |state| {
                let reference = ffi::luaL_ref(state, ffi::LUA_REGISTRYINDEX);
                ffi::lua_pushinteger(state, i64::from(reference));
            })
        } {
            Ok(reference) => reference,
            Err(error) => panic!("storing Lua function in the registry failed: {error}"),
        }
    }

    #[test]
    fn live_bridge_uses_linked_luajit_api_and_exports_only_supported_gateways() {
        let lua = Lua::new();
        // SAFETY: `Lua::exec_raw` supplies its live owner-thread state to the module
        // initializer, which leaves the bridge table at the expected stack position.
        let bridge_result = unsafe {
            lua.exec_raw((), |state| {
                luaopen_polyplug_lua_bridge(state.cast());
            })
        };
        let bridge: Table = match bridge_result {
            Ok(bridge) => bridge,
            Err(error) => panic!("opening the linked LuaJIT bridge failed: {error}"),
        };
        for name in [
            "create_resident",
            "add_provider",
            "release_resident",
            "register_transaction",
            "caller_resolve_from_handle",
            "caller_create_with_implementation",
            "caller_destroy",
            "caller_reset",
        ] {
            assert!(
                match bridge.contains_key(name) {
                    Ok(present) => present,
                    Err(error) => panic!("inspecting bridge field {name} failed: {error}"),
                },
                "bridge must export {name}"
            );
        }
        for name in [
            "caller_create",
            "caller_create_from_handle",
            "caller_prepare",
            "caller_call",
            "write_u32",
        ] {
            assert!(
                !match bridge.contains_key(name) {
                    Ok(present) => present,
                    Err(error) => panic!("inspecting bridge field {name} failed: {error}"),
                },
                "bridge must not export raw gateway {name}"
            );
        }
    }

    #[test]
    fn generic_factory_rejects_non_table_without_publishing_an_instance() {
        let lua = Lua::new();
        let factory: Function = match lua.load("return function() return 17 end").eval() {
            Ok(factory) => factory,
            Err(error) => panic!("creating non-table factory failed: {error}"),
        };
        let factory = registry_reference(&lua, factory);
        let state = lua.exec_raw_lua(|raw| raw.state().cast());
        let resident = Box::into_raw(Box::new(Resident {
            lua: state,
            api: // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
            unsafe { Api::resolve() },
            owner_thread_id: super::owner_thread_id(),
            manifest: String::new(),
            contracts: Mutex::new(Vec::new()),
        }));
        let bridge = Box::into_raw(Box::new(ContractBridge {
            resident,
            factory,
            dispatcher: 0,
            contract_id: 1,
            plugin_name: String::new(),
            contract_name: String::new(),
            descriptor: PluginDescriptor {
                name: StringView::null(),
                contract_name: StringView::null(),
                version: Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
            },
            interface: GuestContractInterface {
                contract_id: GuestContractId::from_u64(1),
                contract_version: Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
                dispatch_type: DispatchType::VirtualMachine,
                adapter_context: ptr::null_mut(),
                create_instance: super::polyplug_lua_internal_plugin_create_instance,
                destroy_instance: super::polyplug_lua_internal_plugin_destroy_instance,
                dispatch: DispatchMechanisms {
                    vm: VmDispatch {
                        call: super::polyplug_lua_internal_plugin_vm_dispatch,
                        loader_data: VmLoaderData::null(),
                    },
                },
            },
            instances: Mutex::new(HashMap::new()),
            pending_implementation: Mutex::new(None),
            next_instance: Mutex::new(1),
        }));
        let mut instance = GuestContractInstance::null();
        // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
        unsafe {
            polyplug_lua_internal_plugin_create_instance(
                bridge.cast(),
                VmLoaderData::null(),
                ptr::null(),
                ptr::null(),
                &mut instance,
            );
        }
        assert!(
            instance.data.is_null(),
            "a non-table factory result must be rejected before instance publication"
        );
        // SAFETY: The validated bridge owns the referenced allocation for this call, and the live owner-thread Lua state keeps the C API pointers and stack valid.
        unsafe {
            ffi::luaL_unref(state.cast(), ffi::LUA_REGISTRYINDEX, factory);
            drop(Box::from_raw(bridge));
            drop(Box::from_raw(resident));
        }
    }

    #[test]
    fn internal_plugin_dispatch_refuses_a_non_owner_thread() {
        let resident = Box::into_raw(Box::new(Resident {
            lua: ptr::null_mut(),
            api: api(),
            owner_thread_id: super::owner_thread_id(),
            manifest: String::new(),
            contracts: Mutex::new(Vec::new()),
        }));
        let bridge = Box::into_raw(Box::new(ContractBridge {
            resident,
            factory: 0,
            dispatcher: 0,
            contract_id: 1,
            plugin_name: String::new(),
            contract_name: String::new(),
            descriptor: PluginDescriptor {
                name: StringView::null(),
                contract_name: StringView::null(),
                version: Version {
                    major: 0,
                    minor: 0,
                    patch: 0,
                },
            },
            interface: GuestContractInterface {
                contract_id: GuestContractId::from_u64(1),
                contract_version: Version {
                    major: 0,
                    minor: 0,
                    patch: 0,
                },
                dispatch_type: DispatchType::VirtualMachine,
                adapter_context: ptr::null_mut(),
                create_instance: super::polyplug_lua_internal_plugin_create_instance,
                destroy_instance: super::polyplug_lua_internal_plugin_destroy_instance,
                dispatch: DispatchMechanisms {
                    vm: VmDispatch {
                        call: super::polyplug_lua_internal_plugin_vm_dispatch,
                        loader_data: VmLoaderData::null(),
                    },
                },
            },
            instances: Mutex::new(HashMap::new()),
            pending_implementation: Mutex::new(None),
            next_instance: Mutex::new(1),
        }));
        let bridge_address = bridge as usize;
        let code = match thread::spawn(move || {
            let mut error = AbiError::ok();
            // SAFETY: The bridge invokes this only with a live Lua state owned by the current thread; pointers and stack indices satisfy the linked Lua C API.
            unsafe {
                polyplug_lua_internal_plugin_vm_dispatch(
                    bridge_address as *mut c_void,
                    VmLoaderData::null(),
                    GuestContractInstance::null(),
                    0,
                    ptr::null(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    &mut error,
                );
            }
            error.code
        })
        .join()
        {
            Ok(code) => code,
            Err(_) => panic!("non-owner dispatch thread panicked"),
        };
        assert_eq!(code, AbiErrorCode::Generic as u32);
        // SAFETY: The validated bridge owns the referenced allocation for this call, and the live owner-thread Lua state keeps the C API pointers and stack valid.
        unsafe {
            drop(Box::from_raw(bridge));
            drop(Box::from_raw(resident));
        }
    }

    #[test]
    fn direct_bridge_rejects_foreign_adapter_contexts() {
        let foreign = GuestContractInterface {
            contract_id: GuestContractId::from_u64(2),
            contract_version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
            dispatch_type: DispatchType::VirtualMachine,
            // This non-null sentinel is compared by address and is never dereferenced.
            adapter_context: ptr::dangling_mut::<c_void>(),
            create_instance: super::polyplug_lua_internal_plugin_create_instance,
            destroy_instance: super::polyplug_lua_internal_plugin_destroy_instance,
            dispatch: DispatchMechanisms {
                vm: VmDispatch {
                    call: super::polyplug_lua_internal_plugin_vm_dispatch,
                    loader_data: VmLoaderData::null(),
                },
            },
        };
        assert!(
            super::bridge_for_interface(&foreign).is_none(),
            "a direct Lua bridge may only receive a bridge owned by this resident"
        );
    }

    #[test]
    fn attempted_commit_does_not_abort_the_outer_transaction() {
        assert!(
            !super::transaction_needs_abort(true, true),
            "a failed commit consumes its transaction and must not pop an outer one"
        );
        assert!(super::transaction_needs_abort(true, false));
        assert!(!super::transaction_needs_abort(false, false));
    }
}

from __future__ import annotations

import ctypes
import enum
from typing import ClassVar



class NativeDispatch(ctypes.Structure):
    """ Native dispatch data — direct function pointer array.
    
     Used when `dispatch_type == DispatchType::Native`.
     The `functions` array contains `function_count` function pointers.
    """
    _fields_ = [
        ("function_count", ctypes.c_uint32),
        ("functions", ctypes.c_void_p),
    ]


class VmDispatch(ctypes.Structure):
    """ VM dispatch data — call through a dispatch function.
    
     Used when `dispatch_type == DispatchType::VirtualMachine`.
     The `call` function receives `loader_data` which contains VM-specific state.
    """
    _fields_ = [
        ("call", unsafeextern"C"fn(loader_data:VmLoaderData,instance:GuestContractInstance,fn_id:u32,args:*const(),out:*mut(),)->AbiError),
        ("loader_data", VmLoaderData),
    ]


class VmLoaderData(ctypes.Structure):
    """ Opaque handle to VM loader-specific data.
    
     Wraps VM-specific state managed by each loader (Python, Lua, JS).
     Opaque to core runtime — loaders know their own state layout.
    
     # OWNERSHIP
     Owned by the loader. Lives for the lifetime of the loaded plugin.
    """
    _fields_ = [
        ("data", ctypes.c_void_p),
    ]


class GuestContractInstance(ctypes.Structure):
    """ Opaque handle to a guest contract instance.
    
     Created by `GuestContractInterface::create_instance`, destroyed by `destroy_instance`.
    
     # Who provides
     Guest code creates instances via create_instance factory.
     The guest owns the underlying data.
    
     # Who calls
     Host code passes instances to dispatch functions and destroy_instance.
    
     # Ownership
     This is an owned handle - the instance must be destroyed via
     `GuestContractInterface::destroy_instance` before hot-reload.
     Failure to destroy causes memory leaks and prevents safe hot-reload.
    
     # Lifetime
     Lives until `destroy_instance` is called. Must be destroyed before
     the bundle is unloaded or hot-reloaded.
    
     # Layout
     - `data`: Opaque instance pointer (owned by guest)
     - `contract_id`: Contract ID for zero-overhead dispatch
    
     # Dispatch
     The `contract_id` field enables `call_guest_method` to dispatch without
     looking up the contract in a map. This is zero-overhead dispatch.
    """
    _fields_ = [
        ("data", ctypes.c_void_p),
        ("contract_id", GuestContractId),
    ]


class GuestContractInterface(ctypes.Structure):
    """ Guest Contract Interface — one per contract implemented by a guest (plugin).
    
     # Who provides
     Guest (plugin) code creates this struct and registers it via `register_contract`.
     Must be `'static` or intentionally leaked.
    
     # Who calls
     Host code calls `create_instance`, `destroy_instance`, and dispatch functions.
    
     # Ownership
     Must be `'static`. Never stack-allocated. Never freed while runtime lives.
     Typically created as `static` or via `Box::leak()`.
    
     # Lifetime
     Lives for the entire runtime lifetime. Must survive hot-reload.
    
     # Instance Lifecycle
     - `create_instance`: Factory function to create new instances
     - `destroy_instance`: Destructor to clean up instances before hot-reload
    
     # Dispatch
     - `dispatch_type == Native`: Call via `dispatch.native.functions[fn_id](instance, args, out)`
     - `dispatch_type == VirtualMachine`: Call via `dispatch.vm.call(loader_data, instance, fn_id, args, out)`
    """
    _fields_ = [
        ("contract_id", GuestContractId),
        ("contract_version", Version),
        ("dispatch_type", DispatchType),
        ("create_instance", unsafeextern"C"fn(host:*constHostInterface,args:*const(),)->GuestContractInstance),
        ("destroy_instance", unsafeextern"C"fn(host:*constHostInterface,instance:GuestContractInstance,)),
        ("dispatch", DispatchMechanisms),
    ]


class HostInterface(ctypes.Structure):
    """ Host Interface — function table passed to guests during initialization.
    
     Contains an opaque runtime pointer and function pointers for guest calls.
     All functions use self-passing pattern (receive HostInterface pointer as first parameter).
    
     # Who provides
     The runtime creates this struct and passes it to `polyplug_init()`.
     The struct is allocated using `Box::leak()` for `'static` lifetime.
    
     # Who calls
     Guest (plugin) code calls these functions to interact with the runtime.
     SDK-generated wrappers handle the self-passing pattern automatically.
    
     # Ownership
     The struct is statically allocated by the runtime. The pointer is valid
     until the runtime is destroyed. Guest must NOT free this pointer.
    
     # Lifetime
     Lives as long as the runtime that created it.
    
     # Thread Safety
     All functions are safe to call from any thread. The runtime uses
     internal synchronization (RwLock/Mutex) for shared state.
    
     # Self-passing pattern
     Each function receives the interface pointer as its first parameter,
     allowing guests to call: `host->find_by_contract(host, id, ver)`
     SDKs hide this pattern: `host.find_by_contract(id, ver)`
    """
    _fields_ = [
        ("runtime", ctypes.c_void_p),
        ("register_contract", unsafeextern"C"fn(this:*constHostInterface,descriptor:*constPluginDescriptor,interface:*constGuestContractInterface,)->AbiError),
        ("alloc", unsafeextern"C"fn(this:*constHostInterface,size:usize,align:usize)->*mutu8),
        ("free", unsafeextern"C"fn(this:*constHostInterface,ptr:*mutu8,size:usize,align:usize)),
        ("find_guest_contract", unsafeextern"C"fn(this:*constHostInterface,contract_id:u64,min_version:u32,)->GuestContractHandle),
        ("find_all_guest_contracts", unsafeextern"C"fn(this:*constHostInterface,contract_id:u64,min_version:u32,)->Array<GuestContractHandle>),
        ("resolve_guest_contract", unsafeextern"C"fn(this:*constHostInterface,handle:GuestContractHandle,)->*constGuestContractInterface),
        ("call_guest_method", unsafeextern"C"fn(this:*constHostInterface,instance:GuestContractInstance,method_id:u32,args:*const(),out:*mut(),)->AbiError),
        ("get_host_contract", unsafeextern"C"fn(this:*constHostInterface,contract_id:u64,min_version:u32,)->crate::host::HostContractInstance),
        ("resolve_host_contract_interface", unsafeextern"C"fn(this:*constHostInterface,contract_id:u64,min_version:u32,)->*constcrate::host::HostContractInterface),
        ("list_bundles", unsafeextern"C"fn(this:*constHostInterface,)->Array<BundleId>),
        ("get_dependencies", unsafeextern"C"fn(this:*constHostInterface,)->Array<DependencyInfo>),
        ("load_bundle", unsafeextern"C"fn(this:*constHostInterface,path:*constu8,path_len:usize,)->AbiError),
        ("reload_bundle", unsafeextern"C"fn(this:*constHostInterface,path:*constu8,path_len:usize,)->AbiError),
        ("register_host_contract", unsafeextern"C"fn(this:*constHostInterface,interface:*constcrate::host::HostContractInterface,)->AbiError),
        ("register_loader", ctypes.c_void_p),
        ("get_last_error", unsafeextern"C"fn(this:*constHostInterface,buf:*mutu8,buf_len:usize,)->usize),
        ("get_error_len", unsafeextern"C"fn(this:*constHostInterface,)->usize),
    ]


class RuntimeInterface(ctypes.Structure):
    """ Runtime Interface — function table returned to host from polyplug_runtime_create().
    
     Contains an opaque runtime pointer and function pointers for host calls.
     All functions take `*const RuntimeInterface` as first parameter.
    
     # Who provides
     The runtime creates this struct and returns it from `polyplug_runtime_create()`.
     The struct is heap-allocated and owned by the host.
    
     # Who calls
     Host application code calls these functions to interact with the runtime.
     SDK-generated wrappers handle the self-passing pattern automatically.
    
     # Ownership
     The struct is allocated by `polyplug_runtime_create()`. The host owns
     the pointer and must call `destroy()` to free the runtime and interface.
    
     # Lifetime
     Lives until `destroy()` is called. After destroy, the pointer is invalid.
    
     # Thread Safety
     All functions are safe to call from any thread. The runtime uses
     internal synchronization for shared state.
    
     # Self-passing pattern
     Each function receives the interface pointer as its first parameter,
     allowing hosts to call: `rt->load_bundle(rt, path)`
     SDKs hide this pattern: `rt.load_bundle(path)`
    """
    _fields_ = [
        ("runtime", ctypes.c_void_p),
        ("load_bundle", unsafeextern"C"fn(this:*constRuntimeInterface,path:*constc_char)->AbiError),
        ("reload_bundle", unsafeextern"C"fn(this:*constRuntimeInterface,bundle_id:BundleId)->AbiError),
        ("unload_bundle", unsafeextern"C"fn(this:*constRuntimeInterface,bundle_id:BundleId)->AbiError),
        ("find_by_contract", unsafeextern"C"fn(this:*constRuntimeInterface,contract_id:u64,min_version:u32,)->GuestContractHandle),
        ("find_all_by_contract", unsafeextern"C"fn(this:*constRuntimeInterface,contract_id:u64,min_version:u32,)->Array<GuestContractHandle>),
        ("resolve_contract", unsafeextern"C"fn(this:*constRuntimeInterface,handle:GuestContractHandle,)->*constGuestContractInterface),
        ("get_host_contract", unsafeextern"C"fn(this:*constRuntimeInterface,contract_id:u64,min_version:u32,)->HostContractInstance),
        ("get_last_error", unsafeextern"C"fn(this:*constRuntimeInterface)->StringView),
        ("list_bundles", unsafeextern"C"fn(this:*constRuntimeInterface,)->Array<BundleId>),
        ("get_dependencies", unsafeextern"C"fn(this:*constRuntimeInterface,)->Array<DependencyInfo>),
        ("destroy", unsafeextern"C"fn(this:*constRuntimeInterface)),
    ]


class HostContractInstance(ctypes.Structure):
    """ Opaque handle to a host contract instance.
    
     Created by `HostContractInterface::create_instance`, destroyed by `destroy_instance`.
     For singleton host contracts, the same instance is returned for all callers.
    """
    _fields_ = [
        ("data", ctypes.c_void_p),
    ]


class HostContractInterface(ctypes.Structure):
    """ Host Contract Interface — for host-provided services.
    
     Host contracts are services provided by the host application to plugins.
    
     # Who provides
     Host application code creates this struct and registers it with the runtime.
     Must be `'static` or intentionally leaked.
    
     # Who calls
     Guest (plugin) code calls the dispatch functions after obtaining an instance
     via `HostInterface::get_host_contract()`.
    
     # Ownership
     Must be `'static`. The runtime holds a reference for the plugin lifetime.
     Never freed while runtime lives.
    
     # Lifetime
     Lives for the entire runtime lifetime. Must survive hot-reload.
    
     # Singleton Mode
     - `singleton == true`: Same instance returned for all callers
     - `singleton == false`: New instance per caller (caller must destroy)
    
     # Self-Passing Pattern
     `create_instance` and `destroy_instance` take `self: *const HostContractInterface`.
     The runtime field provides access to runtime services.
    """
    _fields_ = [
        ("contract_id", HostContractId),
        ("contract_version", Version),
        ("singleton", ctypes.c_bool),
        ("dispatch_type", DispatchType),
        ("runtime", ctypes.c_void_p),
        ("create_instance", unsafeextern"C"fn(this:*constHostContractInterface,args:*const(),)->HostContractInstance),
        ("destroy_instance", unsafeextern"C"fn(this:*constHostContractInterface,instance:HostContractInstance,)),
        ("dispatch", DispatchMechanisms),
    ]


class BundleInitContext(ctypes.Structure):
    """ Context passed to every guest `polyplug_init()` function.
    
     # OWNERSHIP
     The `bundle_path` pointer is runtime-owned and valid for the lifetime of the `PluginRuntime`.
     **Plugin must not store the raw pointer** — copy the string value if persistence is needed.
    """
    _fields_ = [
        ("bundle_id", ctypes.c_uint64),
        ("bundle_path", StringView),
    ]


class PluginDescriptor(ctypes.Structure):
    """ Metadata about a plugin within a bundle.
    
     # OWNERSHIP
     value type passed by pointer during init. The `name` and
     `contract_name` StringViews are borrowed from the plugin's static memory.
     The receiver must not free or outlive the plugin's library.
    """
    _fields_ = [
        ("name", StringView),
        ("contract_name", StringView),
        ("version", Version),
    ]


class GuestContractHandle(ctypes.Structure):
    """ Opaque handle to a registered guest contract.
    
     The handle is just an index into the registry array.
     Out-of-bounds indices return InvalidHandle error.
    
     # Naming
     Named `GuestContractHandle` for consistency with `GuestContractInterface`
     and `GuestContractInstance`.
    
     # Layout
     - `index`: Slot index in the registry (u32)
    
     # Safety
     Handles become stale after unload. Call `resolve_contract` to validate.
     Returns null pointer if the handle is invalid.
    """
    _fields_ = [
        ("index", ctypes.c_uint32),
    ]


class RuntimeConfig(ctypes.Structure):
    """ Configuration for the polyplug runtime passed to `polyplug_runtime_create`.
    
     # OWNERSHIP
     Borrowed for the duration of the runtime build only.
     The runtime copies any data it needs to retain.
    """
    _fields_ = [
        ("compatibility", Compatibility),
        ("hot_reload_enabled", ctypes.c_bool),
        ("on_reload", Option<unsafeextern"C"fn(ReloadPhase)>),
    ]


class ReloadPhase(ctypes.Structure):
    """ FFI-safe reload phase for hot-reload callbacks.
    
     Tagged union style struct — `phase_type` indicates which variant is active.
     Uses `StringView` for FFI compatibility (non-owning borrows).
    
     # Lifetime
     `StringView` fields are borrowed from the caller's strings.
     The callback must not store these views beyond the callback scope.
    """
    _fields_ = [
        ("phase_type", ReloadPhaseType),
        ("bundle_id", BundleId),
        ("bundle_name", StringView),
        ("reason", StringView),
    ]


class AbiError(ctypes.Structure):
    """ ABI error — returned by value from all ABI calls.
    
     OWNERSHIP: `code` is a value type. `message.ptr` is allocated by the callee
     via `host_alloc`. Caller frees with `polyplug_host_free(message.ptr, message.len, 1)`
     after reading. If `code == AbiErrorCode::Ok`, `message.ptr` is NULL — no free needed.
    """
    _fields_ = [
        ("code", AbiErrorCode),
        ("message", StringView),
    ]


class Array(ctypes.Structure):
    """ FFI-safe array with caller-frees ownership model.
    
     # Memory Management
     - Allocated via `host->alloc(self, len * sizeof(T), align)`
     - Freed via `host->free(self, items, len * sizeof(T), align)`
    
     # Ownership
     Caller owns the memory and must free via host allocator.
     CodeGen generates RAII wrappers in each language SDK:
     - Rust: `Drop` impl calls `host->free`
     - Python: `__del__` calls free
     - C#: `IDisposable.Dispose` calls free
    
     # Safety
     The `align` field is required for proper freeing. Generic code must
     track alignment of `T` to free correctly.
    
     # Thread Safety
     Safe to read from multiple threads if underlying data is immutable.
     Send/Sync implemented for T: Send/Sync.
    """
    _fields_ = [
        ("items", ctypes.c_void_p),
        ("len", ctypes.c_size_t),
        ("align", ctypes.c_size_t),
    ]


class Buffer(ctypes.Structure):
    """ Owning byte buffer.
    
     OWNERSHIP: `ptr` is always allocated via `polyplug_host_alloc`.
     Owner calls `polyplug_host_free(ptr, cap, align)` when done.
    """
    _fields_ = [
        ("ptr", ctypes.c_void_p),
        ("len", ctypes.c_size_t),
        ("cap", ctypes.c_size_t),
    ]


class DependencyInfo(ctypes.Structure):
    """ Dependency information returned by get_dependencies introspection API.
    
     Mirrors `manifest.toml` `\[dependency\]` table structure for plugins to query
     their own declared dependencies at runtime.
    
     # Who provides
     Runtime returns this from `HostInterface::get_dependencies`.
    
     # Who calls
     Guest (plugin) code calls `get_dependencies` during initialization
     to discover available dependencies.
    
     # Ownership
     Returned in an Array that caller owns and must free via `host->free`.
    
     # Fields
     - `contract_id`: The contract being depended upon
     - `min_version`: Minimum version required
     - `bundle_id`: Specific bundle if ByBundle, 0 if ByContract
    """
    _fields_ = [
        ("contract_id", GuestContractId),
        ("min_version", ctypes.c_uint32),
        ("bundle_id", BundleId),
    ]


class StringView(ctypes.Structure):
    """ Non-owning UTF-8 string view.
    
     OWNERSHIP: borrowed reference. `ptr` must remain valid for the duration
     of the call. Never freed by the receiver.
    """
    _fields_ = [
        ("ptr", ctypes.c_void_p),
        ("len", ctypes.c_size_t),
    ]


class Version(ctypes.Structure):
    """ A three-component semantic version (major.minor.patch)."""
    _fields_ = [
        ("major", ctypes.c_uint32),
        ("minor", ctypes.c_uint32),
        ("patch", ctypes.c_uint32),
    ]


class ContractType(enum.IntEnum):
    """ABI enum."""
    Host = 0
    Plugin = 1


class DispatchType(enum.IntEnum):
    """ Dispatch mechanism type — determines how function calls are routed."""
    Native = 0
    VirtualMachine = 1


class Compatibility(enum.IntEnum):
    """ How strictly version compatibility is enforced when resolving plugins."""
    Strict = 0
    Relaxed = 1
    Yolo = 2


class ReloadPhaseType(enum.IntEnum):
    """ Type of reload phase for FFI callbacks."""
    Preparing = 0
    Reloaded = 1
    Failed = 2


class RuntimeLanguage(enum.IntEnum):
    """ Runtime type identifier — identifies the language/runtime hosting plugins."""
    Rust = 0
    Cpp = 1
    Dotnet = 2
    Python = 3
    Lua = 4
    JavaScript = 5


class AbiErrorCode(enum.IntEnum):
    """ ABI error codes (reserved: 0-255 runtime, 256+ plugin-defined).
    
     These codes are returned by all ABI functions to indicate success or failure.
     The `code` field of `AbiError` uses these values.
    """
    Ok = 0
    Generic = 1
    BufferTooSmall = 2
    Panic = 3
    NotFound = 4
    StaleHandle = 5
    FunctionNotAvailable = 6
    DuplicateProvider = 7
    InvalidPointer = 8
    HostContractNotFound = 100
    HostContractVersionMismatch = 101
    HostContractCallFailed = 102


class ParseVersionError(enum.IntEnum):
    """ABI enum."""
    InvalidFormat = 0
    InvalidInt = 1


class DispatchMechanisms(ctypes.Union):
    """ Union of dispatch mechanisms — use based on `dispatch_type`.
    
     # Safety
     Access the correct variant based on `GuestContractInterface::dispatch_type`:
     - `dispatch_type == Native` → access `.native`
     - `dispatch_type == VirtualMachine` → access `.vm`
    """
    _fields_ = [
        ("native", NativeDispatch),
        ("vm", VmDispatch),
    ]
POLYPLUG_ABI_VERSION: int = 1

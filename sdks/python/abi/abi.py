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


_vm_dispatch_call_t = ctypes.CFUNCTYPE(AbiError, VmLoaderData, GuestContractInstance, ctypes.c_uint32, ctypes.c_void_p, ctypes.c_void_p)
class VmDispatch(ctypes.Structure):
    """ VM dispatch data — call through a dispatch function.
    
     Used when `dispatch_type == DispatchType::VirtualMachine`.
     The `call` function receives `loader_data` which contains VM-specific state.
    """
    _fields_ = [
        ("call", _vm_dispatch_call_t),
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


_guest_contract_interface_create_instance_t = ctypes.CFUNCTYPE(GuestContractInstance, ctypes.c_void_p, ctypes.c_void_p)
_guest_contract_interface_destroy_instance_t = ctypes.CFUNCTYPE(None, ctypes.c_void_p, GuestContractInstance)
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
        ("create_instance", _guest_contract_interface_create_instance_t),
        ("destroy_instance", _guest_contract_interface_destroy_instance_t),
        ("dispatch", DispatchMechanisms),
    ]


_host_interface_register_contract_t = ctypes.CFUNCTYPE(AbiError, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p)
_host_interface_alloc_t = ctypes.CFUNCTYPE(ctypes.c_void_p, ctypes.c_void_p, ctypes.c_size_t, ctypes.c_size_t)
_host_interface_free_t = ctypes.CFUNCTYPE(None, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_size_t, ctypes.c_size_t)
_host_interface_find_guest_contract_t = ctypes.CFUNCTYPE(GuestContractHandle, ctypes.c_void_p, ctypes.c_uint64, ctypes.c_uint32)
_host_interface_find_all_guest_contracts_t = ctypes.CFUNCTYPE(ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint64, ctypes.c_uint32)
_host_interface_resolve_guest_contract_t = ctypes.CFUNCTYPE(ctypes.c_void_p, ctypes.c_void_p, GuestContractHandle)
_host_interface_call_guest_method_t = ctypes.CFUNCTYPE(AbiError, ctypes.c_void_p, GuestContractInstance, ctypes.c_uint32, ctypes.c_void_p, ctypes.c_void_p)
_host_interface_get_host_contract_t = ctypes.CFUNCTYPE(crate::host::HostContractInstance, ctypes.c_void_p, ctypes.c_uint64, ctypes.c_uint32)
_host_interface_resolve_host_contract_interface_t = ctypes.CFUNCTYPE(ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint64, ctypes.c_uint32)
_host_interface_list_bundles_t = ctypes.CFUNCTYPE(ctypes.c_void_p, ctypes.c_void_p)
_host_interface_get_dependencies_t = ctypes.CFUNCTYPE(ctypes.c_void_p, ctypes.c_void_p)
_host_interface_load_bundle_t = ctypes.CFUNCTYPE(AbiError, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_size_t)
_host_interface_reload_bundle_t = ctypes.CFUNCTYPE(AbiError, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_size_t)
_host_interface_register_host_contract_t = ctypes.CFUNCTYPE(AbiError, ctypes.c_void_p, ctypes.c_void_p)
_host_interface_register_loader_t = ctypes.CFUNCTYPE(AbiError, ctypes.c_void_p, StringView, ctypes.c_void_p)
_host_interface_get_last_error_t = ctypes.CFUNCTYPE(ctypes.c_size_t, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_size_t)
_host_interface_get_error_len_t = ctypes.CFUNCTYPE(ctypes.c_size_t, ctypes.c_void_p)
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
        ("register_contract", _host_interface_register_contract_t),
        ("alloc", _host_interface_alloc_t),
        ("free", _host_interface_free_t),
        ("find_guest_contract", _host_interface_find_guest_contract_t),
        ("find_all_guest_contracts", _host_interface_find_all_guest_contracts_t),
        ("resolve_guest_contract", _host_interface_resolve_guest_contract_t),
        ("call_guest_method", _host_interface_call_guest_method_t),
        ("get_host_contract", _host_interface_get_host_contract_t),
        ("resolve_host_contract_interface", _host_interface_resolve_host_contract_interface_t),
        ("list_bundles", _host_interface_list_bundles_t),
        ("get_dependencies", _host_interface_get_dependencies_t),
        ("load_bundle", _host_interface_load_bundle_t),
        ("reload_bundle", _host_interface_reload_bundle_t),
        ("register_host_contract", _host_interface_register_host_contract_t),
        ("register_loader", _host_interface_register_loader_t),
        ("get_last_error", _host_interface_get_last_error_t),
        ("get_error_len", _host_interface_get_error_len_t),
    ]


_runtime_interface_load_bundle_t = ctypes.CFUNCTYPE(AbiError, ctypes.c_void_p, ctypes.c_void_p)
_runtime_interface_reload_bundle_t = ctypes.CFUNCTYPE(AbiError, ctypes.c_void_p, BundleId)
_runtime_interface_unload_bundle_t = ctypes.CFUNCTYPE(AbiError, ctypes.c_void_p, BundleId)
_runtime_interface_find_by_contract_t = ctypes.CFUNCTYPE(GuestContractHandle, ctypes.c_void_p, ctypes.c_uint64, ctypes.c_uint32)
_runtime_interface_find_all_by_contract_t = ctypes.CFUNCTYPE(ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint64, ctypes.c_uint32)
_runtime_interface_resolve_contract_t = ctypes.CFUNCTYPE(ctypes.c_void_p, ctypes.c_void_p, GuestContractHandle)
_runtime_interface_get_host_contract_t = ctypes.CFUNCTYPE(HostContractInstance, ctypes.c_void_p, ctypes.c_uint64, ctypes.c_uint32)
_runtime_interface_get_last_error_t = ctypes.CFUNCTYPE(StringView, ctypes.c_void_p)
_runtime_interface_list_bundles_t = ctypes.CFUNCTYPE(ctypes.c_void_p, ctypes.c_void_p)
_runtime_interface_get_dependencies_t = ctypes.CFUNCTYPE(ctypes.c_void_p, ctypes.c_void_p)
_runtime_interface_destroy_t = ctypes.CFUNCTYPE(None, ctypes.c_void_p)
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
        ("load_bundle", _runtime_interface_load_bundle_t),
        ("reload_bundle", _runtime_interface_reload_bundle_t),
        ("unload_bundle", _runtime_interface_unload_bundle_t),
        ("find_by_contract", _runtime_interface_find_by_contract_t),
        ("find_all_by_contract", _runtime_interface_find_all_by_contract_t),
        ("resolve_contract", _runtime_interface_resolve_contract_t),
        ("get_host_contract", _runtime_interface_get_host_contract_t),
        ("get_last_error", _runtime_interface_get_last_error_t),
        ("list_bundles", _runtime_interface_list_bundles_t),
        ("get_dependencies", _runtime_interface_get_dependencies_t),
        ("destroy", _runtime_interface_destroy_t),
    ]


class HostContractInstance(ctypes.Structure):
    """ Opaque handle to a host contract instance.
    
     Created by `HostContractInterface::create_instance`, destroyed by `destroy_instance`.
     For singleton host contracts, the same instance is returned for all callers.
    """
    _fields_ = [
        ("data", ctypes.c_void_p),
    ]


_host_contract_interface_create_instance_t = ctypes.CFUNCTYPE(HostContractInstance, ctypes.c_void_p, ctypes.c_void_p)
_host_contract_interface_destroy_instance_t = ctypes.CFUNCTYPE(None, ctypes.c_void_p, HostContractInstance)
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
        ("create_instance", _host_contract_interface_create_instance_t),
        ("destroy_instance", _host_contract_interface_destroy_instance_t),
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


_runtime_config_on_reload_t = ctypes.CFUNCTYPE(None, ReloadPhase)
# Nullable function pointer (Option<fn>). Can be set to None.
class RuntimeConfig(ctypes.Structure):
    """ Configuration for the polyplug runtime passed to `polyplug_runtime_create`.
    
     # OWNERSHIP
     Borrowed for the duration of the runtime build only.
     The runtime copies any data it needs to retain.
    """
    _fields_ = [
        ("compatibility", Compatibility),
        ("hot_reload_enabled", ctypes.c_bool),
        ("on_reload", _runtime_config_on_reload_t),
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

from __future__ import annotations

import ctypes
import enum
from typing import ClassVar



class StringView(ctypes.Structure):
    """ Non-owning UTF-8 string view.
    
     OWNERSHIP: borrowed reference. `ptr` must remain valid for the duration
     of the call. Never freed by the receiver.
    """
    _fields_ = [
        ("ptr", ctypes.c_void_p),
        ("len", ctypes.c_size_t),
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


class AbiError(ctypes.Structure):
    """ ABI error — returned by value from all ABI calls.
    
     OWNERSHIP: `code` is a value type. `message.ptr` is allocated by the callee
     via `host_alloc`. Caller frees with `polyplug_host_free(message.ptr, message.len, 1)`
     after reading. If `code == AbiErrorCode::Ok`, `message.ptr` is NULL — no free needed.
    """
    _fields_ = [
        ("code", ctypes.c_uint32),
        ("message", StringView),
    ]


class PluginHandle(ctypes.Structure):
    """ Opaque handle to a loaded plugin — validated on use.
    
     INTERNAL STRUCTURE: index into registry array + generation counter.
     The generation counter detects use-after-unload.
    """
    _fields_ = [
        ("index", ctypes.c_uint32),
        ("generation", ctypes.c_uint32),
    ]


class HostContext(ctypes.Structure):
    """ Opaque host context passed to plugin functions via rt_ctx parameter.
    
     Contains the runtime pointer and the bundle_id of the calling bundle.
     The actual implementation is in the polyplug crate; this definition
     establishes the ABI layout.
    
     OWNERSHIP: `'static`, lives as long as the runtime.
    """
    _fields_ = [
        ("runtime", ctypes.c_void_p),
        ("bundle_id", ctypes.c_uint64),
    ]


class NativeDispatch(ctypes.Structure):
    """ Native dispatch data — direct function pointer array.
    
     Used when `dispatch_type == DispatchType::Native`.
     The `functions` array contains `function_count` function pointers.
    """
    _fields_ = [
        ("functions", ctypes.c_void_p),
    ]


class VmDispatch(ctypes.Structure):
    """ VM dispatch data — call through a dispatch function.
    
     Used when `dispatch_type == DispatchType::VirtualMachine`.
     The `call` function receives `loader_data` which contains VM-specific state.
    """
    _fields_ = [
        ("call", ctypes.c_void_p),
        ("loader_data", ctypes.c_void_p),
    ]


class PluginInterface(ctypes.Structure):
    """ Plugin interface — one per contract implemented by a plugin.
    
     OWNERSHIP: Must be `'static` or intentionally leaked.
     Never stack-allocated. Never freed while runtime lives.
    
     # Dispatch
     - `dispatch_type == Native`: Call via `dispatch.native.functions[fn_id]`
     - `dispatch_type == VirtualMachine`: Call via `dispatch.vm.call(...)`
    """
    _fields_ = [
        ("rt_ctx", ctypes.c_void_p),
        ("contract_id", ctypes.c_uint64),
        ("contract_version", ctypes.c_uint32),
        ("function_count", ctypes.c_uint32),
        ("dispatch_type", DispatchType),
        ("dispatch", PluginDispatch),
    ]


class HostContractVTableHeader(ctypes.Structure):
    """ Host contract vtable header — metadata for a host-provided contract."""
    _fields_ = [
        ("vtable_version", ctypes.c_uint32),
        ("contract_id", ctypes.c_uint64),
        ("contract_major", ctypes.c_uint32),
        ("contract_minor", ctypes.c_uint32),
        ("function_count", ctypes.c_uint32),
        ("dispatch_type", DispatchType),
    ]


class NativeHostContractDispatch(ctypes.Structure):
    """ Native dispatch for host contracts — direct function pointer array.
    
     Used when `dispatch_type == DispatchType::Native`.
     The `functions` array contains `function_count` function pointers.
    """
    _fields_ = [
        ("impl_ptr", ctypes.c_void_p),
        ("functions", ctypes.c_void_p),
    ]


class VmHostContractDispatch(ctypes.Structure):
    """ VM dispatch for host contracts — call through a dispatch function.
    
     Used when `dispatch_type == DispatchType::VirtualMachine`.
     The `call` function receives `bridge_data` which contains VM-specific state.
    """
    _fields_ = [
        ("call", ctypes.c_void_p),
        ("bridge_data", ctypes.c_void_p),
    ]


class HostContractVTable(ctypes.Structure):
    """ Host contract vtable — complete interface for a host-provided contract.
    
     OWNERSHIP: Must be `'static` or intentionally leaked.
     Never stack-allocated. Never freed while runtime lives.
    
     # Dispatch
     - `dispatch_type == Native`: Call via `dispatch.native.functions[fn_id]`
     - `dispatch_type == VirtualMachine`: Call via `dispatch.vm.call(...)`
    """
    _fields_ = [
        ("header", HostContractVTableHeader),
        ("dispatch", HostContractDispatch),
    ]


class HostVTable(ctypes.Structure):
    """ Host capabilities passed to every plugin at init time.
    
     OWNERSHIP: `'static`, lives as long as the runtime.
    
     All functions take `rt_ctx` as first parameter - an opaque pointer to the Runtime.
     This allows each Runtime to have its own isolated state (no global registry).
    """
    _fields_ = [
        ("register_plugin", ctypes.c_void_p),
        ("alloc", ctypes.c_void_p),
        ("free", ctypes.c_void_p),
        ("find_by_contract", ctypes.c_void_p),
        ("find_by_bundle", ctypes.c_void_p),
        ("find_all_by_contract", ctypes.c_void_p),
        ("resolve_plugin", ctypes.c_void_p),
        ("get_host_contract", ctypes.c_void_p),
    ]


class PluginDescriptor(ctypes.Structure):
    """ Metadata about a plugin within a bundle.
    
     OWNERSHIP: value type passed by pointer during init. The `name` and
     `contract_name` StringViews are borrowed from the plugin's static memory.
     The receiver must not free or outlive the plugin's library.
    """
    _fields_ = [
        ("name", StringView),
        ("contract_name", StringView),
        ("version_major", ctypes.c_uint32),
        ("version_minor", ctypes.c_uint32),
        ("version_patch", ctypes.c_uint32),
    ]


class PluginContext(ctypes.Structure):
    """ Context passed to every guest `polyplug_init()` function.
     The `bundle_path` pointer is runtime-owned and valid for the lifetime of the `PluginRuntime`.
     **Plugin must not store the raw pointer** — copy the string value if persistence is needed.
    """
    _fields_ = [
        ("bundle_path", StringView),
        ("host_abi_version", ctypes.c_uint32),
        ("bundle_id", ctypes.c_uint64),
    ]


class RuntimeConfig(ctypes.Structure):
    """ Configuration passed to `polyplug_runtime_create` during runtime initialisation.
    
     OWNERSHIP: borrowed for the duration of the runtime build only.
     The caller may free all pointed-to memory after the build
     returns. The runtime copies any data it needs to retain.
    """
    _fields_ = [
        ("plugin_dirs", ctypes.c_void_p),
        ("plugin_dir_count", ctypes.c_size_t),
        ("compatibility", ctypes.c_uint32),
    ]


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


class DispatchType(enum.IntEnum):
    """ Dispatch mechanism type — determines how function calls are routed."""
    Native = 0
    VirtualMachine = 1


class HostRuntime(enum.IntEnum):
    """ Host runtime type identifier — identifies the language/runtime hosting plugins."""
    Rust = 0
    Python = 1
    Lua = 2
    JavaScript = 3


class PluginDispatch(ctypes.Union):
    """ Union of dispatch mechanisms — use based on `dispatch_type`.
    
     # Safety
     Access the correct variant based on `PluginInterface::dispatch_type`:
     - `dispatch_type == Native` → access `.native`
     - `dispatch_type == VirtualMachine` → access `.vm`
    """
    _fields_ = [
        ("native", NativeDispatch),
        ("vm", VmDispatch),
    ]


class HostContractDispatch(ctypes.Union):
    """ Union of host contract dispatch mechanisms — use based on `dispatch_type`.
    
     # Safety
     Access the correct variant based on `HostContractVTableHeader::dispatch_type`:
     - `dispatch_type == Native` → access `.native`
     - `dispatch_type == VirtualMachine` → access `.vm`
    """
    _fields_ = [
        ("native", NativeHostContractDispatch),
        ("vm", VmHostContractDispatch),
    ]
def string_view_from_static(bytes: &'static[u8]) -> StringView:
    pass

def string_view_null() -> StringView:
    pass

def string_view_as_str(sv: &StringView) -> &str:
    pass

def string_view_to_string_owned(sv: &StringView) -> String:
    pass

def buffer_as_slice(buf: &Buffer) -> &[u8]:
    pass

def buffer_as_mut_slice(buf: &mutBuffer) -> &mut[u8]:
    pass

def abi_error_ok() -> AbiError:
    pass

def abi_error_panic_caught() -> AbiError:
    pass

def abi_error_is_ok(err: &AbiError) -> ctypes.c_bool:
    pass

def plugin_handle_null() -> PluginHandle:
    pass

def plugin_handle_is_null(handle: &PluginHandle) -> ctypes.c_bool:
    pass

POLYPLUG_ABI_VERSION: int = 1
def fnv1a_64(data: &[u8]) -> ctypes.c_uint64:
    pass

def contract_id(name: &str, major: ctypes.c_uint32) -> ctypes.c_uint64:
    pass

def bundle_id(name: &str) -> ctypes.c_uint64:
    pass

def host_contract_id(name: &str, major: ctypes.c_uint32) -> ctypes.c_uint64:
    pass

def plugin_contract_id(name: &str, major: ctypes.c_uint32) -> ctypes.c_uint64:
    pass


# THIS FILE IS MANUALLY MAINTAINED TO MATCH polyplug_abi
# DO NOT MODIFY FIELD ORDER OR SIZES — these must match the host runtime exactly.

"""ABI constants and types for the polyplug plugin runtime.

This module contains the frozen ABI types that match the Rust ABI exactly.
DO NOT modify field order or sizes — these must match the host runtime.
"""

from __future__ import annotations

import ctypes
import enum
from typing import ClassVar

# ─── ABI Constants ────────────────────────────────────────────────────────────

POLYPLUG_ABI_VERSION: int = 1
ABI_OK: int = 0
ABI_ERROR_GENERIC: int = 1
ABI_BUFFER_TOO_SMALL: int = 2
ABI_ERROR_PANIC: int = 3
ABI_ERROR_NOT_FOUND: int = 4
ABI_ERROR_STALE_HANDLE: int = 5
ABI_FUNCTION_NOT_AVAIL: int = 6
ABI_ERROR_DUPLICATE_PROVIDER: int = 7
ABI_ERROR_INVALID_POINTER: int = 8

# ─── ABI Structs ──────────────────────────────────────────────────────────────


class StringView(ctypes.Structure):
    """Non-owning UTF-8 string view.

    OWNERSHIP: borrowed reference. `ptr` must remain valid for the duration
    of the call. Never freed by the receiver.
    """

    _fields_ = [
        ("ptr", ctypes.c_void_p),
        ("len", ctypes.c_size_t),
    ]


class Buffer(ctypes.Structure):
    """Owning byte buffer.

    OWNERSHIP: `ptr` is always allocated via `polyplug_host_alloc`.
    Owner calls `polyplug_host_free(ptr, cap, align)` when done.
    """

    _fields_ = [
        ("ptr", ctypes.c_void_p),
        ("len", ctypes.c_size_t),
        ("cap", ctypes.c_size_t),
    ]


class Version(ctypes.Structure):
    """Semantic version (major, minor, patch)."""

    _fields_ = [
        ("major", ctypes.c_uint32),
        ("minor", ctypes.c_uint32),
        ("patch", ctypes.c_uint32),
    ]


class AbiError(ctypes.Structure):
    """ABI error — returned by value from all ABI calls.

    OWNERSHIP: `code` is a value type. `message.ptr` is allocated by the callee
    via `host_alloc`. Caller frees with `polyplug_host_free(message.ptr, message.len, 1)`
    after reading. If `code == ABI_OK`, `message.ptr` is NULL — no free needed.
    """

    _fields_ = [
        ("code", ctypes.c_uint32),
        ("message", StringView),
    ]


class GuestContractHandle(ctypes.Structure):
    """Opaque handle to a loaded guest contract — validated on use.

    The handle is just an index into the registry array.
    Out-of-bounds indices return InvalidHandle error.
    """

    _fields_ = [
        ("index", ctypes.c_uint32),
    ]


class GuestContractInstance(ctypes.Structure):
    """Opaque handle to a guest contract instance.

    Created by GuestContractInterface.create_instance, destroyed by destroy_instance.
    """

    _fields_ = [
        ("data", ctypes.c_void_p),
        ("contract_id", ctypes.c_uint64),
    ]


class HostContractInstance(ctypes.Structure):
    """Opaque handle to a host contract instance.

    Created by HostContractInterface.create_instance, destroyed by destroy_instance.
    """

    _fields_ = [
        ("data", ctypes.c_void_p),
    ]


class VmLoaderData(ctypes.Structure):
    """Opaque handle to VM loader state."""

    _fields_ = [
        ("data", ctypes.c_void_p),
    ]


class NativeDispatch(ctypes.Structure):
    """Native dispatch data — direct function pointer array.

    Used when `dispatch_type == DispatchType::Native`.
    The `functions` array contains function pointers.
    """

    _fields_ = [
        ("functions", ctypes.c_void_p),
    ]


class VmDispatch(ctypes.Structure):
    """VM dispatch data — call through a dispatch function.

    Used when `dispatch_type == DispatchType::VirtualMachine`.
    """

    _fields_ = [
        ("call", ctypes.c_void_p),
        ("loader_data", ctypes.c_void_p),
    ]


class DispatchType(enum.IntEnum):
    """Dispatch mechanism type — determines how function calls are routed."""

    Native = 0
    VirtualMachine = 1


class DispatchMechanisms(ctypes.Union):
    """Union of dispatch mechanisms — use based on `dispatch_type`.

    # Safety
    Access the correct variant based on `GuestContractInterface::dispatch_type`:
    - `dispatch_type == Native` → access `.native`
    - `dispatch_type == VirtualMachine` → access `.vm`
    """

    _fields_ = [
        ("native", NativeDispatch),
        ("vm", VmDispatch),
    ]


class GuestContractInterface(ctypes.Structure):
    """Guest Contract Interface — one per contract implemented by a guest (plugin).

    OWNERSHIP: Must be `'static` or intentionally leaked.
    Never stack-allocated. Never freed while runtime lives.

    # Instance Lifecycle
    - `create_instance`: Factory function to create new instances
    - `destroy_instance`: Destructor to clean up instances before hot-reload

    # Dispatch
    - `dispatch_type == Native`: Call via `dispatch.native.functions[fn_id](instance, args, out)`
    - `dispatch_type == VirtualMachine`: Call via `dispatch.vm.call(loader_data, instance, fn_id, args, out)`

    Layout (56 bytes):
    - contract_id (u64): 8 bytes @ 0
    - contract_version (Version): 12 bytes @ 8
    - dispatch_type (u32): 4 bytes @ 20
    - padding: 4 bytes
    - create_instance (fn ptr): 8 bytes @ 24
    - destroy_instance (fn ptr): 8 bytes @ 32
    - dispatch (union): 16 bytes @ 40
    """

    _fields_ = [
        ("contract_id", ctypes.c_uint64),
        ("contract_version", Version),
        ("dispatch_type", ctypes.c_uint32),
        # 4 bytes padding here
        ("create_instance", ctypes.c_void_p),
        ("destroy_instance", ctypes.c_void_p),
        ("dispatch", DispatchMechanisms),
    ]


class HostInterface(ctypes.Structure):
    """Host Interface — function table passed to guests during initialization.

    OWNERSHIP: `'static`, lives as long as the runtime.

    All functions use self-passing pattern (receive HostInterface pointer as first parameter).

    Layout (88 bytes):
    - runtime (*mut c_void): 8 bytes @ 0
    - register_contract: 8 bytes @ 8
    - alloc: 8 bytes @ 16
    - free: 8 bytes @ 24
    - find_by_contract: 8 bytes @ 32
    - find_all_by_contract: 8 bytes @ 40
    - resolve_contract: 8 bytes @ 48
    - get_host_contract: 8 bytes @ 56
    - get_last_error: 8 bytes @ 64
    - list_bundles: 8 bytes @ 72
    - get_dependencies: 8 bytes @ 80
    """

    _fields_ = [
        ("runtime", ctypes.c_void_p),
        ("register_contract", ctypes.c_void_p),
        ("alloc", ctypes.c_void_p),
        ("free", ctypes.c_void_p),
        ("find_by_contract", ctypes.c_void_p),
        ("find_all_by_contract", ctypes.c_void_p),
        ("resolve_contract", ctypes.c_void_p),
        ("get_host_contract", ctypes.c_void_p),
        ("get_last_error", ctypes.c_void_p),
        ("list_bundles", ctypes.c_void_p),
        ("get_dependencies", ctypes.c_void_p),
    ]


class PluginDescriptor(ctypes.Structure):
    """Metadata about a plugin within a bundle.

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
    """Context passed to every guest `polyplug_init()` function.

    The `bundle_path` pointer is runtime-owned and valid for the lifetime of the Runtime.
    **Plugin must not store the raw pointer** — copy the string value if persistence is needed.
    """

    _fields_ = [
        ("bundle_id", ctypes.c_uint64),
        ("bundle_path", StringView),
    ]


class Array(ctypes.Structure):
    """Generic array type for FFI — items, length, alignment.

    Layout (24 bytes):
    - items: 8 bytes @ 0
    - len: 8 bytes @ 8
    - align: 8 bytes @ 16
    """

    _fields_ = [
        ("items", ctypes.c_void_p),
        ("len", ctypes.c_size_t),
        ("align", ctypes.c_size_t),
    ]


class DependencyInfo(ctypes.Structure):
    """Information about a contract dependency.

    Layout (24 bytes):
    - contract_id: 8 bytes @ 0
    - min_version: 8 bytes @ 8 (Version + padding)
    - bundle_id: 8 bytes @ 16
    """

    _fields_ = [
        ("contract_id", ctypes.c_uint64),
        ("min_version", Version),
        ("_padding", ctypes.c_uint32),
        ("bundle_id", ctypes.c_uint64),
    ]


class RuntimeConfig(ctypes.Structure):
    """Configuration passed to `polyplug_runtime_create` during runtime initialisation.

    OWNERSHIP: borrowed for the duration of the runtime build only.
    The caller may free all pointed-to memory after the build returns.
    """

    _fields_ = [
        ("plugin_dirs", ctypes.c_void_p),
        ("plugin_dir_count", ctypes.c_size_t),
        ("compatibility", ctypes.c_uint32),
        ("extensions", ctypes.c_void_p),
        ("extension_count", ctypes.c_size_t),
    ]


# ─── FNV-1a Hash Helpers ──────────────────────────────────────────────────────

FNV_OFFSET: int = 0xCBF29CE484222325
FNV_PRIME: int = 0x00000100000001B3


def fnv1a_64(data: bytes) -> int:
    """Compute FNV-1a 64-bit hash of a byte sequence."""
    hash_val: int = FNV_OFFSET
    for byte in data:
        hash_val ^= byte
        hash_val = (hash_val * FNV_PRIME) & 0xFFFFFFFFFFFFFFFF
    return hash_val


def contract_id(name: str, major_version: int) -> int:
    """Compute the contract ID for 'name@major_version' using FNV-1a 64-bit."""
    canonical: str = f"{name}@{major_version}"
    return fnv1a_64(canonical.encode("utf-8"))


def guest_contract_id(name: str, major: int) -> int:
    """Calculate guest contract ID from name and major version."""
    input_str: str = f"guest_contract:{name}@{major}"
    return fnv1a_64(input_str.encode("utf-8"))


def host_contract_id(name: str, major: int) -> int:
    """Calculate host contract ID from name and major version."""
    input_str: str = f"host_contract:{name}@{major}"
    return fnv1a_64(input_str.encode("utf-8"))


def bundle_id(name: str) -> int:
    """Compute a bundle ID from its name using FNV-1a 64-bit hash."""
    return fnv1a_64(name.encode("utf-8"))


# Legacy function name (use guest_contract_id instead)
def plugin_contract_id(name: str, major: int) -> int:
    """Legacy: Calculate guest contract ID. Use guest_contract_id instead."""
    return guest_contract_id(name, major)


# ─── Utility Functions ────────────────────────────────────────────────────────

def to_str(sv: StringView) -> str:
    """Convert a StringView to a Python string."""
    if sv.ptr is None or sv.len == 0:
        return ""
    return ctypes.string_at(sv.ptr, sv.len).decode("utf-8")
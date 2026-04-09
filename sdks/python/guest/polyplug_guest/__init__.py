"""polyplug_guest — guest-side Python library for polyplug plugin authors.

Re-exports the ABI types needed to write a plugin.
"""

from __future__ import annotations

from polyplug_abi import (
    ABI_ERROR_GENERIC,
    ABI_ERROR_NOT_FOUND,
    ABI_ERROR_PANIC,
    ABI_ERROR_STALE_HANDLE,
    ABI_FUNCTION_NOT_AVAIL,
    ABI_OK,
    ABI_BUFFER_TOO_SMALL,
    AbiError,
    Buffer,
    PluginDescriptor,
    PluginHandle,
    PluginContext,
    HostVTable,
    StringView,
    PluginInterface,
    DispatchType,
    to_str,
)

__all__ = [
    "ABI_OK",
    "ABI_ERROR_GENERIC",
    "ABI_BUFFER_TOO_SMALL",
    "ABI_ERROR_PANIC",
    "ABI_ERROR_NOT_FOUND",
    "ABI_ERROR_STALE_HANDLE",
    "ABI_FUNCTION_NOT_AVAIL",
    "AbiError",
    "Buffer",
    "PluginDescriptor",
    "PluginHandle",
    "PluginContext",
    "HostVTable",
    "StringView",
    "PluginInterface",
    "DispatchType",
    "to_str",
    "alloc_string",
    "store_host_vtable",
    "get_host_vtable",
]


_host_alloc = None
_host_free = None
_host_vtable_ptr: int = 0


def store_host_vtable(host_vtable_ptr: int) -> None:
    global _host_vtable_ptr
    _host_vtable_ptr = host_vtable_ptr


def get_host_vtable() -> int:
    return _host_vtable_ptr


def _init_allocator(host_vtable_ptr: int, rt_ctx: int) -> None:
    """Initialize the allocator with host interface pointers."""
    global _host_alloc, _host_free
    import ctypes

    host = ctypes.cast(host_vtable_ptr, ctypes.POINTER(HostVTable))
    _host_alloc = host.contents.alloc
    _host_free = host.contents.free


def alloc_string(s: str) -> StringView:
    """Allocate a StringView from a Python string using host allocator.

    Args:
        s: Python string to allocate

    Returns:
        StringView pointing to host-allocated memory
    """
    if _host_alloc is None:
        raise RuntimeError("alloc_string called before _init_allocator")
    import ctypes

    encoded = s.encode("utf-8")
    ptr = ctypes.cast(
        _host_alloc, ctypes.CFUNCTYPE(ctypes.c_void_p, ctypes.c_size_t, ctypes.c_size_t)
    )(len(encoded), 1)
    ctypes.memmove(ptr, encoded, len(encoded))
    return StringView(ptr=ctypes.c_char_p(ptr), len=len(encoded))

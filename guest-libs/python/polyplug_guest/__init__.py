# THIS FILE IS HAND-AUTHORED (part of polyplug guest-libs/python)
"""polyplug_guest — guest-side Python library for polyplug plugin authors.

Re-exports the ABI types needed to write a plugin.
"""

from __future__ import annotations

from polyplug_guest.abi import (
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
    PluginRegistrar,
    PluginVTable,
    REGISTER_FN_TYPE,
    StringView,
    host_alloc,
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
    "PluginRegistrar",
    "PluginVTable",
    "REGISTER_FN_TYPE",
    "StringView",
    "host_alloc",
    "to_str",
    "alloc_string",
]


def to_str(sv: StringView) -> str:
    """Convert a StringView to a Python str.

    Args:
        sv: StringView from polyplug ABI

    Returns:
        Python string (UTF-8 decoded)
    """
    if not sv.ptr or sv.len == 0:
        return ""
    import ctypes

    data = ctypes.cast(sv.ptr, ctypes.POINTER(ctypes.c_char * sv.len)).contents
    return bytes(data).decode("utf-8")


def alloc_string(s: str) -> StringView:
    """Allocate a StringView from a Python str using host allocator.

    Args:
        s: Python string to convert

    Returns:
        StringView pointing to host-allocated memory
        Caller (host) must free via polyplug_host_free
    """
    data = s.encode("utf-8")
    ptr = host_alloc(len(data), 1)
    import ctypes

    ctypes.memmove(ptr, data, len(data))
    return StringView(ptr, len(data))

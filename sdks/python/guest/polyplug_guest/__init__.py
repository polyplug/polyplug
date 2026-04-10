"""polyplug_guest — guest-side Python library for polyplug plugin authors.

Re-exports the ABI types needed to write a plugin.
"""

from __future__ import annotations

from polyplug_abi import (
    AbiErrorCode,
    AbiError,
    Buffer,
    PluginDescriptor,
    GuestContractHandle,
    BundleInitContext,
    HostInterface,
    StringView,
    GuestContractInterface,
    DispatchType,
    to_str,
)

__all__ = [
    "AbiErrorCode",
    "AbiError",
    "Buffer",
    "PluginDescriptor",
    "GuestContractHandle",
    "BundleInitContext",
    "HostInterface",
    "StringView",
    "GuestContractInterface",
    "DispatchType",
    "to_str",
    "alloc_string",
    "store_host_interface",
    "get_host_interface",
]


_host_alloc = None
_host_free = None
_host_interface_ptr: int = 0


def store_host_interface(host_interface_ptr: int) -> None:
    global _host_interface_ptr
    _host_interface_ptr = host_interface_ptr


def get_host_interface() -> int:
    return _host_interface_ptr


# Legacy aliases for backwards compatibility
store_host_vtable = store_host_interface
get_host_vtable = get_host_interface


def _init_allocator(host_interface_ptr: int, rt_ctx: int) -> None:
    """Initialize the allocator with host interface pointers."""
    global _host_alloc, _host_free
    import ctypes

    host = ctypes.cast(host_interface_ptr, ctypes.POINTER(HostInterface))
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

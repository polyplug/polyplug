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
    HostApi,
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
    "HostApi",
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


def _init_allocator(host_interface_ptr: int, rt_ctx: int) -> None:
    """Initialize the allocator with host interface pointers."""
    global _host_alloc, _host_free
    import ctypes

    host = ctypes.cast(host_interface_ptr, ctypes.POINTER(HostApi))
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
    # The host allocator uses the self-passing convention: alloc(this, size, align).
    # `_host_alloc` is already the correctly typed HostApi.alloc CFUNCTYPE
    # field, so it must be called with the host interface pointer as the first
    # argument. Dropping it (calling as (size, align)) shifts every argument by a
    # register and corrupts the allocation request.
    ptr = _host_alloc(_host_interface_ptr, len(encoded), 1)
    ctypes.memmove(ptr, encoded, len(encoded))
    # StringView.ptr is c_void_p; pass the raw integer address (ctypes returns
    # the alloc result as an int) rather than a c_char_p, which is incompatible.
    return StringView(ptr=ptr, len=len(encoded))

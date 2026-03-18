# THIS FILE IS HAND-AUTHORED (part of polyplug host-libs/python)
"""
Base ABI types for polyplug host-side code.

These types mirror the C ABI and are used by both hand-authored code
and polyplugc-generated code.
"""

from __future__ import annotations

import ctypes
from typing import ClassVar


# ABI constants
ABI_OK: int = 0
ABI_ERROR_GENERIC: int = 1


class StringView(ctypes.Structure):
    """
    Non-owning UTF-8 string view - mirrors polyplug::abi::StringView.

    This is the canonical ABI string type used across the polyplug boundary.
    Both host and guest use the same layout, but each side manages memory
    according to the host allocator contract.
    """

    _fields_: ClassVar[list[tuple[str, type]]] = [
        ("ptr", ctypes.c_void_p),
        ("len", ctypes.c_size_t),
    ]

    # SAFETY: StringView is non-owning. Callers must keep buffer alive during FFI calls.
    # Use inline buffer creation: buf = (ctypes.c_uint8 * len(data))(*data)

    def to_bytes(self) -> bytes:
        """Convert StringView to bytes."""
        if self.ptr is None or self.ptr == 0 or self.len == 0:
            return b""
        return ctypes.string_at(self.ptr, self.len)

    def to_str(self) -> str:
        """Convert StringView to string (UTF-8)."""
        return self.to_bytes().decode("utf-8", errors="replace")


class PluginHandle(ctypes.Structure):
    """
    Opaque handle to a loaded plugin - mirrors polyplug::abi::PluginHandle.
    """

    _fields_: ClassVar[list[tuple[str, type]]] = [
        ("index", ctypes.c_uint32),
        ("generation", ctypes.c_uint32),
    ]


# Sentinel value for invalid handles
NULL_HANDLE: int = (1 << 64) - 1


class AbiError(ctypes.Structure):
    """ABI error structure."""

    _fields_ = [
        ("code", ctypes.c_uint32),
        ("_pad", ctypes.c_uint32),
        ("message", StringView),
    ]


# Load polyplug library for host_free
try:
    _polyplug_lib = ctypes.CDLL(
        ctypes.util.find_library("polyplug") or "libpolyplug.so"
    )

    def polyplug_host_free(ptr: ctypes.c_void_p, size: int, align: int) -> None:
        """Free memory allocated by polyplug."""
        _polyplug_lib.polyplug_host_free(ptr, size, align)
except:

    def polyplug_host_free(ptr: ctypes.c_void_p, size: int, align: int) -> None:
        """Stub polyplug_host_free."""
        pass


def bundle_id(name: str) -> int:
    """Compute FNV-1a 64-bit hash of bundle name."""
    h = 0xCBF29CE484222325
    for b in name.encode("utf-8"):
        h ^= b
        h = (h * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return h

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

    @classmethod
    def from_bytes(cls, data: bytes) -> "StringView":
        """Create a StringView from bytes (uses host allocator)."""
        sv: StringView = cls()
        if data:
            buf: ctypes.Array = ctypes.create_string_buffer(data)
            sv.ptr = ctypes.cast(buf, ctypes.c_void_p)
            sv.len = len(data)
        else:
            sv.ptr = ctypes.c_void_p(0)
            sv.len = 0
        return sv

    @classmethod
    def from_static(cls, data: bytes) -> "StringView":
        """Create a StringView from static bytes (no copy)."""
        sv: StringView = cls()
        if data:
            sv.ptr = ctypes.cast(ctypes.c_char_p(data), ctypes.c_void_p)
            sv.len = len(data)
        else:
            sv.ptr = ctypes.c_void_p(0)
            sv.len = 0
        return sv

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


def contract_id(name: str, major: int) -> int:
    """Compute FNV-1a 64-bit hash of 'name@major'."""
    s = f"{name}@{major}"
    h = 0xcbf29ce484222325
    for b in s.encode('utf-8'):
        h ^= b
        h = (h * 0x100000001b3) & 0xFFFFFFFFFFFFFFFF
    return h


def bundle_id(name: str) -> int:
    """Compute FNV-1a 64-bit hash of bundle name."""
    h = 0xcbf29ce484222325
    for b in name.encode('utf-8'):
        h ^= b
        h = (h * 0x100000001b3) & 0xFFFFFFFFFFFFFFFF
    return h

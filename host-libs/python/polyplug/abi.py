# THIS FILE IS HAND-AUTHORED (part of polyplug host-libs/python)
"""
Base ABI types for polyplug host-side code.

These types mirror the C ABI and are used by both hand-authored code
and polyplugc-generated code.
"""

from __future__ import annotations

import ctypes
import ctypes.util
from enum import IntEnum
from typing import ClassVar, Optional


# ABI constants
ABI_OK: int = 0
ABI_ERROR_GENERIC: int = 1
ABI_FUNCTION_NOT_AVAIL: int = 6


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

    @classmethod
    def from_str(cls, s: str) -> "StringView":
        data = s.encode("utf-8")
        buf = ctypes.create_string_buffer(data, len(data))
        sv = cls()
        sv.ptr = ctypes.cast(buf, ctypes.c_void_p)
        sv.len = len(data)
        sv._buf = buf
        return sv

    def to_bytes(self) -> bytes:
        if self.ptr is None or self.ptr == 0 or self.len == 0:
            return b""
        return ctypes.string_at(self.ptr, self.len)

    def to_str(self) -> str:
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


# ─── Hot-reload notification types ─────────────────────────────────────────────


class ReloadPhaseType(IntEnum):
    """Type tag for ReloadPhase variants - mirrors ffi::ReloadPhaseType."""

    PREPARING = 0
    RELOADED = 1
    FAILED = 2


class ReloadPhase:
    """
    Python representation of hot-reload phase notification.

    Mirrors the C ABI `ReloadPhaseC` struct from ffi.rs.

    Attributes:
        type: The phase type (PREPARING, RELOADED, or FAILED).
        bundle_id: The FNV-1a hash of the bundle name.
        bundle_name: The human-readable bundle name.
        retry_count: Number of retry attempts (valid only for PREPARING).
        reason: Failure reason string (valid only for FAILED).
    """

    def __init__(
        self,
        type: ReloadPhaseType,
        bundle_id: int,
        bundle_name: str,
        retry_count: int = 0,
        reason: Optional[str] = None,
    ) -> None:
        self.type: ReloadPhaseType = type
        self.bundle_id: int = bundle_id
        self.bundle_name: str = bundle_name
        self.retry_count: int = retry_count
        self.reason: Optional[str] = reason

    @classmethod
    def from_c_struct(cls, c_phase: "ReloadPhaseCStruct") -> "ReloadPhase":
        """Create a ReloadPhase from the C struct received via FFI callback."""
        bundle_name: str = (
            c_phase.bundle_name.to_str() if c_phase.bundle_name.len > 0 else ""
        )
        reason: Optional[str] = None
        if c_phase.reason.len > 0:
            reason = c_phase.reason.to_str()
        return cls(
            type=ReloadPhaseType(c_phase.phase_type),
            bundle_id=c_phase.bundle_id,
            bundle_name=bundle_name,
            retry_count=c_phase.retry_count,
            reason=reason,
        )

    def is_preparing(self) -> bool:
        """Return True if this is a PREPARING phase."""
        return self.type == ReloadPhaseType.PREPARING

    def is_reloaded(self) -> bool:
        """Return True if this is a RELOADED phase."""
        return self.type == ReloadPhaseType.RELOADED

    def is_failed(self) -> bool:
        """Return True if this is a FAILED phase."""
        return self.type == ReloadPhaseType.FAILED

    def __repr__(self) -> str:
        return (
            f"ReloadPhase(type={self.type.name}, bundle_id={self.bundle_id}, "
            f"bundle_name={self.bundle_name!r}, retry_count={self.retry_count}, "
            f"reason={self.reason!r})"
        )


class ReloadPhaseCStruct(ctypes.Structure):
    """
    C-compatible struct for ReloadPhase - mirrors ffi::ReloadPhaseC.

    Used internally for FFI callback handling.
    """

    _fields_: ClassVar[list[tuple[str, type]]] = [
        ("phase_type", ctypes.c_uint32),
        ("bundle_id", ctypes.c_uint64),
        ("bundle_name", StringView),
        ("retry_count", ctypes.c_uint32),
        ("reason", StringView),
    ]

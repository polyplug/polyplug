# THIS FILE IS HAND-AUTHORED (part of polyplug guest-libs/python)
# ABI ctypes definitions for the polyplug guest library.
# These match the frozen polyplug ABI exactly — DO NOT modify field order or sizes.
from __future__ import annotations

import ctypes
from typing import ClassVar

# ── ABI constants ────────────────────────────────────────────────────────────

ABI_OK: int = 0
ABI_ERROR_GENERIC: int = 1
ABI_BUFFER_TOO_SMALL: int = 2
ABI_ERROR_PANIC: int = 3
ABI_ERROR_NOT_FOUND: int = 4
ABI_ERROR_STALE_HANDLE: int = 5
ABI_FUNCTION_NOT_AVAIL: int = 6

# ── ABI structs ──────────────────────────────────────────────────────────────


class StringView(ctypes.Structure):
    """Non-owning UTF-8 string view. ptr+len = 16 bytes."""

    _fields_: ClassVar = [
        ("ptr", ctypes.c_char_p),  # *const u8
        ("len", ctypes.c_size_t),  # usize
    ]
    
    # Keep reference to encoded bytes to prevent GC
    _refs: ClassVar = {}

    def to_str(self) -> str:
        """Decode this StringView as a UTF-8 Python string (copies the bytes)."""
        raw: bytes = ctypes.string_at(self.ptr, self.len)
        return raw.decode("utf-8")

    @staticmethod
    def from_string(s: str) -> StringView:
        """Create a StringView from a Python string.
        The encoded bytes are kept alive internally."""
        encoded = s.encode('utf-8')
        # Create ctypes c_char_p from bytes
        c_str = ctypes.c_char_p(encoded)
        sv = StringView(ptr=c_str, len=len(encoded))
        # Keep reference to prevent GC
        StringView._refs[id(sv)] = (c_str, encoded)
        return sv


class Buffer(ctypes.Structure):
    """Owning byte buffer allocated via host_alloc. 24 bytes."""

    _fields_: ClassVar = [
        ("ptr", ctypes.c_void_p),  # *mut u8
        ("len", ctypes.c_size_t),  # usize (used)
        ("cap", ctypes.c_size_t),  # usize (allocated)
    ]


class PluginContext(ctypes.Structure):
    """Context passed to polyplug_init. bundle_path is runtime-owned; copy if you need persistence."""

    _fields_: ClassVar = [
        ("bundle_path", StringView),
    ]

    def bundle_path_str(self) -> str:
        """Return bundle_path as a Python str (copies the bytes)."""
        ptr: int = self.bundle_path.ptr
        length: int = self.bundle_path.len
        raw: bytes = ctypes.string_at(ptr, length)
        return raw.decode("utf-8")


class AbiError(ctypes.Structure):
    """ABI error: code(4) + _pad(4) + message/StringView(16) = 24 bytes.
    The 4-byte pad between code and message is REQUIRED for correct ABI alignment."""

    _fields_: ClassVar = [
        ("code", ctypes.c_uint32),
        ("_pad", ctypes.c_uint32),  # alignment padding — DO NOT REMOVE
        ("message", StringView),
    ]


class PluginHandle(ctypes.Structure):
    """index(4) + generation(4) = 8 bytes."""

    _fields_: ClassVar = [
        ("index", ctypes.c_uint32),
        ("generation", ctypes.c_uint32),
    ]


class PluginVTable(ctypes.Structure):
    """contract_id(8) + contract_version(4) + function_count(4) + functions(8) = 24 bytes."""

    _fields_: ClassVar = [
        ("contract_id", ctypes.c_uint64),
        ("contract_version", ctypes.c_uint32),
        ("function_count", ctypes.c_uint32),
        ("functions", ctypes.c_void_p),  # *const *const ()
    ]


class PluginDescriptor(ctypes.Structure):
    """name/StringView(16) + contract_name/StringView(16) + version_major(4)
    + version_minor(4) + version_patch(4) + _tail_pad(4) = 48 bytes.
    The 4-byte tail pad is REQUIRED — DO NOT REMOVE."""

    _fields_: ClassVar = [
        ("name", StringView),
        ("contract_name", StringView),
        ("version_major", ctypes.c_uint32),
        ("version_minor", ctypes.c_uint32),
        ("version_patch", ctypes.c_uint32),
        ("_tail_pad", ctypes.c_uint32),  # alignment padding — DO NOT REMOVE
    ]


class PluginRegistrar(ctypes.Structure):
    """register_plugin fnptr(8) + host/HostVTable*(8) = 16 bytes."""

    _fields_: ClassVar = [
        ("register_plugin", ctypes.c_void_p),  # fnptr
        ("host", ctypes.c_void_p),  # *const HostVTable
    ]


# Type alias for the dispatch function signature
REGISTER_FN_TYPE = ctypes.CFUNCTYPE(
    ctypes.c_uint32,  # return: AbiError.code
    ctypes.POINTER(PluginRegistrar),  # registrar
    ctypes.POINTER(PluginDescriptor),  # descriptor
    ctypes.POINTER(PluginVTable),  # vtable
)

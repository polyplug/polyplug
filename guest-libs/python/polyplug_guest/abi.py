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


class Buffer(ctypes.Structure):
    """Owning byte buffer allocated via host_alloc. 24 bytes."""

    _fields_: ClassVar = [
        ("ptr", ctypes.c_void_p),  # *mut u8
        ("len", ctypes.c_size_t),  # usize (used)
        ("cap", ctypes.c_size_t),  # usize (allocated)
    ]


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
        ("_tail_pad", ctypes.c_uint32),  # tail padding — DO NOT REMOVE
    ]


class PluginRegistrar(ctypes.Structure):
    """register_plugin fn ptr(8) + host ptr(8) = 16 bytes."""

    pass  # _fields_ set after REGISTER_FN_TYPE is defined below


# Function type for PluginRegistrar.register_plugin
REGISTER_FN_TYPE = ctypes.CFUNCTYPE(
    AbiError,
    ctypes.POINTER(PluginRegistrar),
    ctypes.POINTER(PluginDescriptor),
    ctypes.POINTER(PluginVTable),
)

PluginRegistrar._fields_ = [
    ("register_plugin", REGISTER_FN_TYPE),
    ("host", ctypes.c_void_p),
]

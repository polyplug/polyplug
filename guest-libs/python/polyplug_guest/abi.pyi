from __future__ import annotations
import ctypes
from typing import ClassVar

ABI_OK: int
ABI_ERROR_GENERIC: int
ABI_BUFFER_TOO_SMALL: int
ABI_ERROR_PANIC: int
ABI_ERROR_NOT_FOUND: int
ABI_ERROR_STALE_HANDLE: int
ABI_FUNCTION_NOT_AVAIL: int

class StringView(ctypes.Structure):
    ptr: bytes | None
    len: int
    _fields_: ClassVar[list[tuple[str, type]]]

class Buffer(ctypes.Structure):
    ptr: int | None
    len: int
    cap: int
    _fields_: ClassVar[list[tuple[str, type]]]

class AbiError(ctypes.Structure):
    code: int
    message: StringView
    _fields_: ClassVar[list[tuple[str, type]]]

class PluginHandle(ctypes.Structure):
    index: int
    generation: int
    _fields_: ClassVar[list[tuple[str, type]]]

class PluginVTable(ctypes.Structure):
    contract_id: int
    contract_version: int
    function_count: int
    functions: int | None
    _fields_: ClassVar[list[tuple[str, type]]]

class PluginDescriptor(ctypes.Structure):
    name: StringView
    contract_name: StringView
    version_major: int
    version_minor: int
    version_patch: int
    _fields_: ClassVar[list[tuple[str, type]]]

class PluginRegistrar(ctypes.Structure):
    host: int | None
    _fields_: ClassVar[list[tuple[str, type]]]

REGISTER_FN_TYPE: type[ctypes.CFUNCTYPE]

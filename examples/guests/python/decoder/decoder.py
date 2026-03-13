"""
examples/guests/python/decoder/decoder.py
Python guest plugin implementing pipeline.decoder@1.

Contract: decode(input: Buffer) -> DataRecord
  - Parses a CSV line ("name,value,count") into a DataRecord.
  - DECODER_CONTRACT_ID = 0x133E62ABD6E7D5BE  (FNV1a-64 of "pipeline.decoder" v1)

Uses polyplug_guest.abi from guest-libs/python for ABI types and registration.
"""

from __future__ import annotations

import ctypes
import sys
from pathlib import Path

# Add guest-libs/python to path so we can import polyplug_guest
# Path: decoder.py -> decoder/ -> python/ -> guests/ -> examples/ -> repo_root
_REPO_ROOT: Path = Path(__file__).parent.parent.parent.parent.parent
sys.path.insert(0, str(_REPO_ROOT / "guest-libs" / "python"))

from polyplug_guest.abi import (
    ABI_OK,
    AbiError,
    PluginDescriptor,
    PluginRegistrar,
    PluginVTable,
    StringView,
)

# ─── Contract Constants ───────────────────────────────────────────────────────

DECODER_CONTRACT_ID: int = 0x133E62ABD6E7D5BE  # FNV1a-64 of "pipeline.decoder" v1

_PLUGIN_NAME: bytes = b"csv_decoder_python"
_CONTRACT_NAME: bytes = b"pipeline.decoder"

# ─── ABI Type Definitions ─────────────────────────────────────────────────────


class Buffer(ctypes.Structure):
    """Raw byte buffer passed from the host. sizeof == 24 on 64-bit."""

    _fields_: list = [
        ("ptr", ctypes.c_void_p),
        ("len", ctypes.c_size_t),
        ("cap", ctypes.c_size_t),
    ]


class DataRecord(ctypes.Structure):
    """Output record. sizeof == 40 on 64-bit."""

    _fields_: list = [
        ("name", StringView),
        ("value", StringView),
        ("count", ctypes.c_uint32),
        ("_pad", ctypes.c_uint32),
    ]


# ─── Persistent storage for string backing buffers ────────────────────────────
# ctypes string buffers must outlive the call — keep as module-level state.

_last_name_buf: bytes = b""
_last_value_buf: bytes = b""

# ─── Decode Implementation ────────────────────────────────────────────────────


def _py_decode(args_ptr: ctypes.c_void_p, out_ptr: ctypes.c_void_p) -> AbiError:
    """
    Implements: decode(input: Buffer) -> DataRecord

    args_ptr: pointer to a Buffer struct (CSV bytes).
    out_ptr:  pointer to a DataRecord struct (pre-allocated by host).

    Returns AbiError(code=0) on success, AbiError(code=1) on error.
    """
    global _last_name_buf, _last_value_buf

    if not args_ptr or not out_ptr:
        return AbiError(code=1)

    buf: Buffer = Buffer.from_address(args_ptr)  # type: ignore[arg-type]
    if buf.ptr is None or buf.len == 0:
        return AbiError(code=1)

    raw_bytes: bytes = bytes((ctypes.c_uint8 * buf.len).from_address(buf.ptr))
    try:
        csv_str: str = raw_bytes.decode("utf-8")
    except UnicodeDecodeError:
        return AbiError(code=1)

    line: str = csv_str.rstrip("\r\n")
    parts: list = line.split(",", 2)
    if len(parts) != 3:
        return AbiError(code=1)

    name_str: str = parts[0]
    value_str: str = parts[1]
    count_str: str = parts[2].strip()

    try:
        count: int = int(count_str)
        if count < 0 or count > 0xFFFF_FFFF:
            return AbiError(code=1)
    except ValueError:
        return AbiError(code=1)

    # Encode strings to UTF-8 and keep alive in module-level storage.
    _last_name_buf = name_str.encode("utf-8")
    _last_value_buf = value_str.encode("utf-8")

    record: DataRecord = DataRecord.from_address(out_ptr)  # type: ignore[arg-type]
    # Use bytes directly as the pointer source — ctypes will hold a reference
    # to the bytes object via the StringView.ptr assignment.
    # The module-level _last_name_buf / _last_value_buf keep the bytes alive.
    record.name.ptr = _last_name_buf
    record.name.len = len(_last_name_buf)

    record.value.ptr = _last_value_buf
    record.value.len = len(_last_value_buf)

    record.count = count
    record._pad = 0

    return AbiError(code=ABI_OK)


# ── ABI entry point type ──────────────────────────────────────────────────────

# On x86_64 System V ABI, a 24-byte struct return (AbiError) is passed as a
# hidden sret pointer prepended before the declared parameters.  ctypes
# callbacks cannot return ctypes.Structure directly, so we declare three
# void* args (sret, args, out) and write the struct into sret manually.
_DISPATCH_FN_TYPE = ctypes.CFUNCTYPE(
    None,  # void return — result written via sret pointer
    ctypes.c_void_p,  # sret: hidden pointer where caller expects AbiError
    ctypes.c_void_p,  # args_ptr
    ctypes.c_void_p,  # out_ptr
)

_ABI_ERROR_SIZE: int = ctypes.sizeof(AbiError)


def _wrap_sret(impl: object) -> object:
    """Wrap a two-arg impl fn with the three-arg sret calling convention."""

    def _sret_wrapper(
        sret_ptr: ctypes.c_void_p, args_ptr: ctypes.c_void_p, out_ptr: ctypes.c_void_p
    ) -> None:
        err: AbiError = impl(args_ptr, out_ptr)  # type: ignore[operator]
        ctypes.memmove(sret_ptr, ctypes.addressof(err), _ABI_ERROR_SIZE)

    return _sret_wrapper


# Module-level function object cache (MUST be module-level — not per-call, GC will collect otherwise)
_FN_DECODE = _DISPATCH_FN_TYPE(_wrap_sret(_py_decode))

_FUNCTIONS_ARRAY = (ctypes.c_void_p * 1)(
    ctypes.cast(_FN_DECODE, ctypes.c_void_p),
)

# ── VTable and descriptor ─────────────────────────────────────────────────────

_VTABLE = PluginVTable(
    contract_id=DECODER_CONTRACT_ID,
    contract_version=0,  # v1.0 → (minor << 16 | patch) = 0
    function_count=1,
    functions=ctypes.cast(_FUNCTIONS_ARRAY, ctypes.c_void_p),
)

_DESCRIPTOR = PluginDescriptor(
    name=StringView(ptr=_PLUGIN_NAME, len=len(_PLUGIN_NAME)),
    contract_name=StringView(ptr=_CONTRACT_NAME, len=len(_CONTRACT_NAME)),
    version_major=1,
    version_minor=0,
    version_patch=0,
)

# ── ABI entry points ──────────────────────────────────────────────────────────


def polyplug_abi_version() -> int:
    """Returns the ABI version supported by this plugin (1)."""
    return 1


def polyplug_init(registrar_addr: int, ctx_ptr: int) -> None:
    """Called by PythonLoader with the PluginRegistrar address and PluginContext pointer."""
    registrar: PluginRegistrar = PluginRegistrar.from_address(registrar_addr)
    err: AbiError = registrar.register_plugin(
        ctypes.byref(registrar),
        ctypes.byref(_DESCRIPTOR),
        ctypes.byref(_VTABLE),
    )
    if err.code != ABI_OK:
        raise RuntimeError(f"register_plugin failed with code {err.code}")

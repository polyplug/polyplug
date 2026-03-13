# examples/guests/python/reporter/summary_reporter.py
#
# Summary Reporter — Python plugin implementing pipeline.reporter@1
# Demonstrates: Python plugin, v1.1 compatibility with v1.0 contract request
from __future__ import annotations

import ctypes
import sys
from pathlib import Path

# Add guest-libs/python to path so we can import polyplug_guest
# Path: summary_reporter.py -> reporter/ -> python/ -> guests/ -> examples/ -> repo_root
_REPO_ROOT: Path = Path(__file__).parent.parent.parent.parent.parent
sys.path.insert(0, str(_REPO_ROOT / "guest-libs" / "python"))

from polyplug_guest.abi import (
    ABI_OK,
    AbiError,
    PluginDescriptor,
    PluginRegistrar,
    PluginVTable,
    StringView,
    REGISTER_FN_TYPE,
)

# Contract ID (FNV-1a-64("pipeline.reporter@1"))
REPORTER_CONTRACT_ID: int = 0xD50E539CAE219A15


# DataRecord — mirrors examples/abi_types.md (frozen ABI)
# Layout: name@0[16], value@16[16], count@32[4], _pad@36[4] — total 40 bytes
class DataRecord(ctypes.Structure):
    _fields_ = [
        ("name", StringView),
        ("value", StringView),
        ("count", ctypes.c_uint32),
        ("_pad", ctypes.c_uint32),
    ]


# Module-level buffer cache — prevents GC from collecting strings mid-call
_last_result_bytes: list = []

# ── Plugin function implementations ──────────────────────────────────────────


def _py_report(args_ptr: ctypes.c_void_p, out_ptr: ctypes.c_void_p) -> AbiError:
    """Implement pipeline.reporter@1 report function."""
    args: DataRecord = DataRecord.from_address(args_ptr)  # type: ignore[arg-type]
    # Decode StringView fields — ptr is c_char_p (raw address integer when accessed)
    name_bytes: bytes = bytes(ctypes.string_at(args.name.ptr, args.name.len))
    value_bytes: bytes = bytes(ctypes.string_at(args.value.ptr, args.value.len))
    name_str: str = name_bytes.decode("utf-8")
    value_str: str = value_bytes.decode("utf-8")
    count: int = int(args.count)

    summary_str: str = f"Summary: name={name_str} value={value_str} count={count}"
    result_bytes: bytes = summary_str.encode("utf-8")

    # Keep bytes alive — module-level list prevents GC collection
    _last_result_bytes.clear()
    _last_result_bytes.append(result_bytes)

    # Write StringView into out_ptr — ptr is c_char_p, assign bytes directly
    sv_ptr: ctypes.Array = ctypes.cast(out_ptr, ctypes.POINTER(StringView))
    sv_ptr[0].ptr = result_bytes
    sv_ptr[0].len = len(result_bytes)

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
_FN_REPORT = _DISPATCH_FN_TYPE(_wrap_sret(_py_report))

_FUNCTIONS_ARRAY = (ctypes.c_void_p * 1)(
    ctypes.cast(_FN_REPORT, ctypes.c_void_p),
)

# ── VTable and descriptor ─────────────────────────────────────────────────────

# v1.1: contract_version = (1 << 16) | 0  (minor=1, patch=0)
_VTABLE = PluginVTable(
    contract_id=REPORTER_CONTRACT_ID,
    contract_version=(1 << 16) | 0,
    function_count=1,
    functions=ctypes.cast(_FUNCTIONS_ARRAY, ctypes.c_void_p),
)

_PLUGIN_NAME_BYTES: bytes = b"summary-reporter-python"
_CONTRACT_NAME_BYTES: bytes = b"pipeline.reporter"

_DESCRIPTOR = PluginDescriptor(
    name=StringView(ptr=_PLUGIN_NAME_BYTES, len=len(_PLUGIN_NAME_BYTES)),
    contract_name=StringView(ptr=_CONTRACT_NAME_BYTES, len=len(_CONTRACT_NAME_BYTES)),
    version_major=1,
    version_minor=1,
    version_patch=0,
)

# ── ABI entry points ──────────────────────────────────────────────────────────


def polyplug_abi_version() -> int:
    """Called by host to verify ABI version."""
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

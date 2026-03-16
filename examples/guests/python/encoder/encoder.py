"""
examples/guests/python/encoder/encoder.py
Python guest plugin implementing pipeline.Encoder@1.

Contract: encode(data: StringView) -> StringView
Input:  "TRANSFORMED:NAME|value (transformed)|43"
Output: "NAME,value (transformed),43"
ENCODER_CONTRACT_ID = 0x127D1703C6EFB432

Uses polyplug_guest.abi from guest-libs/python for ABI types and registration.
"""

from __future__ import annotations

import ctypes
import sys
from pathlib import Path

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

ENCODER_CONTRACT_ID: int = 0x127D1703C6EFB432

_PLUGIN_NAME_BYTES: bytes = b"encoder-python"
_CONTRACT_NAME_BYTES: bytes = b"pipeline.Encoder"

_last_result_bytes: list = []


def _py_encode(args_ptr: ctypes.c_void_p, out_ptr: ctypes.c_void_p) -> AbiError:
    """Implements: encode(data: StringView) -> StringView

    Parses "TRANSFORMED:NAME|value (transformed)|43" format
    and returns "NAME,value (transformed),43" (CSV format).
    """
    args: StringView = StringView.from_address(args_ptr)  # type: ignore[arg-type]
    data_bytes: bytes = bytes(ctypes.string_at(args.ptr, args.len))
    data_str: str = data_bytes.decode("utf-8")

    prefix: str = "TRANSFORMED:"
    if data_str.startswith(prefix):
        payload: str = data_str[len(prefix) :]
    else:
        payload = data_str

    result_str: str = payload.replace("|", ",")
    result_bytes: bytes = result_str.encode("utf-8")

    _last_result_bytes.clear()
    _last_result_bytes.append(result_bytes)

    sv_ptr: ctypes.Array = ctypes.cast(out_ptr, ctypes.POINTER(StringView))
    sv_ptr[0].ptr = result_bytes
    sv_ptr[0].len = len(result_bytes)

    return AbiError(code=ABI_OK)


_DISPATCH_FN_TYPE = ctypes.CFUNCTYPE(
    None,
    ctypes.c_void_p,
    ctypes.c_void_p,
    ctypes.c_void_p,
)

_ABI_ERROR_SIZE: int = ctypes.sizeof(AbiError)


def _wrap_sret(impl: object) -> object:
    def _sret_wrapper(
        sret_ptr: ctypes.c_void_p, args_ptr: ctypes.c_void_p, out_ptr: ctypes.c_void_p
    ) -> None:
        err: AbiError = impl(args_ptr, out_ptr)  # type: ignore[operator]
        ctypes.memmove(sret_ptr, ctypes.addressof(err), _ABI_ERROR_SIZE)

    return _sret_wrapper


_FN_ENCODE = _DISPATCH_FN_TYPE(_wrap_sret(_py_encode))

_FUNCTIONS_ARRAY = (ctypes.c_void_p * 1)(
    ctypes.cast(_FN_ENCODE, ctypes.c_void_p),
)

_VTABLE = PluginVTable(
    contract_id=ENCODER_CONTRACT_ID,
    contract_version=0,
    function_count=1,
    functions=ctypes.cast(_FUNCTIONS_ARRAY, ctypes.c_void_p),
)

_DESCRIPTOR = PluginDescriptor(
    name=StringView(ptr=_PLUGIN_NAME_BYTES, len=len(_PLUGIN_NAME_BYTES)),
    contract_name=StringView(ptr=_CONTRACT_NAME_BYTES, len=len(_CONTRACT_NAME_BYTES)),
    version_major=1,
    version_minor=0,
    version_patch=0,
)


def polyplug_abi_version() -> int:
    return 1


def polyplug_init(registrar_addr: int, ctx_ptr: int) -> None:
    registrar: PluginRegistrar = PluginRegistrar.from_address(registrar_addr)
    err: AbiError = registrar.register_plugin(
        ctypes.byref(registrar),
        ctypes.byref(_DESCRIPTOR),
        ctypes.byref(_VTABLE),
    )
    if err.code != ABI_OK:
        raise RuntimeError(f"register_plugin failed with code {err.code}")

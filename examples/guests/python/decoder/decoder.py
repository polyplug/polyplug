"""
examples/guests/python/decoder/decoder.py
Python guest plugin implementing data.Transformer@1.

Contract: transform(input: string) -> string
Returns: "python:transform({input})"
TRANSFORMER_CONTRACT_ID = 0x3D53C682F3F5A9EF

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

TRANSFORMER_CONTRACT_ID: int = 0x3D53C682F3F5A9EF

_PLUGIN_NAME: bytes = b"transformer_python"
_CONTRACT_NAME: bytes = b"data.Transformer"

_last_result_buf: bytes = b""


def _py_transform(args_ptr: ctypes.c_void_p, out_ptr: ctypes.c_void_p) -> AbiError:
    """Implements: transform(input: StringView) -> StringView"""
    global _last_result_buf

    if not args_ptr or not out_ptr:
        return AbiError(code=1)

    args: StringView = StringView.from_address(args_ptr)  # type: ignore[arg-type]
    input_bytes: bytes = bytes(ctypes.string_at(args.ptr, args.len))
    input_str: str = input_bytes.decode("utf-8")

    result_str: str = f"python:transform({input_str})"
    _last_result_buf = result_str.encode("utf-8")

    out: ctypes.Array = ctypes.cast(out_ptr, ctypes.POINTER(StringView))
    out[0].ptr = _last_result_buf
    out[0].len = len(_last_result_buf)

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


_FN_TRANSFORM = _DISPATCH_FN_TYPE(_wrap_sret(_py_transform))

_FUNCTIONS_ARRAY = (ctypes.c_void_p * 1)(
    ctypes.cast(_FN_TRANSFORM, ctypes.c_void_p),
)

_VTABLE = PluginVTable(
    contract_id=TRANSFORMER_CONTRACT_ID,
    contract_version=0,
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

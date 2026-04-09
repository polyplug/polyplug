# tests/fixtures/test_plugin.py
# Python fixture plugin implementing test.add@1 for integration tests.
# NOT auto-generated — hand-authored test fixture.
from __future__ import annotations

import ctypes
import sys
from pathlib import Path

# Add sdks/python/guest to path for this fixture
_REPO_ROOT = Path(__file__).parent.parent.parent
sys.path.insert(0, str(_REPO_ROOT / "sdks" / "python" / "guest"))

from polyplug_guest.abi import (
    ABI_OK,
    AbiError,
    PluginContext,
    PluginDescriptor,
    PluginRegistrar,
    GuestContractInterface,
    StringView,
    REGISTER_FN_TYPE,
)

# ── Contract constants ────────────────────────────────────────────────────────

# FNV-1a("test.add@1") = 0xCC4232FAB0410D2B (from tests/fixtures/test_api.toml)
_TEST_ADD_CONTRACT_ID: int = 0xCC4232FAB0410D2B

# ── ABI arg-pack struct ───────────────────────────────────────────────────────


class AddArgs(ctypes.Structure):
    _fields_ = [("a", ctypes.c_uint32), ("b", ctypes.c_uint32)]


# ── Module-level state ────────────────────────────────────────────────────────

_counter: int = 0

# ── Plugin function implementations ──────────────────────────────────────────


def _py_add(args_ptr: ctypes.c_void_p, out_ptr: ctypes.c_void_p) -> AbiError:
    args = AddArgs.from_address(args_ptr)  # type: ignore[arg-type]
    result_ptr = ctypes.cast(out_ptr, ctypes.POINTER(ctypes.c_uint32))
    result_ptr[0] = args.a + args.b
    return AbiError(code=ABI_OK)


# NOTE: add_primitive takes two separate u32 params (not a struct).
# Pack them into a ctypes struct for safe access — avoid pointer arithmetic.
class _AddPrimitiveArgs(ctypes.Structure):
    _fields_ = [("a", ctypes.c_uint32), ("b", ctypes.c_uint32)]


def _py_add_primitive(args_ptr: ctypes.c_void_p, out_ptr: ctypes.c_void_p) -> AbiError:
    args = _AddPrimitiveArgs.from_address(args_ptr)  # type: ignore[arg-type]
    result_ptr = ctypes.cast(out_ptr, ctypes.POINTER(ctypes.c_uint32))
    result_ptr[0] = args.a + args.b
    return AbiError(code=ABI_OK)


_VERSION_BYTES: bytes = b"1.0.0-python"


def _py_version(args_ptr: ctypes.c_void_p, out_ptr: ctypes.c_void_p) -> AbiError:
    sv_ptr = ctypes.cast(out_ptr, ctypes.POINTER(StringView))
    sv_ptr[0].ptr = _VERSION_BYTES
    sv_ptr[0].len = len(_VERSION_BYTES)
    return AbiError(code=ABI_OK)


def _py_reset(args_ptr: ctypes.c_void_p, out_ptr: ctypes.c_void_p) -> AbiError:
    global _counter
    _counter = 0
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


# Module-level function object cache (MUST be module-level — not per-call)
_FN_ADD = _DISPATCH_FN_TYPE(_wrap_sret(_py_add))
_FN_ADD_PRIM = _DISPATCH_FN_TYPE(_wrap_sret(_py_add_primitive))
_FN_VERSION = _DISPATCH_FN_TYPE(_wrap_sret(_py_version))
_FN_RESET = _DISPATCH_FN_TYPE(_wrap_sret(_py_reset))

_FUNCTIONS_ARRAY = (ctypes.c_void_p * 4)(
    ctypes.cast(_FN_ADD, ctypes.c_void_p),
    ctypes.cast(_FN_ADD_PRIM, ctypes.c_void_p),
    ctypes.cast(_FN_VERSION, ctypes.c_void_p),
    ctypes.cast(_FN_RESET, ctypes.c_void_p),
)

_VTABLE = GuestContractInterface(
    contract_id=_TEST_ADD_CONTRACT_ID,
    contract_version=(0 << 16) | 0,  # minor=0, patch=0
    function_count=4,
    functions=ctypes.cast(_FUNCTIONS_ARRAY, ctypes.c_void_p),
)

_PLUGIN_NAME_BYTES = b"python_test_adder"
_CONTRACT_NAME_BYTES = b"test.add"

_DESCRIPTOR = PluginDescriptor(
    name=StringView(ptr=_PLUGIN_NAME_BYTES, len=len(_PLUGIN_NAME_BYTES)),
    contract_name=StringView(ptr=_CONTRACT_NAME_BYTES, len=len(_CONTRACT_NAME_BYTES)),
    version_major=1,
    version_minor=0,
    version_patch=0,
)

# ── ABI entry points ──────────────────────────────────────────────────────────


def polyplug_abi_version() -> int:
    """Called by host to verify ABI version."""
    return 1


def polyplug_init(registrar_addr: int, ctx_ptr: int) -> None:
    """Called by PythonLoader with the PluginRegistrar address as an integer."""
    registrar = PluginRegistrar.from_address(registrar_addr)
    # Cast the register_plugin function pointer to the correct type (sret convention)
    register_fn = ctypes.cast(registrar.register_plugin, REGISTER_FN_TYPE)
    # Allocate space for the return value (AbiError struct)
    err = AbiError()
    register_fn(
        ctypes.byref(err),  # sret pointer
        ctypes.byref(registrar),
        ctypes.byref(_DESCRIPTOR),
        ctypes.byref(_VTABLE),
    )
    if err.code != ABI_OK:
        raise RuntimeError(f"register_plugin failed with code {err.code}")

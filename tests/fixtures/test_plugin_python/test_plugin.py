# tests/fixtures/test_plugin.py
# Python fixture plugin implementing test.add@1 for integration tests.
# NOT auto-generated — hand-authored test fixture.
from __future__ import annotations

import ctypes

# polyplug_guest and polyplug_abi are provisioned into the bundle's
# site-packages/ by tests/fixtures/build_all.sh; the PythonLoader prepends
# <bundle_dir>/site-packages to sys.path before importing this module.
from polyplug_guest import (
    AbiErrorCode,
    AbiError,
    PluginDescriptor,
    GuestContractInterface,
    StringView,
)
from polyplug_abi import (
    NativeDispatch,
    DispatchMechanisms,
    HostApi,
    Version,
    DispatchType,
    guest_contract_id,
)

# ── Contract constants ────────────────────────────────────────────────────────

# Canonical scheme: fnv1a_64("guest_contract:test.add@1"). Computed via the SDK
# helper rather than baked so the id stays aligned with guest_contract_id().
_TEST_ADD_CONTRACT_ID: int = guest_contract_id("test.add", 1)

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
    return AbiError(code=AbiErrorCode.Ok)


# NOTE: add_primitive takes two separate u32 params (not a struct).
# Pack them into a ctypes struct for safe access — avoid pointer arithmetic.
class _AddPrimitiveArgs(ctypes.Structure):
    _fields_ = [("a", ctypes.c_uint32), ("b", ctypes.c_uint32)]


def _py_add_primitive(args_ptr: ctypes.c_void_p, out_ptr: ctypes.c_void_p) -> AbiError:
    args = _AddPrimitiveArgs.from_address(args_ptr)  # type: ignore[arg-type]
    result_ptr = ctypes.cast(out_ptr, ctypes.POINTER(ctypes.c_uint32))
    result_ptr[0] = args.a + args.b
    return AbiError(code=AbiErrorCode.Ok)


_VERSION_BYTES: bytes = b"1.0.0-python"
_VERSION_PTR: ctypes.c_void_p = ctypes.cast(
    ctypes.c_char_p(_VERSION_BYTES), ctypes.c_void_p
)


def _py_version(args_ptr: ctypes.c_void_p, out_ptr: ctypes.c_void_p) -> AbiError:
    sv_ptr = ctypes.cast(out_ptr, ctypes.POINTER(StringView))
    sv_ptr[0].ptr = _VERSION_PTR
    sv_ptr[0].len = len(_VERSION_BYTES)
    return AbiError(code=AbiErrorCode.Ok)


def _py_reset(args_ptr: ctypes.c_void_p, out_ptr: ctypes.c_void_p) -> AbiError:
    global _counter
    _counter = 0
    return AbiError(code=AbiErrorCode.Ok)


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

_ABI_STRUCT_SIZE: int = ctypes.sizeof(AbiError)


def _wrap_sret(impl: object) -> object:
    """Wrap a two-arg impl fn with the three-arg sret calling convention."""

    def _sret_wrapper(
        sret_ptr: ctypes.c_void_p, args_ptr: ctypes.c_void_p, out_ptr: ctypes.c_void_p
    ) -> None:
        err: AbiError = impl(args_ptr, out_ptr)  # type: ignore[operator]
        ctypes.memmove(sret_ptr, ctypes.addressof(err), _ABI_STRUCT_SIZE)

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

# ── HostApi definition ───────────────────────────────────────────────────
# Use the auto-generated HostApi from polyplug_abi (imported above).

# ── Plugin interface ──────────────────────────────────────────────────

# Create native dispatch with function pointer array
_native_dispatch = NativeDispatch(
    function_count=4,
    functions=ctypes.cast(_FUNCTIONS_ARRAY, ctypes.c_void_p),
)
_dispatch = DispatchMechanisms(native=_native_dispatch)

# create_instance / destroy_instance are left at their default NULL function
# pointers (ctypes rejects an explicit None for CFUNCTYPE fields); this contract
# uses native dispatch with no instance lifecycle.
_VTABLE = GuestContractInterface(
    contract_id=_TEST_ADD_CONTRACT_ID,
    contract_version=Version(major=1, minor=0, patch=0),
    dispatch_type=DispatchType.Native,
    dispatch=_dispatch,
)

_PLUGIN_NAME_BYTES: bytes = b"python_test_adder"
_CONTRACT_NAME_BYTES: bytes = b"test.add"
_PLUGIN_NAME_PTR: ctypes.c_void_p = ctypes.cast(
    ctypes.c_char_p(_PLUGIN_NAME_BYTES), ctypes.c_void_p
)
_CONTRACT_NAME_PTR: ctypes.c_void_p = ctypes.cast(
    ctypes.c_char_p(_CONTRACT_NAME_BYTES), ctypes.c_void_p
)

_DESCRIPTOR = PluginDescriptor(
    name=StringView(ptr=_PLUGIN_NAME_PTR, len=len(_PLUGIN_NAME_BYTES)),
    contract_name=StringView(ptr=_CONTRACT_NAME_PTR, len=len(_CONTRACT_NAME_BYTES)),
    version=Version(major=1, minor=0, patch=0),
)

# ── ABI entry points ──────────────────────────────────────────────────────────


def polyplug_abi_version() -> int:
    """Called by host to verify ABI version."""
    return 1


def polyplug_init(host_addr: int, ctx_ptr: int) -> None:
    """Called by PythonLoader with the HostApi address and BundleInitContext pointer."""
    if host_addr == 0:
        raise RuntimeError("host interface pointer is null")

    # Cast the host pointer to the auto-generated HostApi structure
    host = HostApi.from_address(host_addr)

    # Call register_guest_contract via the auto-generated delegate (self-passing pattern)
    err = host.register_guest_contract(
        host_addr,  # self: HostApi pointer
        ctypes.byref(_DESCRIPTOR),
        ctypes.byref(_VTABLE),
    )
    if err.code != AbiErrorCode.Ok:
        raise RuntimeError(f"register_guest_contract failed with code {err.code}")

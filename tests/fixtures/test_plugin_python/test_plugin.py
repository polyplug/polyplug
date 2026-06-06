# tests/fixtures/test_plugin.py
# Python fixture plugin implementing test.add@1 for integration tests.
# NOT auto-generated — hand-authored test fixture.
#
# DESIGN: Python plugins are VM-dispatch plugins. This module does NOT build a
# GuestContractInterface or register native function pointers. Instead
# polyplug_init records the contract's functions via
# polyplug_guest.register_contract, which deposits a `_polyplug_registrations`
# list the PythonLoader reads. The loader wraps each function in a VM-dispatch
# trampoline and registers the contract itself.
#
# Each function is invoked by the loader as
#   fn(args_ptr: int, out_ptr: int, arena_ptr: int)
# with raw pointers passed as Python ints (arena_ptr is 0 when null). Returning
# normally signals Ok; raising signals AbiErrorCode::Generic.
from __future__ import annotations

import ctypes
import struct

# polyplug_guest is provisioned into the bundle's site-packages/ by
# tests/fixtures/build_all.sh; the PythonLoader prepends
# <bundle_dir>/site-packages to sys.path before importing this module.
from polyplug_guest import StringView, register_contract

# ── Precompiled marshalling ───────────────────────────────────────────────────
# add / add_primitive both take two u32 args packed as {a: u32, b: u32} and write
# a single u32 result. Precompiled struct.Struct + ctypes.string_at is the fast
# unmarshal path (no per-call ctypes.from_address chains).
_ADD_ARGS = struct.Struct("<II")  # a: u32, b: u32
_U32 = struct.Struct("<I")  # result: u32

# version returns a StringView. The returned bytes must outlive the call, so the
# version string is a module-level buffer with a stable address.
_VERSION_BYTES: bytes = b"1.0.0-python"
_VERSION_BUF: ctypes.Array = ctypes.create_string_buffer(_VERSION_BYTES, len(_VERSION_BYTES))
_VERSION_PTR: int = ctypes.addressof(_VERSION_BUF)
_STRING_VIEW = struct.Struct("<QQ")  # StringView: ptr (u64), len (u64)

# ── Module-level state ────────────────────────────────────────────────────────

_counter: int = 0

# ── Plugin function implementations ──────────────────────────────────────────


def _py_add(args_ptr: int, out_ptr: int, arena_ptr: int) -> None:
    """fn_id 0: add(a: u32, b: u32) -> u32."""
    a, b = _ADD_ARGS.unpack(ctypes.string_at(args_ptr, _ADD_ARGS.size))
    result: bytes = _U32.pack((a + b) & 0xFFFFFFFF)
    ctypes.memmove(out_ptr, result, _U32.size)


def _py_add_primitive(args_ptr: int, out_ptr: int, arena_ptr: int) -> None:
    """fn_id 1: add_primitive(a: u32, b: u32) -> u32 (two primitive params)."""
    a, b = _ADD_ARGS.unpack(ctypes.string_at(args_ptr, _ADD_ARGS.size))
    result: bytes = _U32.pack((a + b) & 0xFFFFFFFF)
    ctypes.memmove(out_ptr, result, _U32.size)


def _py_version(args_ptr: int, out_ptr: int, arena_ptr: int) -> None:
    """fn_id 2: version() -> StringView."""
    packed: bytes = _STRING_VIEW.pack(_VERSION_PTR, len(_VERSION_BYTES))
    ctypes.memmove(out_ptr, packed, _STRING_VIEW.size)


def _py_reset(args_ptr: int, out_ptr: int, arena_ptr: int) -> None:
    """fn_id 3: reset() -> () — clears the module counter."""
    global _counter
    _counter = 0


# ── ABI entry point ──────────────────────────────────────────────────────────


def polyplug_init(host_ptr: int, ctx_ptr: int) -> None:
    """Called by PythonLoader; records the contract's functions for collection.

    The loader derives the contract id canonically from the contract string
    ("test.add@1"); no id is baked here. Functions are ordered by fn_id.
    """
    register_contract(
        globals(),
        contract="test.add@1",
        functions=[
            _py_add,  # fn_id 0: add
            _py_add_primitive,  # fn_id 1: add_primitive
            _py_version,  # fn_id 2: version
            _py_reset,  # fn_id 3: reset
        ],
        plugin_name="python_test_adder",
    )

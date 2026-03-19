"""Lua loader registration for polyplug."""

import ctypes
from polyplug.runtime import Runtime

_lib = None


def _get_lib() -> ctypes.CDLL:
    global _lib
    if _lib is None:
        _lib = ctypes.CDLL("libpolyplug_lua.so")
        _lib.polyplug_lua_loader_create.restype = ctypes.c_void_p
        _lib.polyplug_lua_loader_create.argtypes = [ctypes.c_void_p]
    return _lib


class _LuaConfig(ctypes.Structure):
    _fields_ = [("_reserved", ctypes.c_uint8)]


def register_lua_loader(runtime: Runtime) -> None:
    """Register the Lua loader with the runtime."""
    lib = _get_lib()
    cfg = _LuaConfig(0)
    loader_ptr = lib.polyplug_lua_loader_create(ctypes.byref(cfg))
    if not loader_ptr:
        raise RuntimeError("polyplug: lua loader create failed")
    polyplug_lib = runtime._lib
    polyplug_lib.polyplug_runtime_register_loader.restype = ctypes.c_uint32
    polyplug_lib.polyplug_runtime_register_loader.argtypes = [
        ctypes.c_void_p,
        ctypes.c_void_p,
    ]
    err = polyplug_lib.polyplug_runtime_register_loader(runtime._runtime, loader_ptr)
    if err != 0:
        raise RuntimeError(f"polyplug: lua loader register failed: {err}")

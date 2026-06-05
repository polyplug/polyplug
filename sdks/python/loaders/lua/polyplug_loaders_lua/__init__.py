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


_RUNTIME_NAME: str = "lua"


def register_lua_loader(runtime: Runtime) -> None:
    """Register the Lua loader with the runtime via HostApi.register_loader."""
    lib: ctypes.CDLL = _get_lib()
    cfg = _LuaConfig(0)
    loader_ptr: int = lib.polyplug_lua_loader_create(ctypes.byref(cfg))
    if not loader_ptr:
        raise RuntimeError("polyplug: lua loader create failed")

    runtime.register_loader(_RUNTIME_NAME, loader_ptr)


__all__ = ["register_lua_loader"]

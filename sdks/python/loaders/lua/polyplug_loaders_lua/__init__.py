"""Lua loader registration for polyplug."""

import ctypes
import os
from polyplug.runtime import Runtime

_lib = None


def _get_lib() -> ctypes.CDLL:
    global _lib
    if _lib is None:
        # POLYPLUG_LUA_LIB (set by the test/CI harness) wins over the bare
        # soname so the loader cdylib matches the freshly built core.
        lib_path: str = os.environ.get("POLYPLUG_LUA_LIB", "libpolyplug_lua.so")
        _lib = ctypes.CDLL(lib_path)
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

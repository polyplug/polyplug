"""JS (QuickJS) loader registration for polyplug."""

import ctypes
from polyplug.runtime import Runtime

_lib = None


def _get_lib() -> ctypes.CDLL:
    global _lib
    if _lib is None:
        _lib = ctypes.CDLL("libpolyplug_js.so")
        _lib.polyplug_js_loader_create.restype = ctypes.c_void_p
        _lib.polyplug_js_loader_create.argtypes = [ctypes.c_void_p]
    return _lib


class _JsConfig(ctypes.Structure):
    _fields_ = [("_reserved", ctypes.c_uint8)]


def register_js_loader(runtime: Runtime) -> None:
    """Register the JS (QuickJS) loader with the runtime."""
    lib = _get_lib()
    cfg = _JsConfig(0)
    loader_ptr = lib.polyplug_js_loader_create(ctypes.byref(cfg))
    if not loader_ptr:
        raise RuntimeError("polyplug: js loader create failed")
    polyplug_lib = runtime._lib
    polyplug_lib.polyplug_runtime_register_loader.restype = ctypes.c_uint32
    polyplug_lib.polyplug_runtime_register_loader.argtypes = [
        ctypes.c_void_p,
        ctypes.c_void_p,
    ]
    err = polyplug_lib.polyplug_runtime_register_loader(runtime._runtime, loader_ptr)
    if err != 0:
        raise RuntimeError(f"polyplug: js loader register failed: {err}")

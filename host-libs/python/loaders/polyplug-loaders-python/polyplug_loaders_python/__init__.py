"""Python loader registration for polyplug."""

import ctypes
from polyplug.runtime import Runtime

_lib = None


def _get_lib() -> ctypes.CDLL:
    global _lib
    if _lib is None:
        _lib = ctypes.CDLL("libpolyplug_python.so")
        _lib.polyplug_python_loader_create.restype = ctypes.c_void_p
        _lib.polyplug_python_loader_create.argtypes = [ctypes.c_void_p]
    return _lib


class _PythonConfig(ctypes.Structure):
    _fields_ = [
        ("min_version_ptr", ctypes.c_char_p),
        ("min_version_len", ctypes.c_size_t),
    ]


def register_python_loader(runtime: Runtime, min_version: str = "3.11") -> None:
    """Register the Python loader with the runtime."""
    lib = _get_lib()
    b = min_version.encode("utf-8")
    cfg = _PythonConfig(b, len(b))
    loader_ptr = lib.polyplug_python_loader_create(ctypes.byref(cfg))
    if not loader_ptr:
        raise RuntimeError("polyplug: python loader create failed")
    polyplug_lib = runtime._lib
    polyplug_lib.polyplug_runtime_register_loader.restype = ctypes.c_uint32
    polyplug_lib.polyplug_runtime_register_loader.argtypes = [
        ctypes.c_void_p,
        ctypes.c_void_p,
    ]
    err = polyplug_lib.polyplug_runtime_register_loader(runtime._runtime, loader_ptr)
    if err != 0:
        raise RuntimeError(f"polyplug: python loader register failed: {err}")

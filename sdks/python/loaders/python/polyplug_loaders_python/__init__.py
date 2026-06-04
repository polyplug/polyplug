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


_RUNTIME_NAME: str = "python"


def register_python_loader(runtime: Runtime, min_version: str = "3.11") -> None:
    """Register the Python loader with the runtime via HostInterface.register_loader."""
    lib: ctypes.CDLL = _get_lib()
    version_bytes: bytes = min_version.encode("utf-8")
    cfg = _PythonConfig(version_bytes, len(version_bytes))
    loader_ptr: int = lib.polyplug_python_loader_create(ctypes.byref(cfg))
    if not loader_ptr:
        raise RuntimeError("polyplug: python loader create failed")

    runtime.register_loader(_RUNTIME_NAME, loader_ptr)


__all__ = ["register_python_loader"]

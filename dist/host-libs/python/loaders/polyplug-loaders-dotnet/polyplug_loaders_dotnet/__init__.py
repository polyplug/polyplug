""".NET loader registration for polyplug."""

import ctypes
from polyplug.runtime import Runtime

_lib = None


def _get_lib() -> ctypes.CDLL:
    global _lib
    if _lib is None:
        _lib = ctypes.CDLL("libpolyplug_dotnet.so")
        _lib.polyplug_dotnet_loader_create.restype = ctypes.c_void_p
        _lib.polyplug_dotnet_loader_create.argtypes = [ctypes.c_void_p]
    return _lib


class _DotnetConfig(ctypes.Structure):
    _fields_ = [
        ("min_framework_ptr", ctypes.c_char_p),
        ("min_framework_len", ctypes.c_size_t),
    ]


def register_dotnet_loader(runtime: Runtime, min_framework: str = "10.0") -> None:
    """Register the .NET loader with the runtime."""
    lib = _get_lib()
    b = min_framework.encode("utf-8")
    cfg = _DotnetConfig(b, len(b))
    loader_ptr = lib.polyplug_dotnet_loader_create(ctypes.byref(cfg))
    if not loader_ptr:
        raise RuntimeError("polyplug: dotnet loader create failed")
    polyplug_lib = runtime._lib
    polyplug_lib.polyplug_runtime_register_loader.restype = ctypes.c_uint32
    polyplug_lib.polyplug_runtime_register_loader.argtypes = [
        ctypes.c_void_p,
        ctypes.c_void_p,
    ]
    err = polyplug_lib.polyplug_runtime_register_loader(runtime._runtime, loader_ptr)
    if err != 0:
        raise RuntimeError(f"polyplug: dotnet loader register failed: {err}")

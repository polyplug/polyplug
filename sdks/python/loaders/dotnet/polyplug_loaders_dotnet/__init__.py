""".NET loader registration for polyplug."""

import ctypes
import os
from polyplug.runtime import Runtime

_lib = None


def _get_lib() -> ctypes.CDLL:
    global _lib
    if _lib is None:
        # POLYPLUG_DOTNET_LIB (set by the test/CI harness) wins over the bare
        # soname so the loader cdylib matches the freshly built core.
        lib_path: str = os.environ.get("POLYPLUG_DOTNET_LIB", "libpolyplug_dotnet.so")
        _lib = ctypes.CDLL(lib_path)
        _lib.polyplug_dotnet_loader_create.restype = ctypes.c_void_p
        _lib.polyplug_dotnet_loader_create.argtypes = [ctypes.c_void_p]
    return _lib


class _DotnetConfig(ctypes.Structure):
    _fields_ = [
        ("min_framework_ptr", ctypes.c_char_p),
        ("min_framework_len", ctypes.c_size_t),
    ]


_RUNTIME_NAME: str = "dotnet"


def register_dotnet_loader(runtime: Runtime, min_framework: str = "10.0") -> None:
    """Register the .NET loader with the runtime via HostApi.register_loader."""
    lib: ctypes.CDLL = _get_lib()
    framework_bytes: bytes = min_framework.encode("utf-8")
    cfg = _DotnetConfig(framework_bytes, len(framework_bytes))
    loader_ptr: int = lib.polyplug_dotnet_loader_create(ctypes.byref(cfg))
    if not loader_ptr:
        raise RuntimeError("polyplug: dotnet loader create failed")

    runtime.register_loader(_RUNTIME_NAME, loader_ptr)


__all__ = ["register_dotnet_loader"]

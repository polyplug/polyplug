"""Native loader registration for polyplug."""

import ctypes
import os
from polyplug.runtime import Runtime

_RUNTIME_NAME: str = "native"

_lib: ctypes.CDLL | None = None


def _get_lib() -> ctypes.CDLL:
    global _lib
    if _lib is None:
        # POLYPLUG_NATIVE_LIB (set by the test/CI harness) wins over the bare
        # soname so the loader cdylib matches the freshly built core.
        lib_path: str = os.environ.get("POLYPLUG_NATIVE_LIB", "libpolyplug_native.so")
        _lib = ctypes.CDLL(lib_path)
        _lib.polyplug_native_loader_create.restype = ctypes.c_void_p
        _lib.polyplug_native_loader_create.argtypes = [ctypes.c_void_p]
    return _lib


class _NativeConfig(ctypes.Structure):
    _fields_ = [("_reserved", ctypes.c_uint8)]


def register_native_loader(runtime: Runtime) -> None:
    """Register the native (Rust/C++) loader with the runtime.

    Creates the loader via the loader cdylib's ``polyplug_native_loader_create``
    export and registers it through the canonical ``HostApi.register_loader``
    function-pointer path.
    """
    lib: ctypes.CDLL = _get_lib()
    cfg = _NativeConfig(0)
    loader_ptr: int = lib.polyplug_native_loader_create(ctypes.byref(cfg))
    if not loader_ptr:
        raise RuntimeError("polyplug: native loader create failed")

    runtime.register_loader(loader_ptr)


__all__ = ["register_native_loader"]

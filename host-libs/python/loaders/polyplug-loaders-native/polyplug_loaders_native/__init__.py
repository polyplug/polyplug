"""Native loader registration for polyplug."""

import ctypes
from polyplug.runtime import Runtime

_lib = None


def _get_lib() -> ctypes.CDLL:
    global _lib
    if _lib is None:
        _lib = ctypes.CDLL("libpolyplug_native.so")
        _lib.polyplug_native_loader_create.restype = ctypes.c_void_p
        _lib.polyplug_native_loader_create.argtypes = [ctypes.c_void_p]
    return _lib


class _NativeConfig(ctypes.Structure):
    _fields_ = [("_reserved", ctypes.c_uint8)]


def register_native_loader(runtime: Runtime) -> None:
    """Register the native (Rust/C++) loader with the runtime."""
    lib = _get_lib()
    cfg = _NativeConfig(0)
    loader_ptr = lib.polyplug_native_loader_create(ctypes.byref(cfg))
    if not loader_ptr:
        raise RuntimeError("polyplug: native loader create failed")

    backend = runtime._backend
    # Check if we're using cffi or ctypes backend
    if hasattr(backend, "ffi"):
        # cffi backend - use ffi.cast for the call
        ffi = backend.ffi
        err = backend.lib.polyplug_runtime_register_loader(
            ffi.cast("void*", runtime._runtime), ffi.cast("void*", loader_ptr)
        )
    else:
        # ctypes backend - set up argtypes and restype
        polyplug_lib = backend.lib
        polyplug_lib.polyplug_runtime_register_loader.restype = ctypes.c_uint32
        polyplug_lib.polyplug_runtime_register_loader.argtypes = [
            ctypes.c_void_p,
            ctypes.c_void_p,
        ]
        err = polyplug_lib.polyplug_runtime_register_loader(
            runtime._runtime, loader_ptr
        )

    if err != 0:
        raise RuntimeError(f"polyplug: native loader register failed: {err}")


__all__ = ["register_native_loader"]

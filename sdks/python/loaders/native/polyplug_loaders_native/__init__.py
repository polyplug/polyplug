"""Native loader registration for polyplug."""

import ctypes
import os
import sys
import platform
from polyplug.runtime import Runtime

_RUNTIME_NAME: str = "native"

_lib: ctypes.CDLL | None = None


def _platform_id() -> str:
    machine: str = platform.machine().lower()
    if sys.platform == "linux":
        if machine in ("x86_64", "amd64"):
            return "linux-x64"
        if machine == "aarch64":
            return "linux-arm64"
    elif sys.platform == "darwin":
        if machine == "arm64":
            return "macos-arm64"
        if machine in ("x86_64", "amd64"):
            return "macos-x64"
    elif sys.platform == "win32":
        if machine in ("x86_64", "amd64"):
            return "windows-x64"
    raise RuntimeError(f"polyplug: unsupported platform {sys.platform}/{machine}")


def _lib_filename(base: str) -> str:
    if sys.platform == "darwin":
        return f"lib{base}.dylib"
    if sys.platform == "win32":
        return f"{base}.dll"
    return f"lib{base}.so"


def _resolve_lib_path(env_var: str, base: str) -> str:
    # An explicit env override (set by the test/CI harness) always wins so the
    # loader cdylib matches the freshly built tree.
    override: str = os.environ.get(env_var, "")
    if override and os.path.exists(override):
        return override
    # Native staged into the wheel under <package>/_native/<platform>/.
    embedded: str = os.path.join(
        os.path.dirname(__file__), "_native", _platform_id(), _lib_filename(base)
    )
    if os.path.exists(embedded):
        return embedded
    # Fall back to the bare soname on the system library path.
    return _lib_filename(base)


def _get_lib() -> ctypes.CDLL:
    global _lib
    if _lib is None:
        lib_path: str = _resolve_lib_path("POLYPLUG_NATIVE_LIB", "polyplug_native")
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

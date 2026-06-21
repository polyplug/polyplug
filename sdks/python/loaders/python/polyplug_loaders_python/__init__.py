"""Python loader registration for polyplug."""

import ctypes
import os
import sys
import platform
from polyplug.runtime import Runtime

_lib = None


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
        lib_path: str = _resolve_lib_path("POLYPLUG_PYTHON_LIB", "polyplug_python")
        _lib = ctypes.CDLL(lib_path)
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
    """Register the Python loader with the runtime via HostApi.register_loader."""
    lib: ctypes.CDLL = _get_lib()
    version_bytes: bytes = min_version.encode("utf-8")
    cfg = _PythonConfig(version_bytes, len(version_bytes))
    loader_ptr: int = lib.polyplug_python_loader_create(ctypes.byref(cfg))
    if not loader_ptr:
        raise RuntimeError("polyplug: python loader create failed")

    runtime.register_loader(loader_ptr)


def bridge_lib() -> ctypes.CDLL:
    """Handle to the python loader cdylib for the host-contract bridge.

    The generated host interface factories (host/interface_factories.py) need
    the ``polyplug_python_host_*`` trampolines exported by this cdylib because
    ctypes callbacks cannot return structs by value. The factories cast the
    symbols themselves; this accessor only hands out the CDLL.
    """
    return _get_lib()


__all__ = ["register_python_loader", "bridge_lib"]

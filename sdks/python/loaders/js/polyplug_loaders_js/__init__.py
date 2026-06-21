"""JS (QuickJS) loader registration for polyplug."""

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
        lib_path: str = _resolve_lib_path("POLYPLUG_JS_LIB", "polyplug_js")
        _lib = ctypes.CDLL(lib_path)
        _lib.polyplug_js_loader_create.restype = ctypes.c_void_p
        _lib.polyplug_js_loader_create.argtypes = [ctypes.c_void_p]
    return _lib


class _JsConfig(ctypes.Structure):
    _fields_ = [("_reserved", ctypes.c_uint8)]


_RUNTIME_NAME: str = "js-quickjs"


def register_js_loader(runtime: Runtime) -> None:
    """Register the JS (QuickJS) loader with the runtime via HostApi.register_loader."""
    lib: ctypes.CDLL = _get_lib()
    cfg = _JsConfig(0)
    loader_ptr: int = lib.polyplug_js_loader_create(ctypes.byref(cfg))
    if not loader_ptr:
        raise RuntimeError("polyplug: js loader create failed")

    runtime.register_loader(loader_ptr)


__all__ = ["register_js_loader"]

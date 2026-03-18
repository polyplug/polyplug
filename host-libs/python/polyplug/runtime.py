# THIS FILE IS HAND-AUTHORED (part of polyplug host-libs/python)
from __future__ import annotations

import ctypes
import ctypes.util
import os
from pathlib import Path
from typing import Any

_LIB_NAME: str = "polyplug"
_NULL_HANDLE: int = (1 << 64) - 1
_lib_bindings_initialized: bool = False


def _setup_lib_bindings(lib: ctypes.CDLL) -> None:
    global _lib_bindings_initialized
    if _lib_bindings_initialized:
        return
    _lib_bindings_initialized = True

    lib.polyplug_runtime_create.argtypes = []
    lib.polyplug_runtime_create.restype = ctypes.c_void_p

    lib.polyplug_runtime_destroy.argtypes = [ctypes.c_void_p]
    lib.polyplug_runtime_destroy.restype = None

    lib.polyplug_runtime_load_bundle.argtypes = [
        ctypes.c_void_p,
        ctypes.POINTER(ctypes.c_uint8),
        ctypes.c_size_t,
    ]
    lib.polyplug_runtime_load_bundle.restype = ctypes.c_uint32

    lib.polyplug_runtime_reload_bundle.argtypes = [
        ctypes.c_void_p,
        ctypes.POINTER(ctypes.c_uint8),
        ctypes.c_size_t,
    ]
    lib.polyplug_runtime_reload_bundle.restype = ctypes.c_uint32

    lib.polyplug_runtime_find_by_contract.argtypes = [
        ctypes.c_void_p,
        ctypes.c_uint64,
        ctypes.c_uint32,
    ]
    lib.polyplug_runtime_find_by_contract.restype = ctypes.c_uint64

    lib.polyplug_runtime_find_by_bundle.argtypes = [
        ctypes.c_void_p,
        ctypes.c_uint64,
        ctypes.c_uint64,
        ctypes.c_uint32,
    ]
    lib.polyplug_runtime_find_by_bundle.restype = ctypes.c_uint64

    lib.polyplug_runtime_find_all_by_contract.argtypes = [
        ctypes.c_void_p,
        ctypes.c_uint64,
        ctypes.c_uint32,
        ctypes.POINTER(ctypes.c_uint64),
        ctypes.c_size_t,
    ]
    lib.polyplug_runtime_find_all_by_contract.restype = ctypes.c_size_t

    lib.polyplug_runtime_resolve_plugin.argtypes = [
        ctypes.c_void_p,
        ctypes.c_uint64,
    ]
    lib.polyplug_runtime_resolve_plugin.restype = ctypes.c_void_p

    lib.polyplug_runtime_plugin_vtable.argtypes = [ctypes.c_void_p]
    lib.polyplug_runtime_plugin_vtable.restype = ctypes.c_void_p

    lib.polyplug_runtime_plugin_release.argtypes = [ctypes.c_void_p]
    lib.polyplug_runtime_plugin_release.restype = None

    lib.polyplug_runtime_last_error.argtypes = [
        ctypes.POINTER(ctypes.c_uint8),
        ctypes.c_size_t,
    ]
    lib.polyplug_runtime_last_error.restype = ctypes.c_size_t

    lib.polyplug_runtime_error_message_len.argtypes = []
    lib.polyplug_runtime_error_message_len.restype = ctypes.c_size_t


class StringView(ctypes.Structure):
    _fields_ = [
        ("ptr", ctypes.POINTER(ctypes.c_uint8)),
        ("len", ctypes.c_size_t),
    ]


class PluginHandle(ctypes.Structure):
    _fields_ = [
        ("index", ctypes.c_uint32),
        ("generation", ctypes.c_uint32),
    ]


class PluginGuard:
    def __init__(self, lib: ctypes.CDLL, guard_ptr: ctypes.c_void_p) -> None:
        self._lib: ctypes.CDLL = lib
        self._guard: ctypes.c_void_p = guard_ptr
        # Cache vtable pointer at construction to avoid repeated FFI calls
        if guard_ptr is None or guard_ptr == 0:
            raise RuntimeError("PluginGuard is null")
        vtable_ptr: ctypes.c_void_p = lib.polyplug_runtime_plugin_vtable(guard_ptr)
        if vtable_ptr is None or vtable_ptr == 0:
            msg: str = _last_error(lib)
            raise RuntimeError(msg or "polyplug_runtime_plugin_vtable failed")
        self._vtable: ctypes.c_void_p = vtable_ptr

    def __del__(self) -> None:
        guard: ctypes.c_void_p = getattr(self, "_guard", None)
        lib: ctypes.CDLL = getattr(self, "_lib", None)
        if guard is not None and lib is not None and guard != 0:
            lib.polyplug_runtime_plugin_release(guard)
            self._guard = ctypes.c_void_p()

    @property
    def vtable(self) -> ctypes.c_void_p:
        """Return cached vtable pointer (no FFI call)."""
        return self._vtable

    def get_vtable(self) -> ctypes.c_void_p:
        """Deprecated: use guard.vtable property instead. Returns cached vtable."""
        return self._vtable


def _resolve_lib_path() -> str:
    env_path: str | None = os.getenv("POLYPLUG_LIB")
    if env_path:
        return env_path
    found: str | None = ctypes.util.find_library(_LIB_NAME)
    if found is None:
        return "libpolyplug.so"
    return found


def _last_error(lib: ctypes.CDLL) -> str:
    msg_len: int = int(lib.polyplug_runtime_error_message_len())
    if msg_len == 0:
        return ""
    buf: ctypes.Array[ctypes.c_uint8] = (ctypes.c_uint8 * msg_len)()
    written: int = int(lib.polyplug_runtime_last_error(buf, msg_len))
    if written <= 0:
        return ""
    data: bytes = bytes(buf[:written])
    return data.decode("utf-8", errors="replace")


def _check_error_code(lib: ctypes.CDLL, code: int, context: str) -> None:
    if code == 0:
        return
    msg: str = _last_error(lib)
    if msg:
        raise RuntimeError(msg)
    raise RuntimeError(f"{context} failed with code {code}")


class Runtime:
    def __init__(self) -> None:
        lib_path: str = os.environ.get("POLYPLUG_LIB_PATH") or _resolve_lib_path()
        self._lib: ctypes.CDLL = ctypes.CDLL(lib_path)
        _setup_lib_bindings(self._lib)
        rt_ptr: ctypes.c_void_p = ctypes.c_void_p(self._lib.polyplug_runtime_create())
        if rt_ptr.value is None:
            msg: str = _last_error(self._lib)
            raise RuntimeError(msg or "polyplug_runtime_create failed")
        self._runtime: ctypes.c_void_p = rt_ptr

    def __del__(self) -> None:
        rt_ptr: ctypes.c_void_p = getattr(self, "_runtime", None)
        lib: ctypes.CDLL = getattr(self, "_lib", None)
        if rt_ptr is not None and lib is not None and rt_ptr.value is not None:
            lib.polyplug_runtime_destroy(rt_ptr)
            self._runtime = ctypes.c_void_p()

    def _ensure_runtime(self) -> ctypes.c_void_p:
        if self._runtime.value is None:
            raise RuntimeError("Runtime is closed")
        return self._runtime

    def load_bundle(self, path: str | Path) -> None:
        runtime_ptr: ctypes.c_void_p = self._ensure_runtime()
        path_bytes: bytes = str(Path(path)).encode("utf-8")
        buf: ctypes.Array[ctypes.c_uint8] = (ctypes.c_uint8 * len(path_bytes))(
            *path_bytes
        )
        code: int = int(
            self._lib.polyplug_runtime_load_bundle(
                runtime_ptr, buf, ctypes.c_size_t(len(path_bytes))
            )
        )
        _check_error_code(self._lib, code, "polyplug_runtime_load_bundle")

    def reload_bundle(self, path: str | Path) -> None:
        runtime_ptr: ctypes.c_void_p = self._ensure_runtime()
        path_bytes: bytes = str(Path(path)).encode("utf-8")
        buf: ctypes.Array[ctypes.c_uint8] = (ctypes.c_uint8 * len(path_bytes))(
            *path_bytes
        )
        code: int = int(
            self._lib.polyplug_runtime_reload_bundle(
                runtime_ptr, buf, ctypes.c_size_t(len(path_bytes))
            )
        )
        _check_error_code(self._lib, code, "polyplug_runtime_reload_bundle")

    def find_by_contract(self, contract_id: int, min_version: int) -> int:
        runtime_ptr: ctypes.c_void_p = self._ensure_runtime()
        packed: int = int(
            self._lib.polyplug_runtime_find_by_contract(
                runtime_ptr,
                ctypes.c_uint64(contract_id),
                ctypes.c_uint32(min_version),
            )
        )
        return packed

    def find_by_bundle(self, bundle_id: int, contract_id: int, min_version: int) -> int:
        runtime_ptr: ctypes.c_void_p = self._ensure_runtime()
        packed: int = int(
            self._lib.polyplug_runtime_find_by_bundle(
                runtime_ptr,
                ctypes.c_uint64(bundle_id),
                ctypes.c_uint64(contract_id),
                ctypes.c_uint32(min_version),
            )
        )
        return packed

    def find_all_by_contract(self, contract_id: int, min_version: int) -> list[int]:
        runtime_ptr: ctypes.c_void_p = self._ensure_runtime()
        cap: int = 16
        while True:
            out: ctypes.Array[ctypes.c_uint64] = (ctypes.c_uint64 * cap)()
            count: int = int(
                self._lib.polyplug_runtime_find_all_by_contract(
                    runtime_ptr,
                    ctypes.c_uint64(contract_id),
                    ctypes.c_uint32(min_version),
                    out,
                    ctypes.c_size_t(cap),
                )
            )
            if count < cap:
                return [int(out[i]) for i in range(count)]
            cap = cap * 2

    def resolve_plugin(self, packed_handle: int) -> PluginGuard:
        if packed_handle == _NULL_HANDLE:
            raise RuntimeError("null plugin handle")
        runtime_ptr: ctypes.c_void_p = self._ensure_runtime()
        guard_ptr: ctypes.c_void_p = self._lib.polyplug_runtime_resolve_plugin(
            runtime_ptr, ctypes.c_uint64(packed_handle)
        )
        if guard_ptr is None or guard_ptr == 0:
            msg: str = _last_error(self._lib)
            raise RuntimeError(msg or "polyplug_runtime_resolve_plugin failed")
        return PluginGuard(self._lib, guard_ptr)

    def get_extension(self, extension_id: int) -> None:
        _ = extension_id
        return None

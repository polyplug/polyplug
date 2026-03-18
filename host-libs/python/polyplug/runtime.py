# THIS FILE IS HAND-AUTHORED (part of polyplug host-libs/python)
"""
polyplug Python Host Library

Supports two FFI backends:
- cffi ABI mode (faster, ~380ns/call) - used if cffi is installed
- ctypes (slower, ~670ns/call) - fallback, always available

Install cffi for better performance: pip install cffi
"""

from __future__ import annotations

import os
from pathlib import Path
from typing import Any, Protocol, runtime_checkable

_LIB_NAME: str = "polyplug"
_NULL_HANDLE: int = (1 << 64) - 1

# Backend detection
_BACKEND: str = "ctypes"
_cffi_available: bool = False

try:
    import cffi

    _cffi_available = True
    _BACKEND = "cffi"
except ImportError:
    pass


# ============================================================================
# Backend Protocol (common interface for both ctypes and cffi)
# ============================================================================


@runtime_checkable
class Backend(Protocol):
    """Protocol defining the common interface for FFI backends."""

    def create_runtime(self) -> int: ...
    def destroy_runtime(self, rt: int) -> None: ...
    def load_bundle(self, rt: int, path: bytes) -> int: ...
    def reload_bundle(self, rt: int, path: bytes) -> int: ...
    def find_by_contract(self, rt: int, contract_id: int, min_version: int) -> int: ...
    def find_by_bundle(
        self, rt: int, bundle_id: int, contract_id: int, min_version: int
    ) -> int: ...
    def find_all_by_contract(
        self, rt: int, contract_id: int, min_version: int, out: Any, cap: int
    ) -> int: ...
    def resolve_plugin(self, rt: int, handle: int) -> int: ...
    def last_error(self, rt: int, buf: Any, buf_len: int) -> int: ...
    def error_message_len(self) -> int: ...


# ============================================================================
# ctypes Backend
# ============================================================================


class CTypesBackend:
    """ctypes-based FFI backend (always available, ~670ns/call)."""

    def __init__(self, lib_path: str) -> None:
        import ctypes
        import ctypes.util

        self.ctypes = ctypes
        self.lib: ctypes.CDLL = ctypes.CDLL(lib_path)
        self._setup_bindings()

    def _setup_bindings(self) -> None:
        self.lib.polyplug_runtime_create.argtypes = []
        self.lib.polyplug_runtime_create.restype = self.ctypes.c_void_p

        self.lib.polyplug_runtime_destroy.argtypes = [self.ctypes.c_void_p]
        self.lib.polyplug_runtime_destroy.restype = None

        self.lib.polyplug_runtime_load_bundle.argtypes = [
            self.ctypes.c_void_p,
            self.ctypes.POINTER(self.ctypes.c_uint8),
            self.ctypes.c_size_t,
        ]
        self.lib.polyplug_runtime_load_bundle.restype = self.ctypes.c_uint32

        self.lib.polyplug_runtime_reload_bundle.argtypes = [
            self.ctypes.c_void_p,
            self.ctypes.POINTER(self.ctypes.c_uint8),
            self.ctypes.c_size_t,
        ]
        self.lib.polyplug_runtime_reload_bundle.restype = self.ctypes.c_uint32

        self.lib.polyplug_runtime_find_by_contract.argtypes = [
            self.ctypes.c_void_p,
            self.ctypes.c_uint64,
            self.ctypes.c_uint32,
        ]
        self.lib.polyplug_runtime_find_by_contract.restype = self.ctypes.c_uint64

        self.lib.polyplug_runtime_find_by_bundle.argtypes = [
            self.ctypes.c_void_p,
            self.ctypes.c_uint64,
            self.ctypes.c_uint64,
            self.ctypes.c_uint32,
        ]
        self.lib.polyplug_runtime_find_by_bundle.restype = self.ctypes.c_uint64

        self.lib.polyplug_runtime_find_all_by_contract.argtypes = [
            self.ctypes.c_void_p,
            self.ctypes.c_uint64,
            self.ctypes.c_uint32,
            self.ctypes.POINTER(self.ctypes.c_uint64),
            self.ctypes.c_size_t,
        ]
        self.lib.polyplug_runtime_find_all_by_contract.restype = self.ctypes.c_size_t

        self.lib.polyplug_runtime_resolve_plugin.argtypes = [
            self.ctypes.c_void_p,
            self.ctypes.c_uint64,
        ]
        self.lib.polyplug_runtime_resolve_plugin.restype = self.ctypes.c_void_p

        self.lib.polyplug_runtime_last_error.argtypes = [
            self.ctypes.POINTER(self.ctypes.c_uint8),
            self.ctypes.c_size_t,
        ]
        self.lib.polyplug_runtime_last_error.restype = self.ctypes.c_size_t

        self.lib.polyplug_runtime_error_message_len.argtypes = []
        self.lib.polyplug_runtime_error_message_len.restype = self.ctypes.c_size_t

    def create_runtime(self) -> int:
        return self.lib.polyplug_runtime_create() or 0

    def destroy_runtime(self, rt: int) -> None:
        self.lib.polyplug_runtime_destroy(rt)

    def load_bundle(self, rt: int, path: bytes) -> int:
        buf = (self.ctypes.c_uint8 * len(path))(*path)
        return self.lib.polyplug_runtime_load_bundle(rt, buf, len(path))

    def reload_bundle(self, rt: int, path: bytes) -> int:
        buf = (self.ctypes.c_uint8 * len(path))(*path)
        return self.lib.polyplug_runtime_reload_bundle(rt, buf, len(path))

    def find_by_contract(self, rt: int, contract_id: int, min_version: int) -> int:
        return self.lib.polyplug_runtime_find_by_contract(
            rt, self.ctypes.c_uint64(contract_id), self.ctypes.c_uint32(min_version)
        )

    def find_by_bundle(
        self, rt: int, bundle_id: int, contract_id: int, min_version: int
    ) -> int:
        return self.lib.polyplug_runtime_find_by_bundle(
            rt,
            self.ctypes.c_uint64(bundle_id),
            self.ctypes.c_uint64(contract_id),
            self.ctypes.c_uint32(min_version),
        )

    def find_all_by_contract(
        self, rt: int, contract_id: int, min_version: int, out: Any, cap: int
    ) -> int:
        return self.lib.polyplug_runtime_find_all_by_contract(
            rt,
            self.ctypes.c_uint64(contract_id),
            self.ctypes.c_uint32(min_version),
            out,
            self.ctypes.c_size_t(cap),
        )

    def resolve_plugin(self, rt: int, handle: int) -> int:
        return (
            self.lib.polyplug_runtime_resolve_plugin(rt, self.ctypes.c_uint64(handle))
            or 0
        )

    def last_error(self, rt: int, buf: Any, buf_len: int) -> int:
        return self.lib.polyplug_runtime_last_error(buf, buf_len)

    def error_message_len(self) -> int:
        return self.lib.polyplug_runtime_error_message_len()

    def create_uint64_array(self, cap: int) -> Any:
        return (self.ctypes.c_uint64 * cap)()


# ============================================================================
# cffi Backend
# ============================================================================


class CFFIBackend:
    """cffi ABI mode backend (~380ns/call, 1.7x faster than ctypes)."""

    CDEF = """
        void* polyplug_runtime_create(void);
        void polyplug_runtime_destroy(void* rt);
        uint32_t polyplug_runtime_load_bundle(void* rt, const uint8_t* path, size_t path_len);
        uint32_t polyplug_runtime_reload_bundle(void* rt, const uint8_t* path, size_t path_len);
        uint64_t polyplug_runtime_find_by_contract(void* rt, uint64_t contract_id, uint32_t min_version);
        uint64_t polyplug_runtime_find_by_bundle(void* rt, uint64_t bundle_id, uint64_t contract_id, uint32_t min_version);
        size_t polyplug_runtime_find_all_by_contract(void* rt, uint64_t contract_id, uint32_t min_version, uint64_t* out, size_t out_cap);
        void* polyplug_runtime_resolve_plugin(void* rt, uint64_t packed_handle);
        size_t polyplug_runtime_last_error(uint8_t* buf, size_t buf_len);
        size_t polyplug_runtime_error_message_len(void);
    """

    def __init__(self, lib_path: str) -> None:
        import cffi

        self.ffi = cffi.FFI()
        self.ffi.cdef(self.CDEF)
        self.lib = self.ffi.dlopen(lib_path)

    def create_runtime(self) -> int:
        return self.ffi.cast("uintptr_t", self.lib.polyplug_runtime_create())

    def destroy_runtime(self, rt: int) -> None:
        self.lib.polyplug_runtime_destroy(self.ffi.cast("void*", rt))

    def load_bundle(self, rt: int, path: bytes) -> int:
        cpath = self.ffi.new("uint8_t[]", path)
        return self.lib.polyplug_runtime_load_bundle(
            self.ffi.cast("void*", rt), cpath, len(path)
        )

    def reload_bundle(self, rt: int, path: bytes) -> int:
        cpath = self.ffi.new("uint8_t[]", path)
        return self.lib.polyplug_runtime_reload_bundle(
            self.ffi.cast("void*", rt), cpath, len(path)
        )

    def find_by_contract(self, rt: int, contract_id: int, min_version: int) -> int:
        return self.lib.polyplug_runtime_find_by_contract(
            self.ffi.cast("void*", rt), contract_id, min_version
        )

    def find_by_bundle(
        self, rt: int, bundle_id: int, contract_id: int, min_version: int
    ) -> int:
        return self.lib.polyplug_runtime_find_by_bundle(
            self.ffi.cast("void*", rt), bundle_id, contract_id, min_version
        )

    def find_all_by_contract(
        self, rt: int, contract_id: int, min_version: int, out: Any, cap: int
    ) -> int:
        return self.lib.polyplug_runtime_find_all_by_contract(
            self.ffi.cast("void*", rt), contract_id, min_version, out, cap
        )

    def resolve_plugin(self, rt: int, handle: int) -> int:
        return self.ffi.cast(
            "uintptr_t",
            self.lib.polyplug_runtime_resolve_plugin(
                self.ffi.cast("void*", rt), handle
            ),
        )

    def last_error(self, rt: int, buf: Any, buf_len: int) -> int:
        return self.lib.polyplug_runtime_last_error(buf, buf_len)

    def error_message_len(self) -> int:
        return self.lib.polyplug_runtime_error_message_len()

    def create_uint64_array(self, cap: int) -> Any:
        return self.ffi.new("uint64_t[]", cap)


# ============================================================================
# Public API
# ============================================================================


def get_backend() -> str:
    """Return the current FFI backend name ('cffi' or 'ctypes')."""
    return _BACKEND


def _resolve_lib_path() -> str:
    env_path: str | None = os.getenv("POLYPLUG_LIB")
    if env_path:
        return env_path

    import ctypes.util

    found: str | None = ctypes.util.find_library(_LIB_NAME)
    if found is None:
        return "libpolyplug.so"
    return found


def _create_backend(lib_path: str) -> Backend:
    """Create the appropriate backend based on availability."""
    if _cffi_available:
        return CFFIBackend(lib_path)
    return CTypesBackend(lib_path)


class PluginGuard:
    """Guard for a resolved plugin with cached vtable pointer."""

    def __init__(self, vtable_ptr: int) -> None:
        if vtable_ptr == 0:
            raise RuntimeError("PluginGuard vtable is null")
        self._vtable: int = vtable_ptr

    @property
    def vtable(self) -> int:
        """Return cached vtable pointer (no FFI call)."""
        return self._vtable

    def get_vtable(self) -> int:
        """Deprecated: use guard.vtable property instead."""
        return self._vtable


def _last_error(backend: Backend) -> str:
    msg_len: int = backend.error_message_len()
    if msg_len == 0:
        return ""

    if hasattr(backend, "ffi"):
        # cffi backend
        buf = backend.ffi.new("uint8_t[]", msg_len)
        written: int = backend.last_error(0, buf, msg_len)
        if written <= 0:
            return ""
        return backend.ffi.buffer(buf, written)[:].decode("utf-8", errors="replace")
    else:
        # ctypes backend
        buf = backend.create_uint64_array((msg_len + 7) // 8)
        written: int = backend.last_error(0, buf, msg_len)
        if written <= 0:
            return ""
        return bytes(buf[:written]).decode("utf-8", errors="replace")


def _check_error_code(backend: Backend, code: int, context: str) -> None:
    if code == 0:
        return
    msg: str = _last_error(backend)
    if msg:
        raise RuntimeError(msg)
    raise RuntimeError(f"{context} failed with code {code}")


class Runtime:
    """polyplug runtime for loading and managing plugins."""

    def __init__(self) -> None:
        lib_path: str = os.environ.get("POLYPLUG_LIB_PATH") or _resolve_lib_path()
        self._backend: Backend = _create_backend(lib_path)

        rt_ptr: int = self._backend.create_runtime()
        if rt_ptr == 0:
            msg: str = _last_error(self._backend)
            raise RuntimeError(msg or "polyplug_runtime_create failed")
        self._runtime: int = rt_ptr

    def __del__(self) -> None:
        rt_ptr: int = getattr(self, "_runtime", 0)
        backend: Backend = getattr(self, "_backend", None)
        if rt_ptr != 0 and backend is not None:
            backend.destroy_runtime(rt_ptr)
            self._runtime = 0

    def _ensure_runtime(self) -> int:
        if self._runtime == 0:
            raise RuntimeError("Runtime is closed")
        return self._runtime

    def load_bundle(self, path: str | Path) -> None:
        runtime_ptr: int = self._ensure_runtime()
        path_bytes: bytes = str(Path(path)).encode("utf-8")
        code: int = self._backend.load_bundle(runtime_ptr, path_bytes)
        _check_error_code(self._backend, code, "polyplug_runtime_load_bundle")

    def reload_bundle(self, path: str | Path) -> None:
        runtime_ptr: int = self._ensure_runtime()
        path_bytes: bytes = str(Path(path)).encode("utf-8")
        code: int = self._backend.reload_bundle(runtime_ptr, path_bytes)
        _check_error_code(self._backend, code, "polyplug_runtime_reload_bundle")

    def find_by_contract(self, contract_id: int, min_version: int) -> int:
        runtime_ptr: int = self._ensure_runtime()
        return self._backend.find_by_contract(runtime_ptr, contract_id, min_version)

    def find_by_bundle(self, bundle_id: int, contract_id: int, min_version: int) -> int:
        runtime_ptr: int = self._ensure_runtime()
        return self._backend.find_by_bundle(
            runtime_ptr, bundle_id, contract_id, min_version
        )

    def find_all_by_contract(self, contract_id: int, min_version: int) -> list[int]:
        runtime_ptr: int = self._ensure_runtime()
        cap: int = 16
        while True:
            out = self._backend.create_uint64_array(cap)
            count: int = self._backend.find_all_by_contract(
                runtime_ptr, contract_id, min_version, out, cap
            )
            if count < cap:
                if hasattr(self._backend, "ffi"):
                    # cffi
                    return [out[i] for i in range(count)]
                else:
                    # ctypes
                    return [int(out[i]) for i in range(count)]
            cap = cap * 2

    def resolve_plugin(self, packed_handle: int) -> PluginGuard:
        if packed_handle == _NULL_HANDLE:
            raise RuntimeError("null plugin handle")
        runtime_ptr: int = self._ensure_runtime()
        vtable_ptr: int = self._backend.resolve_plugin(runtime_ptr, packed_handle)
        if vtable_ptr == 0:
            msg: str = _last_error(self._backend)
            raise RuntimeError(msg or "polyplug_runtime_resolve_plugin failed")
        return PluginGuard(vtable_ptr)

    def get_extension(self, extension_id: int) -> None:
        _ = extension_id
        return None

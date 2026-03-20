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
from typing import Any, Callable, Optional, Protocol, runtime_checkable

from polyplug.abi import ReloadPhase, ReloadPhaseCStruct, ReloadPhaseType

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
        uint32_t polyplug_runtime_register_loader(void* rt, void* loader_ptr);
        uint32_t polyplug_runtime_set_config(void* config);
        uint32_t polyplug_runtime_on_reload(void* callback);
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
        result = self.lib.polyplug_runtime_resolve_plugin(
            self.ffi.cast("void*", rt), handle
        )
        return int(self.ffi.cast("uintptr_t", result))

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
    """Guard for a resolved plugin handle.

    Stores runtime + handle for hot-reload safety.
    Re-resolves vtable on each call to detect stale handles.
    """

    def __init__(self, backend: Backend, runtime_ptr: int, handle: int) -> None:
        self._backend: Backend = backend
        self._runtime: int = runtime_ptr
        self._handle: int = handle

    @property
    def vtable(self) -> int:
        """Re-resolve vtable on each call (hot-reload safe)."""
        if self._runtime == 0 or self._handle == _NULL_HANDLE:
            return 0
        return self._backend.resolve_plugin(self._runtime, self._handle)

    @property
    def handle(self) -> int:
        """Return the stored handle."""
        return self._handle

    def get_vtable(self) -> int:
        """Deprecated: use guard.vtable property instead."""
        return self.vtable

    def is_null(self) -> bool:
        """Return True if this guard is null."""
        return self._runtime == 0 or self._handle == _NULL_HANDLE


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

    _on_reload_cb: Optional[Callable[[ReloadPhase], None]] = None

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

    @classmethod
    def on_reload(cls, callback: Callable[[ReloadPhase], None]) -> None:
        """Register a callback for hot-reload notifications.

        The callback is invoked at each phase of a hot-reload:
        - PREPARING: Before vtable swap, includes retry count
        - RELOADED: After successful vtable swap
        - FAILED: When reload fails, includes reason string

        Must be called before creating a Runtime instance.

        Args:
            callback: Function that receives ReloadPhase objects.
        """
        cls._on_reload_cb = callback
        cls._register_reload_callback()

    @classmethod
    def set_config(cls, config: "RuntimeConfig") -> None:
        """Set runtime configuration for subsequently created runtimes.

        Must be called before creating a Runtime instance.

        Args:
            config: RuntimeConfig with hot-reload settings.
        """
        cls._apply_config(config)

    @classmethod
    def _register_reload_callback(cls) -> None:
        """Internal: Register the FFI callback with the library."""
        if not hasattr(cls, "_c_callback"):
            cls._c_callback = cls._make_c_callback()
        if hasattr(cls, "_backend_instance"):
            lib = cls._backend_instance.lib
        else:
            import ctypes
            import ctypes.util

            lib_path: str = os.environ.get("POLYPLUG_LIB_PATH") or _resolve_lib_path()
            lib = ctypes.CDLL(lib_path)
            lib.polyplug_runtime_on_reload.argtypes = [
                ctypes.CFUNCTYPE(None, ReloadPhaseCStruct)
            ]
            lib.polyplug_runtime_on_reload.restype = ctypes.c_uint32
        lib.polyplug_runtime_on_reload(cls._c_callback)

    @classmethod
    def _make_c_callback(cls) -> "ctypes.CFUNCTYPE":
        """Internal: Create a C-compatible callback wrapper."""
        import ctypes

        @ctypes.CFUNCTYPE(None, ReloadPhaseCStruct)
        def c_callback(c_phase: ReloadPhaseCStruct) -> None:
            if cls._on_reload_cb is not None:
                phase: ReloadPhase = ReloadPhase.from_c_struct(c_phase)
                cls._on_reload_cb(phase)

        return c_callback

    @classmethod
    def _apply_config(cls, config: "RuntimeConfig") -> None:
        """Internal: Apply configuration to the FFI layer."""
        import ctypes
        import ctypes.util

        class RuntimeConfigC(ctypes.Structure):
            _fields_ = [
                ("hot_reload_max_retries", ctypes.c_uint32),
                ("hot_reload_retry_interval_ms", ctypes.c_uint64),
                ("hot_reload_abort_on_max_retries", ctypes.c_uint8),
            ]

        config_c = RuntimeConfigC(
            hot_reload_max_retries=config.hot_reload_max_retries,
            hot_reload_retry_interval_ms=config.hot_reload_retry_interval_ms,
            hot_reload_abort_on_max_retries=1
            if config.hot_reload_abort_on_max_retries
            else 0,
        )

        if hasattr(cls, "_backend_instance"):
            lib = cls._backend_instance.lib
        else:
            lib_path: str = os.environ.get("POLYPLUG_LIB_PATH") or _resolve_lib_path()
            lib = ctypes.CDLL(lib_path)
            lib.polyplug_runtime_set_config.argtypes = [ctypes.POINTER(RuntimeConfigC)]
            lib.polyplug_runtime_set_config.restype = ctypes.c_uint32

        lib.polyplug_runtime_set_config(ctypes.byref(config_c))

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
        # Don't resolve vtable here - let PluginGuard do it on each call
        # This ensures hot-reload safety (stale handle detection)
        return PluginGuard(self._backend, runtime_ptr, packed_handle)

    def get_extension(self, extension_id: int) -> None:
        _ = extension_id
        return None

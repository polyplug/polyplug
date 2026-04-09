"""
polyplug Python Host Library

Supports two FFI backends:
- cffi ABI mode (faster, ~380ns/call) - used if cffi is installed
- ctypes (slower, ~670ns/call) - fallback, always available

Install cffi for better performance: pip install cffi
"""

from __future__ import annotations

import ctypes
import os
from pathlib import Path
from typing import Any, Callable, Optional, Protocol, runtime_checkable

from polyplug_abi import ReloadPhase, ReloadPhaseType, StringView

_LIB_NAME: str = "polyplug"

# ─── Compatibility Constants ─────────────────────────────────────────────────────
# These match polyplug_abi::Compatibility #[repr(u32)] enum

COMPATIBILITY_STRICT: int = 0   # Exact major match and minor >= required
COMPATIBILITY_RELAXED: int = 1  # Same major, any minor
COMPATIBILITY_YOLO: int = 2     # Any version accepted


# ─── RuntimeConfig Structure ─────────────────────────────────────────────────────
# FFI RuntimeConfig matching polyplug_abi::RuntimeConfig (24 bytes)
# Layout verified in polyplug_abi/tests: offset_of checks


class RuntimeConfig(ctypes.Structure):
    """FFI RuntimeConfig matching polyplug_abi::RuntimeConfig (24 bytes).

    Layout:
        offset 0: hot_reload_enabled (1 byte, c_uint8)
        offset 1-3: padding (3 bytes)
        offset 4: hot_reload_max_retries (4 bytes, c_uint32)
        offset 8: hot_reload_retry_interval_ms (8 bytes, c_uint64)
        offset 16: hot_reload_abort_on_max_retries (1 byte, c_uint8)
        offset 17-19: padding (3 bytes)
        offset 20: compatibility (4 bytes, c_uint32)
    """

    _fields_ = [
        ("hot_reload_enabled", ctypes.c_uint8),           # offset 0, 1 byte
        ("_pad1", ctypes.c_uint8 * 3),                    # padding 3 bytes
        ("hot_reload_max_retries", ctypes.c_uint32),      # offset 4, 4 bytes
        ("hot_reload_retry_interval_ms", ctypes.c_uint64), # offset 8, 8 bytes
        ("hot_reload_abort_on_max_retries", ctypes.c_uint8), # offset 16, 1 byte
        ("_pad2", ctypes.c_uint8 * 3),                    # padding 3 bytes
        ("compatibility", ctypes.c_uint32),               # offset 20, 4 bytes
    ]


class RuntimeCreateOptionsC(ctypes.Structure):
    """FFI RuntimeCreateOptions matching polyplug_abi::RuntimeCreateOptions."""

    _fields_ = [
        ("config", ctypes.POINTER(RuntimeConfig)),
        ("on_reload", ctypes.c_void_p),
    ]


# ─── Host Contract Interface Structures ─────────────────────────────────────────────
# These structures match the Rust ABI exactly for VM-based host contract registration.


class HostContractInterfaceHeader(ctypes.Structure):
    """Host contract interface header — metadata for a host-provided contract."""

    _fields_ = [
        ("interface_version", ctypes.c_uint32),
        ("contract_id", ctypes.c_uint64),
        ("contract_major", ctypes.c_uint32),
        ("contract_minor", ctypes.c_uint32),
        ("function_count", ctypes.c_uint32),
        ("dispatch_type", ctypes.c_uint32),
    ]


class VmHostContractDispatch(ctypes.Structure):
    """VM dispatch for host contracts — call through a dispatch function."""

    _fields_ = [
        ("call", ctypes.c_void_p),
        ("bridge_data", ctypes.c_void_p),
    ]


class HostContractDispatch(ctypes.Union):
    """Union of host contract dispatch mechanisms."""

    _fields_ = [
        ("vm", VmHostContractDispatch),
    ]


class HostContractInterface(ctypes.Structure):
    """Host contract interface — complete interface for a host-provided contract."""

    _fields_ = [
        ("header", HostContractInterfaceHeader),
        ("dispatch", HostContractDispatch),
    ]


# DispatchType enum values for host contracts
DISPATCH_TYPE_VIRTUAL_MACHINE: int = 1
_NULL_HANDLE: int = (1 << 64) - 1

_BACKEND: str = "ctypes"
_cffi_available: bool = False

try:
    import cffi

    _cffi_available = True
    _BACKEND = "cffi"
except ImportError:
    pass


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
    def release_plugin(self, handle: int) -> None: ...
    def last_error(self, rt: int, buf: Any, buf_len: int) -> int: ...
    def error_message_len(self) -> int: ...
    def register_host_contract(self, rt: int, interface_ptr: int) -> int: ...


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

        self.lib.polyplug_runtime_release_plugin.argtypes = [
            self.ctypes.c_void_p,
        ]
        self.lib.polyplug_runtime_release_plugin.restype = None

        self.lib.polyplug_runtime_last_error.argtypes = [
            self.ctypes.POINTER(self.ctypes.c_uint8),
            self.ctypes.c_size_t,
        ]
        self.lib.polyplug_runtime_last_error.restype = self.ctypes.c_size_t

        self.lib.polyplug_runtime_error_message_len.argtypes = []
        self.lib.polyplug_runtime_error_message_len.restype = self.ctypes.c_size_t

        self.lib.polyplug_runtime_register_host_contract.argtypes = [
            self.ctypes.c_void_p,
            self.ctypes.c_void_p,
        ]
        self.lib.polyplug_runtime_register_host_contract.restype = self.ctypes.c_uint32

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

    def release_plugin(self, handle: int) -> None:
        if handle != 0:
            self.lib.polyplug_runtime_release_plugin(handle)

    def last_error(self, rt: int, buf: Any, buf_len: int) -> int:
        return self.lib.polyplug_runtime_last_error(buf, buf_len)

    def error_message_len(self) -> int:
        return self.lib.polyplug_runtime_error_message_len()

    def register_host_contract(self, rt: int, interface_ptr: int) -> int:
        return self.lib.polyplug_runtime_register_host_contract(rt, interface_ptr)

    def create_uint64_array(self, cap: int) -> Any:
        return (self.ctypes.c_uint64 * cap)()


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
        void polyplug_runtime_release_plugin(void* handle);
        size_t polyplug_runtime_last_error(uint8_t* buf, size_t buf_len);
        size_t polyplug_runtime_error_message_len(void);
        uint32_t polyplug_runtime_register_loader(void* rt, void* loader_ptr);
        uint32_t polyplug_runtime_register_host_contract(void* rt, const void* vtable);

        typedef struct {
            uint8_t hot_reload_enabled;
            uint8_t _pad1[3];
            uint32_t hot_reload_max_retries;
            uint64_t hot_reload_retry_interval_ms;
            uint8_t hot_reload_abort_on_max_retries;
            uint8_t _pad2[3];
            uint32_t compatibility;
        } RuntimeConfig;

        typedef void (*ReloadPhaseCallback)(
            uint32_t phase_type,
            uint64_t bundle_id,
            const uint8_t* bundle_name,
            size_t bundle_name_len,
            uint32_t retry_count,
            const uint8_t* reason,
            size_t reason_len
        );

        typedef struct {
            const RuntimeConfig* config;
            void (*on_reload)(uint32_t, uint64_t, const uint8_t*, size_t, uint32_t, const uint8_t*, size_t);
        } RuntimeCreateOptions;

        void* polyplug_runtime_create_with_options(const RuntimeCreateOptions* options);
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

    def release_plugin(self, handle: int) -> None:
        if handle != 0:
            self.lib.polyplug_runtime_release_plugin(self.ffi.cast("void*", handle))

    def last_error(self, rt: int, buf: Any, buf_len: int) -> int:
        return self.lib.polyplug_runtime_last_error(buf, buf_len)

    def error_message_len(self) -> int:
        return self.lib.polyplug_runtime_error_message_len()

    def register_host_contract(self, rt: int, interface_ptr: int) -> int:
        return self.lib.polyplug_runtime_register_host_contract(
            self.ffi.cast("void*", rt), self.ffi.cast("void*", interface_ptr)
        )

    def create_uint64_array(self, cap: int) -> Any:
        return self.ffi.new("uint64_t[]", cap)


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


def _last_error(backend: Backend) -> str:
    msg_len: int = backend.error_message_len()
    if msg_len == 0:
        return ""

    if hasattr(backend, "ffi"):
        buf = backend.ffi.new("uint8_t[]", msg_len)
        written: int = backend.last_error(0, buf, msg_len)
        if written <= 0:
            return ""
        return backend.ffi.buffer(buf, written)[:].decode("utf-8", errors="replace")
    else:
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


def _read_c_string(ptr: int, length: int) -> str:
    """Read a C string from a pointer and length."""
    if ptr == 0 or length == 0:
        return ""
    import ctypes

    return ctypes.string_at(ptr, length).decode("utf-8", errors="replace")


class ReloadPhaseFfi(ctypes.Structure):
    """FFI-safe struct for ReloadPhase - mirrors ffi::ReloadPhaseFfi (not a 'C suffix' type)."""

    _fields_ = [
        ("phase_type", ctypes.c_uint32),
        ("bundle_id", ctypes.c_uint64),
        ("bundle_name", StringView),
        ("retry_count", ctypes.c_uint32),
        ("reason", StringView),
    ]


class Runtime:
    """polyplug runtime for loading and managing plugins."""

    _on_reload_cb: Optional[Callable[[ReloadPhase], None]] = None
    _config: Optional["RuntimeConfig"] = None

    def __init__(self) -> None:
        lib_path: str = os.environ.get("POLYPLUG_LIB_PATH") or _resolve_lib_path()
        self._backend: Backend = _create_backend(lib_path)

        if self._on_reload_cb is not None or self._config is not None:
            rt_ptr: int = self._create_runtime_with_options()
        else:
            rt_ptr = self._backend.create_runtime()

        if rt_ptr == 0:
            msg: str = _last_error(self._backend)
            raise RuntimeError(msg or "polyplug_runtime_create failed")
        self._runtime: int = rt_ptr

    def _create_runtime_with_options(self) -> int:
        """Create runtime using polyplug_runtime_create_with_options."""
        options = RuntimeCreateOptionsC()
        config_c = None

        if self._config is not None:
            config_c = RuntimeConfig(
                hot_reload_enabled=1 if self._config.hot_reload_enabled else 0,
                hot_reload_max_retries=self._config.hot_reload_max_retries,
                hot_reload_retry_interval_ms=self._config.hot_reload_retry_interval_ms,
                hot_reload_abort_on_max_retries=1
                if self._config.hot_reload_abort_on_max_retries
                else 0,
                compatibility=COMPATIBILITY_STRICT,  # Default to Strict mode
            )
            options.config = ctypes.pointer(config_c)

        if self._on_reload_cb is not None:
            if not hasattr(Runtime, "_c_callback"):
                Runtime._c_callback = self._make_c_callback()
            options.on_reload = ctypes.cast(Runtime._c_callback, ctypes.c_void_p)

        lib = ctypes.CDLL(os.environ.get("POLYPLUG_LIB_PATH") or _resolve_lib_path())
        lib.polyplug_runtime_create_with_options.argtypes = [
            ctypes.POINTER(RuntimeCreateOptionsC)
        ]
        lib.polyplug_runtime_create_with_options.restype = ctypes.c_void_p

        return lib.polyplug_runtime_create_with_options(ctypes.byref(options)) or 0

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
        - PREPARING: Before interface swap, includes retry count
        - RELOADED: After successful interface swap
        - FAILED: When reload fails, includes reason string

        Must be called before creating a Runtime instance.

        Args:
            callback: Function that receives ReloadPhase objects.
        """
        cls._on_reload_cb = callback

    @classmethod
    def set_config(cls, config: "RuntimeConfig") -> None:
        """Set runtime configuration for subsequently created runtimes.

        Must be called before creating a Runtime instance.

        Args:
            config: RuntimeConfig with hot-reload settings.
        """
        cls._config = config

    @classmethod
    def _make_c_callback(cls) -> "ctypes.CFUNCTYPE":
        """Internal: Create a C-compatible callback wrapper."""
        import ctypes

        @ctypes.CFUNCTYPE(
            None,
            ctypes.c_uint32,
            ctypes.c_uint64,
            ctypes.c_void_p,
            ctypes.c_size_t,
            ctypes.c_uint32,
            ctypes.c_void_p,
            ctypes.c_size_t,
        )
        def c_callback(
            phase_type: int,
            bundle_id: int,
            bundle_name_ptr: int,
            bundle_name_len: int,
            retry_count: int,
            reason_ptr: int,
            reason_len: int,
        ) -> None:
            if cls._on_reload_cb is not None:
                phase = ReloadPhase(
                    type=ReloadPhaseType(phase_type),
                    bundle_id=bundle_id,
                    bundle_name=_read_c_string(bundle_name_ptr, bundle_name_len),
                    retry_count=retry_count,
                    reason=_read_c_string(reason_ptr, reason_len)
                    if reason_len > 0
                    else None,
                )
                cls._on_reload_cb(phase)

        return c_callback

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
                    return [out[i] for i in range(count)]
                else:
                    return [int(out[i]) for i in range(count)]
            cap = cap * 2

    def resolve_plugin(self, packed_handle: int) -> int:
        """Resolve a packed handle to a raw resolve_handle.

        Returns the raw resolve_handle (int) that can be used to access
        the plugin interface. The caller is responsible for calling
        release_plugin(handle) when done, especially before hot-reload.

        NOTE: In the instance-based model, callers should:
        1. Get the interface via resolve_plugin (returns raw handle)
        2. Use create_instance/destroy_instance for stateful access
        3. Call release_plugin when done with the handle

        Args:
            packed_handle: The packed handle from find_by_contract.

        Returns:
            Raw resolve_handle (int) for the plugin.

        Raises:
            RuntimeError: If packed_handle is null or resolution fails.
        """
        if packed_handle == _NULL_HANDLE:
            raise RuntimeError("null plugin handle")
        runtime_ptr: int = self._ensure_runtime()
        resolve_handle: int = self._backend.resolve_plugin(runtime_ptr, packed_handle)
        return resolve_handle

    def release_plugin(self, resolve_handle: int) -> None:
        """Release a resolve_handle obtained from resolve_plugin.

        Must be called when the caller is done with the handle,
        especially before hot-reload to avoid stale references.

        Args:
            resolve_handle: The raw handle from resolve_plugin.
        """
        if resolve_handle != 0:
            self._backend.release_plugin(resolve_handle)

    def get_extension(self, extension_id: int) -> None:
        _ = extension_id
        return None

    def register_host_contract(
        self,
        contract_id: int,
        contract_major: int,
        contract_minor: int,
        function_count: int,
        impl: Callable[[int, int, int], None],
    ) -> None:
        """Register a host contract implementation.

        Args:
            contract_id: The FNV-1a hash of the host contract name
                (e.g., fnv1a_64("host_contract:logger@1".encode()))
            contract_major: Major version of the contract
            contract_minor: Minor version of the contract
            function_count: Number of functions in the contract
            impl: Python callable that receives (fn_id, args_ptr, out_ptr)
                and implements the host contract functions

        Raises:
            RuntimeError: If registration fails (duplicate contract or other error)
        """
        runtime_ptr: int = self._ensure_runtime()

        # Store the implementation in a class-level dict to keep it alive
        if not hasattr(Runtime, "_host_contract_impls"):
            Runtime._host_contract_impls: dict[
                int, Callable[[int, int, int], None]
            ] = {}
        Runtime._host_contract_impls[contract_id] = impl

        # Create a ctypes callback for the dispatch function
        @ctypes.CFUNCTYPE(
            ctypes.c_uint32,
            ctypes.c_void_p,
            ctypes.c_uint32,
            ctypes.c_void_p,
            ctypes.c_void_p,
        )
        def dispatch_callback(
            bridge_data: int,
            fn_id: int,
            args_ptr: int,
            out_ptr: int,
        ) -> int:
            # Look up the implementation and call it
            impl_func: Callable[[int, int, int], None] = (
                Runtime._host_contract_impls.get(contract_id)
            )
            if impl_func is None:
                return 100  # ABI_HOST_CONTRACT_NOT_FOUND
            try:
                impl_func(fn_id, args_ptr, out_ptr)
                return 0  # ABI_OK
            except Exception:
                return 102  # ABI_HOST_CONTRACT_CALL_FAILED

        # Store the callback to keep it alive
        if not hasattr(Runtime, "_host_contract_callbacks"):
            Runtime._host_contract_callbacks: dict[int, ctypes.CFUNCTYPE] = {}
        Runtime._host_contract_callbacks[contract_id] = dispatch_callback

        # Create the HostContractInterface structure
        interface: HostContractInterface = HostContractInterface()
        interface.header.interface_version = 1
        interface.header.contract_id = contract_id
        interface.header.contract_major = contract_major
        interface.header.contract_minor = contract_minor
        interface.header.function_count = function_count
        interface.header.dispatch_type = DISPATCH_TYPE_VIRTUAL_MACHINE
        interface.dispatch.vm.call = ctypes.cast(dispatch_callback, ctypes.c_void_p)
        interface.dispatch.vm.bridge_data = 0  # Not used for Python

        # Store the interface to keep it alive
        if not hasattr(Runtime, "_host_contract_interfaces"):
            Runtime._host_contract_interfaces: dict[int, HostContractInterface] = {}
        Runtime._host_contract_interfaces[contract_id] = interface

        # Get pointer to the interface
        interface_ptr: int = ctypes.addressof(interface)

        # Call the FFI to register
        code: int = self._backend.register_host_contract(runtime_ptr, interface_ptr)
        if code == 2:
            raise RuntimeError(
                f"duplicate host contract registration: contract_id={contract_id}"
            )
        elif code != 0:
            msg: str = _last_error(self._backend)
            raise RuntimeError(msg or f"register_host_contract failed with code {code}")

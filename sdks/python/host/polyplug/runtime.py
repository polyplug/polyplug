"""
polyplug Python Host Library

After 18-02/18-03: All operations go through HostInterface struct fields.
Only two FFI exports remain: polyplug_runtime_create, polyplug_runtime_destroy.

The Runtime class holds a HostInterface pointer and calls methods through struct fields.
"""

from __future__ import annotations

import ctypes
import os
from pathlib import Path
from typing import Any, Callable, Optional, Protocol, runtime_checkable

from polyplug_abi import AbiErrorCode, ReloadPhase, ReloadPhaseType, StringView

_LIB_NAME: str = "polyplug"

# ─── Compatibility Constants ─────────────────────────────────────────────────────
# These match polyplug_abi::Compatibility #[repr(u32)] enum

COMPATIBILITY_STRICT: int = 0   # Exact major match and minor >= required
COMPATIBILITY_RELAXED: int = 1  # Same major, any minor
COMPATIBILITY_YOLO: int = 2     # Any version accepted


# ─── RuntimeConfig Structure ─────────────────────────────────────────────────────
# FFI RuntimeConfig matching polyplug_abi::RuntimeConfig (24 bytes)


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


# ─── HostInterface Structure (18-03) ─────────────────────────────────────────────
# FFI HostInterface matching polyplug_abi::HostInterface (144 bytes)


class HostInterface(ctypes.Structure):
    """FFI HostInterface matching polyplug_abi::HostInterface (144 bytes).

    Contains runtime pointer and function pointers for all operations.
    All function pointers use self-passing pattern (receive HostInterface* as first param).

    Layout (18 fields, 144 bytes):
        offset 0: runtime (*mut c_void)
        offset 8: register_contract
        offset 16: alloc
        offset 24: free
        offset 32: find_guest_contract
        offset 40: find_all_guest_contracts
        offset 48: resolve_guest_contract
        offset 56: call_guest_method
        offset 64: get_host_contract
        offset 72: resolve_host_contract_interface
        offset 80: list_bundles
        offset 88: get_dependencies
        offset 96: load_bundle
        offset 104: reload_bundle
        offset 112: register_host_contract
        offset 120: register_loader
        offset 128: get_last_error
        offset 136: get_error_len
    """

    _fields_ = [
        ("runtime", ctypes.c_void_p),                      # offset 0
        ("register_contract", ctypes.c_void_p),            # offset 8
        ("alloc", ctypes.c_void_p),                        # offset 16
        ("free", ctypes.c_void_p),                         # offset 24
        ("find_guest_contract", ctypes.c_void_p),         # offset 32
        ("find_all_guest_contracts", ctypes.c_void_p),    # offset 40
        ("resolve_guest_contract", ctypes.c_void_p),      # offset 48
        ("call_guest_method", ctypes.c_void_p),           # offset 56
        ("get_host_contract", ctypes.c_void_p),           # offset 64
        ("resolve_host_contract_interface", ctypes.c_void_p), # offset 72
        ("list_bundles", ctypes.c_void_p),                # offset 80
        ("get_dependencies", ctypes.c_void_p),            # offset 88
        ("load_bundle", ctypes.c_void_p),                 # offset 96
        ("reload_bundle", ctypes.c_void_p),               # offset 104
        ("register_host_contract", ctypes.c_void_p),      # offset 112
        ("register_loader", ctypes.c_void_p),             # offset 120
        ("get_last_error", ctypes.c_void_p),              # offset 128
        ("get_error_len", ctypes.c_void_p),               # offset 136
    ]


# ─── Host Contract Interface Structures ─────────────────────────────────────────────


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


# ─── Backend Protocol (18-03: HostInterface-based) ─────────────────────────────────

@runtime_checkable
class Backend(Protocol):
    """Protocol for HostInterface-based FFI backend.

    Only two FFI bindings needed: create and destroy.
    All operations go through HostInterface struct fields.
    """

    def create_host_interface(self) -> int: ...
    def destroy_host_interface(self, host: int) -> None: ...
    def load_host_interface(self, host: int) -> HostInterface: ...
    def create_uint64_array(self, cap: int) -> Any: ...


class CTypesBackend:
    """ctypes-based FFI backend for HostInterface operations."""

    def __init__(self, lib_path: str) -> None:
        self.ctypes = ctypes
        self.lib: ctypes.CDLL = ctypes.CDLL(lib_path)
        self._setup_bindings()

    def _setup_bindings(self) -> None:
        # Only two FFI exports (18-02)
        self.lib.polyplug_runtime_create.argtypes = []
        self.lib.polyplug_runtime_create.restype = self.ctypes.c_void_p

        self.lib.polyplug_runtime_destroy.argtypes = [self.ctypes.c_void_p]
        self.lib.polyplug_runtime_destroy.restype = None

        # Options-based create for hot-reload config
        self.lib.polyplug_runtime_create_with_options.argtypes = [
            self.ctypes.POINTER(RuntimeCreateOptionsC)
        ]
        self.lib.polyplug_runtime_create_with_options.restype = self.ctypes.c_void_p

    def create_host_interface(self) -> int:
        """Create runtime and return HostInterface pointer."""
        return self.lib.polyplug_runtime_create() or 0

    def create_host_interface_with_options(self, options: RuntimeCreateOptionsC) -> int:
        """Create runtime with options and return HostInterface pointer."""
        return self.lib.polyplug_runtime_create_with_options(self.ctypes.byref(options)) or 0

    def destroy_host_interface(self, host: int) -> None:
        """Destroy HostInterface and runtime."""
        self.lib.polyplug_runtime_destroy(host)

    def load_host_interface(self, host: int) -> HostInterface:
        """Load HostInterface struct from pointer."""
        return HostInterface.from_address(host)

    def create_uint64_array(self, cap: int) -> Any:
        return (self.ctypes.c_uint64 * cap)()


class CFFIBackend:
    """cffi ABI mode backend for HostInterface operations."""

    CDEF = """
        void* polyplug_runtime_create(void);
        void polyplug_runtime_destroy(void* host);
        void* polyplug_runtime_create_with_options(const void* options);

        typedef struct {
            uint8_t hot_reload_enabled;
            uint8_t _pad1[3];
            uint32_t hot_reload_max_retries;
            uint64_t hot_reload_retry_interval_ms;
            uint8_t hot_reload_abort_on_max_retries;
            uint8_t _pad2[3];
            uint32_t compatibility;
        } RuntimeConfig;

        typedef struct {
            const RuntimeConfig* config;
            void (*on_reload)(uint32_t, uint64_t, const uint8_t*, size_t, uint32_t, const uint8_t*, size_t);
        } RuntimeCreateOptions;
    """

    def __init__(self, lib_path: str) -> None:
        import cffi
        self.ffi = cffi.FFI()
        self.ffi.cdef(self.CDEF)
        self.lib = self.ffi.dlopen(lib_path)

    def create_host_interface(self) -> int:
        """Create runtime and return HostInterface pointer."""
        return self.ffi.cast("uintptr_t", self.lib.polyplug_runtime_create())

    def create_host_interface_with_options(self, options: RuntimeCreateOptionsC) -> int:
        """Create runtime with options and return HostInterface pointer."""
        # Convert ctypes struct to cffi
        opts_ptr = self.ffi.new("RuntimeCreateOptions*")
        if options.config:
            config_cffi = self.ffi.new("RuntimeConfig*")
            config_ptr = self.ctypes.cast(options.config, self.ctypes.POINTER(RuntimeConfig))
            config = config_ptr.contents
            config_cffi.hot_reload_enabled = config.hot_reload_enabled
            config_cffi.hot_reload_max_retries = config.hot_reload_max_retries
            config_cffi.hot_reload_retry_interval_ms = config.hot_reload_retry_interval_ms
            config_cffi.hot_reload_abort_on_max_retries = config.hot_reload_abort_on_max_retries
            config_cffi.compatibility = config.compatibility
            opts_ptr.config = config_cffi
        if options.on_reload:
            opts_ptr.on_reload = self.ffi.cast("void*", options.on_reload)
        return self.ffi.cast("uintptr_t", self.lib.polyplug_runtime_create_with_options(opts_ptr))

    def destroy_host_interface(self, host: int) -> None:
        """Destroy HostInterface and runtime."""
        self.lib.polyplug_runtime_destroy(self.ffi.cast("void*", host))

    def load_host_interface(self, host: int) -> HostInterface:
        """Load HostInterface struct from pointer (via ctypes)."""
        return HostInterface.from_address(host)

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


def _read_c_string(ptr: int, length: int) -> str:
    """Read a C string from a pointer and length."""
    if ptr == 0 or length == 0:
        return ""
    return ctypes.string_at(ptr, length).decode("utf-8", errors="replace")


# ─── Function pointer types for HostInterface calls ───────────────────────────────

# load_bundle: fn(host: *const HostInterface, path: *const u8, path_len: usize) -> AbiError
_LOAD_BUNDLE_FN = ctypes.CFUNCTYPE(
    ctypes.c_uint32,  # AbiError.code
    ctypes.c_void_p,  # HostInterface*
    ctypes.POINTER(ctypes.c_uint8),  # path
    ctypes.c_size_t,  # path_len
)

# reload_bundle: fn(host: *const HostInterface, path: *const u8, path_len: usize) -> AbiError
_RELOAD_BUNDLE_FN = ctypes.CFUNCTYPE(
    ctypes.c_uint32,
    ctypes.c_void_p,
    ctypes.POINTER(ctypes.c_uint8),
    ctypes.c_size_t,
)

# find_guest_contract: fn(host: *const HostInterface, contract_id: u64, min_version: u32) -> GuestContractHandle
_FIND_GUEST_CONTRACT_FN = ctypes.CFUNCTYPE(
    ctypes.c_uint64,  # GuestContractHandle (packed)
    ctypes.c_void_p,
    ctypes.c_uint64,
    ctypes.c_uint32,
)

# find_all_guest_contracts: fn(host: *const HostInterface, contract_id: u64, min_version: u32) -> Array<Handle>
_FIND_ALL_GUEST_CONTRACTS_FN = ctypes.CFUNCTYPE(
    ctypes.c_void_p,  # Array<GuestContractHandle> pointer
    ctypes.c_void_p,
    ctypes.c_uint64,
    ctypes.c_uint32,
)

# resolve_guest_contract: fn(host: *const HostInterface, handle: GuestContractHandle) -> *const GuestContractInterface
_RESOLVE_GUEST_CONTRACT_FN = ctypes.CFUNCTYPE(
    ctypes.c_void_p,
    ctypes.c_void_p,
    ctypes.c_uint64,
)

# get_last_error: fn(host: *const HostInterface, buf: *mut u8, buf_len: usize) -> usize
_GET_LAST_ERROR_FN = ctypes.CFUNCTYPE(
    ctypes.c_size_t,
    ctypes.c_void_p,
    ctypes.POINTER(ctypes.c_uint8),
    ctypes.c_size_t,
)

# get_error_len: fn(host: *const HostInterface) -> usize
_GET_ERROR_LEN_FN = ctypes.CFUNCTYPE(
    ctypes.c_size_t,
    ctypes.c_void_p,
)

# register_host_contract: fn(host: *const HostInterface, interface: *const HostContractInterface) -> AbiError
_REGISTER_HOST_CONTRACT_FN = ctypes.CFUNCTYPE(
    ctypes.c_uint32,
    ctypes.c_void_p,
    ctypes.c_void_p,
)

# free: fn(host: *const HostInterface, ptr: *mut u8, size: usize, align: usize)
_FREE_FN = ctypes.CFUNCTYPE(
    None,
    ctypes.c_void_p,
    ctypes.c_void_p,
    ctypes.c_size_t,
    ctypes.c_size_t,
)


class ReloadPhaseFfi(ctypes.Structure):
    """FFI-safe struct for ReloadPhase."""

    _fields_ = [
        ("phase_type", ctypes.c_uint32),
        ("bundle_id", ctypes.c_uint64),
        ("bundle_name", StringView),
        ("retry_count", ctypes.c_uint32),
        ("reason", StringView),
    ]


class Runtime:
    """polyplug runtime for loading and managing plugins.

    Holds HostInterface pointer and calls methods through struct fields.
    """

    _on_reload_cb: Optional[Callable[[ReloadPhase], None]] = None
    _config: Optional["RuntimeConfig"] = None

    def __init__(self) -> None:
        lib_path: str = os.environ.get("POLYPLUG_LIB_PATH") or _resolve_lib_path()
        self._backend: Backend = _create_backend(lib_path)
        self.ctypes = ctypes

        # Create HostInterface (options or default)
        if self._on_reload_cb is not None or self._config is not None:
            host_ptr: int = self._create_runtime_with_options()
        else:
            host_ptr = self._backend.create_host_interface()

        if host_ptr == 0:
            raise RuntimeError("polyplug_runtime_create returned null")

        # Store HostInterface pointer and load struct
        self._host: int = host_ptr
        self._host_struct: HostInterface = self._backend.load_host_interface(host_ptr)

        # Cache function pointer wrappers
        self._load_bundle_fn = _LOAD_BUNDLE_FN(self._host_struct.load_bundle)
        self._reload_bundle_fn = _RELOAD_BUNDLE_FN(self._host_struct.reload_bundle)
        self._find_guest_contract_fn = _FIND_GUEST_CONTRACT_FN(self._host_struct.find_guest_contract)
        self._find_all_fn = _FIND_ALL_GUEST_CONTRACTS_FN(self._host_struct.find_all_guest_contracts)
        self._resolve_fn = _RESOLVE_GUEST_CONTRACT_FN(self._host_struct.resolve_guest_contract)
        self._get_last_error_fn = _GET_LAST_ERROR_FN(self._host_struct.get_last_error)
        self._get_error_len_fn = _GET_ERROR_LEN_FN(self._host_struct.get_error_len)
        self._register_host_contract_fn = _REGISTER_HOST_CONTRACT_FN(self._host_struct.register_host_contract)
        self._free_fn = _FREE_FN(self._host_struct.free)

    def _create_runtime_with_options(self) -> int:
        """Create runtime using polyplug_runtime_create_with_options."""
        options = RuntimeCreateOptionsC()
        config_c = None

        if self._config is not None:
            config_c = RuntimeConfig(
                hot_reload_enabled=1 if self._config.hot_reload_enabled else 0,
                hot_reload_max_retries=self._config.hot_reload_max_retries,
                hot_reload_retry_interval_ms=self._config.hot_reload_retry_interval_ms,
                hot_reload_abort_on_max_retries=1 if self._config.hot_reload_abort_on_max_retries else 0,
                compatibility=COMPATIBILITY_STRICT,
            )
            options.config = ctypes.pointer(config_c)

        if self._on_reload_cb is not None:
            if not hasattr(Runtime, "_c_callback"):
                Runtime._c_callback = self._make_c_callback()
            options.on_reload = ctypes.cast(Runtime._c_callback, ctypes.c_void_p)

        return self._backend.create_host_interface_with_options(options)

    def __del__(self) -> None:
        host_ptr: int = getattr(self, "_host", 0)
        backend: Backend = getattr(self, "_backend", None)
        if host_ptr != 0 and backend is not None:
            backend.destroy_host_interface(host_ptr)
            self._host = 0

    @classmethod
    def on_reload(cls, callback: Callable[[ReloadPhase], None]) -> None:
        """Register a callback for hot-reload notifications.

        Must be called before creating a Runtime instance.
        """
        cls._on_reload_cb = callback

    @classmethod
    def set_config(cls, config: "RuntimeConfig") -> None:
        """Set runtime configuration for subsequently created runtimes.

        Must be called before creating a Runtime instance.
        """
        cls._config = config

    @classmethod
    def _make_c_callback(cls) -> ctypes.CFUNCTYPE:
        """Internal: Create a C-compatible callback wrapper."""

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
                    reason=_read_c_string(reason_ptr, reason_len) if reason_len > 0 else None,
                )
                cls._on_reload_cb(phase)

        return c_callback

    def _ensure_host(self) -> int:
        if self._host == 0:
            raise RuntimeError("Runtime is closed")
        return self._host

    def _get_error(self) -> str:
        """Get last error message from HostInterface."""
        host: int = self._ensure_host()
        error_len: int = self._get_error_len_fn(host)
        if error_len == 0:
            return ""

        buf = (ctypes.c_uint8 * error_len)()
        written: int = self._get_last_error_fn(host, buf, error_len)
        if written <= 0:
            return ""
        return bytes(buf[:written]).decode("utf-8", errors="replace")

    def _check_error(self, code: int, context: str) -> None:
        """Check error code and raise if non-zero."""
        if code == 0:
            return
        msg: str = self._get_error()
        if msg:
            raise RuntimeError(msg)
        raise RuntimeError(f"{context} failed with code {code}")

    def load_bundle(self, path: str | Path) -> None:
        """Load a plugin bundle from path."""
        host: int = self._ensure_host()
        path_bytes: bytes = str(Path(path)).encode("utf-8")
        buf = (ctypes.c_uint8 * len(path_bytes))(*path_bytes)
        code: int = self._load_bundle_fn(host, buf, len(path_bytes))
        self._check_error(code, "load_bundle")

    def reload_bundle(self, path: str | Path) -> None:
        """Hot-reload a plugin bundle."""
        host: int = self._ensure_host()
        path_bytes: bytes = str(Path(path)).encode("utf-8")
        buf = (ctypes.c_uint8 * len(path_bytes))(*path_bytes)
        code: int = self._reload_bundle_fn(host, buf, len(path_bytes))
        self._check_error(code, "reload_bundle")

    def find_guest_contract(self, contract_id: int, min_version: int) -> int:
        """Find a guest contract by contract_id and minimum version."""
        host: int = self._ensure_host()
        return self._find_guest_contract_fn(host, contract_id, min_version)

    def find_all_by_contract(self, contract_id: int, min_version: int) -> list[int]:
        """Find all guest contracts matching contract_id."""
        host: int = self._ensure_host()
        # Array struct: { data: *mut T, len: usize }
        array_ptr = self._find_all_fn(host, contract_id, min_version)
        if array_ptr == 0:
            return []

        # Read Array<GuestContractHandle> from pointer
        # Array layout: data (8 bytes), len (8 bytes)
        array_data = ctypes.c_void_p.from_address(array_ptr).value
        array_len = ctypes.c_size_t.from_address(array_ptr + 8).value

        if array_len == 0 or array_data == 0:
            return []

        # Read handles from array
        handles = []
        for i in range(array_len):
            handle = ctypes.c_uint64.from_address(array_data + i * 8).value
            handles.append(handle)

        # Free the array via host->free
        self._free_fn(host, array_data, array_len * 8, 8)

        return handles

    def resolve_guest_contract(self, packed_handle: int) -> int:
        """Resolve a packed handle to a GuestContractInterface pointer."""
        if packed_handle == _NULL_HANDLE:
            raise RuntimeError("null plugin handle")
        host: int = self._ensure_host()
        return self._resolve_fn(host, packed_handle)

    def release_plugin(self, resolve_handle: int) -> None:
        """Release a resolve handle (no-op in HostInterface model, managed by registry)."""
        # In HostInterface model, resolve handles are borrowed references
        # No explicit release needed - the registry manages lifetimes
        pass

    def get_extension(self, extension_id: int) -> None:
        return None

    def register_host_contract(
        self,
        contract_id: int,
        contract_major: int,
        contract_minor: int,
        function_count: int,
        impl: Callable[[int, int, int], None],
    ) -> None:
        """Register a host contract implementation."""
        host: int = self._ensure_host()

        # Store implementation to keep it alive
        if not hasattr(Runtime, "_host_contract_impls"):
            Runtime._host_contract_impls: dict[int, Callable[[int, int, int], None]] = {}
        Runtime._host_contract_impls[contract_id] = impl

        # Create dispatch callback
        @ctypes.CFUNCTYPE(
            ctypes.c_uint32,
            ctypes.c_void_p,
            ctypes.c_uint32,
            ctypes.c_void_p,
            ctypes.c_void_p,
        )
        def dispatch_callback(bridge_data: int, fn_id: int, args_ptr: int, out_ptr: int) -> int:
            impl_func = Runtime._host_contract_impls.get(contract_id)
            if impl_func is None:
                return AbiErrorCode.HostContractNotFound
            try:
                impl_func(fn_id, args_ptr, out_ptr)
                return AbiErrorCode.Ok
            except Exception:
                return AbiErrorCode.HostContractCallFailed

        # Store callback
        if not hasattr(Runtime, "_host_contract_callbacks"):
            Runtime._host_contract_callbacks: dict[int, ctypes.CFUNCTYPE] = {}
        Runtime._host_contract_callbacks[contract_id] = dispatch_callback

        # Create HostContractInterface
        interface = HostContractInterface()
        interface.header.interface_version = 1
        interface.header.contract_id = contract_id
        interface.header.contract_major = contract_major
        interface.header.contract_minor = contract_minor
        interface.header.function_count = function_count
        interface.header.dispatch_type = DISPATCH_TYPE_VIRTUAL_MACHINE
        interface.dispatch.vm.call = ctypes.cast(dispatch_callback, ctypes.c_void_p)
        interface.dispatch.vm.bridge_data = 0

        # Store interface
        if not hasattr(Runtime, "_host_contract_interfaces"):
            Runtime._host_contract_interfaces: dict[int, HostContractInterface] = {}
        Runtime._host_contract_interfaces[contract_id] = interface

        # Register via HostInterface
        interface_ptr = ctypes.addressof(interface)
        code: int = self._register_host_contract_fn(host, interface_ptr)
        if code == 2:
            raise RuntimeError(f"duplicate host contract: contract_id={contract_id}")
        self._check_error(code, "register_host_contract")
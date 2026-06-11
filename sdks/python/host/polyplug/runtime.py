"""
polyplug Python Host Library

After 18-02/18-03: All operations go through HostApi struct fields.
Only two FFI exports remain: polyplug_runtime_create, polyplug_runtime_destroy.

The Runtime class holds a HostApi pointer and calls methods through struct fields.
All FFI struct types are imported from the auto-generated polyplug_abi module.
"""

from __future__ import annotations

import ctypes
import os
import sys
from pathlib import Path
from typing import Callable, Optional, Protocol, runtime_checkable

# Import all FFI struct types from the auto-generated abi module.
# The polyplug_abi package re-exports from sdks/python/abi/abi.py (per D-28).
from polyplug_abi import (
    AbiError,
    AbiErrorCode,
    Array,
    Compatibility,
    DispatchMechanisms,
    GuestContractHandle,
    GuestContractInstance,
    GuestContractInterface,
    HostContractInterface,
    HostContractInstance,
    HostApi,
    ReloadPhase,
    ReloadPhaseType,
    RuntimeConfig,
    StringView,
)

# The ABI-level ReloadPhase ctypes Structure (the on_reload callback receives a
# const pointer to this 48-byte struct). The `polyplug_abi` package re-exports a
# higher-level Python `ReloadPhase` wrapper class under the same name, so the
# raw ctypes Structure is imported from its defining module to disambiguate.
from polyplug_abi.abi import ReloadPhase as AbiReloadPhase

_LIB_NAME: str = "polyplug"

# ─── Compatibility Constants ─────────────────────────────────────────────────────
# These match polyplug_abi::Compatibility #[repr(u32)] enum

COMPATIBILITY_STRICT: int = Compatibility.Strict
COMPATIBILITY_RELAXED: int = Compatibility.Relaxed
COMPATIBILITY_YOLO: int = Compatibility.Yolo

# GuestContractHandle is `#[repr(C)] { index: u32, generation: u32 }` (8 bytes, align 4).
# The null handle sentinel has index == u32::MAX (0xFFFFFFFF) and generation == 0.
_NULL_HANDLE_INDEX: int = (1 << 32) - 1

_BACKEND: str = "ctypes"
_cffi_available: bool = False

try:
    import cffi
    _cffi_available = True
    _BACKEND = "cffi"
except ImportError:
    pass


# ─── Backend Protocol (18-03: HostApi-based) ─────────────────────────────────

@runtime_checkable
class Backend(Protocol):
    """Protocol for HostApi-based FFI backend.

    Only two FFI bindings needed: create and destroy.
    All operations go through HostApi struct fields.
    """

    def create_host_interface(self, config_ptr: int = 0) -> int: ...
    def destroy_host_interface(self, host: int) -> None: ...
    def load_host_interface(self, host: int) -> HostApi: ...


class CTypesBackend:
    """ctypes-based FFI backend for HostApi operations."""

    def __init__(self, lib_path: str) -> None:
        self.ctypes = ctypes
        self.lib: ctypes.CDLL = ctypes.CDLL(lib_path)
        self._setup_bindings()

    def _setup_bindings(self) -> None:
        # Only two FFI exports: create (takes optional *const RuntimeConfig,
        # null for defaults) and destroy.
        self.lib.polyplug_runtime_create.argtypes = [self.ctypes.c_void_p]
        self.lib.polyplug_runtime_create.restype = self.ctypes.c_void_p

        self.lib.polyplug_runtime_destroy.argtypes = [self.ctypes.c_void_p]
        self.lib.polyplug_runtime_destroy.restype = None

    def create_host_interface(self, config_ptr: int = 0) -> int:
        """Create runtime and return HostApi pointer.

        Args:
            config_ptr: Address of a RuntimeConfig struct, or 0 for defaults.
        """
        return self.lib.polyplug_runtime_create(config_ptr or None) or 0

    def destroy_host_interface(self, host: int) -> None:
        """Destroy HostApi and runtime."""
        self.lib.polyplug_runtime_destroy(host)

    def load_host_interface(self, host: int) -> HostApi:
        """Load HostApi struct from pointer."""
        return HostApi.from_address(host)


class CFFIBackend:
    """cffi ABI mode backend for HostApi operations."""

    CDEF = """
        void* polyplug_runtime_create(const void* config);
        void polyplug_runtime_destroy(void* host);
    """

    def __init__(self, lib_path: str) -> None:
        import cffi
        self.ffi = cffi.FFI()
        self.ffi.cdef(self.CDEF)
        self.lib = self.ffi.dlopen(lib_path)

    def create_host_interface(self, config_ptr: int = 0) -> int:
        """Create runtime and return HostApi pointer.

        Args:
            config_ptr: Address of a RuntimeConfig struct, or 0 for defaults.
        """
        return int(
            self.ffi.cast(
                "uintptr_t",
                self.lib.polyplug_runtime_create(self.ffi.cast("void*", config_ptr)),
            )
        )

    def destroy_host_interface(self, host: int) -> None:
        """Destroy HostApi and runtime."""
        self.lib.polyplug_runtime_destroy(self.ffi.cast("void*", host))

    def load_host_interface(self, host: int) -> HostApi:
        """Load HostApi struct from pointer (via ctypes)."""
        return HostApi.from_address(host)


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


class Runtime:
    """polyplug runtime for loading and managing plugins.

    Holds HostApi pointer and calls methods through struct fields.

    All configuration is per-instance (Rule 12: no class-level statics shared
    across runtimes): pass ``config`` and ``on_reload`` to the constructor.
    The ctypes callback wrapper and host-contract keepalives live on the
    instance and die with it.
    """

    def __init__(
        self,
        config: Optional["RuntimeConfig"] = None,
        on_reload: Optional[Callable[[ReloadPhase], None]] = None,
    ) -> None:
        lib_path: str = _resolve_lib_path()
        self._backend: Backend = _create_backend(lib_path)
        self.ctypes = ctypes

        # Per-instance reload-callback and config state (set before create so
        # the C callback wrapper outlives the create call).
        self._on_reload_cb: Optional[Callable[[ReloadPhase], None]] = on_reload
        self._config: Optional["RuntimeConfig"] = config
        self._c_callback: Optional[ctypes.CFUNCTYPE] = None
        self._runtime_config: Optional[RuntimeConfig] = None

        # Per-instance host-contract keepalives: registered interface structs
        # (with their thunks/stubs anchored on `_keepalive`), keyed by
        # contract_id. The runtime holds raw pointers into these for its whole
        # lifetime, so they must stay alive on THIS instance (Rule 12).
        self._host_contract_interfaces: dict[int, HostContractInterface] = {}

        # Create HostApi (options or default)
        if self._on_reload_cb is not None or self._config is not None:
            host_ptr: int = self._create_runtime_with_options()
        else:
            host_ptr = self._backend.create_host_interface()

        if host_ptr == 0:
            raise RuntimeError("polyplug_runtime_create returned null")

        # Store HostApi pointer and load struct
        self._host: int = host_ptr
        self._host_struct: HostApi = self._backend.load_host_interface(host_ptr)

        # The HostApi struct fields are already fully-typed C function
        # pointers (CFUNCTYPE with the canonical ABI signatures from abi.py:
        # functions returning AbiError do so BY VALUE as a 24-byte struct).
        # Cache them directly — re-wrapping in a hand-rolled CFUNCTYPE would
        # both duplicate the signature and risk drift from the canonical type.
        self._load_bundle_fn = self._host_struct.load_bundle
        self._reload_bundle_fn = self._host_struct.reload_bundle
        self._unload_bundle_fn = self._host_struct.unload_bundle
        self._find_guest_contract_fn = self._host_struct.find_guest_contract
        self._find_all_fn = self._host_struct.find_all_guest_contracts
        self._resolve_fn = self._host_struct.resolve_guest_contract
        self._get_last_error_fn = self._host_struct.get_last_error
        self._get_error_len_fn = self._host_struct.get_error_len
        self._register_host_contract_fn = self._host_struct.register_host_contract
        self._register_loader_fn = self._host_struct.register_loader
        self._free_fn = self._host_struct.free

    def _create_runtime_with_options(self) -> int:
        """Create runtime via polyplug_runtime_create with a RuntimeConfig.

        The RuntimeConfig (56 bytes) has:
        - compatibility (u32)
        - unload_mode (u32, default Retire)
        - hot_reload_enabled (bool/u8)
        - on_reload (fn pointer or null)
        - on_reload_user_data (pointer or null)
        - log (fn pointer or null)
        - log_user_data (pointer or null)
        - log_max_level (u32)

        The runtime only borrows the config for the duration of the build,
        but the config is retained on the instance so the C callback wrapper
        (referenced by `on_reload`) is not garbage-collected while in use.
        """
        config = RuntimeConfig()
        config.hot_reload_enabled = False
        config.compatibility = COMPATIBILITY_STRICT

        if self._config is not None:
            config.hot_reload_enabled = bool(self._config.hot_reload_enabled)
            if hasattr(self._config, "compatibility"):
                config.compatibility = self._config.compatibility

        if self._on_reload_cb is not None:
            self._c_callback = self._make_c_callback()
            # The generated field type erases the pointee (c_void_p); the typed
            # callback (POINTER(AbiReloadPhase) param) is cast to it. The
            # original wrapper stays referenced via self._c_callback so the
            # ctypes thunk is not garbage-collected while the runtime lives.
            config.on_reload = ctypes.cast(self._c_callback, type(config.on_reload))
        else:
            config.on_reload = ctypes.cast(None, type(config.on_reload))

        self._runtime_config = config
        return self._backend.create_host_interface(ctypes.addressof(config))

    def __del__(self) -> None:
        host_ptr: int = getattr(self, "_host", 0)
        backend: Backend = getattr(self, "_backend", None)
        if host_ptr != 0 and backend is not None:
            backend.destroy_host_interface(host_ptr)
            self._host = 0

    def _make_c_callback(self) -> ctypes.CFUNCTYPE:
        """Internal: Create a C-compatible callback wrapper bound to THIS instance.

        The wrapper never raises: a Python exception inside a ctypes callback
        is swallowed by ctypes (the callback silently dies), so the phase-type
        conversion is total (unknown discriminants pass through as the raw
        ``int``) and the user callback is wrapped in a catch-all that logs to
        stderr.
        """
        user_callback: Callable[[ReloadPhase], None] = self._on_reload_cb  # type: ignore[assignment]

        @ctypes.CFUNCTYPE(None, ctypes.c_void_p, ctypes.POINTER(AbiReloadPhase))
        def c_callback(_user_data: int, abi_phase_ptr: "ctypes._Pointer[AbiReloadPhase]") -> None:
            try:
                if not abi_phase_ptr:
                    # The runtime contract guarantees a non-null pointer; this
                    # guard is pure defence-in-depth.
                    return
                # The pointee is valid only for the duration of this call; all
                # fields (and the strings they reference) are copied into the
                # Python-level ReloadPhase below before the callback returns.
                abi_phase: AbiReloadPhase = abi_phase_ptr.contents
                raw_type: int = abi_phase.phase_type
                # Total conversion: ReloadPhaseType(raw) raises ValueError on an
                # unknown discriminant, which ctypes would swallow — fall back to
                # the raw int and warn instead.
                if raw_type in ReloadPhaseType._value2member_map_:
                    phase_type: ReloadPhaseType | int = ReloadPhaseType(raw_type)
                else:
                    phase_type = raw_type
                    print(
                        f"polyplug: unknown reload phase type {raw_type}; "
                        "passing raw value through",
                        file=sys.stderr,
                    )
                reason: str = _read_c_string(
                    abi_phase.reason.ptr, abi_phase.reason.len
                )
                phase = ReloadPhase(
                    type=phase_type,
                    bundle_id=abi_phase.bundle_id,
                    bundle_name=_read_c_string(
                        abi_phase.bundle_name.ptr, abi_phase.bundle_name.len
                    ),
                    reason=reason if reason else None,
                )
                user_callback(phase)
            except Exception as e:  # noqa: BLE001 — must not unwind across the C ABI
                print(f"polyplug: reload callback error: {e}", file=sys.stderr)

        return c_callback

    def _ensure_host(self) -> int:
        if self._host == 0:
            raise RuntimeError("Runtime is closed")
        return self._host

    def _get_error(self) -> str:
        """Get last error message from HostApi."""
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
        err: AbiError = self._load_bundle_fn(host, buf, len(path_bytes))
        self._check_error(err.code, "load_bundle")

    def reload_bundle(self, path: str | Path) -> None:
        """Hot-reload a plugin bundle."""
        host: int = self._ensure_host()
        path_bytes: bytes = str(Path(path)).encode("utf-8")
        buf = (ctypes.c_uint8 * len(path_bytes))(*path_bytes)
        err: AbiError = self._reload_bundle_fn(host, buf, len(path_bytes))
        self._check_error(err.code, "reload_bundle")

    def unload_bundle(self, bundle_id: int) -> None:
        """Unload a plugin bundle by bundle ID."""
        host: int = self._ensure_host()
        err: AbiError = self._unload_bundle_fn(host, bundle_id)
        self._check_error(err.code, "unload_bundle")

    def find_guest_contract(self, contract_id: int, min_version: int) -> GuestContractHandle:
        """Find a guest contract by contract_id and minimum version.

        Returns a GuestContractHandle struct (index: u32, generation: u32).
        The null/not-found sentinel has index == 0xFFFFFFFF.
        """
        host: int = self._ensure_host()
        return self._find_guest_contract_fn(host, contract_id, min_version)

    def find_all_by_contract(self, contract_id: int, min_version: int) -> list[GuestContractHandle]:
        """Find all guest contracts matching contract_id."""
        host: int = self._ensure_host()
        # `find_all_guest_contracts` returns an `Array` struct BY VALUE
        # (#[repr(C)] { items: *mut T, len: usize, align: usize } = 24 bytes).
        # The CFUNCTYPE restype is the `Array` Structure, so ctypes performs the
        # sret struct-return ABI and `array` is a populated `Array` instance —
        # NOT a pointer. Reading its fields directly is correct; treating the
        # result as a pointer (the old behavior) misread the sret register.
        array: Array = self._find_all_fn(host, contract_id, min_version)

        array_data: int = array.items or 0
        array_len: int = array.len
        if array_len == 0 or array_data == 0:
            return []

        # GuestContractHandle is `#[repr(C)] { index: u32, generation: u32 }` = 8 bytes,
        # so the array has an 8-byte element stride and each element is read as a full struct.
        element_size: int = ctypes.sizeof(GuestContractHandle)
        handles: list[GuestContractHandle] = []
        for i in range(array_len):
            handle: GuestContractHandle = GuestContractHandle.from_address(array_data + i * element_size)
            handles.append(handle)

        # Free the array via host->free using the same size/align the runtime
        # allocated with: size = len * sizeof(element), align = array.align.
        self._free_fn(host, array_data, array_len * element_size, array.align)

        return handles

    def resolve_guest_contract(self, handle: GuestContractHandle) -> int:
        """Resolve a guest contract handle to a GuestContractInterface pointer."""
        # Null handle sentinel has index == u32::MAX (0xFFFFFFFF).
        if handle.index == _NULL_HANDLE_INDEX:
            raise RuntimeError("null plugin handle")
        host: int = self._ensure_host()
        return self._resolve_fn(host, handle)

    def release_plugin(self, resolve_handle: int) -> None:
        """Release a resolve handle (no-op in HostApi model, managed by registry)."""
        # In HostApi model, resolve handles are borrowed references
        # No explicit release needed - the registry manages lifetimes
        pass

    def register_host_contract(self, interface: HostContractInterface) -> None:
        """Register a fully populated host contract interface with the runtime.

        The interface comes from a GENERATED factory
        (``generated/host/interface_factories.py``: ``create_*_interface``),
        which builds the thunks, instance stubs, and dispatch table with the
        correct ABI signatures and anchors them on ``interface._keepalive``.

        The interface struct is kept alive on THIS instance (per-runtime,
        keyed by contract_id) — never on the class, where a second runtime
        registering the same contract_id would clobber the first (Rule 12).
        The runtime holds the raw pointer for its whole lifetime.
        """
        host: int = self._ensure_host()

        contract_id: int = interface.contract_id
        # Instance-owned keepalive: the struct address must stay stable and
        # alive for the runtime lifetime.
        self._host_contract_interfaces[contract_id] = interface

        interface_ptr = ctypes.addressof(interface)
        err: AbiError = self._register_host_contract_fn(host, interface_ptr)
        if err.code == AbiErrorCode.DuplicateProvider:
            raise RuntimeError(f"duplicate host contract: contract_id={contract_id}")
        self._check_error(err.code, "register_host_contract")

    def register_loader(self, runtime_name: str, loader_ptr: int) -> None:
        """Register a language loader with the runtime via HostApi.

        Args:
            runtime_name: Runtime name the loader handles (e.g. "native", "python").
            loader_ptr: Opaque loader pointer from the loader cdylib's create function.
        """
        host: int = self._ensure_host()
        name_bytes: bytes = runtime_name.encode("utf-8")
        # Keep the buffer alive for the duration of the call; the host reads it synchronously.
        name_buf: ctypes.Array[ctypes.c_char] = ctypes.create_string_buffer(
            name_bytes, len(name_bytes)
        )
        name_view: StringView = StringView(
            ptr=ctypes.cast(name_buf, ctypes.c_void_p), len=len(name_bytes)
        )
        err: AbiError = self._register_loader_fn(host, name_view, loader_ptr)
        self._check_error(err.code, "register_loader")
"""polyplug host library — Python string helpers."""

from polyplug.abi import StringView
import ctypes

FNV_OFFSET = 0xCBF29CE484222325
FNV_PRIME = 0x00000100000001B3
MASK_64 = 0xFFFFFFFFFFFFFFFF


def fnv1a_64(data: bytes) -> int:
    """Compute FNV-1a 64-bit hash."""
    h = FNV_OFFSET
    for b in data:
        h ^= b
        h = (h * FNV_PRIME) & MASK_64
    return h


def contract_id(name: str, major_version: int) -> int:
    """Compute polyplug contract ID using FNV-1a 64-bit hash.

    Args:
        name: Contract name (e.g., "pipeline.Decoder")
        major_version: Major version number

    Returns:
        64-bit contract ID
    """
    s = f"{name}@{major_version}"
    return fnv1a_64(s.encode("utf-8"))


def bundle_id(name: str) -> int:
    """Compute bundle ID using FNV-1a 64-bit hash.

    Args:
        name: Bundle name

    Returns:
        64-bit bundle ID
    """
    return fnv1a_64(name.encode("utf-8"))


def to_str(sv: StringView) -> str:
    """Convert StringView to Python str.

    Args:
        sv: StringView from polyplug ABI

    Returns:
        Python string (UTF-8 decoded)
    """
    if not sv.ptr or sv.len == 0:
        return ""
    data = ctypes.cast(sv.ptr, ctypes.POINTER(ctypes.c_char * sv.len)).contents
    return bytes(data).decode("utf-8")


def to_string(sv: StringView) -> str:
    """Alias for to_str() — convert StringView to Python str."""
    return to_str(sv)


def str_as_view(s: str) -> StringView:
    """Create StringView from Python str (borrowed).

    Warning: The StringView is only valid while the Python string exists.
    Use to_string_view_owned() for a copy.

    Args:
        s: Python string

    Returns:
        StringView pointing to Python string memory
    """
    data = s.encode("utf-8")
    return StringView(ctypes.cast(data, ctypes.POINTER(ctypes.c_uint8)), len(data))


def str_as_view_owned(s: str) -> StringView:
    """Create StringView from Python str (owned copy).

    The returned StringView points to allocated memory that must be freed.

    Args:
        s: Python string

    Returns:
        StringView pointing to allocated memory
    """
    data = s.encode("utf-8")
    ptr = ctypes.cast(
        ctypes.create_string_buffer(data, len(data)), ctypes.POINTER(ctypes.c_uint8)
    )
    return StringView(ptr, len(data))


# Module-level cached types for hot path performance
class _DispatchType(ctypes.c_uint32):
    """Dispatch mechanism type enum."""

    NATIVE = 0
    VIRTUAL_MACHINE = 1


class _NativeDispatch(ctypes.Structure):
    """Native dispatch data — direct function pointer array."""

    _fields_ = [
        ("functions", ctypes.c_void_p),  # void** (array of function pointers)
    ]


class _VmDispatch(ctypes.Structure):
    """VM dispatch data — call through a dispatch function."""

    _fields_ = [
        ("call", ctypes.c_void_p),  # Dispatch function pointer
        ("loader_data", ctypes.c_void_p),  # Loader-specific data
    ]


class _PluginDispatch(ctypes.Union):
    """Union of dispatch mechanisms."""

    _fields_ = [
        ("native", _NativeDispatch),
        ("vm", _VmDispatch),
    ]


class _PluginInterface(ctypes.Structure):
    """Plugin interface — one per contract implemented by a plugin."""

    _fields_ = [
        ("rt_ctx", ctypes.c_void_p),  # Pointer to host context
        ("contract_id", ctypes.c_uint64),
        ("contract_version", ctypes.c_uint32),
        ("function_count", ctypes.c_uint32),
        ("dispatch_type", _DispatchType),
        ("dispatch", _PluginDispatch),
    ]


class _AbiError(ctypes.Structure):
    _fields_ = [
        ("code", ctypes.c_uint32),
        ("_pad", ctypes.c_uint32),
        ("message_ptr", ctypes.c_void_p),
        ("message_len", ctypes.c_size_t),
    ]


_AbiErrorFnType = ctypes.CFUNCTYPE(_AbiError, ctypes.c_void_p, ctypes.c_void_p)
_func_cache: dict[int, ctypes._CFuncPtr] = {}


def call_plugin_fn(lib: ctypes.CDLL, vtable_ptr: int, func_idx: int, input: str) -> str:
    from polyplug.abi import StringView, polyplug_host_free

    vtable = _PluginInterface.from_address(vtable_ptr)
    if func_idx >= vtable.function_count:
        raise RuntimeError(f"function index {func_idx} out of bounds")

    # Native dispatch: access via dispatch.native.functions[func_idx]
    funcs = ctypes.cast(
        vtable.dispatch.native.functions, ctypes.POINTER(ctypes.c_void_p)
    )
    func_ptr = funcs[func_idx]

    if func_ptr not in _func_cache:
        _func_cache[func_ptr] = _AbiErrorFnType(func_ptr)
    func = _func_cache[func_ptr]

    input_data = input.encode("utf-8")
    input_buf = ctypes.create_string_buffer(input_data, len(input_data))
    input_sv = StringView()
    input_sv.ptr = ctypes.cast(input_buf, ctypes.c_void_p)
    input_sv.len = len(input_data)

    output_sv = StringView()
    output_sv.ptr = 0
    output_sv.len = 0

    result = func(ctypes.byref(input_sv), ctypes.byref(output_sv))

    if result.code == 0 and output_sv.ptr and output_sv.len > 0:
        output_str = to_str(output_sv)
        polyplug_host_free(output_sv.ptr, output_sv.len, 1)
        return output_str
    else:
        raise RuntimeError(f"plugin returned error code={result.code}")

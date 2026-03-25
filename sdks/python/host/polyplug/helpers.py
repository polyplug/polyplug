"""polyplug host library — Python string helpers."""

from polyplug_abi import (
    StringView,
    PluginInterface,
    AbiError,
    fnv1a_64,
    contract_id,
    bundle_id,
    to_str,
)
import ctypes

# Re-export for backward compatibility
__all__ = [
    "fnv1a_64",
    "contract_id",
    "bundle_id",
    "to_str",
    "to_string",
    "str_as_view",
    "str_as_view_owned",
    "call_plugin_fn",
]


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


_AbiErrorFnType = ctypes.CFUNCTYPE(AbiError, ctypes.c_void_p, ctypes.c_void_p)
_func_cache: dict[int, ctypes._CFuncPtr] = {}


def call_plugin_fn(lib: ctypes.CDLL, vtable_ptr: int, func_idx: int, input: str) -> str:
    vtable = PluginInterface.from_address(vtable_ptr)
    if func_idx >= vtable.function_count:
        raise RuntimeError(f"function index {func_idx} out of bounds")

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
        return output_str
    else:
        raise RuntimeError(f"plugin returned error code={result.code}")

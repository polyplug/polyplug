"""polyplug host library — Python string helpers."""

from polyplug.abi import StringView
import ctypes


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
    return bytes(data).decode('utf-8')


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
    data = s.encode('utf-8')
    return StringView(ctypes.cast(data, ctypes.POINTER(ctypes.c_uint8)), len(data))


def str_as_view_owned(s: str) -> StringView:
    """Create StringView from Python str (owned copy).
    
    The returned StringView points to allocated memory that must be freed.
    
    Args:
        s: Python string
        
    Returns:
        StringView pointing to allocated memory
    """
    data = s.encode('utf-8')
    ptr = ctypes.cast(ctypes.create_string_buffer(data, len(data)), ctypes.POINTER(ctypes.c_uint8))
    return StringView(ptr, len(data))

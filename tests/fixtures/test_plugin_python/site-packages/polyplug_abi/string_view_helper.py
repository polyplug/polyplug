"""polyplug_abi — String helper functions for StringView."""

from __future__ import annotations

import ctypes
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from polyplug_abi.abi import StringView


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


def strip_prefix(sv: StringView, prefix: str) -> str:
    """Strip prefix from StringView if present, otherwise return original string.

    Args:
        sv: StringView from polyplug ABI
        prefix: Prefix string to strip

    Returns:
        String with prefix removed if it was present, otherwise original string
    """
    s: str = to_str(sv)
    if s.startswith(prefix):
        return s[len(prefix) :]
    return s


def starts_with(sv: StringView, prefix: str) -> bool:
    """Check if StringView starts with prefix.

    Args:
        sv: StringView from polyplug ABI
        prefix: Prefix string to check for

    Returns:
        True if the string starts with the prefix, False otherwise
    """
    return to_str(sv).startswith(prefix)


def ends_with(sv: StringView, suffix: str) -> bool:
    """Check if StringView ends with suffix.

    Args:
        sv: StringView from polyplug ABI
        suffix: Suffix string to check for

    Returns:
        True if the string ends with the suffix, False otherwise
    """
    return to_str(sv).endswith(suffix)


def split(sv: StringView, delimiter: str) -> list[str]:
    """Split StringView by delimiter.

    Args:
        sv: StringView from polyplug ABI
        delimiter: Delimiter string to split by

    Returns:
        List of strings resulting from the split
    """
    return to_str(sv).split(delimiter)


def str_as_view(s: str) -> StringView:
    """Create StringView from Python str (borrowed).

    Warning: The StringView is only valid while the Python string exists.

    Args:
        s: Python string

    Returns:
        StringView pointing to Python string memory
    """
    from polyplug_abi.abi import StringView

    data: bytes = s.encode("utf-8")
    return StringView(ctypes.cast(data, ctypes.POINTER(ctypes.c_uint8)), len(data))


def str_as_view_owned(s: str) -> StringView:
    """Create StringView from Python str (owned copy).

    The returned StringView points to allocated memory that must be freed.

    Args:
        s: Python string

    Returns:
        StringView pointing to allocated memory
    """
    from polyplug_abi.abi import StringView

    data: bytes = s.encode("utf-8")
    ptr: ctypes.POINTER(ctypes.c_uint8) = ctypes.cast(
        ctypes.create_string_buffer(data, len(data)), ctypes.POINTER(ctypes.c_uint8)
    )
    return StringView(ptr, len(data))

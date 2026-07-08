"""polyplug_abi — String helper functions for StringView."""

from __future__ import annotations

import ctypes

from polyplug_abi.abi import Buffer, StringView


def to_str(sv: StringView) -> str:
    """Convert StringView to Python str.

    A null/zero-length view decodes to ``""``. A non-null view whose bytes are
    NOT valid UTF-8 raises ``UnicodeDecodeError`` — the strict ``bytes.decode``
    never silently substitutes replacement characters for a readable-but-invalid
    view.

    Args:
        sv: StringView from polyplug ABI

    Returns:
        Python string (UTF-8 decoded)

    Raises:
        UnicodeDecodeError: if the viewed bytes are not valid UTF-8.
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
    """Split StringView by a literal delimiter, keeping empty segments.

    Args:
        sv: StringView from polyplug ABI
        delimiter: Literal delimiter string to split by

    Returns:
        [] for a null/empty view, [s] for an empty delimiter, otherwise the
        segments around every occurrence of the delimiter (empties kept)
    """
    s: str = to_str(sv)
    if not s:
        return []
    if not delimiter:
        return [s]
    return s.split(delimiter)


def as_bytes(buf: Buffer) -> memoryview:
    """Borrow a Buffer's bytes as a zero-copy ``memoryview``.

    Unlike :func:`to_str`, this never decodes: the view is byte-exact, so
    interior NULs and non-UTF-8 bytes are preserved. The returned ``memoryview``
    overlays the buffer's own memory (no copy); the caller must keep the
    buffer's allocation alive for as long as the view is read.

    A null pointer or zero length yields an empty view without dereferencing
    the pointer.

    Args:
        buf: Buffer from polyplug ABI.

    Returns:
        A zero-copy ``memoryview`` over ``buf``'s ``len`` bytes.
    """
    if not buf.ptr or buf.len == 0:
        return memoryview(b"")
    # from_address overlays existing memory (no copy); memoryview roots the
    # ctypes array so it lives as long as the view.
    overlay = (ctypes.c_char * buf.len).from_address(buf.ptr)
    return memoryview(overlay)


def bytes_as_view(data: bytes) -> StringView:
    """Build a borrowed ``StringView`` over a bytes object's internal buffer.

    The caller MUST keep ``data`` alive for as long as the view is read —
    ctypes does not root ``data`` through the view's raw pointer field. Bind
    the bytes to a local (or other owner) that outlives every read of the
    view; never build a view over a temporary such as ``s.encode("utf-8")``
    passed inline. Empty bytes produce a null view (``ptr=None, len=0``),
    which is legal at the ABI boundary.

    Args:
        data: caller-owned UTF-8 bytes backing the view.

    Returns:
        StringView borrowing ``data``'s buffer (null view for empty bytes).
    """
    if not data:
        return StringView(ptr=None, len=0)
    addr: int = ctypes.cast(ctypes.c_char_p(data), ctypes.c_void_p).value or 0
    return StringView(ptr=addr, len=len(data))

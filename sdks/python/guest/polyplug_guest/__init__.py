"""polyplug_guest — guest-side Python library for polyplug plugin authors.

Python plugins are VM-dispatch plugins (like Lua and JavaScript): the guest
never builds a ``GuestContractInterface`` or registers native function
pointers. Instead the loader executes the plugin module and calls its
``polyplug_init(host_ptr: int, ctx_ptr: int) -> tuple[list[dict], AbiError]``.
``polyplug_init`` RETURNS its registrations directly — nothing is deposited into
any module namespace. The loader reads the returned tuple: ``abi_error.code ==
AbiErrorCode.Ok`` selects the registration list it then wraps in a VM-dispatch
interface and registers with the runtime itself; a non-Ok code surfaces as a
loader error.

This library provides the registration helper that appends to that list, the
``StringView`` <-> ``str`` codecs, and the two cross-boundary allocators
(host-allocator for data that must outlive the call, arena for per-call return
buffers). It also re-exports the ABI types plugin authors need.

Per-call state (args, out, arena, arena_alloc) is passed explicitly through each
dispatch call. The ``HostApi`` pointer is NOT stored in this package: it flows
from ``polyplug_init`` into the author factory (``polyplug_create_<plugin>``),
which constructs the implementation with its owning runtime's host pointer.
Helpers that need the host (:func:`alloc_string`, :func:`log`, the generated
peer callers' ``resolve(host_ptr)``) take it as an explicit argument. The arena
allocator the guest forwards to :func:`alloc_string_arena` is likewise NOT a
module global: the loader passes it as the FINAL positional argument of every
dispatch call, and the generated glue threads it through.
"""

from __future__ import annotations

import ctypes
from typing import Callable, List, Optional

from polyplug_abi import (
    AbiErrorCode,
    AbiError,
    Buffer,
    PluginDescriptor,
    GuestContractHandle,
    BundleInitContext,
    HostApi,
    LogLevel,
    StringView,
    DispatchType,
    bytes_as_view,
    to_str,
)

__all__ = [
    "AbiErrorCode",
    "AbiError",
    "Buffer",
    "PluginDescriptor",
    "GuestContractHandle",
    "BundleInitContext",
    "HostApi",
    "LogLevel",
    "StringView",
    "DispatchType",
    "to_str",
    "register_contract",
    "alloc_string",
    "alloc_string_arena",
    "log",
]

def log(host_ptr: int, level: int, scope: str, message: str) -> None:
    """Send a guest diagnostic to the host's logging funnel (``HostApi.log``).

    Routes to the same sink as ``RuntimeConfig::log``: the host-installed
    callback when one is set, otherwise the host's stderr default (Error/Warn
    visibility only). The host delivers ``(level, scope, message)`` verbatim and
    copies what it needs before returning — nothing here outlives the call.

    No-op when ``host_ptr`` is 0, so plugins may call this unconditionally.

    Args:
        host_ptr: the ``HostApi`` pointer handed to the author factory
            (``polyplug_create_<plugin>``) — no host pointer is stored in this
            package.
        level: a :class:`polyplug_abi.LogLevel` value (``Error = 1`` ..
            ``Trace = 5``); the host clamps unknown values to ``Error``.
        scope: short stable tag — use ``"guest.<plugin-name>"`` by convention.
        message: the log message.
    """
    if not host_ptr:
        return
    # ctypes does NOT root Python objects through a StringView's raw `ptr`
    # field, so the encoded bytes objects must be kept alive explicitly. These
    # two locals are the owners: they live until this function returns, which
    # outlives the synchronous host.log call below. (The classic footgun is
    # building the view from a temporary — `bytes_as_view(s.encode("utf-8"))`
    # inline — where the bytes are collected before the call.)
    scope_bytes: bytes = scope.encode("utf-8")
    message_bytes: bytes = message.encode("utf-8")
    scope_view: StringView = bytes_as_view(scope_bytes)
    message_view: StringView = bytes_as_view(message_bytes)
    host: HostApi = HostApi.from_address(host_ptr)
    # Self-passing convention: log(this, level, scope, message). The host reads
    # both views only for the duration of the call; null/empty views are legal.
    host.log(host_ptr, int(level), scope_view, message_view)


def register_contract(
    registrations: List[dict],
    contract: str,
    functions: List[Callable[[object, int, int, int, Callable[[int, int], int]], None]],
    factory: Callable[[int], object],
    plugin_name: Optional[str] = None,
) -> None:
    """Append one contract's registration to ``polyplug_init``'s return list.

    Appends a registration dict — in the exact shape the loader expects — to the
    ``registrations`` list ``polyplug_init`` returns to the loader. Nothing is
    deposited into any module namespace: the loader reads the value
    ``polyplug_init`` returns, not a module attribute. Call this from
    ``polyplug_init`` once per contract the bundle provides, passing the local
    list ``polyplug_init`` will return.

    The loader owns per-instance state: it calls ``factory(host_ptr)`` once per
    ``create_instance`` to build a fresh implementation object, keys it under the
    returned instance handle, and threads it back as the first argument of every
    dispatch callable. No implementation object is stored at module scope, so two
    live instances of the same contract never share state.

    Args:
        registrations: the list ``polyplug_init`` will return to the loader; this
            entry is appended to it.
        contract: canonical contract string ``"<name>@<major>"`` or
            ``"<name>@<major>.<minor>"`` (minor is parsed but does not affect
            the contract id).
        functions: callables ordered by ``fn_id`` — ``functions[0]`` is fn_id 0,
            etc. Each is invoked as ``fn(impl, args_ptr_int: int, out_ptr_int:
            int, arena_ptr_int: int, arena_alloc: Callable[[int, int], int])``
            where ``impl`` is the instance the loader resolved for this call and
            ``arena_alloc`` is the loader-supplied arena allocator (forward it to
            :func:`alloc_string_arena`); return normally on success, raise to
            signal an error.
        factory: the author factory ``factory(host_ptr_int: int) -> impl`` the
            loader calls once per ``create_instance`` (and once at load for the
            stateless default instance) to build a fresh implementation bound to
            its owning runtime's host pointer.
        plugin_name: optional human-readable plugin name; the loader defaults to
            the bundle name when omitted.
    """
    entry: dict = {
        "contract": contract,
        "functions": list(functions),
        "factory": factory,
    }
    if plugin_name is not None:
        entry["plugin_name"] = plugin_name
    registrations.append(entry)


def alloc_string(host_ptr: int, s: str) -> StringView:
    """Allocate a ``StringView`` in HOST memory from a Python string.

    Use this for strings that must OUTLIVE the current call (e.g. data handed to
    a host contract). Cross-boundary data must use the host allocator, so the
    returned bytes live until the host frees them — the guest never frees them.
    For per-call return values, prefer :func:`alloc_string_arena`.

    Args:
        host_ptr: the ``HostApi`` pointer the loader passed to ``polyplug_init``.
        s: the Python string to allocate.

    Returns:
        a ``StringView`` pointing at the host-allocated UTF-8 bytes.
    """
    encoded: bytes = s.encode("utf-8")
    if not encoded:
        return StringView(ptr=None, len=0)
    host: HostApi = HostApi.from_address(host_ptr)
    # The host allocator uses the self-passing convention: alloc(this, size, align).
    # `host.alloc` is the HostApi.alloc CFUNCTYPE field, so it takes the host
    # interface pointer as its first argument; align 1 is valid for byte buffers.
    ptr: int = host.alloc(host_ptr, len(encoded), 1)
    if not ptr:
        raise MemoryError("alloc_string: host allocation failed")
    ctypes.memmove(ptr, encoded, len(encoded))
    return StringView(ptr=ptr, len=len(encoded))


def alloc_string_arena(
    arena_alloc: Callable[[int, int], int], arena_ptr: int, s: str
) -> StringView:
    """Allocate a per-call return ``StringView`` from THIS call's CallArena.

    Use this for strings RETURNED from a contract function: the bytes are served
    from the host's per-call arena and stay valid until the caller's next
    arena-backed call, so the guest never frees them. For data that must outlive
    the call, use :func:`alloc_string` instead.

    The loader passes the arena allocator ``arena_alloc(size: int, arena: int) ->
    int`` as the FINAL positional argument of every dispatch call (nothing is
    injected into the plugin module). The arena pointer is NOT read from any
    shared state: it is the ``arena`` int the dispatch passed to the guest
    callable as its third argument, forwarded here as ``arena_ptr`` and on to
    ``arena_alloc``. The allocator bumps exactly that arena (or falls back to
    ``host->alloc`` when ``arena_ptr`` is 0). Threading both the arena and its
    allocator explicitly — rather than through a shared cell or module global —
    is what makes concurrent and same-thread reentrant dispatch correct: each
    call's arena travels with its own call frame.

    Args:
        arena_alloc: the loader-supplied arena allocator this call received as its
            final dispatch argument.
        arena_ptr: the ``arena`` int this call received as its third argument.
        s: the Python string to allocate.

    Returns:
        a ``StringView`` pointing at the arena-allocated UTF-8 bytes.
    """
    encoded: bytes = s.encode("utf-8")
    if not encoded:
        return StringView(ptr=None, len=0)
    addr: int = arena_alloc(len(encoded), arena_ptr)
    if not addr:
        raise MemoryError("alloc_string_arena: arena allocation failed")
    ctypes.memmove(addr, encoded, len(encoded))
    return StringView(ptr=addr, len=len(encoded))

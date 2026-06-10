"""polyplug_guest — guest-side Python library for polyplug plugin authors.

Python plugins are VM-dispatch plugins (like Lua and JavaScript): the guest
never builds a ``GuestContractInterface`` or registers native function
pointers. Instead the loader executes the plugin module, calls its
``polyplug_init(host_ptr: int, ctx_ptr: int) -> None``, then reads the
module-level ``_polyplug_registrations`` list the guest populated. The loader
wraps each registration in a VM-dispatch interface and registers it with the
runtime itself.

This library provides the registration helper that deposits that list, the
``StringView`` <-> ``str`` codecs, and the two cross-boundary allocators
(host-allocator for data that must outlive the call, arena for per-call return
buffers). It also re-exports the ABI types plugin authors need.

Per-call state (args, out, arena) is passed explicitly through each dispatch
call. The one piece of bundle-lifetime state is the ``HostApi`` pointer: it is
stored once at ``polyplug_init`` time via :func:`store_host_interface` so that
guest→guest peer callers can resolve a peer through the host without threading
the pointer through every ``str``-level impl method — exactly as the Lua, JS,
and C++ guest SDKs do (``get_host_interface`` / ``polyplug::get_host_interface``).
This is module-level storage; it is consistent within a single runtime (one
shared ``HostApi``) and shares the CPython-once-per-process isolation limit the
Python loader already documents. CLAUDE.md Rule 12 governs *host Runtime* state,
not this guest-side SDK accessor.
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
    "store_host_interface",
    "get_host_interface",
    "log",
]

# The module-level attribute the loader reads after polyplug_init. Must match
# `REGISTRATIONS_ATTR` in crates/polyplug_python/src/loader.rs verbatim.
_REGISTRATIONS_ATTR: str = "_polyplug_registrations"

# The HostApi pointer the loader passes to polyplug_init, stored once so peer
# callers can resolve a peer without it being threaded through every call. 0
# until polyplug_init runs. See the module docstring for the isolation scope.
_HOST_INTERFACE_PTR: int = 0


def store_host_interface(host_ptr: int) -> None:
    """Record the ``HostApi`` pointer the loader passed to ``polyplug_init``.

    Call this from the plugin's ``polyplug_init`` (the generated init does so).
    Mirrors the Lua/JS/C++ guest SDKs so guest→guest peer callers can resolve a
    peer through the host without threading the pointer through every call.

    Args:
        host_ptr: the ``HostApi`` pointer received in ``polyplug_init``.
    """
    global _HOST_INTERFACE_PTR
    _HOST_INTERFACE_PTR = host_ptr


def get_host_interface() -> int:
    """Return the stored ``HostApi`` pointer, or 0 if ``polyplug_init`` has not run.

    Peer callers use this as the default host source for ``resolve()``.
    """
    return _HOST_INTERFACE_PTR


def log(level: int, scope: str, message: str) -> None:
    """Send a guest diagnostic to the host's logging funnel (``HostApi.log``).

    Routes to the same sink as ``RuntimeConfig::log``: the host-installed
    callback when one is set, otherwise the host's stderr default (Error/Warn
    visibility only). The host delivers ``(level, scope, message)`` verbatim and
    copies what it needs before returning — nothing here outlives the call.

    No-op until ``polyplug_init`` has stored the host via
    :func:`store_host_interface` (the generated init does so), so plugins may
    call this unconditionally, including from module top level before init.

    Args:
        level: a :class:`polyplug_abi.LogLevel` value (``Error = 1`` ..
            ``Trace = 5``); the host clamps unknown values to ``Error``.
        scope: short stable tag — use ``"guest.<plugin-name>"`` by convention.
        message: the log message.
    """
    host_ptr: int = _HOST_INTERFACE_PTR
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
    module_globals: dict,
    contract: str,
    functions: List[Callable[[int, int, int], None]],
    plugin_name: Optional[str] = None,
) -> None:
    """Register one contract's functions with the polyplug Python loader.

    Appends a registration dict to the caller module's ``_polyplug_registrations``
    list (creating it if absent), in the exact shape the loader expects. Call
    this from the plugin's ``polyplug_init`` (or at module top level) once per
    contract the bundle provides.

    Args:
        module_globals: the plugin module's ``globals()`` — where the loader
            looks for ``_polyplug_registrations``.
        contract: canonical contract string ``"<name>@<major>"`` or
            ``"<name>@<major>.<minor>"`` (minor is parsed but does not affect
            the contract id).
        functions: callables ordered by ``fn_id`` — ``functions[0]`` is fn_id 0,
            etc. Each is invoked as
            ``fn(args_ptr_int: int, out_ptr_int: int, arena_ptr_int: int)``;
            return normally on success, raise to signal an error.
        plugin_name: optional human-readable plugin name; the loader defaults to
            the bundle name when omitted.
    """
    registrations: List[dict] = module_globals.setdefault(_REGISTRATIONS_ATTR, [])
    entry: dict = {
        "contract": contract,
        "functions": list(functions),
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

    The loader injects a module-level
    ``_polyplug_arena_alloc(size: int, arena: int) -> int`` callable into the
    plugin module. The arena pointer is NOT read from any shared state: it is the
    ``arena`` int the dispatch passed to the guest callable as its third argument,
    forwarded here as ``arena_ptr`` and on to the bridge. The bridge bumps exactly
    that arena (or falls back to ``host->alloc`` when ``arena_ptr`` is 0). Threading
    the arena explicitly — rather than through a shared cell — is what makes
    concurrent and same-thread reentrant dispatch correct: each call's arena
    travels with its own call frame.

    Args:
        arena_alloc: the plugin module's ``_polyplug_arena_alloc`` callable.
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

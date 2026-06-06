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

All state is passed explicitly through ``polyplug_init``'s ``host_ptr`` and the
per-call ``arena_ptr`` — there are no module globals or thread-locals (Rule 12).
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
    StringView,
    DispatchType,
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
    "StringView",
    "DispatchType",
    "to_str",
    "register_contract",
    "alloc_string",
    "alloc_string_arena",
]

# The module-level attribute the loader reads after polyplug_init. Must match
# `REGISTRATIONS_ATTR` in crates/polyplug_python/src/loader.rs verbatim.
_REGISTRATIONS_ATTR: str = "_polyplug_registrations"


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


def alloc_string_arena(arena_alloc: Callable[[int], int], s: str) -> StringView:
    """Allocate a per-call return ``StringView`` from the active CallArena.

    Use this for strings RETURNED from a contract function: the bytes are served
    from the host's per-call arena and stay valid until the caller's next
    arena-backed call, so the guest never frees them. For data that must outlive
    the call, use :func:`alloc_string` instead.

    The loader injects a module-level ``_polyplug_arena_alloc(size: int) -> int``
    callable into the plugin module and publishes the active arena before each
    dispatch, so it always targets the arena for the current call (falling back
    to ``host->alloc`` when no arena is active). The plugin passes that callable
    here — the SDK does not bump the raw ``CallArena`` from Python, because the
    bridge already performs the bump and host fallback on the Rust side.

    Args:
        arena_alloc: the plugin module's ``_polyplug_arena_alloc`` callable.
        s: the Python string to allocate.

    Returns:
        a ``StringView`` pointing at the arena-allocated UTF-8 bytes.
    """
    encoded: bytes = s.encode("utf-8")
    if not encoded:
        return StringView(ptr=None, len=0)
    addr: int = arena_alloc(len(encoded))
    if not addr:
        raise MemoryError("alloc_string_arena: arena allocation failed")
    ctypes.memmove(addr, encoded, len(encoded))
    return StringView(ptr=addr, len=len(encoded))

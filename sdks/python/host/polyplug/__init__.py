"""polyplug — host-side Python library for polyplug app developers."""

from __future__ import annotations

from polyplug._native import load_native_lib as _load_native_lib

_native_lib = _load_native_lib()

from polyplug.runtime import PluginGuard, Runtime
from polyplug_abi import ReloadPhase, ReloadPhaseType

__all__ = [
    "PluginGuard",
    "Runtime",
    "ReloadPhase",
    "ReloadPhaseType",
    "load_native_lib",
]


def load_native_lib():
    """Return the loaded native library instance.

    Returns:
        ctypes.CDLL: The loaded libpolyplug instance.
    """
    return _native_lib

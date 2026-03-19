# THIS FILE IS HAND-AUTHORED (part of polyplug host-libs/python)
"""polyplug — host-side Python library for polyplug app developers."""

from __future__ import annotations

# Load native library before importing runtime modules
from polyplug._native import load_native_lib as _load_native_lib

# Pre-load the native library
_native_lib = _load_native_lib()

from polyplug.abi import ReloadPhase, ReloadPhaseType
from polyplug.runtime import Runtime
from polyplug.runtime_config import RuntimeConfig

__all__ = [
    "Runtime",
    "RuntimeConfig",
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

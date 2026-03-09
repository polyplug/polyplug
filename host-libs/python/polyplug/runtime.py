# THIS FILE IS HAND-AUTHORED (part of polyplug host-libs/python)
# PluginRuntime: Python host library for loading polyplug plugins.
from __future__ import annotations

import ctypes
import ctypes.util
from pathlib import Path
from typing import Any

# Module-level ctypes function type cache (argtypes/restype set once at import)
_POLYPLUG_INIT_FN_TYPE = ctypes.CFUNCTYPE(
    ctypes.c_uint32,  # return: ABI version (must equal 1)
)


class PluginRuntime:
    """Host-side runtime for loading polyplug plugin bundles.

    Usage::

        runtime = PluginRuntime()
        runtime.load("path/to/plugin.so")
    """

    def __init__(self) -> None:
        self._handles: list[ctypes.CDLL] = []

    def load(self, path: str | Path) -> None:
        """Load a plugin bundle (.so / .py) at the given path.

        Raises OSError if the library cannot be opened.
        Raises RuntimeError if ABI version is not 1.
        """
        plugin_path: Path = Path(path)
        lib: ctypes.CDLL = ctypes.CDLL(str(plugin_path))

        # Verify ABI version
        abi_version_fn: Any = lib.polyplug_abi_version
        abi_version_fn.argtypes = []
        abi_version_fn.restype = ctypes.c_uint32
        version: int = abi_version_fn()
        if version != 1:
            raise RuntimeError(f"ABI version mismatch: expected 1, found {version}")

        self._handles.append(lib)

    def unload_all(self) -> None:
        """Release all loaded plugin handles."""
        self._handles.clear()

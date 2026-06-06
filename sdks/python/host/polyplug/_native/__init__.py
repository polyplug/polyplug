"""Native library loader for polyplug.

This module handles loading the native libpolyplug shared library.
Resolution order:
  1. Locally staged platform subdirectory (e.g. _native/linux-x64/).
  2. The POLYPLUG_LIB environment variable (absolute path to the .so/.dylib/.dll).
  3. System library paths (LD_LIBRARY_PATH / DYLD_LIBRARY_PATH / PATH).
"""

from __future__ import annotations

import os
import sys
import ctypes
import platform
from typing import Optional

__all__ = ["load_native_lib", "get_native_lib_name", "NativeLibLoader"]


def get_native_lib_name() -> str:
    """Get the native library filename for the current platform.

    Returns:
        The platform-specific library filename.

    Raises:
        RuntimeError: If the current platform is not supported.
    """
    if sys.platform == "linux":
        return "libpolyplug.so"
    elif sys.platform == "darwin":
        return "libpolyplug.dylib"
    elif sys.platform == "win32":
        return "polyplug.dll"
    else:
        raise RuntimeError(f"Unsupported platform: {sys.platform}")


def get_platform_identifier() -> str:
    """Get the platform identifier for GitHub Releases downloads.

    Returns:
        Platform identifier string (e.g., 'linux-x64', 'macos-arm64').

    Raises:
        RuntimeError: If the current platform/architecture is not supported.
    """
    machine = platform.machine().lower()

    if sys.platform == "linux":
        if machine in ("x86_64", "amd64"):
            return "linux-x64"
        elif machine == "aarch64":
            return "linux-arm64"
        else:
            raise RuntimeError(f"Unsupported Linux architecture: {machine}")
    elif sys.platform == "darwin":
        if machine == "arm64":
            return "macos-arm64"
        elif machine in ("x86_64", "amd64"):
            return "macos-x64"
        else:
            raise RuntimeError(f"Unsupported macOS architecture: {machine}")
    elif sys.platform == "win32":
        if machine in ("x86_64", "amd64"):
            return "windows-x64"
        else:
            raise RuntimeError(f"Unsupported Windows architecture: {machine}")
    else:
        raise RuntimeError(f"Unsupported platform: {sys.platform}")


class NativeLibLoader:
    """Manages loading of the native polyplug library.

    This class attempts to load the native library from multiple locations:
    1. Embedded _native/ directory (preferred)
    2. System library paths
    3. POLYPLUG_LIB environment variable

    Attributes:
        lib: The loaded ctypes.CDLL instance, or None if not loaded.
        load_path: The path from which the library was loaded, or None.
    """

    def __init__(self) -> None:
        self.lib: Optional[ctypes.CDLL] = None
        self.load_path: Optional[str] = None

    def load(self) -> ctypes.CDLL:
        """Load the native library.

        Returns:
            The loaded ctypes.CDLL instance.

        Raises:
            RuntimeError: If the library cannot be loaded from any location.
        """
        if self.lib is not None:
            return self.lib

        lib_name = get_native_lib_name()
        platform_id = get_platform_identifier()

        embedded_path = os.path.join(os.path.dirname(__file__), platform_id, lib_name)

        if os.path.exists(embedded_path):
            self.lib = ctypes.CDLL(embedded_path)
            self.load_path = embedded_path
            return self.lib

        env_path = os.environ.get("POLYPLUG_LIB")
        if env_path and os.path.exists(env_path):
            self.lib = ctypes.CDLL(env_path)
            self.load_path = env_path
            return self.lib

        try:
            self.lib = ctypes.CDLL(lib_name)
            self.load_path = lib_name
            return self.lib
        except OSError:
            pass

        raise RuntimeError(
            f"Failed to load native library '{lib_name}'. "
            f"Tried: {embedded_path}"
            f"{', POLYPLUG_LIB=' + env_path if env_path else ''}"
            f", system paths. "
            f"Ensure the library is installed or set POLYPLUG_LIB."
        )


_loader: Optional[NativeLibLoader] = None


def load_native_lib() -> ctypes.CDLL:
    """Load the native polyplug library.

    This function attempts to load the native library from:
    1. The embedded _native/ directory (preferred)
    2. The POLYPLUG_LIB environment variable
    3. System library paths

    Returns:
        The loaded ctypes.CDLL instance.

    Raises:
        RuntimeError: If the library cannot be loaded from any location.
    """
    global _loader
    if _loader is None:
        _loader = NativeLibLoader()
    return _loader.load()

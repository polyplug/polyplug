# THIS FILE IS HAND-AUTHORED (part of polyplug host-libs/python)
"""Loader registration for polyplug non-native guests."""

from __future__ import annotations

import ctypes
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from polyplug.runtime import Runtime

_loader_libs: dict[str, ctypes.CDLL] = {}


def _get_loader_lib(name: str) -> ctypes.CDLL:
    if name not in _loader_libs:
        _loader_libs[name] = ctypes.CDLL(f"lib{name}.so")
    return _loader_libs[name]


def _register(
    runtime: "Runtime", lib_name: str, create_fn: str, config_ptr: ctypes.Structure
) -> None:
    lib: ctypes.CDLL = _get_loader_lib(lib_name)
    create: ctypes._NamedFuncPtr = getattr(lib, create_fn)
    create.restype = ctypes.c_void_p
    create.argtypes = [ctypes.c_void_p]
    loader_ptr: int | None = create(config_ptr)
    if not loader_ptr:
        raise RuntimeError(f"polyplug: {lib_name} loader create failed")
    polyplug_lib: ctypes.CDLL = runtime._lib
    register: ctypes._NamedFuncPtr = polyplug_lib.polyplug_runtime_register_loader
    register.restype = ctypes.c_uint32
    register.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
    err: int = int(register(runtime._runtime, ctypes.c_void_p(loader_ptr)))
    if err != 0:
        raise RuntimeError(f"polyplug: {lib_name} loader register failed: {err}")


class _DotnetConfig(ctypes.Structure):
    _fields_ = [
        ("min_framework_ptr", ctypes.c_char_p),
        ("min_framework_len", ctypes.c_size_t),
    ]


class _PythonConfig(ctypes.Structure):
    _fields_ = [
        ("min_version_ptr", ctypes.c_char_p),
        ("min_version_len", ctypes.c_size_t),
    ]


class _EmptyConfig(ctypes.Structure):
    _fields_ = [("_reserved", ctypes.c_uint8)]


def register_dotnet_loader(runtime: "Runtime", min_framework: str = "10.0") -> None:
    b: bytes = min_framework.encode("utf-8")
    cfg: _DotnetConfig = _DotnetConfig(b, len(b))
    _register(
        runtime, "polyplug_dotnet", "polyplug_dotnet_loader_create", ctypes.byref(cfg)
    )


def register_python_loader(runtime: "Runtime", min_version: str = "3.11") -> None:
    b: bytes = min_version.encode("utf-8")
    cfg: _PythonConfig = _PythonConfig(b, len(b))
    _register(
        runtime, "polyplug_python", "polyplug_python_loader_create", ctypes.byref(cfg)
    )


def register_lua_loader(runtime: "Runtime") -> None:
    cfg: _EmptyConfig = _EmptyConfig(0)
    _register(runtime, "polyplug_lua", "polyplug_lua_loader_create", ctypes.byref(cfg))


def register_js_loader(runtime: "Runtime") -> None:
    cfg: _EmptyConfig = _EmptyConfig(0)
    _register(runtime, "polyplug_js", "polyplug_js_loader_create", ctypes.byref(cfg))


class _NativeConfig(ctypes.Structure):
    _fields_ = [("_reserved", ctypes.c_uint8)]


def register_native_loader(runtime: "Runtime") -> None:
    cfg: _NativeConfig = _NativeConfig(0)
    _register(
        runtime, "polyplug_native", "polyplug_native_loader_create", ctypes.byref(cfg)
    )

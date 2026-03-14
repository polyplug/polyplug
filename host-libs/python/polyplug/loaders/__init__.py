"""Polyplug loader registration modules."""

from .native import register_native_loader
from .dotnet import register_dotnet_loader
from .python import register_python_loader
from .lua import register_lua_loader
from .js import register_js_loader
from .js_deno import register_js_deno_loader

__all__ = [
    "register_native_loader",
    "register_dotnet_loader",
    "register_python_loader",
    "register_lua_loader",
    "register_js_loader",
    "register_js_deno_loader",
]

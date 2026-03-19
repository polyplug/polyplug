# polyplug-loaders-lua

Lua loader for polyplug - loads LuaJIT plugins.

## Installation

```bash
pip install polyplug-loaders-lua
```

## Usage

```python
from polyplug_loaders_lua import LuaLoader

loader = LuaLoader()
# Load plugins written in Lua (LuaJIT)
```

## Description

This loader handles plugins written in Lua, specifically targeting LuaJIT for high-performance FFI integration with the polyplug runtime.
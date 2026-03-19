# polyplug-guest

Guest library for writing polyplug plugins in Python.

## Installation

```bash
pip install polyplug-guest
```

## Usage

```python
from polyplug_guest import Plugin, export

@export
def my_function(arg: str) -> int:
    """A function exported to the host runtime."""
    return len(arg)

class MyPlugin(Plugin):
    """A polyplug plugin implemented in Python."""
    pass
```

## Description

This library provides the guest-side API for writing polyplug plugins in Python. Use this when you want to implement a plugin that will be loaded by a Python host application using the polyplug runtime.

## Features

- Decorator-based exports
- Type-safe ABI bindings
- Automatic vtable generation
- Memory management helpers
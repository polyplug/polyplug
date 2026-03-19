# polyplug-loaders-python

Python loader for polyplug - loads Python plugins via the Python C API.

## Installation

```bash
pip install polyplug-loaders-python
```

## Usage

```python
from polyplug_loaders_python import PythonLoader

loader = PythonLoader()
# Load plugins written in Python
```

## Description

This loader handles plugins written in Python, using the Python C API to bridge between the polyplug runtime and Python code.
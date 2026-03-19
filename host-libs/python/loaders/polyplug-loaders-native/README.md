# polyplug-loaders-native

Native loader for polyplug - loads native C ABI plugins (.so/.dll).

## Installation

```bash
pip install polyplug-loaders-native
```

## Usage

```python
from polyplug_loaders_native import NativeLoader

loader = NativeLoader()
# Load native plugins compiled to shared libraries
```

## Description

This loader handles native plugins compiled as shared libraries (.so on Linux, .dll on Windows, .dylib on macOS). These plugins implement the polyplug C ABI directly.
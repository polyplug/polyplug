# polyplug-loaders-js-deno

JS Deno (V8) loader for polyplug - loads Deno/V8 plugins.

## Installation

```bash
pip install polyplug-loaders-js-deno
```

## Usage

```python
from polyplug_loaders_js_deno import DenoLoader

loader = DenoLoader()
# Load plugins written in JavaScript/TypeScript (Deno runtime)
```

## Description

This loader handles plugins written in JavaScript or TypeScript, using the Deno runtime (V8 engine) for modern JS/TS execution with full async support.
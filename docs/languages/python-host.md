# Python — Host (app)

A Python host embeds the polyplug runtime, registers the loaders it needs, scans
a plugin directory, and calls guest contracts through generated typed callers.
The Python host in `examples/hosts/python/` (`main.py`) is the reference
implementation for this guide.

See the [Python overview](python.md) for install instructions.

---

## 1. Install packages

```bash
pip install polyplug polyplug-abi

# Add loaders for each guest language you want to support:
pip install polyplug-loaders-native    # native cdylib (Rust, C++, C#)
pip install polyplug-loaders-python    # Python guests
pip install polyplug-loaders-lua       # Lua guests
pip install polyplug-loaders-js        # JavaScript (QuickJS) guests
pip install polyplug-loaders-dotnet    # .NET guests

# Install the CLI to generate host callers
pip install polyplugc                  # or: uv tool install polyplugc
```

---

## 2. Generate host callers

Given an `api.toml` contract definition, generate the typed Python callers:

```bash
polyplugc generate --api api.toml --lang python --out host/generated
```

This writes the following files under `host/generated/`:

```
host/generated/
└── host/
    ├── callers.py            typed caller classes + contract ID constants
    ├── callers.pyi           type stubs
    ├── contracts.py          host-contract base classes (if api.toml defines host contracts)
    ├── contracts.pyi
    ├── interface_factories.py  host-contract interface factory helpers
    ├── types.py              generated enums and structs
    └── types.pyi
```

Never edit these files — regenerate with the same command when `api.toml` changes.

The `generated/` directory must be importable as a package root. Place it at
your script's level and add it to `sys.path` before importing from it:

```python
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent / "generated"))

from host.callers import PipelineDecoderContractCaller, PIPELINE_DECODER_CONTRACT_ID
from host.types import LogLevel
```

---

## 3. Build the runtime

```python
from polyplug import Runtime, ReloadPhase
from polyplug_abi import RuntimeConfig

config = RuntimeConfig()
config.hot_reload_enabled = True   # set False to disable reload callbacks

rt = Runtime(config=config)
```

Pass an `on_reload` callback to be notified of hot-reload events:

```python
def handle_reload(phase: ReloadPhase) -> None:
    if phase.is_preparing():
        print(f"[HOT-RELOAD] Preparing: {phase.bundle_name}")
    elif phase.is_reloaded():
        print(f"[HOT-RELOAD] Reloaded:  {phase.bundle_name}")
    elif phase.is_failed():
        print(f"[HOT-RELOAD] Failed:    {phase.bundle_name} — {phase.reason}")

rt = Runtime(config=config, on_reload=handle_reload)
```

---

## 4. Register loaders

Import each loader package and register it with the runtime. Use guarded imports
so the host still runs when a loader package is absent:

```python
try:
    from polyplug_loaders_native import register_native_loader
except ImportError:
    register_native_loader = None

try:
    from polyplug_loaders_python import register_python_loader
except ImportError:
    register_python_loader = None

try:
    from polyplug_loaders_lua import register_lua_loader
except ImportError:
    register_lua_loader = None

try:
    from polyplug_loaders_js import register_js_loader
except ImportError:
    register_js_loader = None

try:
    from polyplug_loaders_dotnet import register_dotnet_loader
except ImportError:
    register_dotnet_loader = None

loaders = [
    ("native",    register_native_loader),
    ("python",    register_python_loader),
    ("lua",       register_lua_loader),
    ("js-quickjs",register_js_loader),
    ("dotnet",    register_dotnet_loader),
]
for name, register in loaders:
    if register is None:
        continue
    try:
        register(rt)
    except RuntimeError as e:
        print(f"  loader {name} unavailable: {e}", file=sys.stderr)
```

---

## 5. Register host contracts (optional)

If your `api.toml` defines host contracts — services the host provides to plugins
— implement the generated base class and register it through the factory. The
factory requires `polyplug_loaders_python` because Python's `ctypes` cannot
create struct-returning callbacks on its own; the loader cdylib provides the
bridge trampolines.

```python
from host.contracts import HostLogger
from host.interface_factories import create_host_logger_interface
from host.types import LogLevel

try:
    from polyplug_loaders_python import bridge_lib as python_bridge_lib
except ImportError:
    python_bridge_lib = None

class ConsoleLogger(HostLogger):
    def log(self, message: str) -> None:
        print(f"[plugin] {message}")

    def log_with_level(self, level: LogLevel, message: str) -> None:
        print(f"[plugin][{level.name}] {message}")

if python_bridge_lib is not None:
    logger_iface = create_host_logger_interface(ConsoleLogger(), python_bridge_lib())
    rt.register_host_contract(logger_iface)
```

---

## 6. Load bundles

Use `polyplug.scanner` to discover assembled bundles in a directory, then call
`rt.load_bundle` for each one:

```python
from polyplug import scanner

bundles = scanner.scan_dir("plugins/")   # returns List[Tuple[str, Manifest]]
for path, manifest in bundles:
    try:
        rt.load_bundle(path)
        print(f"  loaded: {manifest.name}")
    except RuntimeError as e:
        print(f"  failed: {manifest.name} — {e}", file=sys.stderr)
```

`scanner.scan_dir` walks a flat directory of bundle folders, each containing a
`manifest.toml`, and returns `(bundle_path, Manifest)` pairs.

---

## 7. Resolve and call a contract

```python
import ctypes
from polyplug_abi import StringView, to_str

def str_view(s: str, keepalive: list) -> StringView:
    """Build a StringView over a UTF-8 buffer kept alive in `keepalive`."""
    data: bytes = s.encode("utf-8")
    buf = ctypes.create_string_buffer(data, len(data))
    keepalive.append(buf)
    return StringView(ptr=ctypes.cast(buf, ctypes.c_void_p), len=len(data))

# find_guest_contract returns GuestContractHandle {index: u32, generation: u32}.
# The null/missing handle has index == 0xFFFFFFFF.
handle = rt.find_guest_contract(PIPELINE_DECODER_CONTRACT_ID, 0)
if handle.index == 0xFFFFFFFF:
    print("decoder not found")
else:
    caller = PipelineDecoderContractCaller.create(handle, rt._ensure_host(), owner=rt)
    keepalive: list = []
    result_sv: StringView = caller.decode(str_view("name,value,42", keepalive))
    print(to_str(result_sv))   # DECODED:name|value|42
```

The example host wraps this pattern in a `make_caller` helper
(see `examples/hosts/python/main.py`) that returns `None` when the contract is
unavailable:

```python
def make_caller(caller_cls, rt, contract_id: int):
    handle = rt.find_guest_contract(contract_id, 0)
    if handle.index == 0xFFFFFFFF:
        return None
    return caller_cls.create(handle, rt._ensure_host(), owner=rt)

decoder = make_caller(PipelineDecoderContractCaller, rt, PIPELINE_DECODER_CONTRACT_ID)
if decoder:
    print(to_str(decoder.decode(str_view("name,value,42", keepalive))))
```

---

## Known limitations

**CPython initializes once per process.** Multiple `Runtime` instances in the
same process share the underlying CPython interpreter. Python plugins loaded
by different runtimes can see each other's modules and state. For full
isolation, use separate processes.

**Python guests do not support hot-reload.** `rt.reload_bundle` on a Python
bundle returns `RuntimeError::HotReloadDisabled`. Native (cdylib), Lua, and
JavaScript (QuickJS) bundles do support hot-reload.

---

## Full example

`examples/hosts/python/main.py` — entry point `main.py`. Loaders registered:
native, Python, Lua, JavaScript (QuickJS), .NET.

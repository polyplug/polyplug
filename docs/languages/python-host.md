# Python — Host (app)

Embed the polyplug runtime in a Python application, load plugins written in any
supported language, and call their contracts through generated typed callers.

See also: [Python overview](python.md) · [Python — Guest (plugin)](python-guest.md) ·
[glossary](../glossary.md)

## 1. Install

Install the CLI, the runtime, and a loader per guest language you want to support:

```bash
uv tool install polyplugc      # or: pipx install polyplugc / pip install polyplugc
pip install polyplug polyplug-abi
pip install polyplug-loaders-native    # native (.so / .dylib / .dll) bundles
pip install polyplug-loaders-python    # Python bundles
pip install polyplug-loaders-lua       # Lua bundles
pip install polyplug-loaders-js        # JavaScript (QuickJS) bundles
pip install polyplug-loaders-dotnet    # .NET / C# bundles
```

A Python host can load guests written in any supported language — register the
matching loader when you build the runtime.

## 2. Generate host callers

Author or obtain the shared `api.toml` contract (see `examples/api.toml`), then
generate the typed callers. Re-run whenever the contract changes.

```bash
polyplugc generate --api api.toml --lang python --out host/generated
```

This writes `host/generated/host/` with the typed caller classes
(`{Ns}{Type}ContractCaller`), contract-ID constants, host-contract base classes,
interface factories, and generated types. Never edit these files. For the emitted
symbol names, see [Generated names](../generated-names.md).

The `generated/` directory must be importable as a package root — add it to
`sys.path` before importing from it:

```python
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent / "generated"))

from host.callers import PipelineDecoderContractCaller, PIPELINE_DECODER_CONTRACT_ID
from host.types import LogLevel
```

## 3. Build the runtime

```python
from polyplug import Runtime
from polyplug_abi import RuntimeConfig

config = RuntimeConfig()
config.hot_reload_enabled = True

rt = Runtime(config=config)
```

Register one loader per guest language. Import each loader package and register
it:

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
    ("native", register_native_loader),
    ("python", register_python_loader),
    ("lua", register_lua_loader),
    ("js-quickjs", register_js_loader),
    ("dotnet", register_dotnet_loader),
]
for name, register in loaders:
    if register is None:
        continue
    try:
        register(rt)
    except RuntimeError as e:
        print(f"  loader {name} unavailable: {e}", file=sys.stderr)
```

The full multi-loader host is `examples/hosts/python/main.py`.

### Hot-reload callback (optional)

Pass `on_reload` to observe reload phases. Hot-reload applies to native, Lua, and
JS bundles, not Python — see [Reload limitations](../RELOAD_LIMITATIONS.md).

```python
from polyplug import ReloadPhase

def handle_reload(phase: ReloadPhase) -> None:
    if phase.is_preparing():
        print(f"[reload] preparing: {phase.bundle_name}")
    elif phase.is_reloaded():
        print(f"[reload] reloaded:  {phase.bundle_name}")
    elif phase.is_failed():
        print(f"[reload] failed:    {phase.bundle_name} — {phase.reason}")

rt = Runtime(config=config, on_reload=handle_reload)
```

### Signature policy (optional)

```python
from polyplug_abi import SignaturePolicy

config.signature_policy = SignaturePolicy.Required.value
```

`Required` rejects unsigned or tampered bundles. See the
[Trust Model](../TRUST_MODEL.md).

## 4. Register a host contract (optional)

If your `api.toml` defines a host contract (a service the host provides to
plugins), register it before loading bundles. The factory needs
`polyplug_loaders_python`'s `bridge_lib` — install that loader to register host
contracts from Python.

```python
from polyplug_loaders_python import bridge_lib as python_bridge_lib
from host.contracts import HostLogger
from host.interface_factories import create_host_logger_interface
from host.types import LogLevel

class ConsoleLogger(HostLogger):
    def log(self, message: str) -> None:
        print(f"[plugin] {message}")

    def log_with_level(self, level: LogLevel, message: str) -> None:
        print(f"[plugin][{level.name}] {message}")

logger_iface = create_host_logger_interface(ConsoleLogger(), python_bridge_lib())
rt.register_host_contract(logger_iface)
```

## 5. Load bundles

```python
from polyplug import scanner

for path, manifest in scanner.scan_dir("plugins/"):
    try:
        rt.load_bundle(path)
        print(f"  loaded: {manifest.name}")
    except RuntimeError as e:
        print(f"  failed: {manifest.name} — {e}", file=sys.stderr)
```

`scan_dir` returns a `(bundle_path, Manifest)` pair for every `manifest.toml`
under the directory; `load_bundle` dispatches to the loader named in the
manifest.

## 6. Call a contract

Pass strings as `StringView` over a buffer you keep alive for the call:

```python
import ctypes
from polyplug_abi import StringView, to_str

def str_view(s: str, keepalive: list) -> StringView:
    data: bytes = s.encode("utf-8")
    buf = ctypes.create_string_buffer(data, len(data))
    keepalive.append(buf)
    return StringView(ptr=ctypes.cast(buf, ctypes.c_void_p), len=len(data))

handle = rt.find_guest_contract(PIPELINE_DECODER_CONTRACT_ID, 0)
if handle.index == 0xFFFFFFFF:          # null/missing handle
    raise RuntimeError("decoder not found")

caller = PipelineDecoderContractCaller.create(handle, rt.host, owner=rt)
keepalive: list = []
result: StringView = caller.decode(str_view("name,value,42", keepalive))
print(to_str(result))   # DECODED:name|value|42
```

The second argument to `find_guest_contract` is the minimum version to accept;
pass `0` for any version.

Python bundles do not hot-reload — see [Reload limitations](../RELOAD_LIMITATIONS.md).

## Full reference

`examples/hosts/python/main.py` registers all five loaders, a host contract, scans a
directory, loads every bundle, and runs a five-stage pipeline end to end.
Generated callers live at `examples/hosts/python/generated/`.

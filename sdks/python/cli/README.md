# polyplugc — Python CLI package

`polyplugc` is the contract code-generator for the
[polyplug](https://github.com/polyplug/polyplug) plugin runtime.
This package installs a **platform wheel that already contains the prebuilt
binary** — fully offline, no download at install time or runtime.

## Installation

```sh
pip install polyplugc
# or
uv tool install polyplugc
```

After installation `polyplugc` is on your `PATH`:

```sh
polyplugc generate --bundle my_plugin.toml --lang rust --out src/generated
```

## Supported platforms

| Platform | Wheel tag |
|---|---|
| Linux x86-64 | `manylinux2014_x86_64` |
| macOS Apple Silicon | `macosx_11_0_arm64` |
| Windows x86-64 | `win_amd64` |

The binary is bundled inside the wheel — there is no runtime download.
Installing on an unsupported platform produces a clear error that points to
`cargo install polyplugc` and the GitHub Releases page.

## CI build commands

CI injects the correct prebuilt binary into `polyplugc/_bin/` before
building the wheel.  The `has_ext_modules = True` trick in `setup.py`
ensures the wheel is tagged as platform-specific rather than `py3-none-any`,
so each platform produces a distinct, correctly-tagged wheel.

### Linux x86-64

```sh
# Inject binary
cp path/to/linux-x64/polyplugc sdks/python/cli/polyplugc/_bin/polyplugc
chmod +x sdks/python/cli/polyplugc/_bin/polyplugc

# Build wheel with explicit manylinux platform tag
cd sdks/python/cli
python setup.py bdist_wheel --plat-name manylinux2014_x86_64
# Wheel: dist/polyplugc-0.1.3-cp3XX-cp3XX-manylinux2014_x86_64.whl
```

### macOS Apple Silicon (arm64)

```sh
cp path/to/macos-arm64/polyplugc sdks/python/cli/polyplugc/_bin/polyplugc
chmod +x sdks/python/cli/polyplugc/_bin/polyplugc

cd sdks/python/cli
python setup.py bdist_wheel --plat-name macosx_11_0_arm64
# Wheel: dist/polyplugc-0.1.3-cp3XX-cp3XX-macosx_11_0_arm64.whl
```

### Windows x86-64

```cmd
copy path\to\windows-x64\polyplugc.exe sdks\python\cli\polyplugc\_bin\polyplugc.exe

cd sdks\python\cli
python setup.py bdist_wheel --plat-name win_amd64
REM Wheel: dist/polyplugc-0.1.3-cp3XX-cp3XX-win_amd64.whl
```

### Why `python setup.py bdist_wheel --plat-name` instead of `python -m build`?

`python -m build --wheel` reads the platform from the running host and
cannot be overridden.  When a Linux CI runner builds the macOS or Windows
wheel it must pass `--plat-name` explicitly, which requires the legacy
`setup.py bdist_wheel` interface.  Both paths produce identical wheel
contents; only the tag differs.

For a native build (running host matches the target), `python -m build
--wheel` also works — the `has_ext_modules` override ensures the correct
platform tag is emitted automatically.

## Where CI injects the binary

| Platform | Source artifact path | Destination in package |
|---|---|---|
| Linux x86-64 | `polyplugc` (ELF) | `polyplugc/_bin/polyplugc` |
| macOS arm64 | `polyplugc` (Mach-O) | `polyplugc/_bin/polyplugc` |
| Windows x86-64 | `polyplugc.exe` (PE) | `polyplugc/_bin/polyplugc.exe` |

The binary is located at runtime via `pathlib.Path(__file__).parent / "_bin" /
"polyplugc[.exe]"` — no environment variables or registry lookups required.

## Building from source

```sh
cargo install polyplugc
```

Or download a release binary from
<https://github.com/polyplug/polyplug/releases>.

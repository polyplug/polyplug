"""sdks/python/host/tests/test_host_contract_runtime.py

REAL-runtime host-contract test (mirrors sdks/lua/host/tests/test_reload_runtime.lua).

`test_runtime_config_c.py` covers SDK-side ctypes types only — it can never
catch a broken FFI path. This test drives the actual flow: a Python host
creates a runtime through the SDK, registers the `host.logger` contract via
the GENERATED interface factory (which routes dispatch through the native
trampoline `polyplug_python_host_vm_dispatch` exported by the python loader
cdylib — ctypes cannot create struct-returning callbacks), loads the REAL
rust `reporter` example plugin through the native loader, and dispatches its
`report` function. The plugin resolves `host.logger` across the C ABI and
calls back into this Python implementation — including `log_with_level`,
whose first parameter is the `LogLevel` ENUM (proving the repr-ctype enum
marshalling at runtime, not just at the generated-string level).

Skip-honestly policy (matches sdks/lua/host/tests/test_reload_runtime.lua):
when the required environment is absent the test FAILS LOUDLY with
instructions — a runtime test that silently passes hides exactly the
never-run breakage class it exists to catch.

Run from repo root:
  cargo build --release -p polyplug -p polyplug_native -p polyplug_python
  bash examples/build_all.sh
  POLYPLUG_LIB=$PWD/target/release/libpolyplug.so \
  POLYPLUG_NATIVE_LIB=$PWD/target/release/libpolyplug_native.so \
  POLYPLUG_PYTHON_LIB=$PWD/target/release/libpolyplug_python.so \
  python3 sdks/python/host/tests/test_host_contract_runtime.py

Or via the justfile recipe: just test-host-python
"""

from __future__ import annotations

import ctypes
import os
import sys
from pathlib import Path

# ─── Path setup ───────────────────────────────────────────────────────────────
# Resolve everything from this script's own directory (sdks/python/host/tests/),
# mirroring the PYTHON_HOST_PATH used by examples/verify_hosts.sh.
_TESTS_DIR: Path = Path(__file__).resolve().parent
_HOST_DIR: Path = _TESTS_DIR.parent  # sdks/python/host
_PYTHON_SDK_DIR: Path = _HOST_DIR.parent  # sdks/python
_REPO_ROOT: Path = _PYTHON_SDK_DIR.parent.parent  # repo root

sys.path.insert(0, str(_HOST_DIR))
sys.path.insert(0, str(_PYTHON_SDK_DIR / "polyplug_abi"))
sys.path.insert(0, str(_PYTHON_SDK_DIR))
sys.path.insert(0, str(_PYTHON_SDK_DIR / "loaders" / "native"))
# Generated host bindings for examples/api.toml (host.logger factory + callers).
sys.path.insert(0, str(_REPO_ROOT / "examples" / "hosts" / "python" / "generated"))

# ─── Skip-honestly: a runtime test must never silently pass ───────────────────
_MISSING: list[str] = [
    name
    for name in ("POLYPLUG_LIB", "POLYPLUG_NATIVE_LIB", "POLYPLUG_PYTHON_LIB")
    if not os.environ.get(name)
]
if _MISSING:
    sys.stderr.write(
        "FATAL: "
        + ", ".join(_MISSING)
        + " not set — this runtime test must not silently pass.\n"
        "Build the core and point the test at it:\n"
        "  cargo build --release -p polyplug -p polyplug_native -p polyplug_python\n"
        "  export POLYPLUG_LIB=$PWD/target/release/libpolyplug.so\n"
        "  export POLYPLUG_NATIVE_LIB=$PWD/target/release/libpolyplug_native.so\n"
        "  export POLYPLUG_PYTHON_LIB=$PWD/target/release/libpolyplug_python.so\n"
    )
    sys.exit(1)

_REPORTER_BUNDLE: Path = _REPO_ROOT / "examples" / "plugins" / "rust_reporter"
if not _REPORTER_BUNDLE.is_dir():
    sys.stderr.write(
        f"FATAL: reporter example bundle missing: {_REPORTER_BUNDLE}\n"
        "This runtime test drives the REAL rust reporter plugin — build it first:\n"
        "  bash examples/build_all.sh\n"
    )
    sys.exit(1)

from polyplug import Runtime  # noqa: E402  (sys.path setup above)
from polyplug_abi import StringView, to_str  # noqa: E402
from polyplug_loaders_native import register_native_loader  # noqa: E402
from host.contracts import HostLogger  # noqa: E402
from host.interface_factories import create_host_logger_interface  # noqa: E402
from host.types import LogLevel  # noqa: E402
from host.callers import (  # noqa: E402
    DataReporterContractCaller,
)


def _str_view(s: str, keepalive: list) -> StringView:
    """Build a StringView over a UTF-8 buffer kept alive via `keepalive`
    (mirrors examples/hosts/python/main.py)."""
    data: bytes = s.encode("utf-8")
    buf: ctypes.Array = ctypes.create_string_buffer(data, len(data))
    keepalive.append(buf)
    return StringView(ptr=ctypes.cast(buf, ctypes.c_void_p), len=len(data))


class CapturingLogger(HostLogger):
    """Records every call the plugin makes across the C ABI."""

    def __init__(self) -> None:
        self.entries: list[tuple[str, LogLevel | None, str]] = []

    def log(self, message: str) -> None:
        self.entries.append(("log", None, message))

    def log_with_level(self, level: LogLevel, message: str) -> None:
        self.entries.append(("log_with_level", level, message))


def main() -> None:
    rt = Runtime()
    register_native_loader(rt)

    # Register host.logger through the GENERATED factory: dispatch routes
    # through the python loader cdylib's native trampoline into the scalar
    # ctypes dispatcher built by the factory.
    logger = CapturingLogger()
    bridge_lib: ctypes.CDLL = ctypes.CDLL(os.environ["POLYPLUG_PYTHON_LIB"])
    rt.register_host_contract(create_host_logger_interface(logger, bridge_lib))

    rt.load_bundle(_REPORTER_BUNDLE)

    caller = DataReporterContractCaller.create(rt)
    assert caller is not None, "data.Reporter contract must be registered"

    report_input: str = "TRANSFORMED:NAME|value (transformed)|43"
    keepalive: list = []
    result_sv: StringView = caller.report(_str_view(report_input, keepalive))
    result: str = to_str(result_sv)
    assert result == "Report: NAME has value 'value (transformed)' with count 43", (
        f"unexpected report output: {result!r}"
    )

    # The plugin must have called back into the host across the ABI: one plain
    # log plus four log_with_level calls with REAL LogLevel enum values.
    expected: list[tuple[str, LogLevel | None, str]] = [
        ("log", None, f"[plugin] Starting report for: {report_input}"),
        ("log_with_level", LogLevel.INFO, "[plugin] Step 1: Parsing input"),
        ("log_with_level", LogLevel.DEBUG, f"[plugin] Input length: {len(report_input)}"),
        ("log_with_level", LogLevel.WARN, "[plugin] Step 2: Processing data"),
        ("log_with_level", LogLevel.ERROR, "[plugin] Step 3: Finalizing report"),
    ]
    assert logger.entries == expected, (
        f"host.logger callbacks mismatch:\n  got:      {logger.entries}\n"
        f"  expected: {expected}"
    )

    # Enum params must arrive as the generated IntEnum, not a bare int —
    # the dispatcher wraps the raw repr u32 back into LogLevel.
    levels: list[LogLevel | None] = [entry[1] for entry in logger.entries[1:]]
    assert all(isinstance(level, LogLevel) for level in levels), (
        f"levels must be LogLevel instances: {[type(level) for level in levels]}"
    )

    print("PASS: python host registered host.logger via generated factory")
    print("PASS: rust reporter plugin called log() across the ABI")
    print("PASS: log_with_level() delivered REAL LogLevel enum values:")
    for kind, level, message in logger.entries:
        level_str: str = level.name if isinstance(level, LogLevel) else "-"
        print(f"  [{kind}][{level_str}] {message}")
    print("test_host_contract_runtime: all assertions passed")


if __name__ == "__main__":
    main()

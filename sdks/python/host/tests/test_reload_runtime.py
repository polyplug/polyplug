"""sdks/python/host/tests/test_reload_runtime.py

REAL-runtime hot-reload notification test (mirrors
sdks/lua/host/tests/test_reload_runtime.lua and
sdks/js/host/tests/reload_runtime_test.ts).

`test_runtime_config_c.py` covers the SDK-side RuntimeConfig/ReloadPhase ctypes
types only — it builds local structs and asserts on them, which can never catch
a broken FFI path. This test drives the actual flow: create a runtime through
the Python host SDK with an `on_reload` callback (a real ctypes CFUNCTYPE for
the `void(*)(void*, const ReloadPhase*)` ABI signature), load the native reload
fixture bundle, trigger a reload through the runtime, and assert the callback
fired with REAL phase data delivered across the C ABI.

Skip-honestly policy (matches sdks/lua/host/tests/test_reload_runtime.lua):
when POLYPLUG_LIB / POLYPLUG_NATIVE_LIB are absent the test FAILS LOUDLY with
instructions — a runtime test that silently passes hides exactly the never-run
breakage class it exists to catch.

Run from repo root:
  cargo build --release -p polyplug -p polyplug_native
  bash tests/fixtures/build_all.sh
  POLYPLUG_LIB=$PWD/target/release/libpolyplug.so \
  POLYPLUG_NATIVE_LIB=$PWD/target/release/libpolyplug_native.so \
  python3 sdks/python/host/tests/test_reload_runtime.py

Or via the justfile recipe: just test-host-python
"""

from __future__ import annotations

import os
import sys
from pathlib import Path

# ─── Path setup ───────────────────────────────────────────────────────────────
# Resolve everything from this script's own directory (sdks/python/host/tests/),
# mirroring test_host_contract_runtime.py.
_TESTS_DIR: Path = Path(__file__).resolve().parent
_HOST_DIR: Path = _TESTS_DIR.parent  # sdks/python/host
_PYTHON_SDK_DIR: Path = _HOST_DIR.parent  # sdks/python
_REPO_ROOT: Path = _PYTHON_SDK_DIR.parent.parent  # repo root

sys.path.insert(0, str(_HOST_DIR))
sys.path.insert(0, str(_PYTHON_SDK_DIR / "polyplug_abi"))
sys.path.insert(0, str(_PYTHON_SDK_DIR))
sys.path.insert(0, str(_PYTHON_SDK_DIR / "loaders" / "native"))

# ─── Skip-honestly: a runtime test must never silently pass ───────────────────
_MISSING: list[str] = [
    name
    for name in ("POLYPLUG_LIB", "POLYPLUG_NATIVE_LIB")
    if not os.environ.get(name)
]
if _MISSING:
    sys.stderr.write(
        "FATAL: "
        + ", ".join(_MISSING)
        + " not set — this runtime test must not silently pass.\n"
        "Build the core and point the test at it:\n"
        "  cargo build --release -p polyplug -p polyplug_native\n"
        "  export POLYPLUG_LIB=$PWD/target/release/libpolyplug.so\n"
        "  export POLYPLUG_NATIVE_LIB=$PWD/target/release/libpolyplug_native.so\n"
    )
    sys.exit(1)

# Platform-specific cdylib naming (matches tests/fixtures/build_all.sh):
# `<name>.dll` on Windows (no `lib` prefix), `lib<name>.dylib` on macOS,
# `lib<name>.so` on Linux.
if sys.platform == "win32":
    _V1_LIB_NAME: str = "reload_plugin_v1.dll"
    _V2_LIB_NAME: str = "reload_plugin_v2.dll"
elif sys.platform == "darwin":
    _V1_LIB_NAME = "libreload_plugin_v1.dylib"
    _V2_LIB_NAME = "libreload_plugin_v2.dylib"
else:
    _V1_LIB_NAME = "libreload_plugin_v1.so"
    _V2_LIB_NAME = "libreload_plugin_v2.so"

_FIXTURES_DIR: Path = _REPO_ROOT / "tests" / "fixtures"
_V1_DIR: Path = _FIXTURES_DIR / "reload_plugin_v1"
# The reload target is the v2 cdylib INSIDE its bundle dir — the runtime reads
# the sibling manifest.toml during reload (mirrors integration_reload.rs).
_V2_LIB: Path = _FIXTURES_DIR / "reload_plugin_v2" / _V2_LIB_NAME

for _fixture in (_V1_DIR / "manifest.toml", _V1_DIR / _V1_LIB_NAME, _V2_LIB):
    if not _fixture.is_file():
        sys.stderr.write(
            f"FATAL: reload fixture missing: {_fixture}\n"
            "Run `bash tests/fixtures/build_all.sh` first.\n"
        )
        sys.exit(1)

from polyplug import Runtime  # noqa: E402  (sys.path setup above)
from polyplug_abi import (  # noqa: E402
    ReloadPhase,
    RuntimeConfig,
    bundle_id,
)
from polyplug_loaders_native import register_native_loader  # noqa: E402

# Name from tests/fixtures/reload_plugin_v1/manifest.toml; the bundle id is
# FNV-1a 64 of the name (TRUST_MODEL §2) — computed via the SDK helper, never
# hand-rolled.
_V1_BUNDLE_ID: int = bundle_id("reload_plugin_v1")


def main() -> None:
    phases: list[ReloadPhase] = []

    config = RuntimeConfig()
    config.hot_reload_enabled = True
    rt = Runtime(config=config, on_reload=phases.append)
    register_native_loader(rt)

    rt.load_bundle(_V1_DIR)
    assert not phases, f"no reload phases expected before the reload, got: {phases}"

    rt.reload_bundle(_V2_LIB)

    assert len(phases) >= 2, (
        f"reload must deliver at least Preparing + Reloaded, got {len(phases)}: {phases}"
    )

    first: ReloadPhase = phases[0]
    assert first.is_preparing(), f"first phase must be Preparing, got: {first!r}"
    assert first.bundle_id == _V1_BUNDLE_ID, (
        "Preparing phase must carry the real bundle id from the manifest "
        f"(got {first.bundle_id}, want {_V1_BUNDLE_ID})"
    )
    assert first.bundle_name == "reload_plugin_v1", (
        f"Preparing phase must carry the real bundle name (got {first.bundle_name!r})"
    )
    # Non-Failed phases carry the null-view reason; the SDK surfaces it as None.
    assert first.reason is None, (
        f"non-Failed phase must carry the null-view reason as None (got {first.reason!r})"
    )

    assert any(phase.is_reloaded() for phase in phases), (
        f"a Reloaded phase must follow, got: {phases}"
    )

    print("PASS: no reload phases before the reload")
    print("PASS: reload delivered Preparing with real bundle_id/bundle_name/null reason")
    print("PASS: a Reloaded phase followed")
    for phase in phases:
        print(f"  {phase!r}")
    print("test_reload_runtime: all assertions passed")


if __name__ == "__main__":
    main()

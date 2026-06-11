#!/usr/bin/env python3
"""Pipeline Host — Python host demonstrating polyplug usage."""

import ctypes
import os
import sys
from pathlib import Path

from polyplug import Runtime, ReloadPhase
from polyplug import scanner
from polyplug_abi import StringView, RuntimeConfig, to_str


def str_view(s: str, keepalive: list) -> StringView:
    """Build a StringView over a UTF-8 buffer kept alive via `keepalive`."""
    data: bytes = s.encode("utf-8")
    buf: ctypes.Array = ctypes.create_string_buffer(data, len(data))
    keepalive.append(buf)
    return StringView(ptr=ctypes.cast(buf, ctypes.c_void_p), len=len(data))

try:
    from polyplug_loaders_native import register_native_loader
except ImportError:
    register_native_loader = None

try:
    from polyplug_loaders_python import register_python_loader
    from polyplug_loaders_python import bridge_lib as python_bridge_lib
except ImportError:
    register_python_loader = None
    python_bridge_lib = None

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

from generated.host.callers import (
    PipelineDecoderContractCaller,
    DataTransformerContractCaller,
    PipelineEncoderContractCaller,
    DataReporterContractCaller,
    PipelineValidatorContractCaller,
    PIPELINE_DECODER_CONTRACT_ID,
    DATA_TRANSFORMER_CONTRACT_ID,
    PIPELINE_ENCODER_CONTRACT_ID,
    DATA_REPORTER_CONTRACT_ID,
    PIPELINE_VALIDATOR_CONTRACT_ID,
)
# The generated host package imports its siblings as `host.*` (the layout the
# generators emit for sys.path-rooted packages), so `generated/` must be
# importable as a package ROOT. NOTE: this script is named main.py on purpose —
# a sibling regular module named `host` would shadow the generated `host`
# namespace package no matter what the sys.path order is.
sys.path.insert(0, str(Path(__file__).parent / "generated"))

from host.contracts import HostLogger  # noqa: E402  (sys.path setup above)
from host.interface_factories import create_host_logger_interface  # noqa: E402
from host.types import LogLevel  # noqa: E402


class ConsoleLogger(HostLogger):
    """Host-side implementation of the `host.logger` contract.

    Mirrors the rust/cpp reference hosts' ConsoleLogger output format.
    """

    def log(self, message: str) -> None:
        print(f"[plugin] {message}")

    def log_with_level(self, level: LogLevel, message: str) -> None:
        names = {
            LogLevel.DEBUG: "DEBUG",
            LogLevel.INFO: "INFO",
            LogLevel.WARN: "WARN",
            LogLevel.ERROR: "ERROR",
        }
        level_str = names.get(level, "INFO")
        print(f"[plugin][{level_str}] {message}")


def make_caller(caller_cls, rt, contract_id: int):
    """Resolve a contract handle and build a caller, or None if unavailable.

    find_guest_contract returns a GuestContractHandle struct (index: u32, generation: u32);
    the null/invalid handle has index == 0xFFFFFFFF.
    """
    handle = rt.find_guest_contract(contract_id, 0)
    if handle.index == 0xFFFFFFFF:
        return None
    return caller_cls.create(handle, rt._ensure_host(), owner=rt)


def handle_reload(phase: ReloadPhase) -> None:
    if phase.is_preparing():
        print(
            f"[HOT-RELOAD] Preparing: {phase.bundle_name} "
            f"(id=0x{phase.bundle_id:016X})"
        )
    elif phase.is_reloaded():
        print(
            f"[HOT-RELOAD] Reloaded: {phase.bundle_name} (id=0x{phase.bundle_id:016X})"
        )
    elif phase.is_failed():
        print(
            f"[HOT-RELOAD] Failed: {phase.bundle_name} "
            f"(id=0x{phase.bundle_id:016X}) - {phase.reason}"
        )


def main():
    plugin_path = os.environ.get(
        "POLYPLUG_PLUGIN_PATH", str(Path(__file__).parent.parent.parent / "plugins")
    )
    print(f"loading plugins from: {plugin_path}\n")

    config = RuntimeConfig()
    config.hot_reload_enabled = True
    # Config and reload callback are per-instance constructor arguments
    # (no class-level statics shared across runtimes).
    rt = Runtime(config=config, on_reload=handle_reload)

    # Register loaders for every runtime the example plugins may use. Loaders
    # whose backing package or cdylib is unavailable are skipped so the host
    # still runs for the rest.
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

    # Register the host.logger contract through the GENERATED factory so
    # plugins can call back into the host (mirrors the rust/cpp hosts). The
    # factory needs the python loader cdylib for the host-contract bridge
    # trampolines (ctypes cannot create struct-returning callbacks).
    if python_bridge_lib is None:
        print(
            "  host.logger registration unavailable: polyplug_loaders_python not importable",
            file=sys.stderr,
        )
    else:
        logger_iface = create_host_logger_interface(ConsoleLogger(), python_bridge_lib())
        rt.register_host_contract(logger_iface)

    bundles = scanner.scan_dir(plugin_path)
    if not bundles:
        print(f"no plugins found in {plugin_path}")
        sys.exit(1)

    print(f"discovered {len(bundles)} bundles\n")

    for path, manifest in bundles:
        try:
            rt.load_bundle(path)
            print(f"  loaded: {manifest.name}")
        except RuntimeError as e:
            reason = str(e).split("\n", 1)[0]
            print(f"  skipped {manifest.name}: {reason}", file=sys.stderr)

    print("\n=== Pipeline Host (Python) ===\n")

    input_str = "name,value,42"
    print(f'Input: "{input_str}"\n')
    keepalive: list = []

    if decoder := make_caller(PipelineDecoderContractCaller, rt, PIPELINE_DECODER_CONTRACT_ID):
        result = to_str(decoder.decode(str_view(input_str, keepalive)))
        print(f'[decoder] decode("{input_str}") = "{result}"')

    decoded = f"DECODED:{input_str.replace(',', '|')}"
    if transformer := make_caller(DataTransformerContractCaller, rt, DATA_TRANSFORMER_CONTRACT_ID):
        result = to_str(transformer.transform(str_view(decoded, keepalive)))
        print(f'[transformer] transform("{decoded}") = "{result}"')

    transformed = "TRANSFORMED:NAME|value (transformed)|43"
    if encoder := make_caller(PipelineEncoderContractCaller, rt, PIPELINE_ENCODER_CONTRACT_ID):
        result = to_str(encoder.encode(str_view(transformed, keepalive)))
        print(f'[encoder] encode("{transformed}") = "{result}"')

    if reporter := make_caller(DataReporterContractCaller, rt, DATA_REPORTER_CONTRACT_ID):
        result = to_str(reporter.report(str_view(transformed, keepalive)))
        print(f'[reporter] report("{transformed}") = "{result}"')

    if validator := make_caller(PipelineValidatorContractCaller, rt, PIPELINE_VALIDATOR_CONTRACT_ID):
        result = to_str(validator.validate(str_view(decoded, keepalive)))
        print(f'[validator] validate("{decoded}") = "{result}"')

    # Round-trip micro-benchmark (opt-in via POLYPLUG_BENCH_ITERS). Times the full
    # host → runtime → native guest → return path: a Python host calling the native
    # decoder plugin and getting a StringView back. The decoder is a native cdylib
    # guest, so its return is a borrowed zero-copy view (no per-call alloc to leak).
    bench_iters: str = os.environ.get("POLYPLUG_BENCH_ITERS", "")
    if bench_iters:
        run_roundtrip_bench(rt, int(bench_iters))

    print("\ndone.")


def run_roundtrip_bench(rt, iters: int) -> None:
    """Time `decoder.decode(input)` over `iters` calls; print ROUNDTRIP_NS=<ns/call>."""
    import time

    decoder = make_caller(PipelineDecoderContractCaller, rt, PIPELINE_DECODER_CONTRACT_ID)
    if decoder is None:
        print("ROUNDTRIP_NS=nan LANG=python  # decoder unavailable", file=sys.stderr)
        return
    keepalive: list = []
    sv: StringView = str_view("name,value,42", keepalive)
    warmup: int = min(iters, 10000)
    for _ in range(warmup):
        decoder.decode(sv)
    start: int = time.perf_counter_ns()
    for _ in range(iters):
        decoder.decode(sv)
    elapsed: int = time.perf_counter_ns() - start
    print(f"ROUNDTRIP_NS={elapsed / iters:.2f} LANG=python")


if __name__ == "__main__":
    main()

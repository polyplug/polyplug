#!/usr/bin/env python3
"""Pipeline Host — Python host demonstrating polyplug usage."""

import os
import sys
from pathlib import Path

from polyplug import Runtime, ReloadPhase
from polyplug import scanner
from polyplug.helpers import call_plugin_fn
from polyplug.runtime_config import RuntimeConfig

try:
    from polyplug_loaders_native import register_native_loader
except ImportError:
    register_native_loader = None

from generated.host.callers import (
    PIPELINE_DECODER_CONTRACT_ID,
    DATA_TRANSFORMER_CONTRACT_ID,
    PIPELINE_ENCODER_CONTRACT_ID,
    DATA_REPORTER_CONTRACT_ID,
    PIPELINE_VALIDATOR_CONTRACT_ID,
)

NULL_HANDLE = (1 << 64) - 1


def handle_reload(phase: ReloadPhase) -> None:
    if phase.is_preparing():
        print(
            f"[HOT-RELOAD] Preparing: {phase.bundle_name} "
            f"(id=0x{phase.bundle_id:016X}, retry {phase.retry_count})"
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


def call_contract(rt: Runtime, contract_id_val: int, input_str: str) -> str | None:
    handle = rt.find_by_contract(contract_id_val, 0)
    if handle == NULL_HANDLE:
        return None
    guard = rt.resolve_plugin(handle)
    vtable_ptr = guard.vtable
    if vtable_ptr == 0:
        return None
    return call_plugin_fn(rt._backend.lib, vtable_ptr, 0, input_str)


def main():
    plugin_path = os.environ.get(
        "POLYPLUG_PLUGIN_PATH", str(Path(__file__).parent.parent.parent / "plugins")
    )
    print(f"loading plugins from: {plugin_path}\n")

    config = RuntimeConfig(
        hot_reload_max_retries=5,
        hot_reload_retry_interval_ms=200,
        hot_reload_abort_on_max_retries=False,
    )
    Runtime.set_config(config)
    Runtime.on_reload(handle_reload)

    rt = Runtime()
    if register_native_loader is not None:
        try:
            register_native_loader(rt)
        except RuntimeError as e:
            if "register failed: 2" not in str(e):
                raise

    bundles = scanner.scan_dir(plugin_path)
    if not bundles:
        print(f"no plugins found in {plugin_path}")
        sys.exit(1)

    print(f"discovered {len(bundles)} bundles\n")

    for path, manifest in bundles:
        rt.load_bundle(path)
        print(f"  loaded: {manifest.name}")

    print("\n=== Pipeline Host (Python) ===\n")

    input_str = "name,value,42"
    print(f'Input: "{input_str}"\n')

    if result := call_contract(rt, PIPELINE_DECODER_CONTRACT_ID, input_str):
        print(f'[decoder] decode("{input_str}") = "{result}"')

    decoded = f"DECODED:{input_str.replace(',', '|')}"
    if result := call_contract(rt, DATA_TRANSFORMER_CONTRACT_ID, decoded):
        print(f'[transformer] transform("{decoded}") = "{result}"')

    transformed = "TRANSFORMED:NAME|value (transformed)|43"
    if result := call_contract(rt, PIPELINE_ENCODER_CONTRACT_ID, transformed):
        print(f'[encoder] encode("{transformed}") = "{result}"')

    if result := call_contract(rt, DATA_REPORTER_CONTRACT_ID, transformed):
        print(f'[reporter] report("{transformed}") = "{result}"')

    if result := call_contract(rt, PIPELINE_VALIDATOR_CONTRACT_ID, decoded):
        print(f'[validator] validate("{decoded}") = "{result}"')

    print("\ndone.")


if __name__ == "__main__":
    main()

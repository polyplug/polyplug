#!/usr/bin/env python3
"""Pipeline Host — Python host demonstrating polyplug usage.

This example demonstrates:
- Hot-reload with custom configuration
- Instance tracking for proper cleanup during reload
- Factory method pattern for creating plugin callers
"""

import os
import sys
from pathlib import Path

from polyplug import Runtime, ReloadPhase
from polyplug.loaders import register_native_loader
from polyplug import scanner
from polyplug.helpers import call_plugin_fn, to_str, contract_id, bundle_id
from polyplug.runtime_config import RuntimeConfig


# Instance tracking for hot-reload: bundle_id -> list of plugin instances.
# Instances are cleared in Preparing phase and re-created in Reloaded phase.
_instances: dict[int, list] = {}


def handle_reload(phase: ReloadPhase) -> None:
    if phase.is_preparing():
        print(
            f"[HOT-RELOAD] Preparing: {phase.bundle_name} "
            f"(bundle_id=0x{phase.bundle_id:016X}, retry {phase.retry_count})"
        )
        # Clean up instances for this bundle before reload
        if phase.bundle_id in _instances:
            _instances.pop(phase.bundle_id)
            print(f"[HOT-RELOAD] Cleared instances for bundle {phase.bundle_name}")
    elif phase.is_reloaded():
        print(
            f"[HOT-RELOAD] Reloaded: {phase.bundle_name} "
            f"(bundle_id=0x{phase.bundle_id:016X})"
        )
    elif phase.is_failed():
        print(
            f"[HOT-RELOAD] Failed: {phase.bundle_name} "
            f"(bundle_id=0x{phase.bundle_id:016X}) - {phase.reason}"
        )


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
        print(f"  loaded: {manifest['bundle_name']}")

    print("\n=== Pipeline Host (Python) ===\n")

    input_str = "name,value,42"
    print(f'Input: "{input_str}"\n')

    for path, manifest in bundles:
        bundle_name = manifest["bundle_name"]
        bid = bundle_id(bundle_name)
        provides = manifest.get("provides", [])

        for contract in provides:
            parts = contract.split("@")
            if len(parts) != 2:
                continue
            contract_name = parts[0]
            version_parts = parts[1].split(".")
            major = int(version_parts[0]) if version_parts else 1

            cid = contract_id(contract_name, major)
            handle = rt.find_by_bundle(bid, cid, 0)

            if handle == 0xFFFFFFFFFFFFFFFF:
                continue

            guard = rt.resolve_plugin(handle)
            vtable_ptr = guard.get_vtable()

            if contract_name == "pipeline.Decoder":
                result = call_plugin_fn(rt._lib, vtable_ptr, 0, input_str)
                print(f'[{bundle_name}] decode("{input_str}") = "{result}"')
            elif contract_name == "data.Transformer":
                decoded = f"DECODED:{input_str.replace(',', '|')}"
                result = call_plugin_fn(rt._lib, vtable_ptr, 0, decoded)
                print(f'[{bundle_name}] transform("{decoded}") = "{result}"')
            elif contract_name == "pipeline.Encoder":
                transformed = "TRANSFORMED:NAME|value (transformed)|43"
                result = call_plugin_fn(rt._lib, vtable_ptr, 0, transformed)
                print(f'[{bundle_name}] encode("{transformed}") = "{result}"')
            elif contract_name == "data.Reporter":
                transformed = "TRANSFORMED:NAME|value (transformed)|43"
                result = call_plugin_fn(rt._lib, vtable_ptr, 0, transformed)
                print(f'[{bundle_name}] report("{transformed}") = "{result}"')
            elif contract_name == "pipeline.Validator":
                decoded = f"DECODED:{input_str.replace(',', '|')}"
                result = call_plugin_fn(rt._lib, vtable_ptr, 0, decoded)
                print(f'[{bundle_name}] validate("{decoded}") = "{result}"')

    print("\ndone.")


if __name__ == "__main__":
    main()

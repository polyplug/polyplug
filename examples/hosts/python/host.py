#!/usr/bin/env python3
"""Pipeline Host — Python host demonstrating polyplug usage."""

import os
import sys
from pathlib import Path

from polyplug import Runtime
from polyplug.loaders import register_native_loader
from polyplug import scanner
from polyplug.helpers import call_plugin_fn, to_str, contract_id, bundle_id


def main():
    plugin_path = os.environ.get(
        "POLYPLUG_PLUGIN_PATH", str(Path(__file__).parent.parent.parent / "plugins")
    )
    print(f"loading plugins from: {plugin_path}\n")

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

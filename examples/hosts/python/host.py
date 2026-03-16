#!/usr/bin/env python3
"""Pipeline Host — Python host demonstrating polyplug usage."""

import os
import sys
from pathlib import Path

from polyplug import Runtime
from polyplug.loaders import register_native_loader
from polyplug import scanner
from polyplug.abi import contract_id

def main():
    plugin_path = os.environ.get(
        "POLYPLUG_PLUGIN_PATH",
        str(Path(__file__).parent.parent.parent / "plugins")
    )
    print(f"loading plugins from: {plugin_path}\n")

    rt = Runtime()
    try:
        register_native_loader(rt)
    except RuntimeError as e:
        # Error code 2 = DuplicateLoader (already registered, that's OK)
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

    for _, manifest in bundles:
        provides = manifest.get('provides', [])
        bundle_name = manifest['bundle_name']

        if any(c.startswith('pipeline.Decoder@1') for c in provides):
            cid = contract_id('pipeline.Decoder', 1)
            handle = rt.find_by_bundle(bundle_name, cid, 1)
            if handle:
                result = rt.call(handle, 'decode', input_str)
                print(f"[{bundle_name}] decode(\"{input_str}\") = \"{result}\"")

        if any(c.startswith('data.Transformer@1') for c in provides):
            cid = contract_id('data.Transformer', 1)
            handle = rt.find_by_bundle(bundle_name, cid, 1)
            if handle:
                decoded = f"DECODED:{input_str.replace(',', '|')}"
                result = rt.call(handle, 'transform', decoded)
                print(f"[{bundle_name}] transform(\"{decoded}\") = \"{result}\"")

        if any(c.startswith('pipeline.Encoder@1') for c in provides):
            cid = contract_id('pipeline.Encoder', 1)
            handle = rt.find_by_bundle(bundle_name, cid, 1)
            if handle:
                transformed = "TRANSFORMED:NAME|value (transformed)|43"
                result = rt.call(handle, 'encode', transformed)
                print(f"[{bundle_name}] encode(\"{transformed}\") = \"{result}\"")

        if any(c.startswith('data.Reporter@1') for c in provides):
            cid = contract_id('data.Reporter', 1)
            handle = rt.find_by_bundle(bundle_name, cid, 1)
            if handle:
                transformed = "TRANSFORMED:NAME|value (transformed)|43"
                result = rt.call(handle, 'report', transformed)
                print(f"[{bundle_name}] report(\"{transformed}\") = \"{result}\"")

        if any(c.startswith('pipeline.Validator@1') for c in provides):
            cid = contract_id('pipeline.Validator', 1)
            handle = rt.find_by_bundle(bundle_name, cid, 1)
            if handle:
                decoded = f"DECODED:{input_str.replace(',', '|')}"
                result = rt.call(handle, 'validate', decoded)
                print(f"[{bundle_name}] validate(\"{decoded}\") = \"{result}\"")

    print("\ndone.")

if __name__ == '__main__':
    main()

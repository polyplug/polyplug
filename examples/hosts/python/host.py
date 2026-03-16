#!/usr/bin/env python3
"""Pipeline Host — Python host demonstrating polyplug usage."""

import os
import sys
from pathlib import Path

from polyplug import Runtime
from polyplug.loaders import register_native_loader
from polyplug.loader import scanner
from polyplug.helpers import to_str

def main():
    plugin_path = os.environ.get(
        "POLYPLUG_PLUGIN_PATH",
        str(Path(__file__).parent.parent.parent / "plugins")
    )
    print(f"loading plugins from: {plugin_path}\n")

    rt = Runtime()
    register_native_loader(rt)

    bundles = scanner.scan_dir(plugin_path)
    if not bundles:
        print(f"no plugins found in {plugin_path}")
        sys.exit(1)

    print(f"discovered {len(bundles)} bundles\n")

    for path, manifest in bundles:
        rt.load_bundle(path)
        print(f"  loaded: {manifest['bundle_name']}")

    print("\n=== Pipeline Host (Python) ===\n")

    for _, manifest in bundles:
        provides = manifest.get('provides', [])

        if any(c.startswith('pipeline.Decoder') for c in provides):
            handle = rt.find_by_bundle(manifest['bundle_name'], 'pipeline.Decoder', 1)
            if handle:
                print(f"[{manifest['bundle_name']}] decoder ready")

    print("\ndone.")

if __name__ == '__main__':
    main()

#!/usr/bin/env python3
"""Pipeline Host — Python host demonstrating polyplug usage."""

import os
import sys
from pathlib import Path

from polyplug import Runtime
from polyplug.loaders import register_native_loader
from polyplug import scanner

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
    print("Python host loaded all plugins successfully!")
    print("\ndone.")

if __name__ == '__main__':
    main()

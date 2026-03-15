#!/usr/bin/env python3
"""Python host example using polyplugc-generated bindings.

This host demonstrates the real-world polyplug pattern:
  1. Generate host bindings: polyplugc --api api.toml --lang python --out generated/
  2. Import generated types: from generated.host.types import *
  3. Use generated contract IDs instead of hard-coded values
"""

from __future__ import annotations

import os
import sys
from pathlib import Path

# Add the host-libs path for polyplug
sys.path.insert(
    0, str(Path(__file__).parent.parent.parent.parent / "host-libs" / "python")
)

from polyplug import Runtime
from polyplug.loaders import (
    register_native_loader,
    register_dotnet_loader,
    register_python_loader,
    register_lua_loader,
    register_js_loader,
)

# Import generated contract IDs and types
# Note: Full generated callers would require polyplug_guest fixes
# For now, we import the generated constants
import importlib.util

spec = importlib.util.spec_from_file_location(
    "types", Path(__file__).parent / "generated" / "host" / "types.py"
)
types_module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(types_module)

# Get contract IDs from generated module
PIPELINE_DECODER_CONTRACT_ID = getattr(
    types_module, "PIPELINE_DECODER_CONTRACT_ID", 0x12F3C106B0C3DC1E
)
DATA_TRANSFORMER_CONTRACT_ID = getattr(
    types_module, "DATA_TRANSFORMER_CONTRACT_ID", 0x3D53C682F3F5A9EF
)
PIPELINE_ENCODER_CONTRACT_ID = getattr(
    types_module, "PIPELINE_ENCODER_CONTRACT_ID", 0x127D1703C6EFB432
)
DATA_REPORTER_CONTRACT_ID = getattr(
    types_module, "DATA_REPORTER_CONTRACT_ID", 0x81D41D43E511D297
)
PIPELINE_VALIDATOR_CONTRACT_ID = getattr(
    types_module, "PIPELINE_VALIDATOR_CONTRACT_ID", 0xA553FAB5D11C7AF0
)


def main():
    plugin_path = os.environ.get("POLYPLUG_PLUGIN_PATH", "examples/plugins")

    print(f"plugin directory: {plugin_path}", file=sys.stderr)

    # Create runtime with all loaders
    rt = Runtime()
    register_native_loader(rt)
    register_dotnet_loader(rt)
    register_python_loader(rt)
    register_lua_loader(rt)
    register_js_loader(rt)

    # Scan for plugins
    import polyplug.scanner as scanner

    bundles = scanner.scan_dir(plugin_path)

    print(f"discovered {len(bundles)} bundles", file=sys.stderr)

    if not bundles:
        print(
            f"no plugins found in {plugin_path}. Run examples/build_all.sh first.",
            file=sys.stderr,
        )
        return 1

    # Load all discovered bundles
    for bundle_path, manifest in bundles:
        try:
            rt.load_bundle(bundle_path)
            print(f"  loaded: {manifest.bundle_name}", file=sys.stderr)
        except Exception as e:
            print(f"  failed to load {manifest.bundle_name}: {e}", file=sys.stderr)

    print("\n=== polyplug python host example ===")

    # Call each loaded plugin
    for bundle_path, manifest in bundles:
        bid = rt.bundle_id(manifest.bundle_name)
        label = f"[{manifest.bundle_name}]"

        # Check which contract this bundle implements and call appropriate function
        for contract in manifest.provides:
            contract_name = contract.split("@")[0]

            try:
                if "Decoder" in contract_name:
                    handle = rt.find_by_bundle(bid, PIPELINE_DECODER_CONTRACT_ID, 0)
                    if handle:
                        result = call_contract(rt, handle, "decode", b"name,value,42")
                        print(f'{label:<30} decode("name,value,42") = "{result}"')

                elif "Transformer" in contract_name:
                    handle = rt.find_by_bundle(bid, DATA_TRANSFORMER_CONTRACT_ID, 0)
                    if handle:
                        result = call_contract(
                            rt, handle, "transform", b"DECODED:name|value|42"
                        )
                        print(
                            f'{label:<30} transform("DECODED:name|value|42") = "{result}"'
                        )

                elif "Encoder" in contract_name:
                    handle = rt.find_by_bundle(bid, PIPELINE_ENCODER_CONTRACT_ID, 0)
                    if handle:
                        result = call_contract(
                            rt,
                            handle,
                            "encode",
                            b"TRANSFORMED:NAME|value (transformed)|43",
                        )
                        print(
                            f'{label:<30} encode("TRANSFORMED:NAME|value (transformed)|43") = "{result}"'
                        )

                elif "Reporter" in contract_name:
                    handle = rt.find_by_bundle(bid, DATA_REPORTER_CONTRACT_ID, 0)
                    if handle:
                        result = call_contract(
                            rt,
                            handle,
                            "report",
                            b"TRANSFORMED:NAME|value (transformed)|43",
                        )
                        print(
                            f'{label:<30} report("TRANSFORMED:NAME|value (transformed)|43") = "{result}"'
                        )

                elif "Validator" in contract_name:
                    handle = rt.find_by_bundle(bid, PIPELINE_VALIDATOR_CONTRACT_ID, 0)
                    if handle:
                        result = call_contract(
                            rt, handle, "validate", b"DECODED:name|value|42"
                        )
                        print(
                            f'{label:<30} validate("DECODED:name|value|42") = "{result}"'
                        )

            except Exception as e:
                print(f"{label:<30} {contract_name} failed: {e}")

    print("\npython pipeline complete")
    return 0


def call_contract(rt, handle, func_name, input_bytes):
    """Call a contract function with the given input."""
    # This is a simplified version - full implementation would use generated callers
    # For now, we use the runtime's basic call mechanism
    from polyplug.abi import StringView

    input_sv = StringView.from_bytes(input_bytes)
    # Note: In a complete implementation, this would use the generated caller
    # which provides type-safe wrappers around the vtable dispatch
    result_sv = rt.call_function(handle, 0, input_sv)
    return result_sv.to_str()


if __name__ == "__main__":
    sys.exit(main())

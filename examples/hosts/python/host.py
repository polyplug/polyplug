#!/usr/bin/env python3
"""Python host example using polyplugc-generated bindings."""

from __future__ import annotations

import os
import sys
import ctypes
from pathlib import Path

# Add the host-libs path for polyplug
sys.path.insert(0, str(Path(__file__).parent.parent.parent.parent / "host-libs" / "python"))

from polyplug import Runtime
from polyplug.loaders import (
    register_native_loader,
    register_dotnet_loader,
    register_python_loader,
    register_lua_loader,
    register_js_loader,
)
from polyplug.abi import StringView, PluginVTable

# Import generated callers and types
import importlib.util
spec = importlib.util.spec_from_file_location(
    "callers", Path(__file__).parent / "generated" / "host" / "callers.py"
)
callers_module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(callers_module)

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

def resolve_plugin_path() -> Path:
    if "POLYPLUG_PLUGIN_PATH" in os.environ:
        return Path(os.environ["POLYPLUG_PLUGIN_PATH"])
    return Path(__file__).parent.parent.parent / "plugins"

def string_view_to_str(sv: StringView) -> str:
    if not sv.ptr or sv.len == 0:
        return ""
    return ctypes.string_at(sv.ptr, sv.len).decode("utf-8")

def main():
    plugin_path = resolve_plugin_path()
    print(f"plugin directory: {plugin_path}")

    runtime = Runtime()
    register_native_loader(runtime)
    register_dotnet_loader(runtime)
    register_python_loader(runtime)
    register_lua_loader(runtime)
    register_js_loader(runtime)

    # Scan and load plugins
    for entry in os.scandir(plugin_path):
        if entry.is_dir():
            manifest_path = Path(entry.path) / "manifest.toml"
            if manifest_path.exists():
                try:
                    runtime.load_bundle(str(manifest_path))
                    print(f"loaded: {entry.name}")
                except Exception as e:
                    print(f"failed to load {entry.name}: {e}")

    # Find plugins by contract
    decoder_handle = runtime.find_by_contract(PIPELINE_DECODER_CONTRACT_ID, 1)
    if decoder_handle:
        # Get vtable and create caller
        vtable_ptr = runtime.resolve_plugin(decoder_handle)
        if vtable_ptr:
            vtable = ctypes.cast(vtable_ptr, ctypes.POINTER(PluginVTable)).contents
            # Create dispatch function
            DISPATCH_FN = ctypes.CFUNCTYPE(ctypes.c_uint32, ctypes.c_void_p, ctypes.c_void_p)
            dispatch_fn = DISPATCH_FN(vtable.functions[0])
            decoder = PipelineDecoderContractCaller(dispatch_fn)
            
            # Call decode
            input_sv = StringView.from_string("name,value,42")
            result = decoder.decode(input_sv)
            print(f"decode result: {string_view_to_str(result)}")

if __name__ == "__main__":
    main()

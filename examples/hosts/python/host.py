#!/usr/bin/env python3
"""Pipeline Host — Python host demonstrating polyplug usage."""

import os
import sys
from pathlib import Path

from polyplug import Runtime, ReloadPhase
from polyplug import scanner
from polyplug.abi import StringView
from polyplug.runtime_config import RuntimeConfig

try:
    from polyplug_loaders_native import register_native_loader
except ImportError:
    register_native_loader = None

from generated.host.callers import (
    PipelineDecoderContractCaller,
    DataTransformerContractCaller,
    PipelineEncoderContractCaller,
    DataReporterContractCaller,
    PipelineValidatorContractCaller,
)


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

    if decoder := PipelineDecoderContractCaller.create(rt):
        result = decoder.decode(StringView.from_str(input_str)).to_str()
        print(f'[decoder] decode("{input_str}") = "{result}"')

    decoded = f"DECODED:{input_str.replace(',', '|')}"
    if transformer := DataTransformerContractCaller.create(rt):
        result = transformer.transform(StringView.from_str(decoded)).to_str()
        print(f'[transformer] transform("{decoded}") = "{result}"')

    transformed = "TRANSFORMED:NAME|value (transformed)|43"
    if encoder := PipelineEncoderContractCaller.create(rt):
        result = encoder.encode(StringView.from_str(transformed)).to_str()
        print(f'[encoder] encode("{transformed}") = "{result}"')

    if reporter := DataReporterContractCaller.create(rt):
        result = reporter.report(StringView.from_str(transformed)).to_str()
        print(f'[reporter] report("{transformed}") = "{result}"')

    if validator := PipelineValidatorContractCaller.create(rt):
        result = validator.validate(StringView.from_str(decoded)).to_str()
        print(f'[validator] validate("{decoded}") = "{result}"')

    print("\ndone.")


if __name__ == "__main__":
    main()

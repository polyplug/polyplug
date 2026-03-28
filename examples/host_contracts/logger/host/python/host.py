#!/usr/bin/env python3
"""Logger Host — Python host demonstrating host contracts."""

import os
import sys
from pathlib import Path

from polyplug import Runtime
from polyplug.abi import StringView

try:
    from polyplug_loaders_native import register_native_loader
except ImportError:
    register_native_loader = None

from generated.host.callers import ExampleWorkerContractCaller
from generated.host.contracts import HostLogger, HOSTLOGGER_CONTRACT_ID


class ConsoleLogger(HostLogger):
    def log(self, message: str) -> None:
        print(f"[PLUGIN LOG] {message}")


def main():
    plugin_path = os.environ.get(
        "POLYPLUG_PLUGIN_PATH",
        str(Path(__file__).parent.parent.parent.parent / "plugins"),
    )
    print(f"loading plugins from: {plugin_path}\n")

    rt = Runtime()
    if register_native_loader is not None:
        try:
            register_native_loader(rt)
        except RuntimeError as e:
            if "register failed: 2" not in str(e):
                raise

    logger_impl = ConsoleLogger()
    rt.register_host_contract(HOSTLOGGER_CONTRACT_ID, logger_impl)

    if not Path(plugin_path).exists():
        print(f"plugin path does not exist: {plugin_path}")
        sys.exit(1)

    bundles = []
    for entry in Path(plugin_path).iterdir():
        if entry.is_dir() and (entry / "manifest.toml").exists():
            rt.load_bundle(str(entry))
            bundles.append(entry.name)
            print(f"  loaded: {entry.name}")

    if not bundles:
        print(f"no plugins found in {plugin_path}")
        sys.exit(1)

    print(f"\ndiscovered {len(bundles)} bundles\n")

    print("\n=== Logger Host (Python) ===\n")

    input_str = "hello world"
    print(f'Input: "{input_str}"\n')

    if worker := ExampleWorkerContractCaller.create(rt):
        result = worker.do_work(StringView.from_str(input_str)).to_str()
        print(f'[host] do_work("{input_str}") = "{result}"')

    print("\ndone.")


if __name__ == "__main__":
    main()

"""Scanner module for discovering polyplug plugin bundles."""

from __future__ import annotations

import os
import tomllib
from dataclasses import dataclass
from pathlib import Path
from typing import List, Tuple


@dataclass
class Manifest:
    """Plugin bundle manifest."""

    name: str
    version: str = "0.0.0"
    provides: List[str] | None = None


def scan_dir(dir_path: str) -> List[Tuple[str, Manifest]]:
    """Scan a directory for polyplug plugin bundles.

    Args:
        dir_path: Path to directory containing plugin bundles

    Returns:
        List of (bundle_path, Manifest) tuples
    """
    bundles: List[Tuple[str, Manifest]] = []

    for entry in os.scandir(dir_path):
        if not entry.is_dir():
            continue

        manifest_path = os.path.join(entry.path, "manifest.toml")
        if os.path.exists(manifest_path):
            with open(manifest_path, "rb") as f:
                data = tomllib.load(f)
            manifest = Manifest(
                name=data.get("name", ""),
                version=data.get("version", "0.0.0"),
                provides=data.get("provides"),
            )
            bundles.append((entry.path, manifest))

    return bundles

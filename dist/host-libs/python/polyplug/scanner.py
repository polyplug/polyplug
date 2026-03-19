"""Scanner module for discovering polyplug plugin bundles."""

import os
import tomllib
from pathlib import Path
from typing import List, Tuple, Dict, Any


def scan_dir(dir_path: str) -> List[Tuple[str, Dict[str, Any]]]:
    """Scan a directory for polyplug plugin bundles.
    
    Args:
        dir_path: Path to directory containing plugin bundles
        
    Returns:
        List of (bundle_path, manifest_dict) tuples
    """
    bundles = []
    
    for entry in os.scandir(dir_path):
        if not entry.is_dir():
            continue
        
        manifest_path = os.path.join(entry.path, "manifest.toml")
        if os.path.exists(manifest_path):
            with open(manifest_path, "rb") as f:
                manifest = tomllib.load(f)
            bundles.append((entry.path, manifest))
    
    return bundles

#!/usr/bin/env python3
# THIS FILE IS AUTO-MANAGED (part of polyplug host-libs/python)
"""Download native polyplug libraries for CI builds.

This script downloads the appropriate native library from GitHub Releases
and places it in the _native/ directory.

Usage:
    python download-native.py [--version VERSION] [--output-dir DIR]

Examples:
    # Download latest version for current platform
    python download-native.py

    # Download specific version
    python download-native.py --version v0.1.0

    # Download for all platforms (CI)
    python download-native.py --all-platforms
"""

from __future__ import annotations

import os
import sys
import argparse
import urllib.request
import urllib.error
from pathlib import Path


# GitHub Releases URL pattern
RELEASES_BASE = "https://github.com/polyplug/polyplug/releases/download"

# Platform mappings: (sys.platform, machine) -> (release_suffix, lib_filename)
PLATFORM_MAP = {
    ("linux", "x86_64"): ("linux-x64", "libpolyplug.so"),
    ("linux", "amd64"): ("linux-x64", "libpolyplug.so"),
    ("linux", "aarch64"): ("linux-arm64", "libpolyplug.so"),
    ("darwin", "x86_64"): ("macos-x64", "libpolyplug.dylib"),
    ("darwin", "amd64"): ("macos-x64", "libpolyplug.dylib"),
    ("darwin", "arm64"): ("macos-arm64", "libpolyplug.dylib"),
    ("win32", "x86_64"): ("windows-x64", "polyplug.dll"),
    ("win32", "amd64"): ("windows-x64", "polyplug.dll"),
}


def get_current_platform() -> tuple[str, str] | None:
    """Get the current platform identifier.

    Returns:
        Tuple of (sys.platform, machine) or None if unsupported.
    """
    import platform

    plat = sys.platform
    machine = platform.machine().lower()

    if (plat, machine) in PLATFORM_MAP:
        return (plat, machine)

    return None


def get_download_url(version: str, platform_key: tuple[str, str]) -> str:
    """Get the download URL for a specific platform and version.

    Args:
        version: Version string (e.g., "v0.1.0").
        platform_key: Tuple of (sys.platform, machine).

    Returns:
        The full download URL.
    """
    release_suffix, _ = PLATFORM_MAP[platform_key]
    return f"{RELEASES_BASE}/{version}/libpolyplug-{release_suffix}"


def download_file(url: str, output_path: Path) -> None:
    """Download a file from URL to the specified path.

    Args:
        url: The URL to download from.
        output_path: The path to save the file to.

    Raises:
        urllib.error.HTTPError: If the download fails.
    """
    print(f"Downloading: {url}")
    print(f"Output: {output_path}")

    try:
        with urllib.request.urlopen(url) as response:
            if response.status != 200:
                raise urllib.error.HTTPError(
                    url, response.status, f"HTTP {response.status}", None, None
                )

            output_path.parent.mkdir(parents=True, exist_ok=True)
            with open(output_path, "wb") as f:
                f.write(response.read())

        print(f"✓ Downloaded successfully")
    except urllib.error.HTTPError as e:
        if e.code == 404:
            print(f"✗ Not found: {url}")
            print(f"  The version '{version}' may not exist yet.")
        else:
            print(f"✗ Download failed: HTTP {e.code}")
        raise
    except Exception as e:
        print(f"✗ Download failed: {e}")
        raise


def download_for_platform(
    version: str,
    platform_key: tuple[str, str],
    output_dir: Path,
) -> Path:
    """Download the native library for a specific platform.

    Args:
        version: Version string.
        platform_key: Tuple of (sys.platform, machine).
        output_dir: Directory to save the library.

    Returns:
        Path to the downloaded library.
    """
    _, filename = PLATFORM_MAP[platform_key]
    url = get_download_url(version, platform_key)
    output_path = output_dir / filename

    download_file(url, output_path)
    return output_path


def download_all_platforms(version: str, output_dir: Path) -> list[Path]:
    """Download native libraries for all supported platforms.

    Args:
        version: Version string.
        output_dir: Directory to save the libraries.

    Returns:
        List of paths to downloaded libraries.
    """
    downloaded = []

    for platform_key in PLATFORM_MAP:
        try:
            path = download_for_platform(version, platform_key, output_dir)
            downloaded.append(path)
        except Exception as e:
            print(f"Warning: Failed to download for {platform_key}: {e}")

    return downloaded


def main() -> int:
    """Main entry point.

    Returns:
        Exit code (0 for success, 1 for failure).
    """
    parser = argparse.ArgumentParser(
        description="Download native polyplug libraries from GitHub Releases"
    )
    parser.add_argument(
        "--version",
        "-v",
        default="v0.1.0",
        help="Version to download (default: v0.1.0)",
    )
    parser.add_argument(
        "--output-dir",
        "-o",
        type=Path,
        default=None,
        help="Output directory (default: polyplug/_native/)",
    )
    parser.add_argument(
        "--all-platforms",
        action="store_true",
        help="Download for all platforms (CI mode)",
    )

    args = parser.parse_args()

    # Determine output directory
    if args.output_dir:
        output_dir = args.output_dir
    else:
        script_dir = Path(__file__).parent
        output_dir = script_dir / "polyplug" / "_native"

    print(f"Version: {args.version}")
    print(f"Output directory: {output_dir}")
    print()

    try:
        if args.all_platforms:
            print("Downloading for all platforms...")
            downloaded = download_all_platforms(args.version, output_dir)
            if not downloaded:
                print("✗ No libraries downloaded")
                return 1
            print(f"\n✓ Downloaded {len(downloaded)} libraries")
        else:
            platform_key = get_current_platform()
            if platform_key is None:
                print("✗ Unsupported platform")
                return 1

            print(f"Platform: {platform_key[0]} ({platform_key[1]})")
            print()

            path = download_for_platform(args.version, platform_key, output_dir)
            print(f"\n✓ Downloaded: {path}")

        return 0
    except Exception as e:
        print(f"\n✗ Failed: {e}")
        return 1


if __name__ == "__main__":
    sys.exit(main())

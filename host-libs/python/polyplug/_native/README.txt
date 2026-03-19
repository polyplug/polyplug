Native Library Directory
========================

This directory contains the native polyplug library for the current platform.

During CI builds, the appropriate native library is downloaded from GitHub
Releases and placed in this directory:

- Linux:   libpolyplug.so
- macOS:   libpolyplug.dylib
- Windows: polyplug.dll

The library is automatically loaded by the polyplug Python package at runtime.

Manual Download
---------------

To manually download the native library, run:

    python -m polyplug.download_native

Or set the POLYPLUG_LIB environment variable to point to an existing
libpolyplug installation:

    export POLYPLUG_LIB=/path/to/libpolyplug.so

Supported Platforms
-------------------

- Linux x86_64 (x64)
- Linux ARM64 (aarch64)
- macOS x86_64 (Intel)
- macOS ARM64 (Apple Silicon)
- Windows x86_64 (x64)

For more information, see the main README.md.

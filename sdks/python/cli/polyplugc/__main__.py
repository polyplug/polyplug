"""
Entry point for `polyplugc` when installed as a PyPI package.

The bundled binary lives at  polyplugc/_bin/polyplugc  (POSIX) or
polyplugc/_bin/polyplugc.exe  (Windows).  It is injected by CI before
`python -m build --wheel` and ships inside the platform wheel — no network
access or post-install download is needed.

Execution strategy
------------------
* POSIX: os.execv — replace the current process with the binary so that
  signals, exit codes, and interactive TTY behaviour are transparent.
* Windows: subprocess.run — execv is not usable on Windows (no fork/exec
  semantics), so we spawn and forward the exit code.
"""

import os
import pathlib
import stat
import subprocess
import sys


def _bin_dir() -> pathlib.Path:
    return pathlib.Path(__file__).parent / "_bin"


def _binary_path() -> pathlib.Path:
    bin_dir: pathlib.Path = _bin_dir()
    if sys.platform == "win32":
        return bin_dir / "polyplugc.exe"
    return bin_dir / "polyplugc"


def _ensure_executable(path: pathlib.Path) -> None:
    current_mode: int = path.stat().st_mode
    executable_bits: int = stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH
    if not (current_mode & executable_bits):
        path.chmod(current_mode | executable_bits)


def main() -> None:
    binary: pathlib.Path = _binary_path()

    if not binary.exists():
        print(
            f"polyplugc: bundled binary not found at {binary}\n"
            "\n"
            "This wheel does not contain a prebuilt binary for your platform.\n"
            "Alternatives:\n"
            "  cargo install polyplugc\n"
            "  Download a release binary from https://github.com/polyplug/polyplug/releases",
            file=sys.stderr,
        )
        sys.exit(1)

    if sys.platform != "win32":
        _ensure_executable(binary)
        # Replace the current process; all args after 'polyplugc' are forwarded.
        os.execv(str(binary), [str(binary)] + sys.argv[1:])
        # execv never returns on success; reaching here means it failed.
        print(f"polyplugc: execv failed for {binary}", file=sys.stderr)
        sys.exit(1)
    else:
        result: subprocess.CompletedProcess[bytes] = subprocess.run(
            [str(binary)] + sys.argv[1:],
            check=False,
        )
        sys.exit(result.returncode)


if __name__ == "__main__":
    main()

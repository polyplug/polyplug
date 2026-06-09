#!/usr/bin/env python3
"""Fail if any criterion benchmark regressed beyond a threshold vs its baseline.

Walks ``<criterion-dir>`` for ``*/new/estimates.json`` and the sibling
``base/estimates.json`` that criterion writes for the previous run, compares
``mean.point_estimate`` (nanoseconds), and exits non-zero if any benchmark's
mean grew by more than ``--threshold`` times.

A benchmark with no ``base/`` (the first run, or a freshly added bench) is
skipped, not failed. The default threshold is deliberately generous (1.5x) so
ordinary shared-runner noise never trips the gate — only a gross regression
fails it. The CI job restores the prior run's ``target/criterion`` from cache so
``base/`` reflects the previous nightly, giving a rolling run-over-run gate.
"""

import argparse
import json
import sys
from pathlib import Path


def load_mean_ns(estimates_path: Path) -> float:
    """Return the mean point estimate (ns) from a criterion estimates.json."""
    with estimates_path.open() as handle:
        data: dict = json.load(handle)
    return float(data["mean"]["point_estimate"])


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("criterion_dir", help="path to target/criterion")
    parser.add_argument(
        "--threshold",
        type=float,
        default=1.5,
        help="fail if new_mean / base_mean exceeds this ratio (default: 1.5)",
    )
    args = parser.parse_args()

    root: Path = Path(args.criterion_dir)
    if not root.is_dir():
        print(f"no criterion directory at {root}; nothing to check")
        return 0

    regressions: list[tuple[str, float, float, float]] = []
    checked: int = 0

    for new_path in sorted(root.glob("**/new/estimates.json")):
        base_path: Path = new_path.parent.parent / "base" / "estimates.json"
        if not base_path.is_file():
            continue

        bench: str = str(new_path.parent.parent.relative_to(root))
        base_mean: float = load_mean_ns(base_path)
        new_mean: float = load_mean_ns(new_path)
        if base_mean <= 0.0:
            continue

        checked += 1
        ratio: float = new_mean / base_mean
        status: str = "REGRESSED" if ratio > args.threshold else "ok"
        if status == "REGRESSED":
            regressions.append((bench, base_mean, new_mean, ratio))
        print(
            f"{status:10} {ratio:6.2f}x  {base_mean:10.2f} -> {new_mean:10.2f} ns  {bench}"
        )

    print(
        f"\nchecked {checked} benchmark(s) with a baseline; threshold {args.threshold}x"
    )

    if not regressions:
        return 0

    print(f"\n{len(regressions)} regression(s) beyond {args.threshold}x:")
    for bench, base_mean, new_mean, ratio in regressions:
        print(f"  {bench}: {base_mean:.2f} -> {new_mean:.2f} ns ({ratio:.2f}x)")
    return 1


if __name__ == "__main__":
    sys.exit(main())

#!/usr/bin/env python3
"""Render committable SVG charts from a local criterion run.

Walks ``<criterion-dir>`` for the benchmarks this project cares about and emits
self-contained SVG charts (no external charting library, no JS, no web fonts)
into ``<out-dir>``. The SVGs are committed and embedded in ``docs/PERFORMANCE.md``
so the numbers are visible without anyone re-running the suite.

This is a *local* tool, exactly like the benchmarks it draws from. Run it after
a local ``cargo bench -p polyplug`` (a quiet machine gives trustworthy bars):

    cargo bench -p polyplug
    python3 ci/gen_bench_charts.py target/criterion docs/assets/benches

It reads ``median.point_estimate`` (nanoseconds) — the median is robust to the
single-run scheduler noise a developer machine adds, so the bars stay stable
between runs. Throughput metadata in ``benchmark.json`` is used to convert a
whole-loop measurement (e.g. the 1,000,000-iteration ``counter_inc`` loop) into
a per-call figure.
"""

import argparse
import json
import sys
from pathlib import Path

# ─── criterion JSON access ────────────────────────────────────────────────────


def median_ns(criterion_dir: Path, full_id: str) -> float:
    """Median point estimate (ns) for one benchmark's most recent run."""
    estimates: Path = criterion_dir / full_id / "new" / "estimates.json"
    with estimates.open() as handle:
        data: dict = json.load(handle)
    return float(data["median"]["point_estimate"])


def elements(criterion_dir: Path, full_id: str) -> int:
    """Throughput element count for a benchmark, or 1 if it reports none."""
    benchmark: Path = criterion_dir / full_id / "new" / "benchmark.json"
    with benchmark.open() as handle:
        data: dict = json.load(handle)
    throughput: dict = data.get("throughput") or {}
    if "Elements" in throughput:
        return int(throughput["Elements"])
    return 1


def per_call_ns(criterion_dir: Path, full_id: str) -> float:
    """Per-call latency (ns): whole-loop median divided by its element count."""
    return median_ns(criterion_dir, full_id) / elements(criterion_dir, full_id)


# ─── minimal SVG primitives ───────────────────────────────────────────────────

_FONT: str = "font-family='ui-monospace,SFMono-Regular,Menlo,monospace'"


def _esc(text: str) -> str:
    return text.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


def _text(x: float, y: float, content: str, size: int, color: str, anchor: str) -> str:
    return (
        f"<text x='{x:.1f}' y='{y:.1f}' font-size='{size}' fill='{color}' "
        f"text-anchor='{anchor}' {_FONT}>{_esc(content)}</text>"
    )


def _rect(x: float, y: float, w: float, h: float, color: str) -> str:
    return f"<rect x='{x:.1f}' y='{y:.1f}' width='{w:.1f}' height='{h:.1f}' fill='{color}' rx='2'/>"


def _line(x1: float, y1: float, x2: float, y2: float, color: str, width: float) -> str:
    return (
        f"<line x1='{x1:.1f}' y1='{y1:.1f}' x2='{x2:.1f}' y2='{y2:.1f}' "
        f"stroke='{color}' stroke-width='{width}'/>"
    )


def _svg(width: int, height: int, body: str) -> str:
    return (
        f"<svg xmlns='http://www.w3.org/2000/svg' width='{width}' height='{height}' "
        f"viewBox='0 0 {width} {height}'>"
        f"<rect width='{width}' height='{height}' fill='#0d1117'/>{body}</svg>\n"
    )


# ─── charts ───────────────────────────────────────────────────────────────────

# Brand-ish palette on a dark (#0d1117) GitHub-readme background.
_FG: str = "#c9d1d9"
_MUTED: str = "#8b949e"
_GRID: str = "#21262d"
_HILITE: str = "#3fb950"  # polyplug (the product)
_NEUTRAL: str = "#58a6ff"  # FFI / native reference
_FLOOR: str = "#6e7681"  # un-crossable floor


def chart_counter_inc(criterion_dir: Path, out: Path) -> None:
    """Horizontal bar chart: per-call cost of each counter_inc mechanism."""
    bars: list[tuple[str, str, str]] = [
        ("native/inline_never", "direct call (floor)", _FLOOR),
        ("ffi/by_value", "raw dlsym FFI", _NEUTRAL),
        ("native/abi_marshalled", "ABI convention (static)", _NEUTRAL),
        ("polyplug/dispatch", "polyplug — Rust plugin", _HILITE),
        ("polyplug/dispatch_cpp", "polyplug — C++ plugin", _HILITE),
    ]
    values: list[tuple[str, str, float]] = [
        (label, color, per_call_ns(criterion_dir, f"counter_inc_1m/{fid}"))
        for fid, label, color in bars
    ]

    width: int = 720
    pad_l: int = 230
    pad_r: int = 90
    pad_t: int = 56
    row_h: int = 46
    height: int = pad_t + row_h * len(values) + 30
    plot_w: int = width - pad_l - pad_r
    vmax: float = max(v for _, _, v in values) * 1.12

    parts: list[str] = [
        _text(24, 30, "counter_inc — per-call cost (lower is better)", 17, _FG, "start"),
        _text(24, 47, "same 1,000,000x loop, inc reached a different way each bar", 11, _MUTED, "start"),
    ]
    for i, (label, color, value) in enumerate(values):
        y: float = pad_t + i * row_h
        bar_w: float = plot_w * value / vmax
        parts.append(_text(pad_l - 12, y + row_h / 2 + 4, label, 12, _FG, "end"))
        parts.append(_rect(pad_l, y + 7, bar_w, row_h - 18, color))
        parts.append(
            _text(pad_l + bar_w + 8, y + row_h / 2 + 4, f"{value:.2f} ns", 12, _FG, "start")
        )
    parts.append(_line(pad_l, pad_t - 6, pad_l, pad_t + row_h * len(values), _GRID, 1))

    out.write_text(_svg(width, height, "".join(parts)))


def chart_payload_scaling(criterion_dir: Path, out: Path) -> None:
    """Line chart: native vs polyplug per-call cost across payload sizes."""
    sizes: list[int] = [0, 16, 64, 256, 1024, 4096, 16384]
    series: list[tuple[str, str, str]] = [
        ("native_direct", "native (static)", _NEUTRAL),
        ("polyplug_dispatch", "polyplug dispatch", _HILITE),
    ]
    data: dict[str, list[float]] = {
        fid: [per_call_ns(criterion_dir, f"payload_scaling/{fid}/{n}") for n in sizes]
        for fid, _, _ in series
    }

    width: int = 720
    height: int = 420
    pad_l: int = 64
    pad_r: int = 24
    pad_t: int = 64
    pad_b: int = 56
    plot_w: int = width - pad_l - pad_r
    plot_h: int = height - pad_t - pad_b

    all_values: list[float] = [v for col in data.values() for v in col]
    # Log-y so the small-payload detail and the 16 KB tail both stay visible.
    import math

    lo: float = math.log10(min(all_values))
    hi: float = math.log10(max(all_values))

    def x_of(i: int) -> float:
        return pad_l + plot_w * i / (len(sizes) - 1)

    def y_of(v: float) -> float:
        return pad_t + plot_h * (1 - (math.log10(v) - lo) / (hi - lo))

    parts: list[str] = [
        _text(24, 30, "payload_scaling — overhead vanishes as work grows", 17, _FG, "start"),
        _text(24, 47, "per-call cost vs bytes written (log scale); the lines converge", 11, _MUTED, "start"),
    ]
    # x gridlines + labels
    for i, n in enumerate(sizes):
        gx: float = x_of(i)
        parts.append(_line(gx, pad_t, gx, pad_t + plot_h, _GRID, 1))
        parts.append(_text(gx, pad_t + plot_h + 18, str(n), 10, _MUTED, "middle"))
    parts.append(_text(pad_l + plot_w / 2, height - 8, "payload (bytes)", 11, _MUTED, "middle"))
    # series polylines + points
    for fid, label, color in series:
        pts: list[str] = []
        for i, value in enumerate(data[fid]):
            pts.append(f"{x_of(i):.1f},{y_of(value):.1f}")
        parts.append(
            f"<polyline points='{' '.join(pts)}' fill='none' stroke='{color}' stroke-width='2.5'/>"
        )
        for i, value in enumerate(data[fid]):
            parts.append(f"<circle cx='{x_of(i):.1f}' cy='{y_of(value):.1f}' r='3' fill='{color}'/>")
    # legend
    for j, (_, label, color) in enumerate(series):
        ly: float = pad_t + 6 + j * 18
        parts.append(_rect(pad_l + 12, ly - 9, 14, 10, color))
        parts.append(_text(pad_l + 32, ly, label, 11, _FG, "start"))

    out.write_text(_svg(width, height, "".join(parts)))


# ─── entry point ──────────────────────────────────────────────────────────────


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("criterion_dir", type=Path, help="target/criterion")
    parser.add_argument("out_dir", type=Path, help="directory to write SVGs into")
    args = parser.parse_args()

    criterion_dir: Path = args.criterion_dir
    out_dir: Path = args.out_dir
    if not criterion_dir.is_dir():
        print(f"error: {criterion_dir} is not a directory (run cargo bench first)", file=sys.stderr)
        return 1
    out_dir.mkdir(parents=True, exist_ok=True)

    charts: list[tuple[str, object]] = [
        ("counter_inc.svg", chart_counter_inc),
        ("payload_scaling.svg", chart_payload_scaling),
    ]
    for name, render in charts:
        target: Path = out_dir / name
        try:
            render(criterion_dir, target)
        except (FileNotFoundError, KeyError) as error:
            print(f"error: cannot render {name}: missing data ({error})", file=sys.stderr)
            print("       run `cargo bench -p polyplug` first.", file=sys.stderr)
            return 1
        print(f"wrote {target}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

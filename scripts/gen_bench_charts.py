#!/usr/bin/env python3
"""Render committable SVG charts from a local criterion run.

Walks ``<criterion-dir>`` for the benchmarks this project cares about and emits
self-contained SVG charts (no external charting library, no JS, no web fonts)
into ``<out-dir>``. The SVGs are committed and embedded in ``docs/PERFORMANCE.md``
so the numbers are visible without anyone re-running the suite.

This is a *local* tool, exactly like the benchmarks it draws from. Run it after
a local ``cargo bench`` across the core + loader crates (a quiet machine gives
trustworthy bars):

    cargo bench -p polyplug -p polyplug_lua -p polyplug_js \
        -p polyplug_python -p polyplug_dotnet
    python3 scripts/gen_bench_charts.py target/criterion docs/assets/benches

Every bar is read live from criterion's ``median.point_estimate`` (nanoseconds)
— the median is robust to the single-run scheduler noise a developer machine
adds, so the bars stay stable between runs. Throughput metadata in
``benchmark.json`` converts a whole-loop measurement (e.g. the 1,000,000-iteration
``counter_inc`` loop) into a per-call figure.
"""

import argparse
import json
import math
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


# Brand-ish palette on a dark (#0d1117) GitHub-readme background.
_FG: str = "#c9d1d9"
_MUTED: str = "#8b949e"
_GRID: str = "#21262d"
_HILITE: str = "#3fb950"  # polyplug / the fast, zero-copy path
_NEUTRAL: str = "#58a6ff"  # FFI / native reference / VM
_FLOOR: str = "#6e7681"  # un-crossable floor
_SLOW: str = "#d29922"  # the expensive end (owned copy / Python GIL / ctypes)


def _fmt_ns(value: float) -> str:
    if value >= 1_000_000.0:
        return f"{value / 1_000_000.0:.1f} ms"
    if value >= 1000.0:
        return f"{value / 1000.0:.1f} µs"
    if value >= 100.0:
        return f"{value:.0f} ns"
    if value >= 10.0:
        return f"{value:.1f} ns"
    return f"{value:.2f} ns"


def _fmt_bytes(n: int) -> str:
    if n >= 1_048_576:
        return f"{n // 1_048_576} MB"
    if n >= 1024:
        return f"{n // 1024} KB"
    return str(n)


def _wrap(text: str, width_px: int, font_size: int, margin: int = 24) -> list:
    """Greedy word-wrap `text` into lines that fit `width_px` at the given monospace
    font size (≈0.6 em per glyph). Returns a list of line strings (≥1)."""
    max_chars: int = max(8, int((width_px - 2 * margin) / (font_size * 0.6)))
    lines: list = []
    current: str = ""
    for word in text.split():
        if current and len(current) + 1 + len(word) > max_chars:
            lines.append(current)
            current = word
        else:
            current = word if not current else f"{current} {word}"
    if current:
        lines.append(current)
    return lines or [""]


def _header(title: str, subtitle: str, width: int) -> tuple:
    """Build the title + word-wrapped subtitle block. Returns (svg_str, extra_y)
    where extra_y is the additional vertical space used by wrapped subtitle lines
    (0 when the subtitle fits on one line) — callers add it to pad_t / height."""
    lines: list = _wrap(subtitle, width, 11)
    parts: list = [_text(24, 30, title, 17, _FG, "start")]
    for i, line in enumerate(lines):
        parts.append(_text(24, 47 + i * 15, line, 11, _MUTED, "start"))
    return "".join(parts), 15 * (len(lines) - 1)


# ─── reusable chart layouts ───────────────────────────────────────────────────


def _chart_hbar_linear(out: Path, title: str, subtitle: str, rows: list) -> None:
    """Horizontal linear-scale bar chart; rows = [(label, ns, color)].

    Rows are sorted fastest → slowest so every bar chart reads monotonically
    (the label carries the meaning, never the position).
    """
    rows: list = sorted(rows, key=lambda r: r[1])
    width: int = 720
    pad_l: int = 250
    pad_r: int = 96
    header_svg, extra_y = _header(title, subtitle, width)
    pad_t: int = 56 + extra_y
    row_h: int = 46
    height: int = pad_t + row_h * len(rows) + 30
    plot_w: int = width - pad_l - pad_r
    vmax: float = max(ns for _, ns, _ in rows) * 1.12

    parts: list = [header_svg]
    for i, (label, value, color) in enumerate(rows):
        y: float = pad_t + i * row_h
        bar_w: float = plot_w * value / vmax
        parts.append(_text(pad_l - 12, y + row_h / 2 + 4, label, 12, _FG, "end"))
        parts.append(_rect(pad_l, y + 7, max(bar_w, 1.5), row_h - 18, color))
        parts.append(
            _text(pad_l + bar_w + 8, y + row_h / 2 + 4, _fmt_ns(value), 12, _FG, "start")
        )
    parts.append(_line(pad_l, pad_t - 6, pad_l, pad_t + row_h * len(rows), _GRID, 1))
    out.write_text(_svg(width, height, "".join(parts)))


def _chart_hbar_log(out: Path, title: str, subtitle: str, rows: list, note: str) -> None:
    """Horizontal log-scale bar chart; rows = [(label, ns, color)].

    Rows are sorted fastest → slowest so every bar chart reads monotonically
    (the label carries the meaning, never the position).
    """
    rows: list = sorted(rows, key=lambda r: r[1])
    width: int = 760
    pad_l: int = 250
    pad_r: int = 96
    header_svg, extra_y = _header(title, subtitle, width)
    pad_t: int = 60 + extra_y
    row_h: int = 40
    note_lines: list = _wrap(note, width, 9)
    bars_bottom: int = pad_t + row_h * len(rows)
    axis_caption_y: int = bars_bottom + 34
    note_y0: int = bars_bottom + 56
    height: int = note_y0 + 14 * (len(note_lines) - 1) + 12
    plot_w: int = width - pad_l - pad_r

    vmax: float = max(ns for _, ns, _ in rows)
    axis_hi: float = math.log10(vmax * 1.6)

    def x_of(ns: float) -> float:
        # Log axis anchored at 1 ns; 0.3-decade floor so the fastest bar is visible.
        frac: float = max(math.log10(ns), 0.3) / axis_hi
        return pad_l + plot_w * frac

    parts: list = [header_svg]
    # decade gridlines (1 ns, 10 ns, 100 ns, 1 µs, 10 µs …)
    decade: int = 0
    while 10.0**decade <= vmax * 1.6:
        gv: float = 10.0**decade
        gx: float = pad_l + plot_w * (decade / axis_hi)
        parts.append(_line(gx, pad_t - 4, gx, pad_t + row_h * len(rows), _GRID, 1))
        parts.append(_text(gx, pad_t + row_h * len(rows) + 16, _fmt_ns(gv), 9, _MUTED, "middle"))
        decade += 1
    for i, (label, ns, color) in enumerate(rows):
        y: float = pad_t + i * row_h
        bar_w: float = x_of(ns) - pad_l
        parts.append(_text(pad_l - 12, y + row_h / 2 + 4, label, 11, _FG, "end"))
        parts.append(_rect(pad_l, y + 6, max(bar_w, 2.0), row_h - 16, color))
        parts.append(_text(pad_l + bar_w + 8, y + row_h / 2 + 4, _fmt_ns(ns), 11, _FG, "start"))
    parts.append(
        _text(
            pad_l + plot_w / 2,
            axis_caption_y,
            "time per call — log scale, lower is better →",
            10,
            _MUTED,
            "middle",
        )
    )
    for li, line in enumerate(note_lines):
        parts.append(_text(24, note_y0 + li * 14, line, 9, _MUTED, "start"))
    out.write_text(_svg(width, height, "".join(parts)))


def _chart_lines(
    out: Path, title: str, subtitle: str, sizes: list, series: list, data: dict, xlabel: str
) -> None:
    """Log-y line chart over a shared x axis; series = [(key, label, color)]."""
    width: int = 720
    header_svg, extra_y = _header(title, subtitle, width)
    height: int = 420 + extra_y
    pad_l: int = 64
    pad_r: int = 24
    pad_t: int = 64 + extra_y
    pad_b: int = 56
    plot_w: int = width - pad_l - pad_r
    plot_h: int = height - pad_t - pad_b

    all_values: list = [v for col in data.values() for v in col]
    lo: float = math.log10(min(all_values))
    hi: float = math.log10(max(all_values))
    span: float = hi - lo if hi > lo else 1.0

    def x_of(i: int) -> float:
        return pad_l + plot_w * i / (len(sizes) - 1)

    def y_of(v: float) -> float:
        return pad_t + plot_h * (1 - (math.log10(v) - lo) / span)

    parts: list = [header_svg]
    for i, n in enumerate(sizes):
        gx: float = x_of(i)
        parts.append(_line(gx, pad_t, gx, pad_t + plot_h, _GRID, 1))
        parts.append(_text(gx, pad_t + plot_h + 18, str(n), 10, _MUTED, "middle"))
    parts.append(_text(pad_l + plot_w / 2, height - 8, xlabel, 11, _MUTED, "middle"))
    for key, _, color in series:
        pts: list = [f"{x_of(i):.1f},{y_of(v):.1f}" for i, v in enumerate(data[key])]
        parts.append(
            f"<polyline points='{' '.join(pts)}' fill='none' stroke='{color}' stroke-width='2.5'/>"
        )
        for i, v in enumerate(data[key]):
            parts.append(f"<circle cx='{x_of(i):.1f}' cy='{y_of(v):.1f}' r='3' fill='{color}'/>")
    for j, (_, label, color) in enumerate(series):
        ly: float = pad_t + 6 + j * 18
        parts.append(_rect(pad_l + 12, ly - 9, 14, 10, color))
        parts.append(_text(pad_l + 32, ly, label, 11, _FG, "start"))
    out.write_text(_svg(width, height, "".join(parts)))


def _lerp_color(c0: str, c1: str, t: float) -> str:
    """Linear blend between two ``#rrggbb`` colors; t in [0, 1]."""
    r0, g0, b0 = int(c0[1:3], 16), int(c0[3:5], 16), int(c0[5:7], 16)
    r1, g1, b1 = int(c1[1:3], 16), int(c1[3:5], 16), int(c1[5:7], 16)
    r: int = round(r0 + (r1 - r0) * t)
    g: int = round(g0 + (g1 - g0) * t)
    b: int = round(b0 + (b1 - b0) * t)
    return f"#{r:02x}{g:02x}{b:02x}"


def _heat_color(t: float) -> str:
    """Speed ramp: t=0 (fastest) green → amber → t=1 (slowest) red."""
    if t < 0.5:
        return _lerp_color("#2ea043", "#d29922", t / 0.5)
    return _lerp_color("#d29922", "#f85149", (t - 0.5) / 0.5)


def _chart_heatmap(
    out: Path, title: str, subtitle: str, col_labels: list, row_labels: list, cells: dict, note: str
) -> None:
    """Grid heatmap; cells = {(row_label, col_label): ns or None}. Color = speed."""
    n_rows: int = len(row_labels)
    n_cols: int = len(col_labels)
    cell_w: int = 96
    cell_h: int = 46
    pad_l: int = 132
    width: int = pad_l + cell_w * n_cols + 24
    header_svg, extra_y = _header(title, subtitle, width)
    pad_t: int = 104 + extra_y
    grid_bottom: int = pad_t + cell_h * n_rows
    note_lines: list = _wrap(note, width, 9)
    note_y0: int = grid_bottom + 92
    height: int = note_y0 + 14 * (len(note_lines) - 1) + 10

    present: list = [v for v in cells.values() if v is not None]
    lo: float = math.log10(min(present))
    hi: float = math.log10(max(present))
    span: float = hi - lo if hi > lo else 1.0

    parts: list = [header_svg]
    # Axis captions: guest across the top, host down the left (rotated).
    parts.append(
        _text(pad_l + n_cols * cell_w / 2, pad_t - 34, "GUEST — plugin language  →", 11, _MUTED, "middle")
    )
    left_x: float = 22.0
    mid_y: float = pad_t + n_rows * cell_h / 2
    parts.append(
        f"<text x='{left_x:.1f}' y='{mid_y:.1f}' font-size='11' fill='{_MUTED}' "
        f"text-anchor='middle' transform='rotate(-90 {left_x:.1f} {mid_y:.1f})' {_FONT}>"
        f"HOST — app language  ↓</text>"
    )
    for j, col in enumerate(col_labels):
        cx: float = pad_l + j * cell_w + cell_w / 2
        parts.append(_text(cx, pad_t - 12, col, 12, _FG, "middle"))
    for i, row in enumerate(row_labels):
        ry: float = pad_t + i * cell_h
        parts.append(_text(pad_l - 12, ry + cell_h / 2 + 4, row, 12, _FG, "end"))
        for j, col in enumerate(col_labels):
            cx = pad_l + j * cell_w
            value = cells.get((row, col))
            if value is None:
                parts.append(_rect(cx + 2, ry + 2, cell_w - 4, cell_h - 4, _GRID))
                parts.append(_text(cx + cell_w / 2, ry + cell_h / 2 + 4, "N/A", 10, _MUTED, "middle"))
            else:
                t: float = (math.log10(value) - lo) / span
                parts.append(_rect(cx + 2, ry + 2, cell_w - 4, cell_h - 4, _heat_color(t)))
                parts.append(_text(cx + cell_w / 2, ry + cell_h / 2 + 4, _fmt_ns(value), 11, "#0d1117", "middle"))
    # Color legend (fast → slow), anchored just below the grid.
    leg_y: float = grid_bottom + 48
    for k in range(40):
        lx: float = pad_l + k * 4
        parts.append(_rect(lx, leg_y, 4, 10, _heat_color(k / 39)))
    parts.append(_text(pad_l - 8, leg_y + 9, "faster", 9, _MUTED, "end"))
    parts.append(_text(pad_l + 40 * 4 + 8, leg_y + 9, "slower", 9, _MUTED, "start"))
    # Numeric anchors under the gradient so a cell's color reads as a real time.
    leg_w: float = 40 * 4
    fast_ns: float = min(present)
    slow_ns: float = max(present)
    mid_ns: float = 10.0 ** ((lo + hi) / 2)
    parts.append(_text(pad_l, leg_y + 24, _fmt_ns(fast_ns), 9, _MUTED, "middle"))
    parts.append(_text(pad_l + leg_w / 2, leg_y + 24, _fmt_ns(mid_ns), 9, _MUTED, "middle"))
    parts.append(_text(pad_l + leg_w, leg_y + 24, _fmt_ns(slow_ns), 9, _MUTED, "middle"))
    for li, line in enumerate(note_lines):
        parts.append(_text(24, note_y0 + li * 14, line, 9, _MUTED, "start"))
    out.write_text(_svg(width, height, "".join(parts)))


# ─── charts ───────────────────────────────────────────────────────────────────


def chart_counter_inc(criterion_dir: Path, out: Path) -> None:
    """Per-call cost of each counter_inc mechanism (linear bars)."""
    # Native-arm labels are kept identical to chart_hero so the shared bars read
    # the same across both charts.
    bars: list = [
        ("native/inline_never", "direct function call (no plugins)", _FLOOR),
        ("ffi/by_value", "raw FFI (dlsym, no safety)", _NEUTRAL),
        ("native/abi_marshalled", "ABI convention (static)", _NEUTRAL),
        ("polyplug/dispatch", "polyplug — native plugin (Rust)", _HILITE),
        ("polyplug/dispatch_cpp", "polyplug — native plugin (C++)", _HILITE),
    ]
    rows: list = [
        (label, per_call_ns(criterion_dir, f"counter_inc_1m/{fid}"), color)
        for fid, label, color in bars
    ]
    _chart_hbar_linear(
        out,
        "What does a safe plugin call cost?",
        "counter_inc — the same 1,000,000-call loop, reaching the function a different way each bar (lower is better)",
        rows,
    )


def chart_dispatch_by_shape(criterion_dir: Path, out: Path) -> None:
    """Per-call dispatch cost by argument/return shape (linear bars)."""
    bars: list = [
        ("dispatch/struct_arg_and_return/add(42,57)", "struct arg + scalar return", _HILITE),
        ("dispatch/noop/add(0,0)", "noop — add(0,0)", _HILITE),
        ("dispatch/buffer_arg/fill_4096", "4 KB buffer fill", _NEUTRAL),
        ("dispatch/cross_plugin/find+call", "cross-plugin (find+resolve+call)", _NEUTRAL),
    ]
    rows: list = [
        (label, per_call_ns(criterion_dir, fid), color) for fid, label, color in bars
    ]
    _chart_hbar_linear(
        out,
        "What you pass changes the cost",
        "a plain number is almost free; a 4 KB buffer or a fresh plugin lookup adds real work (lower is better)",
        rows,
    )


def chart_payload_scaling(criterion_dir: Path, out: Path) -> None:
    """Native vs polyplug per-call cost across payload sizes (log-y lines)."""
    sizes: list = [0, 16, 64, 256, 1024, 4096, 16384, 65536, 262_144, 1_048_576]
    series: list = [
        ("native_direct", "native (static)", _NEUTRAL),
        ("polyplug_dispatch", "polyplug dispatch", _HILITE),
    ]
    data: dict = {
        key: [per_call_ns(criterion_dir, f"payload_scaling/{key}/{n}") for n in sizes]
        for key, _, _ in series
    }
    _chart_lines(
        out,
        "The more work a call does, the less the call cost matters",
        "per-call cost vs bytes the plugin writes (log scale) — the two lines meet as the payload grows",
        [_fmt_bytes(n) for n in sizes],
        series,
        data,
        "payload written per call",
    )


def chart_marshalling(criterion_dir: Path, out: Path) -> None:
    """Borrowed view vs owned copy return cost across payload sizes (log-y lines).

    Canonical view of the borrow-vs-copy story; native_round_trip.svg shows the
    same effect as bars by return type, payload_scaling.svg as call-cost vs work.
    """
    sizes: list = [16, 64, 256, 1024, 4096, 16384, 65536, 262_144, 1_048_576]
    series: list = [
        ("borrowed", "borrowed view (zero-copy)", _HILITE),
        ("owned", "owned copy (host alloc + memcpy)", _SLOW),
    ]
    data: dict = {
        key: [per_call_ns(criterion_dir, f"marshalling/{key}/{n}") for n in sizes]
        for key, _, _ in series
    }
    _chart_lines(
        out,
        "Returning data: borrow it (free) or copy it (grows)",
        "return cost vs payload bytes, 16 B → 1 MB (log scale) — a borrowed view is flat; an owned copy scales with size",
        [_fmt_bytes(n) for n in sizes],
        series,
        data,
        "data returned per call",
    )


def chart_native_round_trip(criterion_dir: Path, out: Path) -> None:
    """End-to-end native round trip (host -> guest -> return), by return type.

    Composed from existing criterion data: a Rust host with a pre-resolved
    interface dispatches into a native guest and reads the result back. The flat
    scalar case is the `counter_inc` `inc()` loop; the data cases are the
    `marshalling` group. The story: the round trip itself is ~2 ns (resolve is
    cached); the only thing that grows the cost is copying data out.
    """
    rows: list = [
        ("scalar return (a number)", per_call_ns(criterion_dir, "counter_inc_1m/polyplug/dispatch"), _HILITE),
        ("borrowed view return (256 B)", per_call_ns(criterion_dir, "marshalling/borrowed/256"), _HILITE),
        ("borrowed view return (1 MB)", per_call_ns(criterion_dir, "marshalling/borrowed/1048576"), _HILITE),
        ("owned copy return (256 B)", per_call_ns(criterion_dir, "marshalling/owned/256"), _SLOW),
        ("owned copy return (1 MB)", per_call_ns(criterion_dir, "marshalling/owned/1048576"), _SLOW),
    ]
    _chart_hbar_log(
        out,
        "A full call-and-return, by what comes back",
        "Rust app, plugin already looked up — cost depends only on what the plugin hands back (log scale)",
        rows,
        "The round trip itself is ~2 ns — resolve is cached, dispatch is one indirect call. "
        "The only thing that grows the cost is copying data out: borrowed zero-copy views stay "
        "flat, owned copies pay a host alloc + memcpy that scales with the payload. Same story "
        "over a size sweep: marshalling.svg (borrow vs copy) and payload_scaling.svg (call cost "
        "vs work done).",
    )


def chart_amortization(criterion_dir: Path, out: Path) -> None:
    """One-time load / resolve / reload costs (log-scale bars)."""
    rows: list = [
        ("find + resolve (per call)", per_call_ns(criterion_dir, "amortization/find_and_resolve"), _HILITE),
        ("load bundle (dlopen + init)", per_call_ns(criterion_dir, "amortization/load_bundle"), _SLOW),
        ("hot-reload swap (v1 → v2)", per_call_ns(criterion_dir, "amortization/hot_reload_swap"), _SLOW),
    ]
    _chart_hbar_log(
        out,
        "One-time setup costs (paid once, not per call)",
        "loading / looking up / hot-reloading a plugin — log scale; none of these touch the per-call hot path",
        rows,
        "find+resolve is the only one a caller might repeat — and it is ~20 ns (a HashMap hit), "
        "warm; cold_start.svg shows the one-time cold-cache first call. "
        "Load and reload are dominated by the OS dlopen/mmap, not polyplug; see PROFILING.md.",
    )


def chart_call_arena(criterion_dir: Path, out: Path) -> None:
    """Per-call CallArena costs — the retain-and-rewind win (linear bars).

    Every bar is read live from the `call_arena` criterion group. The headline is
    `overflow/cold_first_block` vs `overflow/warm_reuse`: the gap is exactly what
    retain-and-rewind buys — after the first overflowing call, every later call
    reuses the retained block instead of paying a host malloc again.
    """
    bars: list = [
        ("reset (rewind cursor)", "reset/primary_only", _HILITE),
        ("warm bump (64 B, primary)", "primary/alloc_64", _HILITE),
        ("overflow — warm reuse (retained)", "overflow/warm_reuse", _HILITE),
        ("per-call shape (64 B)", "per_call/64", _NEUTRAL),
        ("per-call shape (64 KiB, warm)", "per_call/65536", _NEUTRAL),
        ("overflow — cold first block (malloc)", "overflow/cold_first_block", _SLOW),
    ]
    rows: list = [
        (label, per_call_ns(criterion_dir, f"call_arena/{fid}"), color)
        for label, fid, color in bars
    ]
    _chart_hbar_linear(
        out,
        "Per-call return buffer: the retain-and-rewind win",
        "CallArena costs — a warm bump is ~free; the first overflow mallocs once, "
        "then every later call reuses the retained block (lower is better)",
        rows,
    )


def chart_cold_start(criterion_dir: Path, out: Path) -> None:
    """First dispatch into a just-registered contract vs warm dispatch (log bars).

    All bars live from the `cold_start` group: the cold bar is the FIRST find +
    resolve + dispatch on a freshly populated registry (everything cache-cold);
    the warm bars are the same path hot in cache, and the cached-pointer hot path.
    """
    rows: list = [
        ("cold — first dispatch (cache-cold)", per_call_ns(criterion_dir, "cold_start/cold/first_dispatch"), _SLOW),
        ("warm — find + resolve + dispatch", per_call_ns(criterion_dir, "cold_start/warm/find_resolve_dispatch"), _NEUTRAL),
        ("warm — cached pointer dispatch", per_call_ns(criterion_dir, "cold_start/warm/cached_dispatch"), _HILITE),
    ]
    _chart_hbar_log(
        out,
        "First call is cold; steady state is warm",
        "the very first dispatch into a just-loaded contract pays a cold-cache tax; "
        "every call after is warm (log scale, lower is better)",
        rows,
        "The cold bar is the first find + resolve + dispatch on a freshly populated "
        "registry — a cold HashMap probe + cold interface chase + cold-icache run of "
        "the dispatch path. Warm find+resolve+dispatch is the same work hot in cache; "
        "caching the resolved pointer (resolve once, dispatch many) is the floor. The "
        "cold tax is paid roughly once per contract, then amortizes away.",
    )


def chart_hero(criterion_dir: Path, out: Path) -> None:
    """README hero (embedded only in README.md, not PERFORMANCE.md): one plugin
    call end to end, every bar live from criterion.

    One log-scale chart that tells the whole story at a glance: what a plugin
    call costs next to a direct call and raw FFI, and what each VM language
    adds. Sources (all `median.point_estimate`, per-call via throughput):
      - direct call / raw FFI / polyplug native : the counter_inc_1m arms
      - .NET   : clr_dispatch/clr_init_call (warm [UnmanagedCallersOnly])
      - Lua    : lua_dispatch/vm_dispatch_single_call (warm LuaJIT call)
      - JS     : cached_dispatch/cached_context_single_call (cached QuickJS ctx)
      - Python : cached_dispatch/cached_python_single_call (GIL held, cached fn)
    """
    rows: list = [
        ("direct function call (no plugins)", per_call_ns(criterion_dir, "counter_inc_1m/native/inline_never"), _FLOOR),
        ("raw FFI (dlsym, no safety)", per_call_ns(criterion_dir, "counter_inc_1m/ffi/by_value"), _FLOOR),
        ("polyplug — native plugin (Rust/C++)", per_call_ns(criterion_dir, "counter_inc_1m/polyplug/dispatch"), _HILITE),
        ("polyplug — .NET plugin", per_call_ns(criterion_dir, "clr_dispatch/clr_init_call"), _NEUTRAL),
        ("polyplug — Lua plugin (LuaJIT)", per_call_ns(criterion_dir, "lua_dispatch/vm_dispatch_single_call"), _NEUTRAL),
        ("polyplug — JS plugin (QuickJS)", per_call_ns(criterion_dir, "cached_dispatch/cached_context_single_call"), _NEUTRAL),
        ("polyplug — Python plugin (cached)", per_call_ns(criterion_dir, "cached_dispatch/cached_python_single_call"), _SLOW),
    ]
    _chart_hbar_log(
        out,
        "One plugin call, end to end",
        "time to call one plugin function and get the answer back, by plugin language "
        "(log scale, lower is better)",
        rows,
        "All bars measured live by cargo bench on one machine. A native plugin costs about "
        "one extra nanosecond over a raw function call; VM plugins add their interpreter's "
        "warm per-call cost (Python: GIL held, function cached — its attach-per-call arm is "
        "close behind, see PERFORMANCE.md). A bar twice as long is 10x slower (log scale).",
    )


# ─── cross-language comparison (log-scale horizontal bars) ────────────────────
#
# These two charts compare the *languages* against each other, in both
# directions of the boundary:
#   - guest = the runtime dispatching INTO a plugin written in language X
#   - host  = an application written in language X calling INTO the runtime
#
# The guest bars are read live from each loader's warm steady-state dispatch
# bench (VM/context already created, function cached). The host bars are read
# live from the HOSTCALL sweep (`examples/hosts/roundtrip_bench.sh --hostcall`),
# which times one find_guest_contract call through the runtime in every example
# host. All numbers are from one machine; trust the ordering.


def chart_cross_language_guest(criterion_dir: Path, out: Path) -> None:
    """Per-call dispatch cost, runtime -> guest, by plugin language (log scale).

    Every bar is read live from the warm steady-state dispatch bench for that
    language, so the chart can never drift from the measured numbers:
      - Rust / C++  : counter_inc (native cdylib, marshalled call)
      - .NET        : clr_dispatch/clr_init_call (warm [UnmanagedCallersOnly])
      - Lua         : lua_dispatch/vm_dispatch_single_call (warm LuaJIT call)
      - Python      : cached_dispatch/cached_python_single_call (GIL held, cached fn)
      - JavaScript  : cached_dispatch/cached_context_single_call (cached QuickJS ctx)
    """
    rows: list = [
        ("Rust (native cdylib)", per_call_ns(criterion_dir, "counter_inc_1m/polyplug/dispatch"), _HILITE),
        ("C++ (native cdylib)", per_call_ns(criterion_dir, "counter_inc_1m/polyplug/dispatch_cpp"), _HILITE),
        (".NET (CLR, UnmanagedCallersOnly)", per_call_ns(criterion_dir, "clr_dispatch/clr_init_call"), _NEUTRAL),
        ("Lua (LuaJIT)", per_call_ns(criterion_dir, "lua_dispatch/vm_dispatch_single_call"), _NEUTRAL),
        ("Python (GIL held, cached)", per_call_ns(criterion_dir, "cached_dispatch/cached_python_single_call"), _NEUTRAL),
        ("JavaScript (QuickJS, cached)", per_call_ns(criterion_dir, "cached_dispatch/cached_context_single_call"), _NEUTRAL),
    ]
    _chart_hbar_log(
        out,
        "Calling into a plugin, by plugin language",
        "warm per-call cost the runtime pays to run a plugin written in each language (log scale, lower is better)",
        rows,
        "All bars measured live: native from counter_inc; .NET/Lua/Python/JS from each loader's "
        "warm cached-dispatch bench. Python's attach-per-call arm (gil_acquire_and_call) measures "
        "~56 ns — almost the same as the cached bar, since an uncontended GIL re-attach is nearly "
        "free; the old ~13 µs figure was a recompile-per-iteration artifact (see PERFORMANCE.md).",
    )


def _read_matrix(path: Path) -> dict:
    """Parse `<host> <guest> <ns>` lines from examples/hosts/roundtrip_bench.sh."""
    data: dict = {}
    for line in path.read_text().splitlines():
        parts: list = line.split()
        if len(parts) == 3:
            try:
                data[(parts[0], parts[1])] = float(parts[2])
            except ValueError:
                continue
    return data


# Display names + axis order for the cross-language matrix. Hosts down the rows,
# guests across the columns; every host×guest pairing is measured.
_LANG_LABEL: dict = {
    "rust": "Rust",
    "cpp": "C++",
    "csharp": "C#",
    "lua": "Lua",
    "js": "JavaScript",
    "python": "Python",
}
# ONE canonical language order (compiled → scripted) drives every axis and
# label, so the same language sits in the same place across all charts. Bar
# charts additionally sort by value; the matrix is a 2D grid that cannot be
# value-sorted on both axes, so it uses this order on both for consistency.
_LANG_ORDER: list = list(_LANG_LABEL)
_MATRIX_HOSTS: list = _LANG_ORDER
_MATRIX_GUESTS: list = _LANG_ORDER


def chart_cross_language_matrix(data_path: Path, out: Path) -> None:
    """Full host × guest round-trip heatmap, measured end to end.

    Reads `<host> <guest> <ns>` rows produced by examples/hosts/roundtrip_bench.sh.
    Each cell is one host language calling a guest plugin of another language and
    reading the result back — every combination, color-coded by speed.
    """
    raw: dict = _read_matrix(data_path)
    row_labels: list = [_LANG_LABEL[h] for h in _MATRIX_HOSTS]
    col_labels: list = [_LANG_LABEL[g] for g in _MATRIX_GUESTS]
    cells: dict = {}
    for host in _MATRIX_HOSTS:
        for guest in _MATRIX_GUESTS:
            cells[(_LANG_LABEL[host], _LANG_LABEL[guest])] = raw.get((host, guest))
    if not any(v is not None for v in cells.values()):
        print(f"  skip {out.name}: no matrix data in {data_path}", file=sys.stderr)
        return
    _chart_heatmap(
        out,
        "Call cost: any app language × any plugin language",
        "one host calls one plugin's decode() and reads the string back — full round trip, lower is better",
        col_labels,
        row_labels,
        cells,
        "Each cell is measured end to end (build the argument, call, read the returned string). "
        "Compiled hosts (Rust/C++/C#) add a small constant; scripted hosts pay per-call marshalling. "
        "A C# app loading a C# plugin reuses the host's own .NET runtime. Local-only; see examples/hosts/roundtrip_bench.sh.",
    )


def _read_hostcall(path: Path) -> dict:
    """Parse `<host> <ns>` lines from examples/hosts/roundtrip_bench.sh --hostcall."""
    data: dict = {}
    for line in path.read_text().splitlines():
        parts: list = line.split()
        if len(parts) == 2:
            try:
                data[parts[0]] = float(parts[1])
            except ValueError:
                continue
    return data


# Fixed row order + per-host label/color for the host-call chart. The mechanism
# each host pays to cross into the runtime is part of the label so the chart
# explains itself.
_HOSTCALL_ROWS: list = [
    ("rust", "Rust (links the crate, no FFI)", _HILITE),
    ("cpp", "C++ (C ABI)", _NEUTRAL),
    ("csharp", "C# (.NET function pointer)", _NEUTRAL),
    ("lua", "Lua (LuaJIT FFI)", _NEUTRAL),
    ("js", "JavaScript (Deno FFI)", _NEUTRAL),
    ("python", "Python (ctypes)", _SLOW),
]


def chart_cross_language_host(data_path: Path, out: Path) -> bool:
    """Per-call host → runtime cost, by host language (log scale), measured.

    Reads `<host> <ns>` rows produced by `examples/hosts/roundtrip_bench.sh
    --hostcall`: each example host times one find_guest_contract call through
    the runtime (one FFI hop + the registry lookup, no guest dispatch) in a
    POLYPLUG_BENCH_ITERS-gated loop. Without a data file the chart is left
    untouched — it never regenerates from assumptions. Returns True if the
    SVG was written.
    """
    if not data_path.is_file():
        print(
            f"  skip {out.name}: no host-call data file at {data_path} "
            "(run examples/hosts/roundtrip_bench.sh --hostcall)",
            file=sys.stderr,
        )
        return False
    raw: dict = _read_hostcall(data_path)
    rows: list = [
        (label, raw[host], color) for host, label, color in _HOSTCALL_ROWS if host in raw
    ]
    if not rows:
        print(f"  skip {out.name}: no host-call data in {data_path}", file=sys.stderr)
        return False
    _chart_hbar_log(
        out,
        "Reaching the runtime, by app language",
        "one find_guest_contract call through the runtime, per host language — "
        "measured, log scale, lower is better",
        rows,
        "Each bar is measured live: the example host for that language calls "
        "find_guest_contract in a tight loop (one FFI hop + the runtime's registry lookup; "
        "no guest code runs). Rust links the crate, so its bar is the lookup itself. "
        "Local-only; reproduce with `just bench-hostcall`. See docs/PERFORMANCE.md.",
    )
    return True


def _read_soak(path: Path) -> list:
    """Parse `<cycle> <rss_kib>` lines from the load/unload soak harness.

    Produced by the gated `soak_load_unload_churn` test
    (`crates/polyplug/tests/soak_load_unload.rs`) when `POLYPLUG_SOAK_OUT` is
    set. Returns a list of (cycle, rss_kib) tuples in file order.
    """
    rows: list = []
    for line in path.read_text().splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        parts: list = line.split()
        if len(parts) == 2:
            try:
                rows.append((int(parts[0]), int(parts[1])))
            except ValueError:
                continue
    return rows


def chart_soak_rss(data_path: Path, out: Path) -> bool:
    """Process RSS over a load → dispatch → unload → drop soak (linear axes).

    Reads `<cycle> <rss_kib>` rows produced by the gated soak harness. A
    *linear* y axis is used on purpose: a leak shows as a straight upward slope
    and a steady-state run shows as a flat line — both must be read at face
    value, which a log axis would distort. Without a data file the chart is left
    untouched (it never regenerates from assumptions). Returns True if written.
    """
    if not data_path.is_file():
        print(
            f"  skip {out.name}: no soak data file at {data_path} "
            "(run the gated soak with POLYPLUG_SOAK_OUT set)",
            file=sys.stderr,
        )
        return False
    rows: list = _read_soak(data_path)
    if len(rows) < 2:
        print(f"  skip {out.name}: not enough soak samples in {data_path}", file=sys.stderr)
        return False

    width: int = 720
    title: str = "Memory across many load → unload → drop cycles"
    subtitle: str = (
        "process RSS sampled over a full-teardown soak — each cycle builds a fresh "
        "Runtime, loads a native plugin, dispatches it, unloads, then drops the Runtime"
    )
    header_svg, extra_y = _header(title, subtitle, width)
    height: int = 420 + extra_y
    pad_l: int = 72
    pad_r: int = 24
    pad_t: int = 64 + extra_y
    pad_b: int = 56
    plot_w: int = width - pad_l - pad_r
    plot_h: int = height - pad_t - pad_b

    xs: list = [c for c, _ in rows]
    ys: list = [r for _, r in rows]
    x_max: int = max(xs) if max(xs) > 0 else 1
    y_lo: int = 0
    y_hi: float = max(ys) * 1.1 if max(ys) > 0 else 1.0

    def x_of(c: int) -> float:
        return pad_l + plot_w * c / x_max

    def y_of(v: float) -> float:
        return pad_t + plot_h * (1 - (v - y_lo) / (y_hi - y_lo))

    parts: list = [header_svg]

    # Horizontal grid + y labels (KiB), 4 steps.
    for i in range(5):
        v: float = y_lo + (y_hi - y_lo) * i / 4
        gy: float = y_of(v)
        parts.append(_line(pad_l, gy, pad_l + plot_w, gy, _GRID, 1))
        parts.append(_text(pad_l - 8, gy + 4, f"{v / 1024:.1f}", 10, _MUTED, "end"))
    parts.append(_text(24, pad_t + plot_h / 2, "MiB", 11, _MUTED, "start"))

    # Vertical grid + x labels (cycles), 4 steps.
    for i in range(5):
        c: int = round(x_max * i / 4)
        gx: float = x_of(c)
        parts.append(_line(gx, pad_t, gx, pad_t + plot_h, _GRID, 1))
        parts.append(_text(gx, pad_t + plot_h + 18, f"{c:,}", 10, _MUTED, "middle"))
    parts.append(
        _text(
            pad_l + plot_w / 2,
            height - 8,
            "load/unload cycles  ·  linear y axis (a leak slopes up; flat = no leak)",
            11,
            _MUTED,
            "middle",
        )
    )

    # The RSS polyline. A rising slope here is a leak signal; the color flags it.
    # Drift mirrors the soak test's steady-state heuristic exactly
    # (`soak_load_unload.rs`): skip the first quartile as warmup / allocator-arena
    # growth, then compare the last-quartile mean to the second-quartile mean. A
    # naive first-vs-last ratio would count the one-time warmup jump as a leak.
    q: int = max(len(ys) // 4, 1)
    mid_slice: list = ys[q : min(2 * q, len(ys))]
    tail_slice: list = ys[len(ys) - q :]
    mid_mean: float = sum(mid_slice) / len(mid_slice) if mid_slice else 0.0
    tail_mean: float = sum(tail_slice) / len(tail_slice) if tail_slice else 0.0
    drift: float = (tail_mean - mid_mean) / mid_mean if mid_mean else 0.0
    line_color: str = _SLOW if drift > 0.10 else _HILITE
    pts: list = [f"{x_of(c):.1f},{y_of(v):.1f}" for c, v in rows]
    parts.append(
        f"<polyline points='{' '.join(pts)}' fill='none' stroke='{line_color}' stroke-width='2.5'/>"
    )
    for c, v in rows:
        parts.append(f"<circle cx='{x_of(c):.1f}' cy='{y_of(v):.1f}' r='2.5' fill='{line_color}'/>")

    verdict: str = (
        f"RSS climbs {drift * 100:+.0f}% across the run (a flat line would mean no leak)."
        if drift > 0.10
        else f"RSS stays flat ({drift * 100:+.0f}%) — full-teardown cycling reclaims each Runtime."
    )
    parts.append(_text(pad_l + 12, pad_t + 14, verdict, 10, line_color, "start"))

    out.write_text(_svg(width, height, "".join(parts)))
    return True


# ─── entry point ──────────────────────────────────────────────────────────────


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("criterion_dir", type=Path, help="target/criterion")
    parser.add_argument("out_dir", type=Path, help="directory to write SVGs into")
    parser.add_argument(
        "--matrix",
        type=Path,
        metavar="DATAFILE",
        help="render ONLY the cross-language matrix from a `<host> <guest> <ns>` "
        "data file (no criterion data required — used by examples/hosts/roundtrip_bench.sh)",
    )
    parser.add_argument(
        "--hostcall",
        type=Path,
        metavar="DATAFILE",
        help="render ONLY the host-call chart (cross_lang_host.svg) from a "
        "`<host> <ns>` data file (no criterion data required — used by "
        "examples/hosts/roundtrip_bench.sh --hostcall)",
    )
    parser.add_argument(
        "--soak",
        type=Path,
        metavar="DATAFILE",
        help="render ONLY the load/unload soak chart (soak_rss.svg) from a "
        "`<cycle> <rss_kib>` data file (no criterion data required — produced by "
        "the gated soak_load_unload_churn test with POLYPLUG_SOAK_OUT set)",
    )
    args = parser.parse_args()

    criterion_dir: Path = args.criterion_dir
    out_dir: Path = args.out_dir
    out_dir.mkdir(parents=True, exist_ok=True)

    # Matrix / hostcall modes: the data comes from a live sweep of the example
    # hosts, not criterion, so each renders on its own and never touches the
    # criterion-sourced charts.
    if args.matrix is not None:
        chart_cross_language_matrix(args.matrix, out_dir / "cross_lang_matrix.svg")
        print(f"wrote {out_dir / 'cross_lang_matrix.svg'}")
        return 0
    if args.hostcall is not None:
        if chart_cross_language_host(args.hostcall, out_dir / "cross_lang_host.svg"):
            print(f"wrote {out_dir / 'cross_lang_host.svg'}")
        return 0
    if args.soak is not None:
        if chart_soak_rss(args.soak, out_dir / "soak_rss.svg"):
            print(f"wrote {out_dir / 'soak_rss.svg'}")
        return 0

    if not criterion_dir.is_dir():
        print(f"error: {criterion_dir} is not a directory (run cargo bench first)", file=sys.stderr)
        return 1

    # cross_lang_host.svg is NOT in this list: it renders only from a live
    # --hostcall sweep (it has no criterion source and never regenerates from
    # stale data). cross_lang_matrix.svg likewise renders only via --matrix.
    charts: list = [
        ("hero.svg", chart_hero),
        ("counter_inc.svg", chart_counter_inc),
        ("dispatch_by_shape.svg", chart_dispatch_by_shape),
        ("payload_scaling.svg", chart_payload_scaling),
        ("marshalling.svg", chart_marshalling),
        ("native_round_trip.svg", chart_native_round_trip),
        ("amortization.svg", chart_amortization),
        ("cross_lang_guest.svg", chart_cross_language_guest),
        ("call_arena.svg", chart_call_arena),
        ("cold_start.svg", chart_cold_start),
    ]
    for name, render in charts:
        target: Path = out_dir / name
        try:
            render(criterion_dir, target)
        except (FileNotFoundError, KeyError) as error:
            print(f"error: cannot render {name}: missing data ({error})", file=sys.stderr)
            print("       run the full `cargo bench` across polyplug + loader crates first.", file=sys.stderr)
            return 1
        print(f"wrote {target}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

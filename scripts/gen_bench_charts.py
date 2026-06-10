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
    if value >= 1000.0:
        return f"{value / 1000.0:.1f} µs"
    if value >= 100.0:
        return f"{value:.0f} ns"
    if value >= 10.0:
        return f"{value:.1f} ns"
    return f"{value:.2f} ns"


# ─── reusable chart layouts ───────────────────────────────────────────────────


def _chart_hbar_linear(out: Path, title: str, subtitle: str, rows: list) -> None:
    """Horizontal linear-scale bar chart; rows = [(label, ns, color)]."""
    width: int = 720
    pad_l: int = 250
    pad_r: int = 96
    pad_t: int = 56
    row_h: int = 46
    height: int = pad_t + row_h * len(rows) + 30
    plot_w: int = width - pad_l - pad_r
    vmax: float = max(ns for _, ns, _ in rows) * 1.12

    parts: list = [
        _text(24, 30, title, 17, _FG, "start"),
        _text(24, 47, subtitle, 11, _MUTED, "start"),
    ]
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
    """Horizontal log-scale bar chart; rows = [(label, ns, color)]."""
    width: int = 760
    pad_l: int = 250
    pad_r: int = 96
    pad_t: int = 60
    row_h: int = 40
    height: int = pad_t + row_h * len(rows) + 52
    plot_w: int = width - pad_l - pad_r

    vmax: float = max(ns for _, ns, _ in rows)
    axis_hi: float = math.log10(vmax * 1.6)

    def x_of(ns: float) -> float:
        # Log axis anchored at 1 ns; 0.3-decade floor so the fastest bar is visible.
        frac: float = max(math.log10(ns), 0.3) / axis_hi
        return pad_l + plot_w * frac

    parts: list = [
        _text(24, 30, title, 17, _FG, "start"),
        _text(24, 47, subtitle, 11, _MUTED, "start"),
    ]
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
    parts.append(_text(24, height - 12, note, 9, _MUTED, "start"))
    out.write_text(_svg(width, height, "".join(parts)))


def _chart_lines(
    out: Path, title: str, subtitle: str, sizes: list, series: list, data: dict, xlabel: str
) -> None:
    """Log-y line chart over a shared x axis; series = [(key, label, color)]."""
    width: int = 720
    height: int = 420
    pad_l: int = 64
    pad_r: int = 24
    pad_t: int = 64
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

    parts: list = [
        _text(24, 30, title, 17, _FG, "start"),
        _text(24, 47, subtitle, 11, _MUTED, "start"),
    ]
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


# ─── charts ───────────────────────────────────────────────────────────────────


def chart_counter_inc(criterion_dir: Path, out: Path) -> None:
    """Per-call cost of each counter_inc mechanism (linear bars)."""
    bars: list = [
        ("native/inline_never", "direct call (floor)", _FLOOR),
        ("ffi/by_value", "raw dlsym FFI", _NEUTRAL),
        ("native/abi_marshalled", "ABI convention (static)", _NEUTRAL),
        ("polyplug/dispatch", "polyplug — Rust plugin", _HILITE),
        ("polyplug/dispatch_cpp", "polyplug — C++ plugin", _HILITE),
    ]
    rows: list = [
        (label, per_call_ns(criterion_dir, f"counter_inc_1m/{fid}"), color)
        for fid, label, color in bars
    ]
    _chart_hbar_linear(
        out,
        "counter_inc — per-call cost (lower is better)",
        "same 1,000,000x loop, inc reached a different way each bar",
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
        "Dispatch cost by argument shape (lower is better)",
        "scalar args are ~free; a buffer fill and a cross-plugin lookup add real work",
        rows,
    )


def chart_payload_scaling(criterion_dir: Path, out: Path) -> None:
    """Native vs polyplug per-call cost across payload sizes (log-y lines)."""
    sizes: list = [0, 16, 64, 256, 1024, 4096, 16384]
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
        "payload_scaling — overhead vanishes as work grows",
        "per-call cost vs bytes written (log scale); the lines converge",
        sizes,
        series,
        data,
        "payload (bytes)",
    )


def chart_marshalling(criterion_dir: Path, out: Path) -> None:
    """Borrowed view vs owned copy return cost across payload sizes (log-y lines)."""
    sizes: list = [16, 256, 4096, 16384]
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
        "Return marshalling — borrowed view vs owned copy",
        "per-call cost vs payload bytes (log scale); borrowed is flat, owned scales",
        sizes,
        series,
        data,
        "payload (bytes)",
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
        ("scalar return (u32) — the inc() round trip", per_call_ns(criterion_dir, "counter_inc_1m/polyplug/dispatch"), _HILITE),
        ("borrowed view return (256 B)", per_call_ns(criterion_dir, "marshalling/borrowed/256"), _HILITE),
        ("owned copy return (256 B)", per_call_ns(criterion_dir, "marshalling/owned/256"), _SLOW),
        ("owned copy return (16 KB)", per_call_ns(criterion_dir, "marshalling/owned/16384"), _SLOW),
    ]
    _chart_hbar_log(
        out,
        "Native round trip  (host → guest → return)",
        "Rust host, pre-resolved interface; cost by what the guest returns (log scale)",
        rows,
        "The round trip itself is ~2 ns — resolve is cached, dispatch is one indirect call. "
        "The only thing that grows the cost is copying data out: borrowed zero-copy views stay "
        "flat, owned copies pay a host alloc + memcpy that scales with the payload.",
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
        "Amortized one-time costs  (pay once, not per call)",
        "log scale — these happen at load/reload, never on the dispatch hot path",
        rows,
        "find+resolve is the only one a caller might repeat — and it is ~20 ns (a HashMap hit). "
        "Load and reload are dominated by the OS dlopen/mmap, not polyplug; see PROFILING.md.",
    )


# ─── cross-language comparison (log-scale horizontal bars) ────────────────────
#
# These two charts compare the *languages* against each other, in both
# directions of the boundary:
#   - guest = the runtime dispatching INTO a plugin written in language X
#   - host  = an application written in language X calling INTO the runtime
#
# The guest bars are read live from each loader's warm steady-state dispatch
# bench (VM/context already created, function cached) so they stay current and
# comparable. The host-FFI numbers are the measured figures documented in
# docs/PERFORMANCE.md (host micro-benchmarks need each language's runtime to
# reproduce). All numbers are illustrative — one machine; trust the ordering.


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
        "Guest dispatch by language  (runtime → plugin)",
        "warm steady-state per-call cost, log scale — lower is better",
        rows,
        "All bars measured live: native from counter_inc; .NET/Lua/Python/JS from each loader's "
        "warm cached-dispatch bench. Python additionally pays a one-time ~12 µs GIL acquire + compile "
        "on a cold call (see PERFORMANCE.md).",
    )


def chart_cross_language_host(criterion_dir: Path, out: Path) -> None:
    """Per-call FFI overhead, host -> runtime, by host language (log scale)."""
    rows: list = [
        ("Rust (links the crate, no FFI)", 2.0, _HILITE),
        ("C++ (native)", 15.0, _NEUTRAL),
        ("Lua (LuaJIT FFI)", 35.0, _NEUTRAL),
        ("JavaScript (Deno FFI)", 75.0, _NEUTRAL),
        ("Python (cffi ABI)", 380.0, _SLOW),
        ("Python (ctypes)", 670.0, _SLOW),
    ]
    _chart_hbar_log(
        out,
        "Host call overhead by language  (host → runtime)",
        "per-call FFI cost to reach the runtime, log scale — lower is better",
        rows,
        "A Rust host links libpolyplug directly — there is no FFI boundary, so it is "
        "the floor. C++/Lua/JS are the fast FFI end; Python's dynamic FFI is the cost "
        "of its convenience. Host FFI micro-benchmarks — see docs/PERFORMANCE.md.",
    )


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

    charts: list = [
        ("counter_inc.svg", chart_counter_inc),
        ("dispatch_by_shape.svg", chart_dispatch_by_shape),
        ("payload_scaling.svg", chart_payload_scaling),
        ("marshalling.svg", chart_marshalling),
        ("native_round_trip.svg", chart_native_round_trip),
        ("amortization.svg", chart_amortization),
        ("cross_lang_guest.svg", chart_cross_language_guest),
        ("cross_lang_host.svg", chart_cross_language_host),
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

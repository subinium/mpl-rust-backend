"""Benchmark mpl_rust_backend vs matplotlib Agg — speed and visual quality.

Usage:
    python benchmarks/bench_compare.py

Outputs:
    - Console table with timing, file size, pixel diff metrics
    - benchmarks/artifacts/{case}_agg.png, {case}_rust.png, {case}_diff.png
"""

from __future__ import annotations

import io
import os
import statistics
import sys
import time
from pathlib import Path

import numpy as np
from matplotlib.backends.backend_agg import FigureCanvasAgg
from matplotlib.figure import Figure
from PIL import Image

from mpl_rust_backend._canvas import FigureCanvasRust

ARTIFACTS_DIR = Path(__file__).parent / "artifacts"
WARMUP_RUNS = 1
TIMED_RUNS = 3
# Two different rendering engines (Agg/FreeType vs tiny_skia/fontdue) will always
# have text glyph and anti-aliasing differences.  MSE ~400-1000 is normal.
MSE_THRESHOLD = 1500.0  # structural match: same layout, font/AA diffs only

# ---------------------------------------------------------------------------
# Benchmark case definitions
# ---------------------------------------------------------------------------


def case_line(fig: Figure, ax, n: int = 1_000) -> str:
    rng = np.random.default_rng(0)
    x = np.linspace(0, 10, n)
    y = np.sin(x) + rng.normal(0, 0.1, n)
    ax.plot(x, y)
    ax.set_title(f"Line plot ({n:,} pts)")
    return f"line_{n}"


def case_scatter(fig: Figure, ax, n: int = 500) -> str:
    rng = np.random.default_rng(1)
    x = rng.normal(size=n)
    y = rng.normal(size=n)
    ax.scatter(x, y, alpha=0.6)
    ax.set_title(f"Scatter ({n:,} pts)")
    return f"scatter_{n}"


def case_bar(fig: Figure, ax, n: int = 10) -> str:
    rng = np.random.default_rng(2)
    labels = [f"c{i}" for i in range(n)]
    values = rng.integers(1, 100, size=n)
    ax.bar(labels, values)
    ax.set_title(f"Bar chart ({n} bars)")
    if n > 20:
        ax.tick_params(axis="x", labelsize=6, rotation=90)
    return f"bar_{n}"


def case_heatmap(fig: Figure, ax, size: int = 64) -> str:
    rng = np.random.default_rng(3)
    data = rng.normal(size=(size, size))
    ax.imshow(data, cmap="viridis", aspect="auto")
    ax.set_title(f"Heatmap ({size}x{size})")
    return f"heatmap_{size}"


def case_text_heavy(fig: Figure, ax, **_) -> str:
    ax.plot([0, 1, 2], [1, 3, 2], label="data")
    ax.set_title("Title with $math$ symbols")
    ax.set_xlabel("X axis label")
    ax.set_ylabel("Y axis label")
    ax.legend(loc="upper right")
    ax.annotate("peak", xy=(1, 3), xytext=(1.5, 2.5), arrowprops=dict(arrowstyle="->"))
    return "text_heavy"


def case_subplots(fig: Figure, ax, **_) -> str:
    """ax is ignored; we create a 2x2 grid on fig directly."""
    ax.remove()
    axes = fig.subplots(2, 2)
    rng = np.random.default_rng(4)
    for i, a in enumerate(axes.flat):
        x = np.linspace(0, 5, 200)
        a.plot(x, np.sin(x + i), label=f"sin(x+{i})")
        a.legend(fontsize=7)
    fig.suptitle("2x2 Subplots")
    return "subplots"


def case_fill_between(fig: Figure, ax, n: int = 1_000) -> str:
    x = np.linspace(0, 4 * np.pi, n)
    y1 = np.sin(x)
    y2 = np.sin(x) * 0.5
    ax.fill_between(x, y1, y2, alpha=0.4)
    ax.plot(x, y1, "b-")
    ax.set_title(f"fill_between ({n:,} pts)")
    return f"fill_between_{n}"


# Registry: (factory, kwargs_list)
BENCHMARKS: list[tuple[callable, list[dict]]] = [
    (case_line, [{"n": 1_000}, {"n": 10_000}, {"n": 100_000}]),
    (case_scatter, [{"n": 500}, {"n": 5_000}, {"n": 50_000}]),
    (case_bar, [{"n": 10}, {"n": 100}]),
    (case_heatmap, [{"size": 64}, {"size": 256}]),
    (case_text_heavy, [{}]),
    (case_subplots, [{}]),
    (case_fill_between, [{"n": 1_000}]),
]

# ---------------------------------------------------------------------------
# Rendering helpers
# ---------------------------------------------------------------------------


def _render_png(canvas_cls, case_fn, kwargs: dict) -> tuple[bytes, float]:
    """Render a case and return (png_bytes, median_ms).

    Runs WARMUP_RUNS warmups then TIMED_RUNS timed iterations.
    """
    timings: list[float] = []
    png_data: bytes = b""

    for i in range(WARMUP_RUNS + TIMED_RUNS):
        fig = Figure(figsize=(6.4, 4.8), dpi=100)
        canvas = canvas_cls(fig)
        ax = fig.add_subplot(111)

        case_fn(fig, ax, **kwargs)

        buf = io.BytesIO()
        t0 = time.perf_counter()
        fig.savefig(buf, format="png")
        elapsed = (time.perf_counter() - t0) * 1000  # ms

        if i >= WARMUP_RUNS:
            timings.append(elapsed)
            png_data = buf.getvalue()

        del fig, canvas

    return png_data, statistics.median(timings)


def _compute_diff(
    agg_bytes: bytes, rust_bytes: bytes
) -> tuple[float, float, np.ndarray]:
    """Compare two PNGs. Returns (mse, max_error, diff_image_array)."""
    agg_img = np.array(
        Image.open(io.BytesIO(agg_bytes)).convert("RGB"), dtype=np.float64
    )
    rust_img = np.array(
        Image.open(io.BytesIO(rust_bytes)).convert("RGB"), dtype=np.float64
    )

    # Handle potential size mismatch by cropping to minimum
    h = min(agg_img.shape[0], rust_img.shape[0])
    w = min(agg_img.shape[1], rust_img.shape[1])
    agg_img = agg_img[:h, :w]
    rust_img = rust_img[:h, :w]

    diff = agg_img - rust_img
    mse = float(np.mean(diff**2))
    max_err = float(np.max(np.abs(diff)))

    # Amplified diff image for visual inspection
    diff_vis = np.clip(np.abs(diff) * 10, 0, 255).astype(np.uint8)
    return mse, max_err, diff_vis


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------


def main() -> int:
    ARTIFACTS_DIR.mkdir(parents=True, exist_ok=True)

    results: list[dict] = []

    for case_fn, kwargs_list in BENCHMARKS:
        for kwargs in kwargs_list:
            # Determine case name via a dry-run
            tmp_fig = Figure()
            tmp_ax = tmp_fig.add_subplot(111)
            name = case_fn(tmp_fig, tmp_ax, **kwargs)
            del tmp_fig, tmp_ax

            print(f"  Running: {name} ...", end="", flush=True)

            # Render with both backends
            agg_png, agg_ms = _render_png(FigureCanvasAgg, case_fn, kwargs)
            rust_png, rust_ms = _render_png(FigureCanvasRust, case_fn, kwargs)

            # Pixel comparison
            mse, max_err, diff_vis = _compute_diff(agg_png, rust_png)
            verdict = "MATCH" if mse < MSE_THRESHOLD else "DIFF"

            speedup = agg_ms / rust_ms if rust_ms > 0 else float("inf")

            results.append(
                {
                    "name": name,
                    "agg_ms": agg_ms,
                    "rust_ms": rust_ms,
                    "speedup": speedup,
                    "agg_kb": len(agg_png) / 1024,
                    "rust_kb": len(rust_png) / 1024,
                    "mse": mse,
                    "max_err": max_err,
                    "verdict": verdict,
                }
            )

            # Save artifacts
            (ARTIFACTS_DIR / f"{name}_agg.png").write_bytes(agg_png)
            (ARTIFACTS_DIR / f"{name}_rust.png").write_bytes(rust_png)
            Image.fromarray(diff_vis).save(ARTIFACTS_DIR / f"{name}_diff.png")

            print(f" {verdict} ({speedup:.2f}x)")

    # Print summary table
    print()
    header = (
        f"{'Case':<25} {'Agg (ms)':>9} {'Rust (ms)':>10} {'Speedup':>8} "
        f"{'Agg KB':>7} {'Rust KB':>8} {'MSE':>10} {'MaxErr':>7} {'Verdict':>8}"
    )
    print(header)
    print("-" * len(header))
    for r in results:
        print(
            f"{r['name']:<25} {r['agg_ms']:>9.1f} {r['rust_ms']:>10.1f} "
            f"{r['speedup']:>7.2f}x {r['agg_kb']:>7.1f} {r['rust_kb']:>8.1f} "
            f"{r['mse']:>10.1f} {r['max_err']:>7.0f} {r['verdict']:>8}"
        )

    # Summary stats
    match_count = sum(1 for r in results if r["verdict"] == "MATCH")
    total = len(results)
    avg_speedup = statistics.mean(r["speedup"] for r in results)
    print(f"\nVisual match: {match_count}/{total}")
    print(f"Average speedup: {avg_speedup:.2f}x")

    # Return non-zero if any case has DIFF
    return 0 if match_count == total else 1


if __name__ == "__main__":
    sys.exit(main())

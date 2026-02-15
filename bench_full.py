"""Comprehensive speed benchmark: matplotlib Agg vs mpl_rust_backend.

Tests ALL chart types (basic + extended) and reports a clean comparison table.

Usage:
    python bench_full.py
"""

from __future__ import annotations

import importlib
import io
import math
import statistics
import subprocess
import sys
import time

import numpy as np


# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------

WARMUP = 2
TIMED = 5
DPI = 100
FIGSIZE = (6, 4)
FMT = "png"

BACKENDS = {
    "Agg": "Agg",
    "Rust": "module://mpl_rust_backend",
}

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _run_single(backend_str: str, case_name: str, builder_code: str) -> float:
    """Run a single benchmark in a *subprocess* so the backend is truly fresh.

    Returns median wall-time in ms for savefig().
    """
    script = f"""
import io, time, statistics, numpy as np

import matplotlib
matplotlib.use({backend_str!r})
import matplotlib.pyplot as plt
plt.rcdefaults()

WARMUP = {WARMUP}
TIMED = {TIMED}
DPI = {DPI}
FIGSIZE = {FIGSIZE!r}
FMT = {FMT!r}

rng = np.random.default_rng(42)

timings = []
for _i in range(WARMUP + TIMED):
    # --- build figure ---
{builder_code}
    # --- measure savefig ---
    buf = io.BytesIO()
    t0 = time.perf_counter()
    fig.savefig(buf, format=FMT, dpi=DPI)
    elapsed_ms = (time.perf_counter() - t0) * 1000.0
    if _i >= WARMUP:
        timings.append(elapsed_ms)
    plt.close(fig)

print(statistics.median(timings))
"""
    result = subprocess.run(
        [sys.executable, "-c", script],
        capture_output=True,
        text=True,
        timeout=300,
    )
    if result.returncode != 0:
        raise RuntimeError(
            f"[{case_name}/{backend_str}] subprocess failed:\n{result.stderr.strip()}"
        )
    return float(result.stdout.strip())


# ---------------------------------------------------------------------------
# Benchmark case definitions
# Each value is a multi-line string that will be indented inside the loop.
# It MUST create `fig` (and optionally `ax`).
# `rng` and numpy `np` are already available.
# ---------------------------------------------------------------------------

BASIC_CASES: dict[str, str] = {}
EXTENDED_CASES: dict[str, str] = {}

# --- Basic cases ---

BASIC_CASES[
    "line_1k"
] = """
    fig, ax = plt.subplots(figsize=FIGSIZE)
    x = np.linspace(0, 10, 1_000)
    ax.plot(x, np.sin(x) + rng.normal(0, 0.1, 1_000))
    ax.set_title("Line 1k")
"""

BASIC_CASES[
    "line_10k"
] = """
    fig, ax = plt.subplots(figsize=FIGSIZE)
    x = np.linspace(0, 10, 10_000)
    ax.plot(x, np.sin(x) + rng.normal(0, 0.1, 10_000))
    ax.set_title("Line 10k")
"""

BASIC_CASES[
    "line_100k"
] = """
    fig, ax = plt.subplots(figsize=FIGSIZE)
    x = np.linspace(0, 10, 100_000)
    ax.plot(x, np.sin(x) + rng.normal(0, 0.1, 100_000))
    ax.set_title("Line 100k")
"""

BASIC_CASES[
    "scatter_1k"
] = """
    fig, ax = plt.subplots(figsize=FIGSIZE)
    ax.scatter(rng.normal(size=1_000), rng.normal(size=1_000), alpha=0.6)
    ax.set_title("Scatter 1k")
"""

BASIC_CASES[
    "scatter_10k"
] = """
    fig, ax = plt.subplots(figsize=FIGSIZE)
    ax.scatter(rng.normal(size=10_000), rng.normal(size=10_000), alpha=0.6)
    ax.set_title("Scatter 10k")
"""

BASIC_CASES[
    "scatter_50k"
] = """
    fig, ax = plt.subplots(figsize=FIGSIZE)
    ax.scatter(rng.normal(size=50_000), rng.normal(size=50_000), alpha=0.6, s=2)
    ax.set_title("Scatter 50k")
"""

BASIC_CASES[
    "bar_20"
] = """
    fig, ax = plt.subplots(figsize=FIGSIZE)
    ax.bar([f"c{i}" for i in range(20)], rng.integers(1, 100, size=20))
    ax.set_title("Bar 20")
    ax.tick_params(axis="x", rotation=45)
"""

BASIC_CASES[
    "hist_10k"
] = """
    fig, ax = plt.subplots(figsize=FIGSIZE)
    ax.hist(rng.normal(size=10_000), bins=50)
    ax.set_title("Histogram 10k")
"""

BASIC_CASES[
    "fill_between"
] = """
    fig, ax = plt.subplots(figsize=FIGSIZE)
    x = np.linspace(0, 4 * np.pi, 1_000)
    y1 = np.sin(x)
    y2 = np.sin(x) * 0.5
    ax.fill_between(x, y1, y2, alpha=0.4)
    ax.plot(x, y1, "b-")
    ax.set_title("fill_between")
"""

BASIC_CASES[
    "imshow_100"
] = """
    fig, ax = plt.subplots(figsize=FIGSIZE)
    ax.imshow(rng.normal(size=(100, 100)), cmap="viridis", aspect="auto")
    ax.set_title("imshow 100x100")
"""

BASIC_CASES[
    "imshow_500"
] = """
    fig, ax = plt.subplots(figsize=FIGSIZE)
    ax.imshow(rng.normal(size=(500, 500)), cmap="viridis", aspect="auto")
    ax.set_title("imshow 500x500")
"""

BASIC_CASES[
    "multiline_20"
] = """
    fig, ax = plt.subplots(figsize=FIGSIZE)
    x = np.linspace(0, 10, 500)
    for j in range(20):
        ax.plot(x, np.sin(x + j * 0.3) + rng.normal(0, 0.05, 500), alpha=0.7)
    ax.set_title("20 overlaid lines")
"""

BASIC_CASES[
    "subplots_2x2"
] = """
    fig, axes = plt.subplots(2, 2, figsize=FIGSIZE)
    for i, a in enumerate(axes.flat):
        x = np.linspace(0, 5, 200)
        a.plot(x, np.sin(x + i), label=f"sin(x+{i})")
        a.legend(fontsize=7)
    fig.suptitle("2x2 Subplots")
"""

BASIC_CASES[
    "step_50"
] = """
    fig, ax = plt.subplots(figsize=FIGSIZE)
    x = np.arange(50)
    ax.step(x, rng.integers(0, 20, size=50), where="mid")
    ax.set_title("Step 50")
"""

BASIC_CASES[
    "errorbar_20"
] = """
    fig, ax = plt.subplots(figsize=FIGSIZE)
    x = np.arange(20)
    y = rng.normal(10, 2, size=20)
    yerr = rng.uniform(0.5, 2, size=20)
    ax.errorbar(x, y, yerr=yerr, fmt="o-", capsize=3)
    ax.set_title("Errorbar 20")
"""

# --- Extended cases ---

EXTENDED_CASES[
    "polar_line"
] = """
    fig = plt.figure(figsize=FIGSIZE)
    ax = fig.add_subplot(111, projection="polar")
    theta = np.linspace(0, 2 * np.pi, 200)
    ax.plot(theta, np.abs(np.sin(3 * theta)))
    ax.set_title("Polar line")
"""

EXTENDED_CASES[
    "polar_bar"
] = """
    fig = plt.figure(figsize=FIGSIZE)
    ax = fig.add_subplot(111, projection="polar")
    theta = np.linspace(0, 2 * np.pi, 8, endpoint=False)
    radii = rng.integers(3, 15, size=8)
    width = 2 * np.pi / 8
    ax.bar(theta, radii, width=width, alpha=0.7)
    ax.set_title("Polar bar")
"""

EXTENDED_CASES[
    "polar_scatter"
] = """
    fig = plt.figure(figsize=FIGSIZE)
    ax = fig.add_subplot(111, projection="polar")
    theta = rng.uniform(0, 2 * np.pi, 200)
    r = rng.uniform(0.5, 1.0, 200)
    ax.scatter(theta, r, c=theta, cmap="hsv", alpha=0.7)
    ax.set_title("Polar scatter")
"""

EXTENDED_CASES[
    "3d_surface"
] = """
    from mpl_toolkits.mplot3d import Axes3D
    fig = plt.figure(figsize=FIGSIZE)
    ax = fig.add_subplot(111, projection="3d")
    X = np.linspace(-3, 3, 50)
    Y = np.linspace(-3, 3, 50)
    X, Y = np.meshgrid(X, Y)
    Z = np.sin(np.sqrt(X**2 + Y**2))
    ax.plot_surface(X, Y, Z, cmap="viridis", alpha=0.8)
    ax.set_title("3D Surface")
"""

EXTENDED_CASES[
    "3d_wireframe"
] = """
    from mpl_toolkits.mplot3d import Axes3D
    fig = plt.figure(figsize=FIGSIZE)
    ax = fig.add_subplot(111, projection="3d")
    X = np.linspace(-3, 3, 30)
    Y = np.linspace(-3, 3, 30)
    X, Y = np.meshgrid(X, Y)
    Z = np.cos(X) * np.sin(Y)
    ax.plot_wireframe(X, Y, Z, color="steelblue")
    ax.set_title("3D Wireframe")
"""

EXTENDED_CASES[
    "3d_scatter"
] = """
    from mpl_toolkits.mplot3d import Axes3D
    fig = plt.figure(figsize=FIGSIZE)
    ax = fig.add_subplot(111, projection="3d")
    n = 500
    ax.scatter(rng.normal(size=n), rng.normal(size=n), rng.normal(size=n), alpha=0.6, s=10)
    ax.set_title("3D Scatter")
"""

EXTENDED_CASES[
    "3d_bar"
] = """
    from mpl_toolkits.mplot3d import Axes3D
    fig = plt.figure(figsize=FIGSIZE)
    ax = fig.add_subplot(111, projection="3d")
    xpos = np.arange(5)
    ypos = np.arange(5)
    xpos, ypos = np.meshgrid(xpos, ypos)
    xpos = xpos.flatten()
    ypos = ypos.flatten()
    zpos = np.zeros_like(xpos)
    dz = rng.integers(1, 10, size=len(xpos))
    ax.bar3d(xpos, ypos, zpos, 0.8, 0.8, dz, alpha=0.7)
    ax.set_title("3D Bar")
"""

EXTENDED_CASES[
    "contour"
] = """
    fig, ax = plt.subplots(figsize=FIGSIZE)
    X = np.linspace(-3, 3, 100)
    Y = np.linspace(-3, 3, 100)
    X, Y = np.meshgrid(X, Y)
    Z = np.sin(X) * np.cos(Y)
    ax.contour(X, Y, Z, levels=15)
    ax.set_title("Contour")
"""

EXTENDED_CASES[
    "contourf"
] = """
    fig, ax = plt.subplots(figsize=FIGSIZE)
    X = np.linspace(-3, 3, 100)
    Y = np.linspace(-3, 3, 100)
    X, Y = np.meshgrid(X, Y)
    Z = np.sin(X) * np.cos(Y)
    ax.contourf(X, Y, Z, levels=15, cmap="RdBu_r")
    ax.set_title("Contourf")
"""

EXTENDED_CASES[
    "pie"
] = """
    fig, ax = plt.subplots(figsize=FIGSIZE)
    sizes = [25, 20, 18, 15, 12, 10]
    labels = ["A", "B", "C", "D", "E", "F"]
    ax.pie(sizes, labels=labels, autopct="%1.1f%%", startangle=90)
    ax.set_title("Pie chart")
"""

EXTENDED_CASES[
    "quiver"
] = """
    fig, ax = plt.subplots(figsize=FIGSIZE)
    X = np.linspace(-2, 2, 20)
    Y = np.linspace(-2, 2, 20)
    X, Y = np.meshgrid(X, Y)
    U = -Y
    V = X
    ax.quiver(X, Y, U, V, np.sqrt(U**2 + V**2), cmap="coolwarm")
    ax.set_title("Quiver")
"""

EXTENDED_CASES[
    "streamplot"
] = """
    fig, ax = plt.subplots(figsize=FIGSIZE)
    X = np.linspace(-2, 2, 40)
    Y = np.linspace(-2, 2, 40)
    X, Y = np.meshgrid(X, Y)
    U = -Y
    V = X
    ax.streamplot(X, Y, U, V, color=np.sqrt(U**2 + V**2), cmap="coolwarm")
    ax.set_title("Streamplot")
"""

EXTENDED_CASES[
    "stem"
] = """
    fig, ax = plt.subplots(figsize=FIGSIZE)
    x = np.arange(30)
    y = np.sin(x * 0.3) * rng.uniform(0.5, 1.5, size=30)
    ax.stem(x, y)
    ax.set_title("Stem")
"""

EXTENDED_CASES[
    "stackplot"
] = """
    fig, ax = plt.subplots(figsize=FIGSIZE)
    x = np.arange(50)
    y1 = rng.uniform(1, 3, size=50)
    y2 = rng.uniform(1, 3, size=50)
    y3 = rng.uniform(1, 3, size=50)
    ax.stackplot(x, y1, y2, y3, alpha=0.7, labels=["a", "b", "c"])
    ax.legend(loc="upper left")
    ax.set_title("Stackplot")
"""

EXTENDED_CASES[
    "multiaxis"
] = """
    fig, ax1 = plt.subplots(figsize=FIGSIZE)
    x = np.linspace(0, 10, 200)
    ax1.plot(x, np.sin(x), "b-", label="sin")
    ax1.set_ylabel("sin", color="b")
    ax2 = ax1.twinx()
    ax2.plot(x, np.exp(-x / 5), "r-", label="exp")
    ax2.set_ylabel("exp", color="r")
    ax1.set_title("Multi-axis")
"""

EXTENDED_CASES[
    "log_scale"
] = """
    fig, ax = plt.subplots(figsize=FIGSIZE)
    x = np.logspace(0, 5, 200)
    ax.loglog(x, x**1.5 + rng.normal(0, 1, 200) * x)
    ax.set_title("Log-log scale")
    ax.grid(True, which="both", ls="--", alpha=0.5)
"""

EXTENDED_CASES[
    "heatmap_text"
] = """
    fig, ax = plt.subplots(figsize=FIGSIZE)
    data = rng.integers(0, 100, size=(8, 8))
    im = ax.imshow(data, cmap="YlOrRd")
    for i in range(8):
        for j in range(8):
            ax.text(j, i, str(data[i, j]), ha="center", va="center", fontsize=7)
    ax.set_title("Heatmap + text annotations")
"""

# ---------------------------------------------------------------------------
# Runner
# ---------------------------------------------------------------------------


def run_benchmarks() -> list[dict]:
    all_cases: dict[str, str] = {}
    all_cases.update(BASIC_CASES)
    all_cases.update(EXTENDED_CASES)

    results: list[dict] = []
    total = len(all_cases)

    for idx, (name, builder) in enumerate(all_cases.items(), 1):
        print(f"  [{idx:>2}/{total}] {name:<20s} ", end="", flush=True)

        agg_ms = None
        rust_ms = None
        error = None

        try:
            agg_ms = _run_single(BACKENDS["Agg"], name, builder)
        except Exception as e:
            error = f"Agg error: {e}"

        try:
            rust_ms = _run_single(BACKENDS["Rust"], name, builder)
        except Exception as e:
            error = f"Rust error: {e}"

        if agg_ms is not None and rust_ms is not None and rust_ms > 0:
            speedup = agg_ms / rust_ms
            tag = (
                f"{speedup:.2f}x faster"
                if speedup > 1.05
                else f"{1/speedup:.2f}x slower" if speedup < 0.95 else "parity"
            )
            print(f"Agg={agg_ms:>8.1f}ms  Rust={rust_ms:>8.1f}ms  -> {tag}")
        else:
            speedup = None
            print(f"ERROR: {error}")

        results.append(
            {
                "name": name,
                "agg_ms": agg_ms,
                "rust_ms": rust_ms,
                "speedup": speedup,
                "error": error,
            }
        )

    return results


def print_table(results: list[dict]) -> None:
    hdr = f"{'Test':<22s} {'Agg(ms)':>9s} {'Rust(ms)':>9s} {'Speedup':>9s}"
    sep = "-" * len(hdr)

    print()
    print("=" * len(hdr))
    print("  FULL BENCHMARK: matplotlib Agg vs mpl_rust_backend")
    print(
        f"  Warmup={WARMUP}, Timed={TIMED} (median), dpi={DPI}, figsize={FIGSIZE}, format={FMT}"
    )
    print("=" * len(hdr))
    print()

    # Basic section
    basic_names = set(BASIC_CASES.keys())
    extended_names = set(EXTENDED_CASES.keys())

    for section, names in [("BASIC", basic_names), ("EXTENDED", extended_names)]:
        print(f"  --- {section} ---")
        print(f"  {hdr}")
        print(f"  {sep}")
        for r in results:
            if r["name"] not in names:
                continue
            if r["speedup"] is not None:
                spdstr = f"{r['speedup']:.2f}x"
            else:
                spdstr = "ERROR"
            print(
                f"  {r['name']:<22s} {r['agg_ms']:>9.1f} {r['rust_ms']:>9.1f} {spdstr:>9s}"
            )
        print()

    # Summary stats
    valid = [r for r in results if r["speedup"] is not None]
    if not valid:
        print("  No valid results.")
        return

    speedups = [r["speedup"] for r in valid]
    geo_mean = math.exp(sum(math.log(s) for s in speedups) / len(speedups))
    faster = sum(1 for s in speedups if s > 1.05)
    slower = sum(1 for s in speedups if s < 0.95)
    parity = len(speedups) - faster - slower
    errors = sum(1 for r in results if r["speedup"] is None)

    print("  --- SUMMARY ---")
    print(f"  Total tests:       {len(results)}")
    print(f"  Successful:        {len(valid)}")
    if errors:
        print(f"  Errors:            {errors}")
    print(f"  Geometric mean:    {geo_mean:.2f}x")
    print(f"  Arithmetic mean:   {statistics.mean(speedups):.2f}x")
    print(f"  Median speedup:    {statistics.median(speedups):.2f}x")
    print(
        f"  Min speedup:       {min(speedups):.2f}x  ({min(valid, key=lambda r: r['speedup'])['name']})"
    )
    print(
        f"  Max speedup:       {max(speedups):.2f}x  ({max(valid, key=lambda r: r['speedup'])['name']})"
    )
    print(f"  Rust faster:       {faster}/{len(valid)}")
    print(f"  Rust slower:       {slower}/{len(valid)}")
    print(f"  Parity (0.95-1.05x): {parity}/{len(valid)}")
    print()


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

if __name__ == "__main__":
    print()
    print("Running comprehensive Agg vs Rust backend benchmark...")
    print(f"  Each test: {WARMUP} warmup + {TIMED} timed iterations (median)")
    print()

    results = run_benchmarks()
    print_table(results)

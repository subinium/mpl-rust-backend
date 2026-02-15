"""Generate showcase gallery images using mpl_rust_backend.

Produces six visually appealing example plots saved to the assets/ directory,
demonstrating the breadth of matplotlib features supported by the Rust backend.
"""

from __future__ import annotations

import os
import sys
from pathlib import Path

import matplotlib

matplotlib.use("module://mpl_rust_backend")

import matplotlib.pyplot as plt
import numpy as np

ASSETS_DIR = Path(__file__).resolve().parent / "assets"
ASSETS_DIR.mkdir(exist_ok=True)

np.random.seed(42)


# ---------------------------------------------------------------------------
# 1. Line plot: sin/cos with fill_between
# ---------------------------------------------------------------------------
def generate_line_plot() -> None:
    fig, ax = plt.subplots(figsize=(8, 5), dpi=150)
    x = np.linspace(0, 2 * np.pi, 300)
    y_sin = np.sin(x)
    y_cos = np.cos(x)

    ax.plot(x, y_sin, color="#2196F3", linewidth=2.2, label="sin(x)")
    ax.plot(x, y_cos, color="#FF5722", linewidth=2.2, label="cos(x)")
    ax.fill_between(x, y_sin, alpha=0.15, color="#2196F3")
    ax.fill_between(x, y_cos, alpha=0.15, color="#FF5722")

    ax.set_title("Trigonometric Functions", fontsize=16, fontweight="bold")
    ax.set_xlabel("x (radians)", fontsize=12)
    ax.set_ylabel("Amplitude", fontsize=12)
    ax.legend(fontsize=11, loc="upper right")
    ax.set_xlim(0, 2 * np.pi)
    ax.grid(True, linestyle="--", alpha=0.5)

    out = ASSETS_DIR / "example_line.png"
    fig.savefig(str(out))
    plt.close(fig)
    print(f"  [OK] {out}")


# ---------------------------------------------------------------------------
# 2. Scatter plot: 2000 colormapped points
# ---------------------------------------------------------------------------
def generate_scatter_plot() -> None:
    fig, ax = plt.subplots(figsize=(8, 5), dpi=150)

    n = 2000
    x = np.random.randn(n)
    y = np.random.randn(n)
    colors = np.sqrt(x**2 + y**2)
    sizes = np.random.uniform(10, 80, n)

    sc = ax.scatter(
        x, y, c=colors, s=sizes, cmap="plasma", alpha=0.7, edgecolors="none"
    )
    fig.colorbar(sc, ax=ax, label="Distance from origin")

    ax.set_title("Colorful Scatter Plot (2000 points)", fontsize=16, fontweight="bold")
    ax.set_xlabel("X", fontsize=12)
    ax.set_ylabel("Y", fontsize=12)
    ax.grid(True, linestyle="--", alpha=0.3)

    out = ASSETS_DIR / "example_scatter.png"
    fig.savefig(str(out))
    plt.close(fig)
    print(f"  [OK] {out}")


# ---------------------------------------------------------------------------
# 3. 3D surface plot
# ---------------------------------------------------------------------------
def generate_3d_surface() -> None:
    fig = plt.figure(figsize=(8, 6), dpi=150)
    ax = fig.add_subplot(111, projection="3d")

    u = np.linspace(-3, 3, 80)
    v = np.linspace(-3, 3, 80)
    X, Y = np.meshgrid(u, v)
    Z = np.sin(np.sqrt(X**2 + Y**2))

    ax.plot_surface(X, Y, Z, cmap="viridis", edgecolor="none", alpha=0.9)
    ax.set_title("3D Surface Plot", fontsize=16, fontweight="bold")
    ax.set_xlabel("X")
    ax.set_ylabel("Y")
    ax.set_zlabel("Z")

    out = ASSETS_DIR / "example_3d.png"
    fig.savefig(str(out))
    plt.close(fig)
    print(f"  [OK] {out}")


# ---------------------------------------------------------------------------
# 4. Polar rose curve
# ---------------------------------------------------------------------------
def generate_polar_plot() -> None:
    fig, ax = plt.subplots(figsize=(6, 6), dpi=150, subplot_kw={"projection": "polar"})

    theta = np.linspace(0, 2 * np.pi, 500)
    r = 1 + np.cos(5 * theta)

    ax.plot(theta, r, color="#9C27B0", linewidth=2.5)
    ax.fill(theta, r, alpha=0.2, color="#CE93D8")
    ax.set_title(
        "Polar Rose Curve: r = 1 + cos(5\u03b8)", fontsize=14, fontweight="bold", pad=20
    )

    out = ASSETS_DIR / "example_polar.png"
    fig.savefig(str(out))
    plt.close(fig)
    print(f"  [OK] {out}")


# ---------------------------------------------------------------------------
# 5. Statistics 2x2: histogram, boxplot, bar chart, pie chart
# ---------------------------------------------------------------------------
def generate_stats_plots() -> None:
    fig, axes = plt.subplots(2, 2, figsize=(10, 8), dpi=150)
    fig.suptitle("Statistical Visualizations", fontsize=18, fontweight="bold", y=0.98)

    # -- Histogram --
    ax = axes[0, 0]
    data = np.random.normal(0, 1, 5000)
    ax.hist(data, bins=40, color="#26A69A", edgecolor="white", alpha=0.85)
    ax.set_title("Histogram (Normal Distribution)", fontsize=11)
    ax.set_xlabel("Value")
    ax.set_ylabel("Frequency")

    # -- Boxplot --
    ax = axes[0, 1]
    bp_data = [np.random.normal(loc, 0.8, 200) for loc in [2, 4, 3, 5]]
    ax.boxplot(
        bp_data,
        patch_artist=True,
        boxprops=dict(facecolor="#42A5F5", alpha=0.7),
        medianprops=dict(color="#D32F2F", linewidth=2),
    )
    ax.set_title("Box Plot Comparison", fontsize=11)
    ax.set_xticklabels(["A", "B", "C", "D"])
    ax.set_ylabel("Value")

    # -- Bar chart --
    ax = axes[1, 0]
    categories = ["Python", "Rust", "C++", "Go", "Java"]
    values = [92, 78, 65, 54, 48]
    bar_colors = ["#EF5350", "#FFA726", "#66BB6A", "#42A5F5", "#AB47BC"]
    ax.bar(categories, values, color=bar_colors, edgecolor="white", linewidth=0.8)
    ax.set_title("Language Popularity", fontsize=11)
    ax.set_ylabel("Score")

    # -- Pie chart --
    ax = axes[1, 1]
    sizes = [35, 25, 20, 12, 8]
    labels = ["Web", "Data", "DevOps", "Mobile", "Other"]
    pie_colors = ["#FF7043", "#FFCA28", "#66BB6A", "#42A5F5", "#AB47BC"]
    ax.pie(
        sizes,
        labels=labels,
        colors=pie_colors,
        autopct="%1.1f%%",
        startangle=140,
        textprops={"fontsize": 9},
    )
    ax.set_title("Domain Distribution", fontsize=11)

    fig.tight_layout(rect=[0, 0, 1, 0.94])

    out = ASSETS_DIR / "example_stats.png"
    fig.savefig(str(out))
    plt.close(fig)
    print(f"  [OK] {out}")


# ---------------------------------------------------------------------------
# 6. Advanced 2x2: contourf, quiver, streamplot, heatmap
# ---------------------------------------------------------------------------
def generate_advanced_plots() -> None:
    fig, axes = plt.subplots(2, 2, figsize=(10, 8), dpi=150)
    fig.suptitle("Advanced Visualizations", fontsize=18, fontweight="bold", y=0.98)

    # -- Contourf --
    ax = axes[0, 0]
    x = np.linspace(-3, 3, 100)
    y = np.linspace(-3, 3, 100)
    X, Y = np.meshgrid(x, y)
    Z = np.sin(X) * np.cos(Y)
    cf = ax.contourf(X, Y, Z, levels=20, cmap="RdYlBu_r")
    fig.colorbar(cf, ax=ax, shrink=0.8)
    ax.set_title("Filled Contour Plot", fontsize=11)
    ax.set_xlabel("X")
    ax.set_ylabel("Y")

    # -- Quiver --
    ax = axes[0, 1]
    xq = np.linspace(-2, 2, 16)
    yq = np.linspace(-2, 2, 16)
    Xq, Yq = np.meshgrid(xq, yq)
    U = -Yq
    V = Xq
    speed = np.sqrt(U**2 + V**2)
    ax.quiver(Xq, Yq, U, V, speed, cmap="cool", scale=30)
    ax.set_title("Quiver Plot (Vector Field)", fontsize=11)
    ax.set_xlabel("X")
    ax.set_ylabel("Y")
    ax.set_aspect("equal")

    # -- Streamplot --
    ax = axes[1, 0]
    xs = np.linspace(-3, 3, 40)
    ys = np.linspace(-3, 3, 40)
    Xs, Ys = np.meshgrid(xs, ys)
    Us = -1 - Xs**2 + Ys
    Vs = 1 + Xs - Ys**2
    speed_s = np.sqrt(Us**2 + Vs**2)
    ax.streamplot(
        Xs, Ys, Us, Vs, color=speed_s, cmap="autumn", linewidth=1.5, density=1.2
    )
    ax.set_title("Streamplot (Flow Field)", fontsize=11)
    ax.set_xlabel("X")
    ax.set_ylabel("Y")

    # -- Heatmap with annotations --
    ax = axes[1, 1]
    data = np.random.rand(6, 6)
    im = ax.imshow(data, cmap="YlOrRd", aspect="auto")
    fig.colorbar(im, ax=ax, shrink=0.8)
    # Add text annotations
    for i in range(6):
        for j in range(6):
            val = data[i, j]
            text_color = "white" if val > 0.6 else "black"
            ax.text(
                j,
                i,
                f"{val:.2f}",
                ha="center",
                va="center",
                color=text_color,
                fontsize=7,
            )
    ax.set_title("Heatmap with Annotations", fontsize=11)
    ax.set_xlabel("Column")
    ax.set_ylabel("Row")

    fig.tight_layout(rect=[0, 0, 1, 0.94])

    out = ASSETS_DIR / "example_advanced.png"
    fig.savefig(str(out))
    plt.close(fig)
    print(f"  [OK] {out}")


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------
def main() -> None:
    print("Generating gallery images with mpl_rust_backend...")
    print(f"  Output directory: {ASSETS_DIR}\n")

    generators = [
        ("Line plot", generate_line_plot),
        ("Scatter plot", generate_scatter_plot),
        ("3D surface", generate_3d_surface),
        ("Polar rose", generate_polar_plot),
        ("Statistics 2x2", generate_stats_plots),
        ("Advanced 2x2", generate_advanced_plots),
    ]

    success = 0
    failed = 0
    for name, fn in generators:
        try:
            fn()
            success += 1
        except Exception as exc:
            print(f"  [FAIL] {name}: {exc}", file=sys.stderr)
            failed += 1

    print(f"\nDone: {success} succeeded, {failed} failed.")
    if failed:
        sys.exit(1)


if __name__ == "__main__":
    main()

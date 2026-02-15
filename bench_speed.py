"""Speed benchmark: matplotlib Agg vs mpl_rust_backend.

Measures savefig() wall time for identical plots.
Each test runs 3 warm-up + 5 timed iterations.
"""

import io, time, gc
import numpy as np


WARMUP = 2
REPEATS = 5


# ── test cases ──────────────────────────────────────────────────


def make_line_1k(fig, ax):
    x = np.linspace(0, 10, 1_000)
    ax.plot(x, np.sin(x))


def make_line_10k(fig, ax):
    x = np.linspace(0, 10, 10_000)
    ax.plot(x, np.sin(x))


def make_line_100k(fig, ax):
    x = np.linspace(0, 10, 100_000)
    ax.plot(x, np.sin(x), linewidth=0.5)


def make_scatter_1k(fig, ax):
    np.random.seed(42)
    ax.scatter(np.random.randn(1000), np.random.randn(1000), s=5, alpha=0.5)


def make_scatter_10k(fig, ax):
    np.random.seed(42)
    ax.scatter(np.random.randn(10_000), np.random.randn(10_000), s=3, alpha=0.3)


def make_scatter_50k(fig, ax):
    np.random.seed(42)
    ax.scatter(np.random.randn(50_000), np.random.randn(50_000), s=1, alpha=0.2)


def make_bar(fig, ax):
    ax.bar(range(20), np.random.RandomState(42).randint(1, 10, 20), color="steelblue")


def make_hist(fig, ax):
    np.random.seed(42)
    ax.hist(np.random.randn(10_000), bins=50, edgecolor="white")


def make_fill_between(fig, ax):
    x = np.linspace(0, 4 * np.pi, 1000)
    ax.fill_between(x, np.sin(x), np.cos(x), alpha=0.4)


def make_imshow_small(fig, ax):
    np.random.seed(42)
    ax.imshow(np.random.rand(100, 100), cmap="viridis")


def make_imshow_large(fig, ax):
    np.random.seed(42)
    ax.imshow(np.random.rand(500, 500), cmap="viridis")


def make_multiline(fig, ax):
    x = np.linspace(0, 10, 1000)
    for i in range(20):
        ax.plot(x, np.sin(x + i * 0.3), linewidth=0.8)


def make_subplots_4(fig, ax):
    axes = fig.subplots(2, 2)
    np.random.seed(42)
    x = np.linspace(0, 6, 200)
    axes[0, 0].plot(x, np.sin(x))
    axes[0, 1].bar(range(8), np.random.randint(1, 10, 8))
    axes[1, 0].scatter(np.random.randn(200), np.random.randn(200), s=5)
    axes[1, 1].hist(np.random.randn(500), bins=20)


def make_step(fig, ax):
    x = np.arange(50)
    ax.step(x, np.random.RandomState(42).randint(0, 10, 50), where="mid")


def make_errorbar(fig, ax):
    np.random.seed(42)
    x = np.arange(20)
    y = np.random.randn(20)
    ax.errorbar(x, y, yerr=0.5, fmt="o-", capsize=3)


TESTS = [
    ("line_1k", make_line_1k, False),
    ("line_10k", make_line_10k, False),
    ("line_100k", make_line_100k, False),
    ("scatter_1k", make_scatter_1k, False),
    ("scatter_10k", make_scatter_10k, False),
    ("scatter_50k", make_scatter_50k, False),
    ("bar_20", make_bar, False),
    ("hist_10k", make_hist, False),
    ("fill_between", make_fill_between, False),
    ("imshow_100", make_imshow_small, False),
    ("imshow_500", make_imshow_large, False),
    ("multiline_20", make_multiline, False),
    ("subplots_2x2", make_subplots_4, True),
    ("step_50", make_step, False),
    ("errorbar_20", make_errorbar, False),
]


def bench_one(backend, test_fn, uses_fig, fmt="png", figsize=(6, 4), dpi=100):
    """Benchmark savefig time for one test case. Returns median ms."""
    import matplotlib

    matplotlib.use(backend)
    import matplotlib.pyplot as plt

    plt.rcdefaults()

    times = []
    for i in range(WARMUP + REPEATS):
        fig = plt.figure(figsize=figsize, dpi=dpi)
        if uses_fig:
            test_fn(fig, None)
        else:
            ax = fig.add_subplot(111)
            test_fn(fig, ax)

        buf = io.BytesIO()
        gc.disable()
        t0 = time.perf_counter()
        fig.savefig(buf, format=fmt, dpi=dpi)
        elapsed = time.perf_counter() - t0
        gc.enable()
        plt.close(fig)

        if i >= WARMUP:
            times.append(elapsed * 1000)  # ms

    return sorted(times)[len(times) // 2]  # median


def main():
    results = []
    print(f"{'Test':<18} {'Agg (ms)':>10} {'Rust (ms)':>10} {'Speedup':>9}")
    print("-" * 52)

    for name, test_fn, uses_fig in TESTS:
        t_agg = bench_one("Agg", test_fn, uses_fig)
        t_rust = bench_one("module://mpl_rust_backend", test_fn, uses_fig)
        speedup = t_agg / t_rust if t_rust > 0 else float("inf")
        results.append((name, t_agg, t_rust, speedup))
        marker = (
            ">>>"
            if speedup >= 2.0
            else (">" if speedup >= 1.2 else "~" if speedup >= 0.8 else "<")
        )
        print(f"{name:<18} {t_agg:>9.1f} {t_rust:>9.1f} {speedup:>7.2f}x {marker}")

    print("=" * 52)
    faster = sum(1 for _, _, _, s in results if s > 1.0)
    avg_speedup = np.mean([s for _, _, _, s in results])
    geo_speedup = np.exp(np.mean(np.log([s for _, _, _, s in results])))
    print(f"Faster in {faster}/{len(results)} tests")
    print(f"Arithmetic mean speedup: {avg_speedup:.2f}x")
    print(f"Geometric mean speedup:  {geo_speedup:.2f}x")

    # Also output SVG benchmarks for a subset
    print(f"\n{'─'*52}")
    print(f"SVG output (subset):")
    print(f"{'Test':<18} {'Agg (ms)':>10} {'Rust (ms)':>10} {'Speedup':>9}")
    print("-" * 52)
    svg_tests = [
        t for t in TESTS if t[0] in ("line_10k", "scatter_10k", "bar_20", "imshow_100")
    ]
    for name, test_fn, uses_fig in svg_tests:
        t_agg = bench_one("Agg", test_fn, uses_fig, fmt="svg")
        t_rust = bench_one("module://mpl_rust_backend", test_fn, uses_fig, fmt="svg")
        speedup = t_agg / t_rust if t_rust > 0 else float("inf")
        marker = ">>>" if speedup >= 2.0 else (">" if speedup >= 1.2 else "~")
        print(f"{name:<18} {t_agg:>9.1f} {t_rust:>9.1f} {speedup:>7.2f}x {marker}")


if __name__ == "__main__":
    main()

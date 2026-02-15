"""Compare matplotlib Agg vs mpl_rust_backend — pixel-level fidelity check."""

import os, sys, time
import numpy as np

# ── output dir ──
OUT = os.path.join(os.path.dirname(__file__), "compare")
os.makedirs(OUT, exist_ok=True)


# ── test cases ──
def make_line_100k(fig, ax):
    np.random.seed(42)
    x = np.linspace(0, 10, 100_000)
    ax.plot(x, np.sin(x) + 0.1 * np.random.randn(len(x)), linewidth=0.5)
    ax.set_title("Line 100K points")


def make_scatter_5k(fig, ax):
    np.random.seed(42)
    x = np.random.randn(5000)
    y = np.random.randn(5000)
    ax.scatter(x, y, s=4, alpha=0.5)
    ax.set_title("Scatter 5K")


def make_bar(fig, ax):
    x = list(range(10))
    ax.bar(x, [3, 7, 2, 5, 8, 1, 6, 4, 9, 3], color="steelblue", edgecolor="white")
    ax.set_title("Bar chart")


def make_fill_between(fig, ax):
    x = np.linspace(0, 4 * np.pi, 500)
    y1 = np.sin(x)
    y2 = np.sin(x) * 0.5
    ax.fill_between(x, y1, y2, alpha=0.4)
    ax.plot(x, y1)
    ax.set_title("fill_between")


def make_histogram(fig, ax):
    np.random.seed(42)
    ax.hist(np.random.randn(5000), bins=50, color="steelblue", edgecolor="white")
    ax.set_title("Histogram 5K")


def make_imshow(fig, ax):
    np.random.seed(42)
    data = np.random.rand(50, 50)
    ax.imshow(data, cmap="viridis")
    ax.set_title("imshow 50x50")


def make_subplots(fig, ax):
    # ax is ignored, we use fig directly
    axes = fig.subplots(2, 2)
    np.random.seed(42)
    axes[0, 0].plot([1, 2, 3, 4], [1, 4, 2, 3])
    axes[0, 0].set_title("Line")
    axes[0, 1].bar([1, 2, 3], [3, 1, 2])
    axes[0, 1].set_title("Bar")
    axes[1, 0].scatter(np.random.randn(100), np.random.randn(100), s=10)
    axes[1, 0].set_title("Scatter")
    axes[1, 1].hist(np.random.randn(500), bins=20)
    axes[1, 1].set_title("Hist")


def make_step(fig, ax):
    x = np.arange(20)
    y = np.random.RandomState(42).randint(0, 10, 20)
    ax.step(x, y, where="mid", linewidth=2)
    ax.set_title("Step plot")


TESTS = [
    ("01_line_100k", make_line_100k, False),
    ("02_scatter_5k", make_scatter_5k, False),
    ("03_bar", make_bar, False),
    ("04_fill_between", make_fill_between, False),
    ("05_histogram", make_histogram, False),
    ("06_imshow", make_imshow, False),
    ("07_subplots", make_subplots, True),  # True = uses fig directly
    ("08_step", make_step, False),
]


def render_with_backend(backend_name, test_fn, uses_fig, figsize=(6, 4), dpi=100):
    """Render a test case with a given backend, return RGBA numpy array."""
    import matplotlib

    matplotlib.use(backend_name)
    import matplotlib.pyplot as plt

    plt.rcdefaults()

    fig = plt.figure(figsize=figsize, dpi=dpi)
    if uses_fig:
        test_fn(fig, None)
    else:
        ax = fig.add_subplot(111)
        test_fn(fig, ax)

    fig.canvas.draw()
    buf = fig.canvas.tostring_argb()
    w, h = fig.canvas.get_width_height()
    arr = np.frombuffer(buf, dtype=np.uint8).reshape(h, w, 4)
    # ARGB → RGBA
    rgba = np.empty_like(arr)
    rgba[..., 0] = arr[..., 1]  # R
    rgba[..., 1] = arr[..., 2]  # G
    rgba[..., 2] = arr[..., 3]  # B
    rgba[..., 3] = arr[..., 0]  # A
    plt.close(fig)
    return rgba


def render_rust(test_fn, uses_fig, figsize=(6, 4), dpi=100):
    """Render with mpl_rust_backend via savefig to PNG, return RGBA array."""
    import matplotlib

    matplotlib.use("module://mpl_rust_backend")
    import matplotlib.pyplot as plt

    plt.rcdefaults()

    fig = plt.figure(figsize=figsize, dpi=dpi)
    if uses_fig:
        test_fn(fig, None)
    else:
        ax = fig.add_subplot(111)
        test_fn(fig, ax)

    import io
    from PIL import Image

    buf = io.BytesIO()
    t0 = time.perf_counter()
    fig.savefig(buf, format="png", dpi=dpi)
    elapsed = time.perf_counter() - t0
    buf.seek(0)
    img = Image.open(buf).convert("RGBA")
    plt.close(fig)
    return np.array(img), elapsed


def compute_mse(a, b):
    """MSE between two uint8 RGBA arrays (resize if needed)."""
    from PIL import Image

    if a.shape != b.shape:
        # resize b to match a
        img_b = Image.fromarray(b)
        img_b = img_b.resize((a.shape[1], a.shape[0]), Image.LANCZOS)
        b = np.array(img_b)
    diff = a.astype(float) - b.astype(float)
    return float(np.mean(diff**2))


def make_comparison_image(name, mpl_img, rust_img, mse_val, elapsed):
    """Create side-by-side: matplotlib | rust | diff×10."""
    from PIL import Image, ImageDraw, ImageFont

    h, w = mpl_img.shape[:2]

    if rust_img.shape != mpl_img.shape:
        ri = Image.fromarray(rust_img).resize((w, h), Image.LANCZOS)
        rust_img = np.array(ri)

    diff = np.abs(mpl_img.astype(float) - rust_img.astype(float))
    diff_amp = np.clip(diff * 10, 0, 255).astype(np.uint8)

    canvas = Image.new("RGBA", (w * 3 + 20, h + 40), (255, 255, 255, 255))
    canvas.paste(Image.fromarray(mpl_img), (0, 40))
    canvas.paste(Image.fromarray(rust_img), (w + 10, 40))
    canvas.paste(Image.fromarray(diff_amp), (w * 2 + 20, 40))

    draw = ImageDraw.Draw(canvas)
    status = "MATCH" if mse_val < 1500 else ("CLOSE" if mse_val < 3000 else "DIFF")
    title = f"{name}  |  MSE={mse_val:.1f} [{status}]  |  rust={elapsed*1000:.0f}ms"
    draw.text((10, 10), title, fill=(0, 0, 0, 255))
    labels = ["matplotlib (Agg)", "mpl_rust_backend", "diff ×10"]
    for i, lbl in enumerate(labels):
        draw.text((i * (w + 10) + 10, 26), lbl, fill=(100, 100, 100, 255))

    return canvas


def main():
    from PIL import Image

    results = []

    for name, test_fn, uses_fig in TESTS:
        print(f"  {name} ...", end=" ", flush=True)

        # Render matplotlib Agg
        mpl_img = render_with_backend("Agg", test_fn, uses_fig)

        # Render Rust backend
        rust_img, elapsed = render_rust(test_fn, uses_fig)

        mse_val = compute_mse(mpl_img, rust_img)
        status = "MATCH" if mse_val < 1500 else ("CLOSE" if mse_val < 3000 else "DIFF")
        results.append((name, mse_val, status, elapsed))
        print(f"MSE={mse_val:.1f}  {status}  ({elapsed*1000:.0f}ms)")

        comp = make_comparison_image(name, mpl_img, rust_img, mse_val, elapsed)
        comp.save(os.path.join(OUT, f"{name}.png"))

    print("\n" + "=" * 65)
    print(f"{'Test':<25} {'MSE':>8} {'Status':>8} {'Time':>8}")
    print("-" * 65)
    for name, mse, status, elapsed in results:
        print(f"{name:<25} {mse:>8.1f} {status:>8} {elapsed*1000:>7.0f}ms")
    print("=" * 65)


if __name__ == "__main__":
    main()

"""Extended fidelity comparison: matplotlib Agg vs mpl_rust_backend.
Includes polar, 3D, contour, pie, quiver, and other advanced chart types.
"""

import os, io, time
import numpy as np

OUT = os.path.join(os.path.dirname(__file__), "compare_extended")
os.makedirs(OUT, exist_ok=True)


# ── test cases ──────────────────────────────────────────────────


def make_polar_line(fig, ax):
    theta = np.linspace(0, 2 * np.pi, 100)
    r = 1 + np.cos(5 * theta)
    ax.plot(theta, r, linewidth=2)
    ax.set_title("Polar: rose curve", pad=15)


def make_polar_bar(fig, ax):
    N = 12
    theta = np.linspace(0, 2 * np.pi, N, endpoint=False)
    radii = np.random.RandomState(42).randint(3, 15, N)
    width = 2 * np.pi / N * 0.8
    bars = ax.bar(theta, radii, width=width, bottom=0.0, alpha=0.7)
    for bar, t in zip(bars, theta):
        bar.set_facecolor(plt.cm.viridis(t / (2 * np.pi)))
    ax.set_title("Polar bar", pad=15)


def make_polar_scatter(fig, ax):
    np.random.seed(42)
    N = 200
    theta = np.random.uniform(0, 2 * np.pi, N)
    r = np.random.uniform(0, 1, N)
    colors = theta
    ax.scatter(theta, r, c=colors, s=20, cmap="hsv", alpha=0.7)
    ax.set_title("Polar scatter", pad=15)


def make_3d_surface(fig, ax):
    X = np.linspace(-3, 3, 40)
    Y = np.linspace(-3, 3, 40)
    X, Y = np.meshgrid(X, Y)
    Z = np.sin(np.sqrt(X**2 + Y**2))
    ax.plot_surface(X, Y, Z, cmap="viridis", alpha=0.9)
    ax.set_title("3D Surface")


def make_3d_wireframe(fig, ax):
    X = np.linspace(-3, 3, 25)
    Y = np.linspace(-3, 3, 25)
    X, Y = np.meshgrid(X, Y)
    Z = np.cos(X) * np.sin(Y)
    ax.plot_wireframe(X, Y, Z, linewidth=0.5)
    ax.set_title("3D Wireframe")


def make_3d_scatter(fig, ax):
    np.random.seed(42)
    N = 500
    x = np.random.randn(N)
    y = np.random.randn(N)
    z = np.random.randn(N)
    ax.scatter(x, y, z, c=z, cmap="coolwarm", s=10, alpha=0.6)
    ax.set_title("3D Scatter")


def make_3d_bar(fig, ax):
    x = np.arange(5)
    y = np.arange(5)
    x, y = np.meshgrid(x, y)
    x, y = x.ravel(), y.ravel()
    z = np.zeros_like(x)
    dx = dy = 0.6
    dz = np.random.RandomState(42).randint(1, 10, len(x))
    ax.bar3d(x, y, z, dx, dy, dz, alpha=0.8)
    ax.set_title("3D Bar")


def make_contour(fig, ax):
    X, Y = np.meshgrid(np.linspace(-3, 3, 100), np.linspace(-3, 3, 100))
    Z = np.sin(X) * np.cos(Y)
    cs = ax.contour(X, Y, Z, levels=15)
    ax.clabel(cs, inline=True, fontsize=8)
    ax.set_title("Contour")


def make_contourf(fig, ax):
    X, Y = np.meshgrid(np.linspace(-3, 3, 100), np.linspace(-3, 3, 100))
    Z = np.exp(-(X**2 + Y**2) / 2) - np.exp(-((X - 1) ** 2 + (Y - 1) ** 2) / 2)
    ax.contourf(X, Y, Z, levels=20, cmap="RdBu_r")
    ax.set_title("Filled contour")


def make_pie(fig, ax):
    sizes = [35, 25, 20, 15, 5]
    labels = ["A", "B", "C", "D", "E"]
    explode = (0.05, 0, 0, 0, 0)
    ax.pie(
        sizes,
        explode=explode,
        labels=labels,
        autopct="%1.1f%%",
        shadow=False,
        startangle=90,
    )
    ax.set_title("Pie chart")


def make_quiver(fig, ax):
    X, Y = np.meshgrid(np.linspace(-2, 2, 15), np.linspace(-2, 2, 15))
    U = -Y
    V = X
    ax.quiver(X, Y, U, V, np.sqrt(U**2 + V**2), cmap="autumn")
    ax.set_title("Quiver")


def make_streamplot(fig, ax):
    Y, X = np.mgrid[-3:3:100j, -3:3:100j]
    U = -1 - X**2 + Y
    V = 1 + X - Y**2
    speed = np.sqrt(U**2 + V**2)
    ax.streamplot(X, Y, U, V, color=speed, cmap="cool", linewidth=1)
    ax.set_title("Streamplot")


def make_errorbar(fig, ax):
    np.random.seed(42)
    x = np.arange(10)
    y = np.random.randn(10).cumsum()
    yerr = np.random.uniform(0.3, 1.0, 10)
    ax.errorbar(x, y, yerr=yerr, fmt="o-", capsize=4, capthick=1.5)
    ax.set_title("Errorbar")


def make_stem(fig, ax):
    x = np.linspace(0, 2 * np.pi, 20)
    y = np.sin(x)
    ax.stem(x, y)
    ax.set_title("Stem plot")


def make_stackplot(fig, ax):
    x = np.arange(10)
    np.random.seed(42)
    y1 = np.random.randint(1, 5, 10)
    y2 = np.random.randint(1, 5, 10)
    y3 = np.random.randint(1, 5, 10)
    ax.stackplot(x, y1, y2, y3, labels=["A", "B", "C"], alpha=0.7)
    ax.legend(loc="upper left")
    ax.set_title("Stackplot")


def make_multiaxis(fig, ax):
    x = np.linspace(0, 10, 100)
    ax.plot(x, np.sin(x), "b-", label="sin")
    ax.set_ylabel("sin", color="b")
    ax2 = ax.twinx()
    ax2.plot(x, np.exp(x / 10), "r-", label="exp")
    ax2.set_ylabel("exp", color="r")
    ax.set_title("Twin axes")


def make_log_scale(fig, ax):
    x = np.logspace(0, 5, 100)
    ax.loglog(x, x**1.5, label="x^1.5")
    ax.loglog(x, x**2, label="x^2")
    ax.legend()
    ax.grid(True, which="both", ls="-", alpha=0.3)
    ax.set_title("Log-log scale")


def make_heatmap_text(fig, ax):
    np.random.seed(42)
    data = np.random.rand(8, 8)
    im = ax.imshow(data, cmap="YlOrRd")
    for i in range(8):
        for j in range(8):
            ax.text(j, i, f"{data[i,j]:.1f}", ha="center", va="center", fontsize=7)
    ax.set_title("Annotated heatmap")


# (name, func, subplot_type)
# subplot_type: "normal", "polar", "3d"
TESTS = [
    ("01_polar_line", make_polar_line, "polar"),
    ("02_polar_bar", make_polar_bar, "polar"),
    ("03_polar_scatter", make_polar_scatter, "polar"),
    ("04_3d_surface", make_3d_surface, "3d"),
    ("05_3d_wireframe", make_3d_wireframe, "3d"),
    ("06_3d_scatter", make_3d_scatter, "3d"),
    ("07_3d_bar", make_3d_bar, "3d"),
    ("08_contour", make_contour, "normal"),
    ("09_contourf", make_contourf, "normal"),
    ("10_pie", make_pie, "normal"),
    ("11_quiver", make_quiver, "normal"),
    ("12_streamplot", make_streamplot, "normal"),
    ("13_errorbar", make_errorbar, "normal"),
    ("14_stem", make_stem, "normal"),
    ("15_stackplot", make_stackplot, "normal"),
    ("16_multiaxis", make_multiaxis, "normal"),
    ("17_log_scale", make_log_scale, "normal"),
    ("18_heatmap_text", make_heatmap_text, "normal"),
]


def _make_fig_ax(subplot_type, figsize=(6, 4), dpi=100):
    fig = plt.figure(figsize=figsize, dpi=dpi)
    if subplot_type == "polar":
        ax = fig.add_subplot(111, projection="polar")
    elif subplot_type == "3d":
        ax = fig.add_subplot(111, projection="3d")
    else:
        ax = fig.add_subplot(111)
    return fig, ax


def render_agg(test_fn, subplot_type, figsize=(6, 4), dpi=100):
    import matplotlib

    matplotlib.use("Agg")
    global plt
    import matplotlib.pyplot as plt

    plt.rcdefaults()
    fig, ax = _make_fig_ax(subplot_type, figsize, dpi)
    test_fn(fig, ax)
    fig.canvas.draw()
    buf = fig.canvas.tostring_argb()
    w, h = fig.canvas.get_width_height()
    arr = np.frombuffer(buf, dtype=np.uint8).reshape(h, w, 4)
    rgba = np.empty_like(arr)
    rgba[..., 0] = arr[..., 1]
    rgba[..., 1] = arr[..., 2]
    rgba[..., 2] = arr[..., 3]
    rgba[..., 3] = arr[..., 0]
    plt.close(fig)
    return rgba


def render_rust(test_fn, subplot_type, figsize=(6, 4), dpi=100):
    import matplotlib

    matplotlib.use("module://mpl_rust_backend")
    global plt
    import matplotlib.pyplot as plt

    plt.rcdefaults()
    fig, ax = _make_fig_ax(subplot_type, figsize, dpi)
    test_fn(fig, ax)
    buf = io.BytesIO()
    t0 = time.perf_counter()
    try:
        fig.savefig(buf, format="png", dpi=dpi)
    except Exception as e:
        plt.close(fig)
        return None, 0, str(e)
    elapsed = time.perf_counter() - t0
    buf.seek(0)
    from PIL import Image

    img = Image.open(buf).convert("RGBA")
    plt.close(fig)
    return np.array(img), elapsed, None


def compute_mse(a, b):
    from PIL import Image

    if a.shape != b.shape:
        img_b = Image.fromarray(b)
        img_b = img_b.resize((a.shape[1], a.shape[0]), Image.LANCZOS)
        b = np.array(img_b)
    diff = a.astype(float) - b.astype(float)
    return float(np.mean(diff**2))


def make_comparison_image(name, mpl_img, rust_img, mse_val, elapsed, error=None):
    from PIL import Image, ImageDraw

    h, w = mpl_img.shape[:2]

    if rust_img is None:
        # Error case — show matplotlib only + error message
        canvas = Image.new("RGBA", (w * 3 + 20, h + 40), (255, 255, 255, 255))
        canvas.paste(Image.fromarray(mpl_img), (0, 40))
        draw = ImageDraw.Draw(canvas)
        draw.text((10, 10), f"{name}  |  ERROR: {error}", fill=(200, 0, 0, 255))
        draw.text((10, 26), "matplotlib (Agg)", fill=(100, 100, 100, 255))
        draw.text((w + 20, 26), "FAILED", fill=(200, 0, 0, 255))
        return canvas

    if rust_img.shape != mpl_img.shape:
        from PIL import Image as PILImage

        ri = PILImage.fromarray(rust_img).resize((w, h), PILImage.LANCZOS)
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
    labels = ["matplotlib (Agg)", "mpl_rust_backend", "diff x10"]
    for i, lbl in enumerate(labels):
        draw.text((i * (w + 10) + 10, 26), lbl, fill=(100, 100, 100, 255))
    return canvas


def main():
    results = []
    print(f"{'Test':<22} {'MSE':>8} {'Status':>8} {'Time':>8}")
    print("-" * 52)

    for name, test_fn, subplot_type in TESTS:
        mpl_img = render_agg(test_fn, subplot_type)
        rust_img, elapsed, error = render_rust(test_fn, subplot_type)

        if error:
            print(f"{name:<22} {'':>8} {'ERROR':>8} {'':>8}  {error[:40]}")
            results.append((name, None, "ERROR", 0))
            comp = make_comparison_image(name, mpl_img, None, 0, 0, error)
        else:
            mse_val = compute_mse(mpl_img, rust_img)
            status = (
                "MATCH" if mse_val < 1500 else ("CLOSE" if mse_val < 3000 else "DIFF")
            )
            results.append((name, mse_val, status, elapsed))
            print(f"{name:<22} {mse_val:>8.1f} {status:>8} {elapsed*1000:>7.0f}ms")
            comp = make_comparison_image(name, mpl_img, rust_img, mse_val, elapsed)

        comp.save(os.path.join(OUT, f"{name}.png"))

    print("=" * 52)
    match_count = sum(1 for _, m, s, _ in results if s == "MATCH")
    close_count = sum(1 for _, m, s, _ in results if s == "CLOSE")
    diff_count = sum(1 for _, m, s, _ in results if s == "DIFF")
    err_count = sum(1 for _, m, s, _ in results if s == "ERROR")
    total = len(results)
    print(
        f"MATCH: {match_count}/{total}  CLOSE: {close_count}/{total}  DIFF: {diff_count}/{total}  ERROR: {err_count}/{total}"
    )


if __name__ == "__main__":
    main()

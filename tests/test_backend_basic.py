"""Smoke tests for the mpl_rust_backend."""

import io
import os
import sys

import pytest

# Ensure we can import the backend
os.environ.setdefault("MPLBACKEND", "module://mpl_rust_backend")


def _has_native():
    """Check if the native Rust module is available."""
    try:
        from mpl_rust_backend._rust import render_scene_bytes

        return True
    except ImportError:
        return False


needs_native = pytest.mark.skipif(not _has_native(), reason="native module not built")


@needs_native
def test_import_backend():
    """Backend module imports without error."""
    import mpl_rust_backend

    assert hasattr(mpl_rust_backend, "FigureCanvas")
    assert hasattr(mpl_rust_backend, "FigureManager")


@needs_native
def test_simple_line_png():
    """A simple line plot produces valid PNG bytes."""
    import matplotlib

    matplotlib.use("module://mpl_rust_backend")
    import matplotlib.pyplot as plt

    fig, ax = plt.subplots()
    ax.plot([1, 2, 3], [1, 4, 2])
    buf = io.BytesIO()
    fig.savefig(buf, format="png")
    data = buf.getvalue()
    assert len(data) > 100
    assert data[:8] == b"\x89PNG\r\n\x1a\n"
    plt.close(fig)


@needs_native
def test_simple_line_svg():
    """A simple line plot produces valid SVG."""
    import matplotlib

    matplotlib.use("module://mpl_rust_backend")
    import matplotlib.pyplot as plt

    fig, ax = plt.subplots()
    ax.plot([1, 2, 3], [1, 4, 2])
    buf = io.BytesIO()
    fig.savefig(buf, format="svg")
    data = buf.getvalue()
    assert b"<svg" in data
    assert b"</svg>" in data
    plt.close(fig)


@needs_native
def test_scatter_plot():
    """Scatter plot renders without error."""
    import matplotlib

    matplotlib.use("module://mpl_rust_backend")
    import matplotlib.pyplot as plt
    import numpy as np

    fig, ax = plt.subplots()
    np.random.seed(42)
    ax.scatter(np.random.randn(50), np.random.randn(50))
    buf = io.BytesIO()
    fig.savefig(buf, format="png")
    assert len(buf.getvalue()) > 100
    plt.close(fig)


@needs_native
def test_bar_chart():
    """Bar chart renders without error."""
    import matplotlib

    matplotlib.use("module://mpl_rust_backend")
    import matplotlib.pyplot as plt

    fig, ax = plt.subplots()
    ax.bar(["A", "B", "C"], [3, 7, 5])
    buf = io.BytesIO()
    fig.savefig(buf, format="png")
    assert len(buf.getvalue()) > 100
    plt.close(fig)


@needs_native
def test_text_rendering():
    """Text elements render without error."""
    import matplotlib

    matplotlib.use("module://mpl_rust_backend")
    import matplotlib.pyplot as plt

    fig, ax = plt.subplots()
    ax.set_title("Test Title")
    ax.set_xlabel("X Label")
    ax.set_ylabel("Y Label")
    ax.plot([1, 2], [1, 2])
    buf = io.BytesIO()
    fig.savefig(buf, format="png")
    assert len(buf.getvalue()) > 100
    plt.close(fig)


@needs_native
def test_subplots():
    """Multiple subplots render without error."""
    import matplotlib

    matplotlib.use("module://mpl_rust_backend")
    import matplotlib.pyplot as plt

    fig, axes = plt.subplots(2, 2)
    for ax in axes.flat:
        ax.plot([1, 2, 3])
    buf = io.BytesIO()
    fig.savefig(buf, format="png")
    assert len(buf.getvalue()) > 100
    plt.close(fig)


@needs_native
def test_fill_between():
    """fill_between renders without error."""
    import matplotlib

    matplotlib.use("module://mpl_rust_backend")
    import matplotlib.pyplot as plt
    import numpy as np

    fig, ax = plt.subplots()
    x = np.linspace(0, 2 * np.pi, 100)
    ax.fill_between(x, np.sin(x), alpha=0.3)
    buf = io.BytesIO()
    fig.savefig(buf, format="png")
    assert len(buf.getvalue()) > 100
    plt.close(fig)


@needs_native
def test_legend():
    """Legend renders without error."""
    import matplotlib

    matplotlib.use("module://mpl_rust_backend")
    import matplotlib.pyplot as plt

    fig, ax = plt.subplots()
    ax.plot([1, 2, 3], label="Line A")
    ax.plot([3, 2, 1], label="Line B")
    ax.legend()
    buf = io.BytesIO()
    fig.savefig(buf, format="png")
    assert len(buf.getvalue()) > 100
    plt.close(fig)

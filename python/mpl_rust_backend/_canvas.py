"""FigureCanvasRust — matplotlib canvas backed by the Rust rendering engine."""

from __future__ import annotations

import io

from matplotlib.backend_bases import FigureCanvasBase

from ._renderer import RendererRust

try:
    from ._rust import render_scene_bytes, render_scene_with_blobs
except ImportError as e:
    _import_error = e

    def render_scene_bytes(*args, **kwargs):
        raise ImportError(
            "mpl_rust_backend native module not found. " "Build with: maturin develop"
        ) from _import_error

    def render_scene_with_blobs(*args, **kwargs):
        raise ImportError(
            "mpl_rust_backend native module not found. " "Build with: maturin develop"
        ) from _import_error


class FigureCanvasRust(FigureCanvasBase):
    """A matplotlib canvas that renders through a Rust scene graph engine."""

    def draw(self):
        renderer = self.get_renderer()
        self.figure.draw(renderer)

    def get_renderer(self):
        w, h = self.figure.get_size_inches()
        dpi = self.figure.dpi
        return RendererRust(w * dpi, h * dpi, dpi)

    def _render_to_bytes(self, fmt):
        """Render the figure and return raw bytes in the given format."""
        renderer = self.get_renderer()
        self.figure.draw(renderer)
        json_bytes, blobs = renderer.scene_builder.build()

        if blobs:
            return bytes(render_scene_with_blobs(json_bytes, blobs, fmt))
        else:
            return bytes(render_scene_bytes(json_bytes, fmt))

    def print_png(self, fname_or_fh, **kwargs):
        data = self._render_to_bytes("png")
        if hasattr(fname_or_fh, "write"):
            fname_or_fh.write(data)
        else:
            with open(fname_or_fh, "wb") as f:
                f.write(data)

    def print_svg(self, fname_or_fh, **kwargs):
        data = self._render_to_bytes("svg")
        if hasattr(fname_or_fh, "write"):
            if hasattr(fname_or_fh, "mode") and "b" not in getattr(
                fname_or_fh, "mode", "b"
            ):
                fname_or_fh.write(data.decode("utf-8"))
            else:
                fname_or_fh.write(data)
        else:
            with open(fname_or_fh, "wb") as f:
                f.write(data)

    def get_default_filetype(self):
        return "png"

    @classmethod
    def get_default_renderer(cls):
        return RendererRust

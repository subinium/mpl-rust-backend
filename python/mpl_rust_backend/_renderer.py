"""RendererRust — implements matplotlib's RendererBase using the Rust scene graph."""

from __future__ import annotations

import numpy as np
from matplotlib.backend_bases import RendererBase, GraphicsContextBase
from matplotlib.transforms import Affine2D

from ._scene_builder import SceneBuilder
from ._translators import (
    path_to_segments,
    gc_to_stroke,
    gc_to_fill,
    affine_to_array,
    is_rectilinear,
)


class RendererRust(RendererBase):
    """Renderer that translates matplotlib draw calls into a JSON scene graph.

    The scene is later handed to the Rust engine for rasterization.
    """

    def __init__(self, width, height, dpi):
        super().__init__()
        self.width = width  # pixels
        self.height = height  # pixels
        self.dpi = dpi
        # Scene uses dpi=72 so Rust base_transform scale = 72/72 = 1.0.
        # matplotlib already sends all coordinates in display pixels.
        self._scene = SceneBuilder(width, height, 72.0)
        self._current_clip = None  # track active clip for group management
        # Cache y-flip transform — constant for the lifetime of this renderer.
        self._y_flip_transform = Affine2D().scale(1, -1).translate(0, self.height)

    @property
    def scene_builder(self) -> SceneBuilder:
        return self._scene

    # ── Coordinate helpers ──────────────────────────────────────────

    def _y_flip(self):
        """Return a transform that flips y from matplotlib's y-up to tiny_skia's y-down.

        matplotlib path coordinates use y-up (y=0 at bottom); flipy() only
        affects text.  tiny_skia uses y-down (y=0 at top), so we need
        y' = height - y for all path/marker/image coordinates.
        """
        return self._y_flip_transform

    # ── Clip handling ─────────────────────────────────────────────

    def _apply_clip(self, gc):
        """Ensure the scene graph has the correct clip group for this GC.

        Groups consecutive draws with the same clip rect to avoid creating
        a temporary pixmap for every single draw call.
        """
        clip_rect = gc.get_clip_rectangle()
        if clip_rect is not None:
            bbox = clip_rect.bounds  # (x0, y0, width, height) in y-up display
            x, y_up, w, h = bbox
            clip_key = (round(x), round(self.height - y_up - h), round(w), round(h))
        else:
            clip_key = None

        if clip_key == self._current_clip:
            return

        # Close the previous clip group
        if self._current_clip is not None:
            self._scene.pop_group()

        # Open a new clip group
        if clip_key is not None:
            self._scene.push_group(
                clip={
                    "x": float(clip_key[0]),
                    "y": float(clip_key[1]),
                    "width": float(clip_key[2]),
                    "height": float(clip_key[3]),
                }
            )

        self._current_clip = clip_key

    # ── RendererBase required methods ──────────────────────────────

    def draw_path(self, gc, path, transform, rgbFace=None):
        self._apply_clip(gc)
        combined = transform + self._y_flip()
        fill = gc_to_fill(gc, rgbFace)
        stroke = gc_to_stroke(gc, self)

        # For large paths, use binary transport to skip Python loop overhead
        n_vertices = path.vertices.shape[0] if path.vertices is not None else 0
        if n_vertices > 20:
            path_t = path.transformed(combined)
            verts = np.ascontiguousarray(path_t.vertices, dtype=np.float64)
            codes = path_t.codes

            # Detect snap from stroke width (avoid double-transform via
            # is_rectilinear) — only snap thin rectilinear paths.
            snap = self._should_snap_fast(gc, codes)

            if codes is None:
                # Pure polyline (MOVETO + N LINETO) — use PolylineData for
                # stroke-only paths to avoid sending codes entirely.
                if fill is None and stroke is not None:
                    pts = np.ascontiguousarray(verts, dtype=np.float32)
                    self._scene.add_polyline_data(
                        pts.tobytes(),
                        len(verts),
                        stroke=stroke,
                    )
                    return
                codes = np.full(len(verts), 2, dtype=np.uint8)
                codes[0] = 1
            else:
                codes = np.ascontiguousarray(codes, dtype=np.uint8)

            self._scene.add_path_data(
                verts.tobytes(),
                codes.tobytes(),
                len(verts),
                snap=snap,
                fill=fill,
                stroke=stroke,
            )
        else:
            snap = self._should_snap(gc, path, combined)
            segments = path_to_segments(path, combined, snap=snap)
            if not segments:
                return
            self._scene.add_path(segments, fill=fill, stroke=stroke)

    def _should_snap(self, gc, path, combined_transform):
        """Check if the path should be snapped to pixel centers (Agg compat).

        Agg snaps rectilinear paths when linewidth <= snap_threshold (~1px).
        This includes fill-only paths (lw=0) which are also snapped.
        """
        snap = gc.get_snap()
        if snap is False:
            return False
        if snap is True:
            return True
        # Auto: snap rectilinear paths with thin or no stroke
        lw_px = gc.get_linewidth() * self.dpi / 72.0
        if lw_px > 1.5:
            return False
        return is_rectilinear(path, combined_transform)

    def _should_snap_fast(self, gc, codes):
        """Fast snap check for binary-transport paths (already transformed).

        Avoids the expensive is_rectilinear re-transform by checking codes
        directly — rectilinear paths have only MOVETO(1), LINETO(2),
        CLOSEPOLY(79) codes.
        """
        snap = gc.get_snap()
        if snap is False:
            return False
        if snap is True:
            return True
        lw_px = gc.get_linewidth() * self.dpi / 72.0
        if lw_px > 1.5:
            return False
        if codes is None:
            # Pure MOVETO+LINETO polyline — always rectilinear-eligible
            # but we don't know without checking vertices.  Skip snap for
            # large polylines (they are rarely axis lines).
            return False
        # Check if codes contain only M/L/Z (rectilinear candidates)
        mask = (codes == 1) | (codes == 2) | (codes == 79) | (codes == 0)
        return bool(mask.all())

    def draw_markers(self, gc, marker_path, marker_trans, path, trans, rgbFace=None):
        self._apply_clip(gc)
        # Apply marker_trans (encodes size + direction) then y-flip for
        # y-down display.  marker_trans signs encode direction — e.g.
        # x-tick has y_scale=-4.86 (downward in y-up).  The y-flip
        # converts that to +4.86 (downward in y-down).  Pass size=1.0
        # to Rust since the marker path is already at full pixel size.
        #
        corrected = marker_trans + Affine2D().scale(1, -1)
        marker_segments = path_to_segments(marker_path, corrected)
        if not marker_segments:
            return

        # Extract marker positions with y-flip applied.
        # Vectorized numpy path: transform vertices directly, avoiding the
        # expensive Python-level iter_segments loop.
        flip = self._y_flip()
        combined_trans = trans + flip
        path_t = path.transformed(combined_trans)
        verts = path_t.vertices
        if verts is None or len(verts) == 0:
            return

        # Agg snaps marker positions to integer pixels via floor(x + 0.5),
        # which is equivalent to round().  Round in numpy for all positions.
        positions = np.round(verts).astype(np.float64)

        # Filter out degenerate positions (NaN/Inf)
        finite_mask = np.isfinite(positions).all(axis=1)
        if not finite_mask.all():
            positions = positions[finite_mask]

        if len(positions) == 0:
            return

        fill = gc_to_fill(gc, rgbFace)
        stroke = gc_to_stroke(gc, self)

        self._scene.add_markers(
            marker_segments,
            positions,
            1.0,
            fill=fill,
            stroke=stroke,
        )

    def draw_text(self, gc, x, y, s, prop, angle, ismath=False, mtext=None):
        self._apply_clip(gc)
        # Render ALL text as FreeType paths for pixel-perfect match with Agg.
        # _get_text_path_transform handles flipy() internally (translates to
        # height - y), and draw_path applies _y_flip(), so the final position
        # is correct in y-down display space.
        self._draw_text_as_path(gc, x, y, s, prop, angle, ismath)

    def draw_image(self, gc, x, y, im, transform=None):
        self._apply_clip(gc)
        # im is an MxNx4 uint8 RGBA array.
        # matplotlib uses y-up convention: im[0] is the TOP row but (x, y)
        # is the bottom-left in y-up display coords. Agg draws the image
        # bottom-to-top, so im[-1] appears at screen top. Flip the array
        # so that im[-1] is the first row for tiny_skia's y-down rendering.
        im_flipped = im[::-1]
        h, w = im_flipped.shape[:2]
        raw = np.ascontiguousarray(im_flipped).tobytes()

        # Convert (x, y_bottom_yup) to (x, y_top_ydown).
        # Round to integer pixels to match Agg's integer placement.
        x_px = round(x)
        y_px = round(self.height - y - h)

        # Use blob transport (skip base64 encode/decode overhead)
        self._scene.add_image_blob(
            raw_bytes=raw,
            x=float(x_px),
            y=float(y_px),
            width=float(w),
            height=float(h),
        )

    def get_text_width_height_descent(self, s, prop, ismath):
        # Delegate to FreeType-based measurement in RendererBase for
        # identical text layout as Agg.
        return super().get_text_width_height_descent(s, prop, ismath)

    def get_canvas_width_height(self):
        return self.width, self.height

    def new_gc(self):
        return GraphicsContextBase()

    def points_to_pixels(self, points):
        return points * self.dpi / 72.0

    def flipy(self):
        return True

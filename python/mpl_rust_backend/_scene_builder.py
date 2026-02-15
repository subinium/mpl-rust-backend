"""Lightweight scene graph accumulator for the matplotlib backend."""

from __future__ import annotations

import struct

import numpy as np

try:
    import orjson as _json_mod

    def _dumps(obj: dict) -> bytes:
        return _json_mod.dumps(obj)

except ImportError:
    import json as _json_mod

    def _dumps(obj: dict) -> bytes:
        return _json_mod.dumps(obj, separators=(",", ":")).encode("utf-8")


_IDENTITY = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]


class SceneBuilder:
    """Accumulates scene graph nodes and optional binary blobs.

    Usage::

        sb = SceneBuilder(width=640, height=480, dpi=100)
        sb.add_path(segments, fill, stroke)
        sb.add_text(...)
        json_bytes, blobs = sb.build()
    """

    def __init__(self, width: float, height: float, dpi: float):
        self.width = width
        self.height = height
        self.dpi = dpi
        self._nodes: list[dict] = []
        self._blobs: list[bytes] = []
        self._group_stack: list[list[dict]] = []

    def push_group(self, transform=None, alpha=1.0, clip=None):
        """Open a new group. Subsequent nodes are added as children."""
        self._group_stack.append(self._nodes)
        self._nodes = []
        self._group_stack[-1].append(
            {
                "type": "group",
                "transform": transform or _IDENTITY,
                "alpha": alpha,
                "clip": clip,
                "children": self._nodes,
            }
        )

    def pop_group(self):
        """Close the current group."""
        if self._group_stack:
            self._nodes = self._group_stack.pop()

    def add_path(self, segments, fill=None, stroke=None, transform=None):
        node = {
            "type": "path",
            "segments": segments,
            "transform": transform or _IDENTITY,
        }
        if fill is not None:
            node["fill"] = fill
        if stroke is not None:
            node["stroke"] = stroke
        self._nodes.append(node)

    def add_text(
        self,
        content,
        x,
        y,
        font_size=10.0,
        font_family="DejaVu Sans",
        font_weight=400,
        color=None,
        rotation=0.0,
        ha="left",
        va="baseline",
        transform=None,
    ):
        node = {
            "type": "text",
            "content": content,
            "x": x,
            "y": y,
            "font_size": font_size,
            "font_family": font_family,
            "font_weight": font_weight,
            "color": color or [0.0, 0.0, 0.0, 1.0],
            "rotation": rotation,
            "ha": ha,
            "va": va,
            "transform": transform or _IDENTITY,
        }
        self._nodes.append(node)

    def add_markers(
        self,
        marker_segments,
        positions,
        size,
        fill=None,
        stroke=None,
        positions_transform=None,
        transform=None,
    ):
        """Add markers.

        For large position arrays (>30 points), uses MarkersData with a binary blob
        for zero-copy transport.  Accepts numpy arrays or lists of tuples.
        """
        n = len(positions)
        if n > 30:
            self._add_markers_data(
                marker_segments,
                positions,
                size,
                fill,
                stroke,
                positions_transform,
                transform,
            )
        else:
            node = {
                "type": "markers",
                "path": marker_segments,
                "positions": [[float(p[0]), float(p[1])] for p in positions],
                "size": float(size),
                "transform": transform or _IDENTITY,
            }
            if fill is not None:
                node["fill"] = fill
            if stroke is not None:
                node["stroke"] = stroke
            if positions_transform is not None:
                node["positions_transform"] = positions_transform
            self._nodes.append(node)

    def _add_markers_data(
        self,
        marker_segments,
        positions,
        size,
        fill,
        stroke,
        positions_transform,
        transform,
    ):
        pos_array = np.ascontiguousarray(
            positions if isinstance(positions, np.ndarray) else positions,
            dtype=np.float32,
        )
        blob_data = pos_array.tobytes()
        blob_idx = len(self._blobs)
        self._blobs.append(blob_data)

        node = {
            "type": "markers_data",
            "path": marker_segments,
            "positions_blob": blob_idx,
            "positions_dtype": "f32",
            "count": len(positions),
            "size": float(size),
            "transform": transform or _IDENTITY,
        }
        if fill is not None:
            node["fill"] = fill
        if stroke is not None:
            node["stroke"] = stroke
        if positions_transform is not None:
            node["positions_transform"] = positions_transform
        self._nodes.append(node)

    def add_path_data(
        self,
        vertices_bytes,
        codes_bytes,
        count,
        snap=False,
        fill=None,
        stroke=None,
        transform=None,
    ):
        """Add a path using binary vertex/code blobs (zero-copy transport)."""
        vblob = len(self._blobs)
        self._blobs.append(vertices_bytes)
        cblob = len(self._blobs)
        self._blobs.append(codes_bytes)
        node = {
            "type": "path_data",
            "vertices_blob": vblob,
            "vertices_dtype": "f64",
            "codes_blob": cblob,
            "count": count,
            "snap": snap,
            "transform": transform or _IDENTITY,
        }
        if fill is not None:
            node["fill"] = fill
        if stroke is not None:
            node["stroke"] = stroke
        self._nodes.append(node)

    def add_polyline_data(
        self,
        points_bytes,
        count,
        stroke=None,
        transform=None,
    ):
        """Add a stroke-only polyline using binary XY blob (no codes needed)."""
        blob_idx = len(self._blobs)
        self._blobs.append(points_bytes)
        node = {
            "type": "polyline_data",
            "points_blob": blob_idx,
            "points_dtype": "f32",
            "count": count,
            "transform": transform or _IDENTITY,
        }
        if stroke is not None:
            node["stroke"] = stroke
        self._nodes.append(node)

    def add_image_blob(self, raw_bytes, x, y, width, height, transform=None):
        """Add an image using raw RGBA blob (skip base64 encoding)."""
        blob_idx = len(self._blobs)
        self._blobs.append(raw_bytes)
        self._nodes.append(
            {
                "type": "image_blob",
                "data_blob": blob_idx,
                "x": x,
                "y": y,
                "width": width,
                "height": height,
                "transform": transform or _IDENTITY,
            }
        )

    def build(self) -> tuple[bytes, list[bytes]]:
        """Serialize the scene graph to JSON bytes + blob list."""
        # Close any open groups
        while self._group_stack:
            self.pop_group()

        scene = {
            "width": self.width,
            "height": self.height,
            "dpi": self.dpi,
            "background": [1.0, 1.0, 1.0, 1.0],
            "nodes": self._nodes,
        }
        json_bytes = _dumps(scene)
        return json_bytes, self._blobs

    def build_packet(self) -> bytes:
        """Serialize into a single PXPK packet (JSON + blobs)."""
        json_bytes, blobs = self.build()
        parts = [
            b"PXPK",
            struct.pack("<I", len(json_bytes)),
            struct.pack("<I", len(blobs)),
            json_bytes,
        ]
        for blob in blobs:
            parts.append(struct.pack("<I", len(blob)))
            parts.append(blob)
        return b"".join(parts)

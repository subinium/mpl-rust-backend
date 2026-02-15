"""Translate matplotlib primitives to scene graph dicts."""

from __future__ import annotations

import numpy as np

# matplotlib path codes
MOVETO = 1
LINETO = 2
CURVE3 = 3
CURVE4 = 4
CLOSEPOLY = 79


def path_to_segments(path, transform=None, snap=False):
    """Convert a matplotlib Path + transform into a list of scene PathSegment dicts.

    Each segment is ``{"cmd": "M"/"L"/"C"/"Q"/"Z", "points": [...]}``.
    When *snap* is True, MOVETO/LINETO coordinates are rounded to the nearest
    integer (pixel-center in Agg convention) to match Agg's path snapping.
    """
    if path is None:
        return []

    if transform is not None:
        path = path.transformed(transform)

    _rnd = round  # local alias for speed

    segments = []
    for points, code in path.iter_segments(simplify=False, curves=True):
        if code == MOVETO:
            x, y = float(points[0]), float(points[1])
            if snap:
                x, y = _rnd(x), _rnd(y)
            segments.append({"cmd": "M", "points": [x, y]})
        elif code == LINETO:
            x, y = float(points[0]), float(points[1])
            if snap:
                x, y = _rnd(x), _rnd(y)
            segments.append({"cmd": "L", "points": [x, y]})
        elif code == CURVE3:
            segments.append(
                {
                    "cmd": "Q",
                    "points": [float(v) for v in points[:4]],
                }
            )
        elif code == CURVE4:
            segments.append(
                {
                    "cmd": "C",
                    "points": [float(v) for v in points[:6]],
                }
            )
        elif code == CLOSEPOLY:
            segments.append({"cmd": "Z", "points": []})
    return segments


def is_rectilinear(path, transform=None):
    """Return True if the transformed path contains only H/V line segments."""
    if path is None:
        return True
    if transform is not None:
        path = path.transformed(transform)
    prev_x = prev_y = None
    for points, code in path.iter_segments(simplify=False, curves=True):
        if code in (CURVE3, CURVE4):
            return False
        if code in (MOVETO, LINETO):
            x, y = float(points[0]), float(points[1])
            if code == LINETO and prev_x is not None:
                dx = abs(x - prev_x)
                dy = abs(y - prev_y)
                if dx > 0.01 and dy > 0.01:
                    return False
            prev_x, prev_y = x, y
    return True


def gc_to_stroke(gc, renderer):
    """Extract stroke properties from a matplotlib GraphicsContext.

    Line widths and dash lengths arrive in points from matplotlib;
    convert them to display pixels so the Rust engine (running at
    scale=1.0) renders them at the correct size.
    """
    rgb = gc.get_rgb()
    linewidth = gc.get_linewidth()
    if linewidth <= 0:
        return None

    # points → display pixels
    pt2px = renderer.dpi / 72.0 if renderer is not None else 1.0

    stroke = {
        "color": [
            float(rgb[0]),
            float(rgb[1]),
            float(rgb[2]),
            float(gc.get_alpha() if gc.get_forced_alpha() else rgb[3]),
        ],
        "width": float(linewidth * pt2px),
        "line_cap": _cap_style(gc),
        "line_join": _join_style(gc),
        "dash_array": [],
        "dash_offset": 0.0,
    }

    dash = gc.get_dashes()
    if dash is not None:
        offset, dashes = dash
        if dashes is not None and len(dashes) > 0:
            stroke["dash_array"] = [float(d * pt2px) for d in dashes]
            stroke["dash_offset"] = float(offset * pt2px) if offset else 0.0

    return stroke


def gc_to_fill(gc, rgbFace):
    """Build a FillStyle dict from rgbFace color."""
    if rgbFace is None:
        return None
    alpha = (
        gc.get_alpha()
        if gc.get_forced_alpha()
        else (rgbFace[3] if len(rgbFace) > 3 else 1.0)
    )
    return {
        "color": [
            float(rgbFace[0]),
            float(rgbFace[1]),
            float(rgbFace[2]),
            float(alpha),
        ],
    }


def affine_to_array(transform):
    """Convert a matplotlib Affine2D/Transform to a 6-element list [a, b, c, d, tx, ty].

    matplotlib's frozen matrix is [[a, c, tx], [b, d, ty], [0, 0, 1]].
    """
    if transform is None:
        return [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]
    m = transform.frozen().get_matrix()
    return [
        float(m[0, 0]),
        float(m[1, 0]),
        float(m[0, 1]),
        float(m[1, 1]),
        float(m[0, 2]),
        float(m[1, 2]),
    ]


def _cap_style(gc):
    cap = gc.get_capstyle()
    if hasattr(cap, "name"):
        cap = cap.name
    cap = str(cap)
    return {"butt": "butt", "round": "round", "projecting": "square"}.get(cap, "butt")


def _join_style(gc):
    join = gc.get_joinstyle()
    if hasattr(join, "name"):
        join = join.name
    join = str(join)
    return {"miter": "miter", "round": "round", "bevel": "bevel"}.get(join, "miter")

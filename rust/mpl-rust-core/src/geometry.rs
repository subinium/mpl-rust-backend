use tiny_skia::PathBuilder;

use crate::scene::{ClipRect, PathCmd, PathSegment};

/// Convert a sequence of `PathSegment` commands into a `tiny_skia::Path`.
///
/// Returns `None` if the resulting path is empty or degenerate.
pub fn segments_to_path(segments: &[PathSegment]) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();

    for seg in segments {
        match seg.cmd {
            PathCmd::M => {
                if seg.points.len() >= 2 {
                    pb.move_to(seg.points[0] as f32, seg.points[1] as f32);
                }
            }
            PathCmd::L => {
                if seg.points.len() >= 2 {
                    pb.line_to(seg.points[0] as f32, seg.points[1] as f32);
                }
            }
            PathCmd::C => {
                if seg.points.len() >= 6 {
                    pb.cubic_to(
                        seg.points[0] as f32,
                        seg.points[1] as f32,
                        seg.points[2] as f32,
                        seg.points[3] as f32,
                        seg.points[4] as f32,
                        seg.points[5] as f32,
                    );
                }
            }
            PathCmd::Q => {
                if seg.points.len() >= 4 {
                    pb.quad_to(
                        seg.points[0] as f32,
                        seg.points[1] as f32,
                        seg.points[2] as f32,
                        seg.points[3] as f32,
                    );
                }
            }
            PathCmd::Z => {
                pb.close();
            }
        }
    }

    pb.finish()
}

/// Read an (x, y) vertex pair from raw bytes at the given byte offset.
#[inline(always)]
pub fn read_vertex_f32(raw: &[u8], off: usize) -> (f64, f64) {
    let mut bx = [0u8; 4];
    let mut by = [0u8; 4];
    bx.copy_from_slice(&raw[off..off + 4]);
    by.copy_from_slice(&raw[off + 4..off + 8]);
    (f32::from_le_bytes(bx) as f64, f32::from_le_bytes(by) as f64)
}

#[inline(always)]
pub fn read_vertex_f64(raw: &[u8], off: usize) -> (f64, f64) {
    let mut bx = [0u8; 8];
    let mut by = [0u8; 8];
    bx.copy_from_slice(&raw[off..off + 8]);
    by.copy_from_slice(&raw[off + 8..off + 16]);
    (f64::from_le_bytes(bx), f64::from_le_bytes(by))
}

/// Build a `tiny_skia::Path` from raw vertices and codes (u8 N) blobs.
///
/// Supports both f32 and f64 vertex data via the `dtype` parameter.
///
/// Codes follow the matplotlib convention:
/// - 1 = MOVETO
/// - 2 = LINETO
/// - 3 = CURVE3 (quadratic Bézier, consumes next vertex as control point)
/// - 4 = CURVE4 (cubic Bézier, consumes next 2 vertices as control points)
/// - 79 = CLOSEPOLY
///
/// If `snap` is true, MOVETO and LINETO coordinates are rounded to the nearest
/// integer (pixel-aligned rendering for Agg compatibility).
pub fn raw_path_from_vertices_codes(
    vertices_raw: &[u8],
    codes_raw: &[u8],
    count: usize,
    snap: bool,
    dtype: &str,
) -> Option<tiny_skia::Path> {
    if count == 0 {
        return None;
    }
    let is_f32 = dtype == "f32";
    let elem = if is_f32 { 4usize } else { 8usize };
    let stride = elem * 2; // bytes per vertex (2 components)
    let expected_verts = count.checked_mul(stride)?;
    if vertices_raw.len() != expected_verts || codes_raw.len() != count {
        return None;
    }

    let read_xy = if is_f32 { read_vertex_f32 } else { read_vertex_f64 };

    let mut pb = PathBuilder::new();
    let mut i = 0usize;

    while i < count {
        let code = codes_raw[i];
        let (x, y) = read_xy(vertices_raw, i * stride);

        match code {
            1 => {
                // MOVETO
                let (fx, fy) = if snap {
                    (x.round() as f32, y.round() as f32)
                } else {
                    (x as f32, y as f32)
                };
                if fx.is_finite() && fy.is_finite() {
                    pb.move_to(fx, fy);
                }
                i += 1;
            }
            2 => {
                // LINETO
                let (fx, fy) = if snap {
                    (x.round() as f32, y.round() as f32)
                } else {
                    (x as f32, y as f32)
                };
                if fx.is_finite() && fy.is_finite() {
                    pb.line_to(fx, fy);
                }
                i += 1;
            }
            3 => {
                // CURVE3 (quadratic): current vertex is control point,
                // next vertex is endpoint
                if i + 1 >= count {
                    i += 1;
                    continue;
                }
                let (x2, y2) = read_xy(vertices_raw, (i + 1) * stride);
                let (x1f, y1f) = (x as f32, y as f32);
                let (x2f, y2f) = (x2 as f32, y2 as f32);
                if x1f.is_finite() && y1f.is_finite() && x2f.is_finite() && y2f.is_finite() {
                    pb.quad_to(x1f, y1f, x2f, y2f);
                }
                i += 2;
            }
            4 => {
                // CURVE4 (cubic): current vertex is 1st control point,
                // next vertex is 2nd control point, vertex after is endpoint
                if i + 2 >= count {
                    i += 1;
                    continue;
                }
                let (x2, y2) = read_xy(vertices_raw, (i + 1) * stride);
                let (x3, y3) = read_xy(vertices_raw, (i + 2) * stride);
                let (x1f, y1f) = (x as f32, y as f32);
                let (x2f, y2f) = (x2 as f32, y2 as f32);
                let (x3f, y3f) = (x3 as f32, y3 as f32);
                if x1f.is_finite() && y1f.is_finite()
                    && x2f.is_finite() && y2f.is_finite()
                    && x3f.is_finite() && y3f.is_finite()
                {
                    pb.cubic_to(x1f, y1f, x2f, y2f, x3f, y3f);
                }
                i += 3;
            }
            79 => {
                // CLOSEPOLY
                pb.close();
                i += 1;
            }
            _ => {
                // Unknown code (incl. STOP=0) — skip
                i += 1;
            }
        }
    }

    pb.finish()
}

/// Build a clip mask from a `ClipRect`.
///
/// Returns `None` if the rect is degenerate or the mask cannot be allocated.
pub fn build_clip_mask(
    clip: &ClipRect,
    transform: tiny_skia::Transform,
    width: u32,
    height: u32,
) -> Option<tiny_skia::Mask> {
    let mut pb = PathBuilder::new();
    pb.push_rect(tiny_skia::Rect::from_xywh(
        clip.x as f32,
        clip.y as f32,
        clip.width as f32,
        clip.height as f32,
    )?);
    let path = pb.finish()?;

    let mut mask = tiny_skia::Mask::new(width, height)?;
    mask.fill_path(&path, tiny_skia::FillRule::Winding, true, transform);
    Some(mask)
}

/// Convert a 6-element affine array `[a, b, c, d, tx, ty]` into a `tiny_skia::Transform`.
///
/// The mapping is:
/// ```text
/// | sx  kx tx |     [a, b, c, d, tx, ty]
/// | ky  sy ty |  =>  sx=a, ky=b, kx=c, sy=d, tx=tx, ty=ty
/// |  0   0  1 |
/// ```
pub fn affine_to_transform(m: &[f64; 6]) -> tiny_skia::Transform {
    tiny_skia::Transform::from_row(
        m[0] as f32, // sx
        m[1] as f32, // ky
        m[2] as f32, // kx
        m[3] as f32, // sy
        m[4] as f32, // tx
        m[5] as f32, // ty
    )
}

/// Multiply (compose) two affine transforms represented as `[a, b, c, d, tx, ty]`.
/// Result = parent * child  (child applied first).
pub fn compose_affine(parent: &[f64; 6], child: &[f64; 6]) -> [f64; 6] {
    let p = parent;
    let c = child;
    [
        p[0] * c[0] + p[2] * c[1],
        p[1] * c[0] + p[3] * c[1],
        p[0] * c[2] + p[2] * c[3],
        p[1] * c[2] + p[3] * c[3],
        p[0] * c[4] + p[2] * c[5] + p[4],
        p[1] * c[4] + p[3] * c[5] + p[5],
    ]
}

/// The identity affine transform.
pub const IDENTITY_AFFINE: [f64; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identity_compose() {
        let id = IDENTITY_AFFINE;
        let m = [2.0, 0.0, 0.0, 3.0, 10.0, 20.0];
        let result = compose_affine(&id, &m);
        assert_eq!(result, m);
    }

    #[test]
    fn test_segments_to_path_basic() {
        let segments = vec![
            PathSegment {
                cmd: PathCmd::M,
                points: vec![0.0, 0.0],
            },
            PathSegment {
                cmd: PathCmd::L,
                points: vec![100.0, 0.0],
            },
            PathSegment {
                cmd: PathCmd::L,
                points: vec![100.0, 100.0],
            },
            PathSegment {
                cmd: PathCmd::Z,
                points: vec![],
            },
        ];
        let path = segments_to_path(&segments);
        assert!(path.is_some());
    }
}

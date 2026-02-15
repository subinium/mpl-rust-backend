use tiny_skia::PathBuilder;

use crate::scene::{ClipRect, PathSegment};

/// Convert a sequence of `PathSegment` commands into a `tiny_skia::Path`.
///
/// Returns `None` if the resulting path is empty or degenerate.
pub fn segments_to_path(segments: &[PathSegment]) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();

    for seg in segments {
        match seg.cmd.as_str() {
            "M" => {
                if seg.points.len() >= 2 {
                    pb.move_to(seg.points[0] as f32, seg.points[1] as f32);
                }
            }
            "L" => {
                if seg.points.len() >= 2 {
                    pb.line_to(seg.points[0] as f32, seg.points[1] as f32);
                }
            }
            "C" => {
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
            "Q" => {
                if seg.points.len() >= 4 {
                    pb.quad_to(
                        seg.points[0] as f32,
                        seg.points[1] as f32,
                        seg.points[2] as f32,
                        seg.points[3] as f32,
                    );
                }
            }
            "Z" => {
                pb.close();
            }
            _ => {
                // Unknown command — skip
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
                cmd: "M".to_string(),
                points: vec![0.0, 0.0],
            },
            PathSegment {
                cmd: "L".to_string(),
                points: vec![100.0, 0.0],
            },
            PathSegment {
                cmd: "L".to_string(),
                points: vec![100.0, 100.0],
            },
            PathSegment {
                cmd: "Z".to_string(),
                points: vec![],
            },
        ];
        let path = segments_to_path(&segments);
        assert!(path.is_some());
    }
}

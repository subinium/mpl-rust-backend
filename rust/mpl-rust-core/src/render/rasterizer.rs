use std::borrow::Cow;
use std::thread;

use png::{BitDepth, ColorType, Compression, FilterType};
use tiny_skia::{Color, FillRule, LineCap, LineJoin, Paint, Pixmap, Stroke, Transform};

use crate::color::Colormap;
use crate::geometry::{affine_to_transform, build_clip_mask, segments_to_path};
use crate::scene::{FillStyle, Scene, SceneNode, StrokeStyle};
use crate::text;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PngProfile {
    Native,
    Size,
}

fn resolve_png_profile() -> PngProfile {
    match std::env::var("PLOTIX_PNG_PROFILE") {
        Ok(v) => {
            let t = v.trim().to_ascii_lowercase();
            if t == "size" || t == "small" || t == "best" {
                PngProfile::Size
            } else {
                PngProfile::Native
            }
        }
        Err(_) => PngProfile::Native,
    }
}

#[inline]
fn image_parallel_worker_count(units: usize, structure_limit: usize) -> usize {
    if units == 0 || structure_limit <= 1 {
        return 1;
    }

    let mode = std::env::var("PLOTIX_IMAGE_PARALLEL")
        .unwrap_or_else(|_| "auto".to_string())
        .trim()
        .to_ascii_lowercase();
    let min_units = std::env::var("PLOTIX_IMAGE_PARALLEL_MIN_UNITS")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|v| *v >= 16_384)
        .unwrap_or(262_144);
    let enabled = match mode.as_str() {
        "0" | "false" | "off" | "no" => false,
        "1" | "true" | "on" | "yes" => true,
        _ => units >= min_units,
    };
    if !enabled {
        return 1;
    }

    let max_threads = thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let env_cap = std::env::var("PLOTIX_IMAGE_PARALLEL_MAX_WORKERS")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|v| *v >= 1)
        .unwrap_or(max_threads);
    max_threads
        .min(env_cap)
        .min(structure_limit)
        .min(units)
        .max(1)
}

fn has_non_opaque_alpha(raw: &[u8]) -> bool {
    raw.chunks_exact(4).any(|px| px[3] != 255)
}

fn unpremultiply_rgba(raw: &[u8]) -> Vec<u8> {
    #[inline]
    fn unpremultiply_slice(src: &[u8], dst: &mut [u8]) {
        for (s, d) in src.chunks_exact(4).zip(dst.chunks_exact_mut(4)) {
            let a = s[3];
            if a == 0 {
                d.copy_from_slice(&[0, 0, 0, 0]);
                continue;
            }
            if a == 255 {
                d.copy_from_slice(s);
                continue;
            }
            let a_u32 = a as u32;
            let r = ((s[0] as u32 * 255) + (a_u32 / 2)) / a_u32;
            let g = ((s[1] as u32 * 255) + (a_u32 / 2)) / a_u32;
            let b = ((s[2] as u32 * 255) + (a_u32 / 2)) / a_u32;
            d[0] = r.min(255) as u8;
            d[1] = g.min(255) as u8;
            d[2] = b.min(255) as u8;
            d[3] = a;
        }
    }

    let mut out = vec![0u8; raw.len()];
    let pixel_count = raw.len() / 4;
    let workers = image_parallel_worker_count(pixel_count, pixel_count);
    if workers > 1 {
        let chunk_pixels = pixel_count.div_ceil(workers);
        let chunk_bytes = chunk_pixels.saturating_mul(4);
        thread::scope(|scope| {
            for (chunk_idx, dst_chunk) in out.chunks_mut(chunk_bytes).enumerate() {
                let start_px = chunk_idx.saturating_mul(chunk_pixels);
                let start_b = start_px.saturating_mul(4);
                let src_chunk = &raw[start_b..start_b + dst_chunk.len()];
                scope.spawn(move || {
                    unpremultiply_slice(src_chunk, dst_chunk);
                });
            }
        });
        return out;
    }

    unpremultiply_slice(raw, &mut out);
    out
}

fn encode_pixmap_png(pixmap: &Pixmap) -> Vec<u8> {
    let profile = resolve_png_profile();
    if profile == PngProfile::Native {
        return pixmap.encode_png().expect("failed to encode PNG");
    }

    let w = pixmap.width();
    let h = pixmap.height();

    let raw = pixmap.data();
    let png_bytes: Cow<'_, [u8]> = if has_non_opaque_alpha(raw) {
        Cow::Owned(unpremultiply_rgba(raw))
    } else {
        Cow::Borrowed(raw)
    };

    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, w, h);
    encoder.set_color(ColorType::Rgba);
    encoder.set_depth(BitDepth::Eight);
    encoder.set_compression(Compression::Best);
    encoder.set_filter(FilterType::Paeth);

    match encoder
        .write_header()
        .and_then(|mut writer| writer.write_image_data(&png_bytes))
    {
        Ok(()) => out,
        Err(_) => pixmap.encode_png().expect("failed to encode PNG"),
    }
}

/// Render a scene graph to a PNG byte buffer.
///
/// The scene uses a coordinate system in points (1 pt = 1/72 inch).
/// Pixel dimensions are derived from `scene.width * dpi / 72`.
pub fn render_to_png(scene: &Scene) -> Vec<u8> {
    let scale = scene.dpi / 72.0;
    let px_width = (scene.width * scale).ceil() as u32;
    let px_height = (scene.height * scale).ceil() as u32;

    // Clamp to at least 1x1 to avoid zero-size pixmap.
    let px_width = px_width.max(1);
    let px_height = px_height.max(1);

    let mut pixmap = Pixmap::new(px_width, px_height).expect("failed to allocate pixmap");

    // Fill background.
    let bg = &scene.background;
    pixmap.fill(
        Color::from_rgba(bg[0] as f32, bg[1] as f32, bg[2] as f32, bg[3] as f32)
            .unwrap_or(Color::WHITE),
    );

    // The base transform scales from points to pixels.
    let base_transform = Transform::from_scale(scale as f32, scale as f32);

    for node in &scene.nodes {
        render_node(&mut pixmap, node, base_transform, 1.0, None);
    }

    encode_pixmap_png(&pixmap)
}

/// Render a scene graph to PNG using optional raw blobs referenced by scene nodes.
pub fn render_to_png_with_blobs(scene: &Scene, blobs: &[&[u8]]) -> Vec<u8> {
    let scale = scene.dpi / 72.0;
    let px_width = (scene.width * scale).ceil() as u32;
    let px_height = (scene.height * scale).ceil() as u32;
    let px_width = px_width.max(1);
    let px_height = px_height.max(1);

    let mut pixmap = Pixmap::new(px_width, px_height).expect("failed to allocate pixmap");
    let bg = &scene.background;
    pixmap.fill(
        Color::from_rgba(bg[0] as f32, bg[1] as f32, bg[2] as f32, bg[3] as f32)
            .unwrap_or(Color::WHITE),
    );
    let base_transform = Transform::from_scale(scale as f32, scale as f32);
    for node in &scene.nodes {
        render_node(&mut pixmap, node, base_transform, 1.0, Some(blobs));
    }
    encode_pixmap_png(&pixmap)
}

/// Recursively render a single scene node onto the pixmap.
fn render_node(
    pixmap: &mut Pixmap,
    node: &SceneNode,
    parent_transform: Transform,
    parent_alpha: f64,
    blobs: Option<&[&[u8]]>,
) {
    match node {
        SceneNode::Group {
            transform,
            alpha,
            clip,
            children,
        } => {
            let local = affine_to_transform(transform);
            let combined = parent_transform.pre_concat(local);
            let combined_alpha = parent_alpha * alpha;

            if let Some(clip_rect) = clip {
                // Render children into a temporary pixmap and composite with clip.
                let width = pixmap.width();
                let height = pixmap.height();

                if let Some(mask) = build_clip_mask(clip_rect, combined, width, height) {
                    // Render children into a temp buffer.
                    let mut tmp = Pixmap::new(width, height).expect("alloc temp pixmap");
                    for child in children {
                        render_node(&mut tmp, child, combined, combined_alpha, blobs);
                    }

                    // Composite the temp buffer through the clip mask.
                    pixmap.draw_pixmap(
                        0,
                        0,
                        tmp.as_ref(),
                        &tiny_skia::PixmapPaint {
                            opacity: 1.0,
                            blend_mode: tiny_skia::BlendMode::SourceOver,
                            quality: tiny_skia::FilterQuality::Bilinear,
                        },
                        Transform::identity(),
                        Some(&mask),
                    );
                }
            } else {
                for child in children {
                    render_node(pixmap, child, combined, combined_alpha, blobs);
                }
            }
        }

        SceneNode::Path {
            segments,
            fill,
            stroke,
            transform,
        } => {
            let local = affine_to_transform(transform);
            let combined = parent_transform.pre_concat(local);

            if let Some(path) = segments_to_path(segments) {
                if let Some(fill_style) = fill {
                    let paint = make_fill_paint(&fill_style.color, parent_alpha);
                    pixmap.fill_path(&path, &paint, FillRule::Winding, combined, None);
                }

                if let Some(stroke_style) = stroke {
                    let paint = make_fill_paint(&stroke_style.color, parent_alpha);
                    let sk_stroke = make_stroke(stroke_style, parent_transform);
                    // Shift stroke by +0.5px to convert from matplotlib's
                    // pixel-center convention to tiny_skia's pixel-edge convention.
                    let stroke_transform = combined.pre_concat(Transform::from_translate(0.5, 0.5));
                    pixmap.stroke_path(&path, &paint, &sk_stroke, stroke_transform, None);
                }
            }
        }

        SceneNode::PolylineData {
            points_data,
            points_blob,
            points_dtype,
            count,
            stroke,
            transform,
        } => {
            let local = affine_to_transform(transform);
            let combined = parent_transform.pre_concat(local);

            if *count >= 2 {
                let path_result = if let Some(blob_idx) = points_blob {
                    blobs
                        .and_then(|all| all.get(*blob_idx))
                        .ok_or_else(|| "missing points_blob".to_string())
                        .and_then(|raw| polyline_path_from_raw(raw, points_dtype, *count))
                } else if let Some(points_b64) = points_data.as_ref() {
                    polyline_path_from_base64(points_b64, points_dtype, *count)
                } else {
                    Err("polyline_data requires points_data or points_blob".to_string())
                };

                if let Ok(Some(path)) = path_result {
                    if let Some(stroke_style) = stroke {
                        let paint = make_fill_paint(&stroke_style.color, parent_alpha);
                        let sk_stroke = make_stroke(stroke_style, parent_transform);
                        let stroke_transform = combined.pre_concat(Transform::from_translate(0.5, 0.5));
                        pixmap.stroke_path(&path, &paint, &sk_stroke, stroke_transform, None);
                    }
                }
            }
        }

        SceneNode::PolygonsData {
            points_data,
            points_blob,
            points_dtype,
            point_count,
            ring_sizes_data,
            ring_sizes_blob,
            ring_sizes_dtype,
            polygon_count,
            fill_colors_data,
            fill_colors_blob,
            fill_colors_dtype,
            fill,
            stroke,
            transform,
        } => {
            let local = affine_to_transform(transform);
            let combined = parent_transform.pre_concat(local);

            if *polygon_count == 0 || *point_count == 0 {
                return;
            }

            let points_raw_owned = if points_blob.is_some() {
                None
            } else if let Some(points_b64) = points_data.as_ref() {
                base64_decode(points_b64).ok()
            } else {
                None
            };
            let points_raw: &[u8] = if let Some(blob_idx) = points_blob {
                let Some(all) = blobs else {
                    return;
                };
                let Some(bytes) = all.get(*blob_idx) else {
                    return;
                };
                bytes
            } else if let Some(ref owned) = points_raw_owned {
                owned.as_slice()
            } else {
                return;
            };

            let ring_sizes_raw_owned = if ring_sizes_blob.is_some() {
                None
            } else if let Some(ring_b64) = ring_sizes_data.as_ref() {
                base64_decode(ring_b64).ok()
            } else {
                None
            };
            let ring_sizes_raw: &[u8] = if let Some(blob_idx) = ring_sizes_blob {
                let Some(all) = blobs else {
                    return;
                };
                let Some(bytes) = all.get(*blob_idx) else {
                    return;
                };
                bytes
            } else if let Some(ref owned) = ring_sizes_raw_owned {
                owned.as_slice()
            } else {
                return;
            };

            let fill_colors_raw_owned = if fill_colors_blob.is_some() {
                None
            } else {
                fill_colors_data
                    .as_ref()
                    .and_then(|s| base64_decode(s).ok())
            };
            let fill_colors_raw: Option<&[u8]> = if let Some(blob_idx) = fill_colors_blob {
                let Some(all) = blobs else {
                    return;
                };
                let Some(bytes) = all.get(*blob_idx) else {
                    return;
                };
                Some(*bytes)
            } else {
                fill_colors_raw_owned.as_deref()
            };

            let _ = render_polygons_data_raw(
                pixmap,
                points_raw,
                points_dtype,
                *point_count,
                ring_sizes_raw,
                ring_sizes_dtype,
                *polygon_count,
                fill_colors_raw,
                fill_colors_dtype,
                fill,
                stroke,
                combined,
                parent_transform,
                parent_alpha,
            );
        }

        SceneNode::Text {
            content,
            x,
            y,
            font_size,
            color,
            font_weight,
            rotation,
            ha,
            va,
            transform,
            ..
        } => {
            let local = affine_to_transform(transform);
            let combined = parent_transform.pre_concat(local);

            // Measure text to determine bounding box for alignment.
            let (w, h, descent) = text::measure_text_weighted(content, *font_size, *font_weight);

            // Compute anchor offset based on horizontal/vertical alignment.
            let dx = match ha.as_str() {
                "center" => -w / 2.0,
                "right" => -w,
                _ => 0.0, // "left"
            };
            let dy = match va.as_str() {
                "top" => 0.0,
                "center" => -h / 2.0,
                // Matplotlib uses center_baseline for y-ticks; for single-line
                // labels this should visually match center alignment.
                "center_baseline" | "centerbaseline" => -h / 2.0,
                "bottom" => -h,
                _ => -(h - descent), // "baseline"
            };

            let text_x = *x + dx;
            let text_y = *y + dy;

            // Apply rotation around the anchor point (rotation is in degrees).
            // Negate because matplotlib convention is counter-clockwise,
            // but tiny-skia (y-down) treats positive as clockwise.
            let rot_transform = if rotation.abs() > 1e-6 {
                Transform::from_rotate_at(-(*rotation as f32), *x as f32, *y as f32)
            } else {
                Transform::identity()
            };
            let text_combined = combined.pre_concat(rot_transform);

            // Render actual text glyphs via fontdue.
            let text_color = [color[0], color[1], color[2], color[3] * parent_alpha];
            let text_oversample = resolve_text_oversample(text_combined, w as f32, h as f32);
            if let Some(text_pixmap) = text::render_text_to_pixmap_scaled_weighted(
                content,
                *font_size,
                text_color,
                text_oversample,
                *font_weight,
            ) {
                let mut draw_transform = text_combined
                    .pre_concat(Transform::from_translate(text_x as f32, text_y as f32))
                    .pre_concat(Transform::from_scale(
                        1.0 / text_oversample,
                        1.0 / text_oversample,
                    ));

                // Optional pixel snapping for non-rotated text to reduce blur.
                if should_snap_text_to_pixels(*rotation) {
                    draw_transform.tx = draw_transform.tx.round();
                    draw_transform.ty = draw_transform.ty.round();
                }

                let scale_after_resample =
                    (transform_effective_scale(text_combined) / text_oversample).max(1e-6);
                let text_filter =
                    if rotation.abs() <= 1e-6 && (scale_after_resample - 1.0).abs() <= 0.08 {
                        tiny_skia::FilterQuality::Nearest
                    } else {
                        tiny_skia::FilterQuality::Bilinear
                    };
                pixmap.draw_pixmap(
                    0,
                    0,
                    text_pixmap.as_ref(),
                    &tiny_skia::PixmapPaint {
                        opacity: 1.0,
                        blend_mode: tiny_skia::BlendMode::SourceOver,
                        quality: text_filter,
                    },
                    draw_transform,
                    None,
                );
            }
        }

        SceneNode::Markers {
            path: marker_path,
            positions,
            size,
            fill,
            stroke,
            positions_transform,
            transform,
        } => {
            let local = affine_to_transform(transform);
            let combined = parent_transform.pre_concat(local);
            render_markers_positions(
                pixmap,
                marker_path,
                positions,
                *size,
                fill,
                stroke,
                positions_transform.as_ref(),
                combined,
                parent_transform,
                parent_alpha,
            );
        }

        SceneNode::MarkersData {
            path: marker_path,
            positions_data,
            positions_blob,
            positions_dtype,
            count,
            size,
            fill,
            stroke,
            positions_transform,
            transform,
        } => {
            let local = affine_to_transform(transform);
            let combined = parent_transform.pre_concat(local);
            let _ = if let Some(blob_idx) = positions_blob {
                blobs
                    .and_then(|all| all.get(*blob_idx))
                    .ok_or_else(|| "missing positions_blob".to_string())
                    .and_then(|raw| {
                        render_markers_positions_raw_interleaved(
                            pixmap,
                            marker_path,
                            raw,
                            positions_dtype,
                            *count,
                            *size,
                            fill,
                            stroke,
                            positions_transform.as_ref(),
                            combined,
                            parent_transform,
                            parent_alpha,
                        )
                    })
            } else if let Some(positions_b64) = positions_data.as_ref() {
                render_markers_positions_base64_interleaved(
                    pixmap,
                    marker_path,
                    positions_b64,
                    positions_dtype,
                    *count,
                    *size,
                    fill,
                    stroke,
                    positions_transform.as_ref(),
                    combined,
                    parent_transform,
                    parent_alpha,
                )
            } else {
                Err("markers_data requires positions_data or positions_blob".to_string())
            };
        }

        SceneNode::Image {
            data,
            x,
            y,
            width,
            height,
            transform,
        } => {
            let local = affine_to_transform(transform);
            let combined = parent_transform.pre_concat(local);

            // Decode base64 RGBA data.
            if let Ok(mut raw_bytes) = base64_decode(data) {
                let img_w = *width as u32;
                let img_h = *height as u32;
                let expected_len = (img_w * img_h * 4) as usize;

                if raw_bytes.len() == expected_len {
                    // Convert RGBA to premultiplied RGBA as required by tiny-skia.
                    for pixel in raw_bytes.chunks_exact_mut(4) {
                        let a = pixel[3] as f32 / 255.0;
                        pixel[0] = (pixel[0] as f32 * a + 0.5) as u8;
                        pixel[1] = (pixel[1] as f32 * a + 0.5) as u8;
                        pixel[2] = (pixel[2] as f32 * a + 0.5) as u8;
                    }

                    if let Some(img_pixmap) = Pixmap::from_vec(
                        raw_bytes,
                        tiny_skia::IntSize::from_wh(img_w, img_h).unwrap(),
                    ) {
                        let img_transform =
                            combined.pre_concat(Transform::from_translate(*x as f32, *y as f32));

                        pixmap.draw_pixmap(
                            0,
                            0,
                            img_pixmap.as_ref(),
                            &tiny_skia::PixmapPaint {
                                opacity: parent_alpha as f32,
                                blend_mode: tiny_skia::BlendMode::SourceOver,
                                quality: tiny_skia::FilterQuality::Nearest,
                            },
                            img_transform,
                            None,
                        );
                    }
                }
            }
        }

        SceneNode::ImageData {
            shape,
            dtype,
            data,
            data_blob,
            cmap,
            norm,
            origin,
            interpolation,
            alpha,
            x,
            y,
            width,
            height,
            transform,
            ..
        } => {
            let local = affine_to_transform(transform);
            let combined = parent_transform.pre_concat(local);
            let alpha_factor = alpha.unwrap_or(1.0).clamp(0.0, 1.0) * parent_alpha;

            let decoded = if let Some(blob_idx) = data_blob {
                blobs
                    .and_then(|all| all.get(*blob_idx))
                    .ok_or_else(|| "missing data_blob".to_string())
                    .and_then(|raw| {
                        decode_image_data_to_premul_rgba_raw(
                            shape,
                            dtype,
                            raw,
                            cmap,
                            norm,
                            origin,
                            alpha_factor,
                        )
                    })
            } else if let Some(data_b64) = data.as_ref() {
                decode_image_data_to_premul_rgba(
                    shape,
                    dtype,
                    data_b64,
                    cmap,
                    norm,
                    origin,
                    alpha_factor,
                )
            } else {
                Err("image_data requires data or data_blob".to_string())
            };

            if let Ok((premul_rgba, src_w, src_h)) = decoded {
                let target_w = width.unwrap_or(src_w as f64).max(1.0);
                let target_h = height.unwrap_or(src_h as f64).max(1.0);
                let sx = (target_w as f32 / src_w as f32).max(1e-6);
                let sy = (target_h as f32 / src_h as f32).max(1e-6);

                if let Some(img_pixmap) = Pixmap::from_vec(
                    premul_rgba,
                    tiny_skia::IntSize::from_wh(src_w, src_h).unwrap(),
                ) {
                    let img_transform = combined
                        .pre_concat(Transform::from_translate(*x as f32, *y as f32))
                        .pre_concat(Transform::from_scale(sx, sy));
                    pixmap.draw_pixmap(
                        0,
                        0,
                        img_pixmap.as_ref(),
                        &tiny_skia::PixmapPaint {
                            opacity: 1.0,
                            blend_mode: tiny_skia::BlendMode::SourceOver,
                            quality: interpolation_quality(interpolation),
                        },
                        img_transform,
                        None,
                    );
                }
            }
        }
    }
}

/// Create a `Paint` from an RGBA color array, applying alpha.
fn make_fill_paint(color: &[f64; 4], alpha: f64) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color_rgba8(
        (color[0].clamp(0.0, 1.0) * 255.0).round() as u8,
        (color[1].clamp(0.0, 1.0) * 255.0).round() as u8,
        (color[2].clamp(0.0, 1.0) * 255.0).round() as u8,
        (color[3].clamp(0.0, 1.0) * alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
    );
    paint.anti_alias = true;
    paint
}

/// Build a `tiny_skia::Stroke` from our `StrokeStyle`.
///
/// We scale the stroke width by the current transform's average scale factor
/// so that strokes appear consistent regardless of zoom level.
fn make_stroke(style: &StrokeStyle, _parent_transform: Transform) -> Stroke {
    let mut stroke = Stroke::default();
    stroke.width = style.width as f32;

    stroke.line_cap = match style.line_cap.as_str() {
        "round" => LineCap::Round,
        "square" => LineCap::Square,
        _ => LineCap::Butt,
    };

    stroke.line_join = match style.line_join.as_str() {
        "round" => LineJoin::Round,
        "bevel" => LineJoin::Bevel,
        _ => LineJoin::Miter,
    };

    if !style.dash_array.is_empty() {
        let dashes: Vec<f32> = style.dash_array.iter().map(|&d| d as f32).collect();
        if let Some(dash) = tiny_skia::StrokeDash::new(dashes, style.dash_offset as f32) {
            stroke.dash = Some(dash);
        }
    }

    stroke
}

fn make_marker_stroke(
    style: &StrokeStyle,
    parent_transform: Transform,
    marker_scale: f32,
) -> Stroke {
    let mut stroke = make_stroke(style, parent_transform);
    let scale = marker_scale.abs().max(1e-6);
    stroke.width /= scale;
    stroke
}

trait Point2 {
    fn x_f32(&self) -> f32;
    fn y_f32(&self) -> f32;
}

impl Point2 for [f32; 2] {
    #[inline]
    fn x_f32(&self) -> f32 {
        self[0]
    }
    #[inline]
    fn y_f32(&self) -> f32 {
        self[1]
    }
}

impl Point2 for [f64; 2] {
    #[inline]
    fn x_f32(&self) -> f32 {
        self[0] as f32
    }
    #[inline]
    fn y_f32(&self) -> f32 {
        self[1] as f32
    }
}

fn polyline_path_from_base64(
    points_b64: &str,
    dtype: &str,
    count: usize,
) -> Result<Option<tiny_skia::Path>, String> {
    if count < 2 {
        return Ok(None);
    }
    let raw = base64_decode(points_b64)?;
    polyline_path_from_raw(&raw, dtype, count)
}

fn polyline_path_from_raw(
    raw: &[u8],
    dtype: &str,
    count: usize,
) -> Result<Option<tiny_skia::Path>, String> {
    if count < 2 {
        return Ok(None);
    }
    let normalized = normalized_dtype(dtype);

    let mut pb = tiny_skia::PathBuilder::new();
    let mut started = false;

    match normalized {
        "f32" => {
            let expected = count
                .checked_mul(2)
                .and_then(|n| n.checked_mul(4))
                .ok_or_else(|| "points byte-size overflow".to_string())?;
            if raw.len() != expected {
                return Err(format!(
                    "points byte-size mismatch: expected {}, got {}",
                    expected,
                    raw.len()
                ));
            }
            // Fast path on little-endian targets: reinterpret as f32 pairs
            // and avoid per-point byte copying.
            if cfg!(target_endian = "little") {
                let (head, floats, tail) = unsafe { raw.align_to::<f32>() };
                if head.is_empty() && tail.is_empty() && floats.len() == count * 2 {
                    for pair in floats.chunks_exact(2) {
                        let x = pair[0];
                        let y = pair[1];
                        if !x.is_finite() || !y.is_finite() {
                            started = false;
                            continue;
                        }
                        if !started {
                            pb.move_to(x, y);
                            started = true;
                        } else {
                            pb.line_to(x, y);
                        }
                    }
                    return Ok(pb.finish());
                }
            }
            for chunk in raw.chunks_exact(8) {
                let mut bx = [0u8; 4];
                let mut by = [0u8; 4];
                bx.copy_from_slice(&chunk[0..4]);
                by.copy_from_slice(&chunk[4..8]);
                let x = f32::from_le_bytes(bx);
                let y = f32::from_le_bytes(by);
                if !x.is_finite() || !y.is_finite() {
                    started = false;
                    continue;
                }
                if !started {
                    pb.move_to(x, y);
                    started = true;
                } else {
                    pb.line_to(x, y);
                }
            }
        }
        "f64" => {
            let expected = count
                .checked_mul(2)
                .and_then(|n| n.checked_mul(8))
                .ok_or_else(|| "points byte-size overflow".to_string())?;
            if raw.len() != expected {
                return Err(format!(
                    "points byte-size mismatch: expected {}, got {}",
                    expected,
                    raw.len()
                ));
            }
            // Fast path on little-endian targets: reinterpret as f64 pairs.
            if cfg!(target_endian = "little") {
                let (head, floats, tail) = unsafe { raw.align_to::<f64>() };
                if head.is_empty() && tail.is_empty() && floats.len() == count * 2 {
                    for pair in floats.chunks_exact(2) {
                        let x = pair[0] as f32;
                        let y = pair[1] as f32;
                        if !x.is_finite() || !y.is_finite() {
                            started = false;
                            continue;
                        }
                        if !started {
                            pb.move_to(x, y);
                            started = true;
                        } else {
                            pb.line_to(x, y);
                        }
                    }
                    return Ok(pb.finish());
                }
            }
            for chunk in raw.chunks_exact(16) {
                let mut bx = [0u8; 8];
                let mut by = [0u8; 8];
                bx.copy_from_slice(&chunk[0..8]);
                by.copy_from_slice(&chunk[8..16]);
                let x = f64::from_le_bytes(bx) as f32;
                let y = f64::from_le_bytes(by) as f32;
                if !x.is_finite() || !y.is_finite() {
                    started = false;
                    continue;
                }
                if !started {
                    pb.move_to(x, y);
                    started = true;
                } else {
                    pb.line_to(x, y);
                }
            }
        }
        other => return Err(format!("unsupported points dtype: {}", other)),
    }

    Ok(pb.finish())
}

fn decode_xy_points_raw(raw: &[u8], dtype: &str, count: usize) -> Result<Vec<[f32; 2]>, String> {
    if count == 0 {
        return Ok(Vec::new());
    }

    let mut points = Vec::with_capacity(count);
    match normalized_dtype(dtype) {
        "f32" => {
            let expected = count
                .checked_mul(2)
                .and_then(|n| n.checked_mul(4))
                .ok_or_else(|| "polygons_data points byte-size overflow".to_string())?;
            if raw.len() != expected {
                return Err(format!(
                    "polygons_data points byte-size mismatch: expected {}, got {}",
                    expected,
                    raw.len()
                ));
            }
            if cfg!(target_endian = "little") {
                let (head, floats, tail) = unsafe { raw.align_to::<f32>() };
                if head.is_empty() && tail.is_empty() && floats.len() == count * 2 {
                    for pair in floats.chunks_exact(2) {
                        points.push([pair[0], pair[1]]);
                    }
                    return Ok(points);
                }
            }
            for chunk in raw.chunks_exact(8) {
                let mut bx = [0u8; 4];
                let mut by = [0u8; 4];
                bx.copy_from_slice(&chunk[0..4]);
                by.copy_from_slice(&chunk[4..8]);
                points.push([f32::from_le_bytes(bx), f32::from_le_bytes(by)]);
            }
        }
        "f64" => {
            let expected = count
                .checked_mul(2)
                .and_then(|n| n.checked_mul(8))
                .ok_or_else(|| "polygons_data points byte-size overflow".to_string())?;
            if raw.len() != expected {
                return Err(format!(
                    "polygons_data points byte-size mismatch: expected {}, got {}",
                    expected,
                    raw.len()
                ));
            }
            if cfg!(target_endian = "little") {
                let (head, floats, tail) = unsafe { raw.align_to::<f64>() };
                if head.is_empty() && tail.is_empty() && floats.len() == count * 2 {
                    for pair in floats.chunks_exact(2) {
                        points.push([pair[0] as f32, pair[1] as f32]);
                    }
                    return Ok(points);
                }
            }
            for chunk in raw.chunks_exact(16) {
                let mut bx = [0u8; 8];
                let mut by = [0u8; 8];
                bx.copy_from_slice(&chunk[0..8]);
                by.copy_from_slice(&chunk[8..16]);
                points.push([f64::from_le_bytes(bx) as f32, f64::from_le_bytes(by) as f32]);
            }
        }
        other => return Err(format!("unsupported polygons_data points dtype: {}", other)),
    }
    Ok(points)
}

fn decode_ring_sizes_raw(raw: &[u8], dtype: &str, count: usize) -> Result<Vec<usize>, String> {
    if count == 0 {
        return Ok(Vec::new());
    }

    let mut rings = Vec::with_capacity(count);
    match normalized_index_dtype(dtype) {
        "u32" => {
            let expected = count
                .checked_mul(4)
                .ok_or_else(|| "polygons_data ring_sizes byte-size overflow".to_string())?;
            if raw.len() != expected {
                return Err(format!(
                    "polygons_data ring_sizes byte-size mismatch: expected {}, got {}",
                    expected,
                    raw.len()
                ));
            }
            if cfg!(target_endian = "little") {
                let (head, vals, tail) = unsafe { raw.align_to::<u32>() };
                if head.is_empty() && tail.is_empty() && vals.len() == count {
                    for &v in vals {
                        rings.push(v as usize);
                    }
                    return Ok(rings);
                }
            }
            for chunk in raw.chunks_exact(4) {
                let mut b = [0u8; 4];
                b.copy_from_slice(chunk);
                rings.push(u32::from_le_bytes(b) as usize);
            }
        }
        "u64" => {
            let expected = count
                .checked_mul(8)
                .ok_or_else(|| "polygons_data ring_sizes byte-size overflow".to_string())?;
            if raw.len() != expected {
                return Err(format!(
                    "polygons_data ring_sizes byte-size mismatch: expected {}, got {}",
                    expected,
                    raw.len()
                ));
            }
            if cfg!(target_endian = "little") {
                let (head, vals, tail) = unsafe { raw.align_to::<u64>() };
                if head.is_empty() && tail.is_empty() && vals.len() == count {
                    for &v in vals {
                        rings.push(v as usize);
                    }
                    return Ok(rings);
                }
            }
            for chunk in raw.chunks_exact(8) {
                let mut b = [0u8; 8];
                b.copy_from_slice(chunk);
                rings.push(u64::from_le_bytes(b) as usize);
            }
        }
        other => {
            return Err(format!(
                "unsupported polygons_data ring_sizes dtype: {}",
                other
            ))
        }
    }
    Ok(rings)
}

fn decode_rgba_rows_raw(raw: &[u8], dtype: &str, count: usize) -> Result<Vec<[f64; 4]>, String> {
    if count == 0 {
        return Ok(Vec::new());
    }

    let mut colors = Vec::with_capacity(count);
    match normalized_color_dtype(dtype) {
        "f32" => {
            let expected = count
                .checked_mul(4)
                .and_then(|n| n.checked_mul(4))
                .ok_or_else(|| "polygons_data fill_colors byte-size overflow".to_string())?;
            if raw.len() != expected {
                return Err(format!(
                    "polygons_data fill_colors byte-size mismatch: expected {}, got {}",
                    expected,
                    raw.len()
                ));
            }
            if cfg!(target_endian = "little") {
                let (head, vals, tail) = unsafe { raw.align_to::<f32>() };
                if head.is_empty() && tail.is_empty() && vals.len() == count * 4 {
                    for rgba in vals.chunks_exact(4) {
                        colors.push([
                            rgba[0] as f64,
                            rgba[1] as f64,
                            rgba[2] as f64,
                            rgba[3] as f64,
                        ]);
                    }
                    return Ok(colors);
                }
            }
            for chunk in raw.chunks_exact(16) {
                let mut b0 = [0u8; 4];
                let mut b1 = [0u8; 4];
                let mut b2 = [0u8; 4];
                let mut b3 = [0u8; 4];
                b0.copy_from_slice(&chunk[0..4]);
                b1.copy_from_slice(&chunk[4..8]);
                b2.copy_from_slice(&chunk[8..12]);
                b3.copy_from_slice(&chunk[12..16]);
                colors.push([
                    f32::from_le_bytes(b0) as f64,
                    f32::from_le_bytes(b1) as f64,
                    f32::from_le_bytes(b2) as f64,
                    f32::from_le_bytes(b3) as f64,
                ]);
            }
        }
        "f64" => {
            let expected = count
                .checked_mul(4)
                .and_then(|n| n.checked_mul(8))
                .ok_or_else(|| "polygons_data fill_colors byte-size overflow".to_string())?;
            if raw.len() != expected {
                return Err(format!(
                    "polygons_data fill_colors byte-size mismatch: expected {}, got {}",
                    expected,
                    raw.len()
                ));
            }
            if cfg!(target_endian = "little") {
                let (head, vals, tail) = unsafe { raw.align_to::<f64>() };
                if head.is_empty() && tail.is_empty() && vals.len() == count * 4 {
                    for rgba in vals.chunks_exact(4) {
                        colors.push([rgba[0], rgba[1], rgba[2], rgba[3]]);
                    }
                    return Ok(colors);
                }
            }
            for chunk in raw.chunks_exact(32) {
                let mut b0 = [0u8; 8];
                let mut b1 = [0u8; 8];
                let mut b2 = [0u8; 8];
                let mut b3 = [0u8; 8];
                b0.copy_from_slice(&chunk[0..8]);
                b1.copy_from_slice(&chunk[8..16]);
                b2.copy_from_slice(&chunk[16..24]);
                b3.copy_from_slice(&chunk[24..32]);
                colors.push([
                    f64::from_le_bytes(b0),
                    f64::from_le_bytes(b1),
                    f64::from_le_bytes(b2),
                    f64::from_le_bytes(b3),
                ]);
            }
        }
        other => {
            return Err(format!(
                "unsupported polygons_data fill_colors dtype: {}",
                other
            ))
        }
    }
    Ok(colors)
}

fn render_polygons_data_raw(
    pixmap: &mut Pixmap,
    points_raw: &[u8],
    points_dtype: &str,
    point_count: usize,
    ring_sizes_raw: &[u8],
    ring_sizes_dtype: &str,
    polygon_count: usize,
    fill_colors_raw: Option<&[u8]>,
    fill_colors_dtype: &str,
    fill: &Option<FillStyle>,
    stroke: &Option<StrokeStyle>,
    combined: Transform,
    parent_transform: Transform,
    parent_alpha: f64,
) -> Result<(), String> {
    let points = decode_xy_points_raw(points_raw, points_dtype, point_count)?;
    let ring_sizes = decode_ring_sizes_raw(ring_sizes_raw, ring_sizes_dtype, polygon_count)?;
    let total_ring_points = ring_sizes
        .iter()
        .fold(0usize, |acc, &v| acc.saturating_add(v));
    if total_ring_points != point_count {
        return Err(format!(
            "polygons_data ring_sizes sum mismatch: expected {}, got {}",
            point_count, total_ring_points
        ));
    }

    let fill_colors = if let Some(raw) = fill_colors_raw {
        let decoded = decode_rgba_rows_raw(raw, fill_colors_dtype, polygon_count)?;
        Some(decoded)
    } else {
        None
    };

    let uniform_fill_paint = fill
        .as_ref()
        .map(|fill_style| make_fill_paint(&fill_style.color, parent_alpha));
    let stroke_paint = stroke
        .as_ref()
        .map(|stroke_style| make_fill_paint(&stroke_style.color, parent_alpha));
    let stroke_shape = stroke
        .as_ref()
        .map(|stroke_style| make_stroke(stroke_style, parent_transform));

    let mut cursor = 0usize;
    for (poly_i, &ring_size) in ring_sizes.iter().enumerate() {
        if ring_size < 3 {
            cursor = cursor.saturating_add(ring_size);
            continue;
        }
        let end = cursor.saturating_add(ring_size);
        if end > points.len() {
            return Err("polygons_data vertex range out of bounds".to_string());
        }
        let ring = &points[cursor..end];
        cursor = end;

        let mut pb = tiny_skia::PathBuilder::new();
        let mut started = false;
        for pt in ring {
            let x = pt[0];
            let y = pt[1];
            if !x.is_finite() || !y.is_finite() {
                started = false;
                continue;
            }
            if !started {
                pb.move_to(x, y);
                started = true;
            } else {
                pb.line_to(x, y);
            }
        }
        if !started {
            continue;
        }
        pb.close();
        let Some(path) = pb.finish() else {
            continue;
        };

        if let Some(colors) = fill_colors.as_ref() {
            if let Some(color) = colors.get(poly_i) {
                let paint = make_fill_paint(color, parent_alpha);
                pixmap.fill_path(&path, &paint, FillRule::Winding, combined, None);
            } else if let Some(paint) = uniform_fill_paint.as_ref() {
                pixmap.fill_path(&path, paint, FillRule::Winding, combined, None);
            }
        } else if let Some(paint) = uniform_fill_paint.as_ref() {
            pixmap.fill_path(&path, paint, FillRule::Winding, combined, None);
        }

        if let (Some(paint), Some(stroke_shape)) = (stroke_paint.as_ref(), stroke_shape.as_ref()) {
            let stroke_transform = combined.pre_concat(Transform::from_translate(0.5, 0.5));
            pixmap.stroke_path(&path, paint, stroke_shape, stroke_transform, None);
        }
    }

    Ok(())
}

fn render_markers_positions_base64_interleaved(
    pixmap: &mut Pixmap,
    marker_path: &[crate::scene::PathSegment],
    points_b64: &str,
    dtype: &str,
    count: usize,
    size: f64,
    fill: &Option<crate::scene::FillStyle>,
    stroke: &Option<StrokeStyle>,
    positions_transform: Option<&[f64; 6]>,
    combined: Transform,
    parent_transform: Transform,
    parent_alpha: f64,
) -> Result<(), String> {
    let raw = base64_decode(points_b64)?;
    render_markers_positions_raw_interleaved(
        pixmap,
        marker_path,
        &raw,
        dtype,
        count,
        size,
        fill,
        stroke,
        positions_transform,
        combined,
        parent_transform,
        parent_alpha,
    )
}

fn render_markers_positions_raw_interleaved(
    pixmap: &mut Pixmap,
    marker_path: &[crate::scene::PathSegment],
    raw: &[u8],
    dtype: &str,
    count: usize,
    size: f64,
    fill: &Option<crate::scene::FillStyle>,
    stroke: &Option<StrokeStyle>,
    positions_transform: Option<&[f64; 6]>,
    combined: Transform,
    parent_transform: Transform,
    parent_alpha: f64,
) -> Result<(), String> {
    if count == 0 {
        return Ok(());
    }

    let normalized = normalized_dtype(dtype);
    match normalized {
        "f32" => render_markers_positions_raw_impl(
            pixmap,
            marker_path,
            raw,
            count,
            8,
            decode_point_interleaved_f32,
            size,
            fill,
            stroke,
            position_affine_from_opt(positions_transform),
            combined,
            parent_transform,
            parent_alpha,
        ),
        "f64" => render_markers_positions_raw_impl(
            pixmap,
            marker_path,
            raw,
            count,
            16,
            decode_point_interleaved_f64,
            size,
            fill,
            stroke,
            position_affine_from_opt(positions_transform),
            combined,
            parent_transform,
            parent_alpha,
        ),
        other => Err(format!("unsupported points dtype: {}", other)),
    }
}

#[inline]
fn decode_point_interleaved_f32(chunk: &[u8]) -> (f32, f32) {
    let mut bx = [0u8; 4];
    let mut by = [0u8; 4];
    bx.copy_from_slice(&chunk[0..4]);
    by.copy_from_slice(&chunk[4..8]);
    (f32::from_le_bytes(bx), f32::from_le_bytes(by))
}

#[inline]
fn decode_point_interleaved_f64(chunk: &[u8]) -> (f32, f32) {
    let mut bx = [0u8; 8];
    let mut by = [0u8; 8];
    bx.copy_from_slice(&chunk[0..8]);
    by.copy_from_slice(&chunk[8..16]);
    (f64::from_le_bytes(bx) as f32, f64::from_le_bytes(by) as f32)
}

fn render_markers_positions_raw_impl<F>(
    pixmap: &mut Pixmap,
    marker_path: &[crate::scene::PathSegment],
    raw: &[u8],
    count: usize,
    bytes_per_point: usize,
    decode_point: F,
    size: f64,
    fill: &Option<crate::scene::FillStyle>,
    stroke: &Option<StrokeStyle>,
    positions_affine: Option<PositionAffine>,
    combined: Transform,
    parent_transform: Transform,
    parent_alpha: f64,
) -> Result<(), String>
where
    F: Fn(&[u8]) -> (f32, f32) + Copy + Send + Sync,
{
    if count == 0 {
        return Ok(());
    }

    let expected = count
        .checked_mul(bytes_per_point)
        .ok_or_else(|| "points byte-size overflow".to_string())?;
    if raw.len() != expected {
        return Err(format!(
            "points byte-size mismatch: expected {}, got {}",
            expected,
            raw.len()
        ));
    }

    let Some(path) = segments_to_path(marker_path) else {
        return Ok(());
    };

    let marker_scale = size as f32;
    if marker_scale <= 0.0 {
        return Ok(());
    }

    let scaled_sx = combined.sx * marker_scale;
    let scaled_ky = combined.ky * marker_scale;
    let scaled_kx = combined.kx * marker_scale;
    let scaled_sy = combined.sy * marker_scale;

    let use_parallel = match std::env::var("PLOTIX_MARKERS_PARALLEL") {
        Ok(v) => {
            let t = v.trim().to_ascii_lowercase();
            if t == "0" || t == "false" || t == "off" || t == "no" {
                false
            } else if t == "1" || t == "true" || t == "on" || t == "yes" {
                true
            } else {
                count >= 30_000
            }
        }
        Err(_) => count >= 30_000,
    };
    let dense_tiny_markers = count >= 20_000 && size <= 3.0;
    let dense_markers = dense_tiny_markers && should_disable_dense_marker_aa();
    let skip_dense_stroke = should_skip_dense_marker_stroke(dense_tiny_markers, size, fill, stroke);
    let circle_like_marker = is_circle_like_marker_path(marker_path);

    if should_use_marker_stamp_fastpath()
        && skip_dense_stroke
        && dense_tiny_markers
        && circle_like_marker
    {
        if let Some(fill_style) = fill.as_ref() {
            if try_render_dense_marker_stamp_raw(
                pixmap,
                &path,
                raw,
                bytes_per_point,
                decode_point,
                count,
                fill_style.color,
                parent_alpha,
                combined,
                marker_scale,
            ) {
                return Ok(());
            }
        }
    }

    if should_use_marker_splat_fastpath()
        && skip_dense_stroke
        && dense_tiny_markers
        && circle_like_marker
    {
        if let Some(fill_style) = fill.as_ref() {
            if try_render_dense_circle_splat_raw(
                pixmap,
                raw,
                bytes_per_point,
                decode_point,
                count,
                fill_style.color,
                parent_alpha,
                combined,
                marker_scale,
                positions_affine,
            ) {
                return Ok(());
            }
        }
    }

    if use_parallel && count >= 25_000 && dense_tiny_markers && should_use_marker_stripes(count) {
        if try_render_markers_parallel_stripes_raw(
            pixmap,
            marker_path,
            raw,
            bytes_per_point,
            decode_point,
            count,
            fill,
            stroke,
            parent_alpha,
            combined,
            marker_scale,
            skip_dense_stroke,
            dense_markers,
            scaled_sx,
            scaled_ky,
            scaled_kx,
            scaled_sy,
            positions_affine,
        ) {
            return Ok(());
        }
    }

    if use_parallel && count >= 25_000 {
        let max_threads = thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        let target_chunk = markers_parallel_target_chunk(count);
        let desired_workers = count.div_ceil(target_chunk).max(1);
        let workers = max_threads
            .min(markers_parallel_worker_cap(count))
            .min(desired_workers)
            .min(count.max(1));

        if workers > 1 {
            let width = pixmap.width();
            let height = pixmap.height();
            let layer_bytes = (width as usize)
                .saturating_mul(height as usize)
                .saturating_mul(4);
            let pixel_count = (width as usize).saturating_mul(height as usize);
            let max_extra_bytes =
                markers_parallel_max_extra_mb(count, pixel_count).saturating_mul(1024 * 1024);
            let max_workers_by_mem = if layer_bytes == 0 {
                1
            } else {
                (max_extra_bytes / layer_bytes).max(1)
            };
            let workers = workers.min(max_workers_by_mem.max(1));
            if workers > 1 {
                let chunk_size = count.div_ceil(workers);
                let marker_template = marker_path.to_vec();
                let fill_color = fill.as_ref().map(|s| s.color);
                let stroke_style = if skip_dense_stroke {
                    None
                } else {
                    stroke.as_ref().cloned()
                };
                let worker_stack = markers_parallel_stack_bytes();
                let mut rendered_any = false;

                thread::scope(|scope| {
                    let mut handles = Vec::new();
                    for worker_idx in 0..workers {
                        let point_start = worker_idx.saturating_mul(chunk_size);
                        if point_start >= count {
                            break;
                        }
                        let point_end = ((worker_idx + 1).saturating_mul(chunk_size)).min(count);
                        let byte_start = point_start.saturating_mul(bytes_per_point);
                        let byte_end = point_end.saturating_mul(bytes_per_point);
                        let raw_chunk = &raw[byte_start..byte_end];

                        let marker_template = marker_template.clone();
                        let stroke_style = stroke_style.clone();
                        let spawned = thread::Builder::new()
                            .stack_size(worker_stack)
                            .spawn_scoped(scope, move || -> Option<Pixmap> {
                                let path = segments_to_path(&marker_template)?;
                                let mut layer = Pixmap::new(width, height)?;
                                let mut fill_paint = fill_color
                                    .as_ref()
                                    .map(|c| make_fill_paint(c, parent_alpha));
                                let mut stroke_paint = stroke_style
                                    .as_ref()
                                    .map(|s| make_fill_paint(&s.color, parent_alpha));
                                if dense_markers {
                                    if let Some(p) = fill_paint.as_mut() {
                                        p.anti_alias = false;
                                    }
                                    if let Some(p) = stroke_paint.as_mut() {
                                        p.anti_alias = false;
                                    }
                                }
                                let sk_stroke = stroke_style
                                    .as_ref()
                                    .map(|s| make_marker_stroke(s, parent_transform, marker_scale));

                                for chunk in raw_chunk.chunks_exact(bytes_per_point) {
                                    let (x_raw, y_raw) = decode_point(chunk);
                                    let (x, y) =
                                        apply_position_affine(positions_affine, x_raw, y_raw);
                                    let marker_transform = marker_transform_for_point(
                                        combined, scaled_sx, scaled_ky, scaled_kx, scaled_sy, x, y,
                                    );

                                    if let Some(paint) = fill_paint.as_ref() {
                                        layer.fill_path(
                                            &path,
                                            paint,
                                            FillRule::Winding,
                                            marker_transform,
                                            None,
                                        );
                                    }
                                    if let (Some(paint), Some(stroke_shape)) =
                                        (stroke_paint.as_ref(), sk_stroke.as_ref())
                                    {
                                        let stroke_mt = marker_transform.pre_concat(Transform::from_translate(0.5, 0.5));
                                        layer.stroke_path(
                                            &path,
                                            paint,
                                            stroke_shape,
                                            stroke_mt,
                                            None,
                                        );
                                    }
                                }

                                Some(layer)
                            });
                        if let Ok(handle) = spawned {
                            handles.push(handle);
                        }
                    }

                    for handle in handles {
                        if let Ok(Some(layer)) = handle.join() {
                            rendered_any = true;
                            pixmap.draw_pixmap(
                                0,
                                0,
                                layer.as_ref(),
                                &tiny_skia::PixmapPaint {
                                    opacity: 1.0,
                                    blend_mode: tiny_skia::BlendMode::SourceOver,
                                    quality: tiny_skia::FilterQuality::Bilinear,
                                },
                                Transform::identity(),
                                None,
                            );
                        }
                    }
                });
                if rendered_any {
                    return Ok(());
                }
            }
        }
    }

    let mut fill_paint = fill
        .as_ref()
        .map(|fill_style| make_fill_paint(&fill_style.color, parent_alpha));
    let mut stroke_paint = if skip_dense_stroke {
        None
    } else {
        stroke
            .as_ref()
            .map(|stroke_style| make_fill_paint(&stroke_style.color, parent_alpha))
    };
    if dense_markers {
        if let Some(p) = fill_paint.as_mut() {
            p.anti_alias = false;
        }
        if let Some(p) = stroke_paint.as_mut() {
            p.anti_alias = false;
        }
    }
    let sk_stroke = if skip_dense_stroke {
        None
    } else {
        stroke
            .as_ref()
            .map(|stroke_style| make_marker_stroke(stroke_style, parent_transform, marker_scale))
    };

    for chunk in raw.chunks_exact(bytes_per_point) {
        let (x_raw, y_raw) = decode_point(chunk);
        let (x, y) = apply_position_affine(positions_affine, x_raw, y_raw);
        let marker_transform =
            marker_transform_for_point(combined, scaled_sx, scaled_ky, scaled_kx, scaled_sy, x, y);

        if let Some(paint) = fill_paint.as_ref() {
            pixmap.fill_path(&path, paint, FillRule::Winding, marker_transform, None);
        }

        if let (Some(paint), Some(stroke_shape)) = (stroke_paint.as_ref(), sk_stroke.as_ref()) {
            let stroke_mt = marker_transform.pre_concat(Transform::from_translate(0.5, 0.5));
            pixmap.stroke_path(&path, paint, stroke_shape, stroke_mt, None);
        }
    }

    Ok(())
}

fn render_markers_positions<P: Point2 + Sync>(
    pixmap: &mut Pixmap,
    marker_path: &[crate::scene::PathSegment],
    positions: &[P],
    size: f64,
    fill: &Option<crate::scene::FillStyle>,
    stroke: &Option<StrokeStyle>,
    positions_transform: Option<&[f64; 6]>,
    combined: Transform,
    parent_transform: Transform,
    parent_alpha: f64,
) {
    if positions.is_empty() {
        return;
    }
    let Some(path) = segments_to_path(marker_path) else {
        return;
    };

    let marker_scale = size as f32;
    if marker_scale <= 0.0 {
        return;
    }
    // Precompute scaled linear terms once; only translation depends on each point.
    let scaled_sx = combined.sx * marker_scale;
    let scaled_ky = combined.ky * marker_scale;
    let scaled_kx = combined.kx * marker_scale;
    let scaled_sy = combined.sy * marker_scale;
    let use_parallel = match std::env::var("PLOTIX_MARKERS_PARALLEL") {
        Ok(v) => {
            let t = v.trim().to_ascii_lowercase();
            if t == "0" || t == "false" || t == "off" || t == "no" {
                false
            } else if t == "1" || t == "true" || t == "on" || t == "yes" {
                true
            } else {
                positions.len() >= 30_000
            }
        }
        Err(_) => positions.len() >= 30_000,
    };
    let dense_tiny_markers = positions.len() >= 20_000 && size <= 3.0;
    let dense_markers = dense_tiny_markers && should_disable_dense_marker_aa();
    let skip_dense_stroke = should_skip_dense_marker_stroke(dense_tiny_markers, size, fill, stroke);
    let positions_affine = position_affine_from_opt(positions_transform);

    if use_parallel && positions.len() >= 25_000 {
        let max_threads = thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        let target_chunk = markers_parallel_target_chunk(positions.len());
        let desired_workers = positions.len().div_ceil(target_chunk).max(1);
        let workers = max_threads
            .min(markers_parallel_worker_cap(positions.len()))
            .min(desired_workers)
            .min(positions.len().max(1));

        if workers > 1 {
            let width = pixmap.width();
            let height = pixmap.height();
            // Each worker allocates one full-size layer pixmap. Bound worker
            // count by a memory budget to avoid large RSS spikes on high-DPI scenes.
            let layer_bytes = (width as usize)
                .saturating_mul(height as usize)
                .saturating_mul(4);
            let pixel_count = (width as usize).saturating_mul(height as usize);
            let max_extra_bytes = markers_parallel_max_extra_mb(positions.len(), pixel_count)
                .saturating_mul(1024 * 1024);
            let max_workers_by_mem = if layer_bytes == 0 {
                1
            } else {
                (max_extra_bytes / layer_bytes).max(1)
            };
            let workers = workers.min(max_workers_by_mem.max(1));
            if workers <= 1 {
                // Fall through to sequential rendering below.
            } else {
                let chunk_size = positions.len().div_ceil(workers);
                let marker_template = marker_path.to_vec();
                let fill_color = fill.as_ref().map(|s| s.color);
                let stroke_style = if skip_dense_stroke {
                    None
                } else {
                    stroke.as_ref().cloned()
                };
                let worker_stack = markers_parallel_stack_bytes();
                let mut rendered_any = false;

                thread::scope(|scope| {
                    let mut handles = Vec::new();
                    for chunk in positions.chunks(chunk_size) {
                        let marker_template = marker_template.clone();
                        let stroke_style = stroke_style.clone();
                        let spawned = thread::Builder::new()
                            .stack_size(worker_stack)
                            .spawn_scoped(scope, move || -> Option<Pixmap> {
                                let path = segments_to_path(&marker_template)?;
                                let mut layer = Pixmap::new(width, height)?;
                                let mut fill_paint = fill_color
                                    .as_ref()
                                    .map(|c| make_fill_paint(c, parent_alpha));
                                let mut stroke_paint = stroke_style
                                    .as_ref()
                                    .map(|s| make_fill_paint(&s.color, parent_alpha));
                                if dense_markers {
                                    if let Some(p) = fill_paint.as_mut() {
                                        p.anti_alias = false;
                                    }
                                    if let Some(p) = stroke_paint.as_mut() {
                                        p.anti_alias = false;
                                    }
                                }
                                let sk_stroke = stroke_style
                                    .as_ref()
                                    .map(|s| make_marker_stroke(s, parent_transform, marker_scale));

                                for pos in chunk {
                                    let (x, y) = apply_position_affine(
                                        positions_affine,
                                        pos.x_f32(),
                                        pos.y_f32(),
                                    );
                                    let marker_transform = marker_transform_for_point(
                                        combined, scaled_sx, scaled_ky, scaled_kx, scaled_sy, x, y,
                                    );

                                    if let Some(paint) = fill_paint.as_ref() {
                                        layer.fill_path(
                                            &path,
                                            paint,
                                            FillRule::Winding,
                                            marker_transform,
                                            None,
                                        );
                                    }
                                    if let (Some(paint), Some(stroke_shape)) =
                                        (stroke_paint.as_ref(), sk_stroke.as_ref())
                                    {
                                        let stroke_mt = marker_transform.pre_concat(Transform::from_translate(0.5, 0.5));
                                        layer.stroke_path(
                                            &path,
                                            paint,
                                            stroke_shape,
                                            stroke_mt,
                                            None,
                                        );
                                    }
                                }

                                Some(layer)
                            });
                        if let Ok(handle) = spawned {
                            handles.push(handle);
                        }
                    }

                    for handle in handles {
                        if let Ok(Some(layer)) = handle.join() {
                            rendered_any = true;
                            pixmap.draw_pixmap(
                                0,
                                0,
                                layer.as_ref(),
                                &tiny_skia::PixmapPaint {
                                    opacity: 1.0,
                                    blend_mode: tiny_skia::BlendMode::SourceOver,
                                    quality: tiny_skia::FilterQuality::Bilinear,
                                },
                                Transform::identity(),
                                None,
                            );
                        }
                    }
                });
                if rendered_any {
                    return;
                }
            }
        }
    }

    let mut fill_paint = fill
        .as_ref()
        .map(|fill_style| make_fill_paint(&fill_style.color, parent_alpha));
    let mut stroke_paint = if skip_dense_stroke {
        None
    } else {
        stroke
            .as_ref()
            .map(|stroke_style| make_fill_paint(&stroke_style.color, parent_alpha))
    };
    if dense_markers {
        if let Some(p) = fill_paint.as_mut() {
            p.anti_alias = false;
        }
        if let Some(p) = stroke_paint.as_mut() {
            p.anti_alias = false;
        }
    }
    let sk_stroke = if skip_dense_stroke {
        None
    } else {
        stroke
            .as_ref()
            .map(|stroke_style| make_marker_stroke(stroke_style, parent_transform, marker_scale))
    };

    for pos in positions {
        let (x, y) = apply_position_affine(positions_affine, pos.x_f32(), pos.y_f32());
        let marker_transform =
            marker_transform_for_point(combined, scaled_sx, scaled_ky, scaled_kx, scaled_sy, x, y);

        if let Some(paint) = fill_paint.as_ref() {
            pixmap.fill_path(&path, paint, FillRule::Winding, marker_transform, None);
        }

        if let (Some(paint), Some(stroke_shape)) = (stroke_paint.as_ref(), sk_stroke.as_ref()) {
            let stroke_mt = marker_transform.pre_concat(Transform::from_translate(0.5, 0.5));
            pixmap.stroke_path(&path, paint, stroke_shape, stroke_mt, None);
        }
    }
}

#[derive(Clone, Copy)]
struct PositionAffine {
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    tx: f32,
    ty: f32,
}

#[inline]
fn position_affine_from_opt(transform: Option<&[f64; 6]>) -> Option<PositionAffine> {
    transform.map(|t| PositionAffine {
        a: t[0] as f32,
        b: t[1] as f32,
        c: t[2] as f32,
        d: t[3] as f32,
        tx: t[4] as f32,
        ty: t[5] as f32,
    })
}

#[inline]
fn apply_position_affine(affine: Option<PositionAffine>, x: f32, y: f32) -> (f32, f32) {
    if let Some(t) = affine {
        (
            t.a.mul_add(x, t.c.mul_add(y, t.tx)),
            t.b.mul_add(x, t.d.mul_add(y, t.ty)),
        )
    } else {
        (x, y)
    }
}

#[inline]
fn marker_transform_for_point(
    combined: Transform,
    scaled_sx: f32,
    scaled_ky: f32,
    scaled_kx: f32,
    scaled_sy: f32,
    x: f32,
    y: f32,
) -> Transform {
    Transform {
        sx: scaled_sx,
        ky: scaled_ky,
        kx: scaled_kx,
        sy: scaled_sy,
        tx: combined.sx * x + combined.kx * y + combined.tx,
        ty: combined.ky * x + combined.sy * y + combined.ty,
    }
}

#[allow(clippy::too_many_arguments)]
fn try_render_markers_parallel_stripes_raw<F>(
    pixmap: &mut Pixmap,
    marker_path: &[crate::scene::PathSegment],
    raw: &[u8],
    bytes_per_point: usize,
    decode_point: F,
    count: usize,
    fill: &Option<crate::scene::FillStyle>,
    stroke: &Option<StrokeStyle>,
    parent_alpha: f64,
    combined: Transform,
    marker_scale: f32,
    skip_dense_stroke: bool,
    dense_markers: bool,
    scaled_sx: f32,
    scaled_ky: f32,
    scaled_kx: f32,
    scaled_sy: f32,
    positions_affine: Option<PositionAffine>,
) -> bool
where
    F: Fn(&[u8]) -> (f32, f32) + Copy + Send + Sync,
{
    // This fast path renders fill-only markers.
    // Enable when there is no stroke, or stroke can be safely skipped.
    if fill.is_none() {
        return false;
    }
    if stroke.is_some() && !skip_dense_stroke {
        return false;
    }
    if count < 20_000 {
        return false;
    }

    let max_threads = thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let target_chunk = markers_parallel_target_chunk(count);
    let desired_workers = count.div_ceil(target_chunk).max(1);
    let workers = max_threads
        .min(markers_parallel_worker_cap(count))
        .min(desired_workers)
        .min(count.max(1));
    if workers <= 1 {
        return false;
    }

    let width = pixmap.width();
    let height = pixmap.height();
    if width == 0 || height == 0 {
        return false;
    }

    let stripe_h = ((height as usize).div_ceil(workers)).max(1) as i32;
    let marker_px = marker_scale.abs().max(1e-3);
    let pad = (marker_px * 2.0 + 2.0).ceil() as i32;
    let mut bins: Vec<Vec<u32>> = (0..workers)
        .map(|_| Vec::with_capacity(count.div_ceil(workers)))
        .collect();

    for idx in 0..count {
        let byte_start = idx.saturating_mul(bytes_per_point);
        let byte_end = byte_start.saturating_add(bytes_per_point);
        if byte_end > raw.len() {
            break;
        }
        let (x_raw, y_raw) = decode_point(&raw[byte_start..byte_end]);
        let (x, y) = apply_position_affine(positions_affine, x_raw, y_raw);
        let py = combined.ky * x + combined.sy * y + combined.ty;
        if !py.is_finite() {
            continue;
        }
        let py_i = py.round() as i32;
        if py_i < -pad || py_i >= height as i32 + pad {
            continue;
        }
        let mut stripe = (py_i.max(0) / stripe_h) as usize;
        if stripe >= workers {
            stripe = workers - 1;
        }
        bins[stripe].push(idx as u32);
    }

    let marker_template = marker_path.to_vec();
    let fill_color = fill.as_ref().map(|s| s.color);
    let worker_stack = markers_parallel_stack_bytes();
    let mut rendered_any = false;

    thread::scope(|scope| {
        let mut handles = Vec::new();
        for stripe in 0..workers {
            let top = stripe as i32 * stripe_h;
            let bottom = ((stripe + 1) as i32 * stripe_h).min(height as i32);
            if top >= bottom {
                continue;
            }
            let layer_y0 = (top - pad).max(0);
            let layer_y1 = (bottom + pad).min(height as i32);
            let layer_h = (layer_y1 - layer_y0).max(1) as u32;

            let marker_template = marker_template.clone();
            let fill_color = fill_color;
            let stripe_indices = std::mem::take(&mut bins[stripe]);
            let spawned = thread::Builder::new()
                .stack_size(worker_stack)
                .spawn_scoped(scope, move || -> Option<(i32, Pixmap)> {
                    if stripe_indices.is_empty() {
                        return None;
                    }
                    let path = segments_to_path(&marker_template)?;
                    let mut layer = Pixmap::new(width, layer_h)?;
                    let mut fill_paint = fill_color
                        .as_ref()
                        .map(|c| make_fill_paint(c, parent_alpha))?;
                    if dense_markers {
                        fill_paint.anti_alias = false;
                    }

                    for point_idx in stripe_indices {
                        let byte_start = point_idx as usize * bytes_per_point;
                        let byte_end = byte_start + bytes_per_point;
                        if byte_end > raw.len() {
                            continue;
                        }
                        let (x_raw, y_raw) = decode_point(&raw[byte_start..byte_end]);
                        let (x, y) = apply_position_affine(positions_affine, x_raw, y_raw);
                        let mut marker_transform = marker_transform_for_point(
                            combined, scaled_sx, scaled_ky, scaled_kx, scaled_sy, x, y,
                        );
                        marker_transform.ty -= layer_y0 as f32;
                        layer.fill_path(
                            &path,
                            &fill_paint,
                            FillRule::Winding,
                            marker_transform,
                            None,
                        );
                    }

                    Some((layer_y0, layer))
                });
            if let Ok(handle) = spawned {
                handles.push(handle);
            }
        }

        for handle in handles {
            if let Ok(Some((layer_y0, layer))) = handle.join() {
                rendered_any = true;
                pixmap.draw_pixmap(
                    0,
                    layer_y0,
                    layer.as_ref(),
                    &tiny_skia::PixmapPaint {
                        opacity: 1.0,
                        blend_mode: tiny_skia::BlendMode::SourceOver,
                        quality: tiny_skia::FilterQuality::Nearest,
                    },
                    Transform::identity(),
                    None,
                );
            }
        }
    });

    rendered_any
}

fn is_circle_like_marker_path(marker_path: &[crate::scene::PathSegment]) -> bool {
    if marker_path.len() < 9 {
        return false;
    }
    let Some(last) = marker_path.last() else {
        return false;
    };
    if last.cmd != "Z" {
        return false;
    }

    let mut radii_sum = 0.0f32;
    let mut radii_count = 0usize;
    let mut radii: Vec<f32> = Vec::with_capacity(marker_path.len());

    for seg in marker_path.iter().take(marker_path.len().saturating_sub(1)) {
        if seg.points.len() < 2 {
            return false;
        }
        let x = seg.points[0] as f32;
        let y = seg.points[1] as f32;
        let r = (x * x + y * y).sqrt();
        if !r.is_finite() || r <= 0.0 {
            return false;
        }
        radii_sum += r;
        radii_count += 1;
        radii.push(r);
    }

    if radii_count < 8 {
        return false;
    }
    let mean = radii_sum / radii_count as f32;
    if !mean.is_finite() || mean <= 0.0 {
        return false;
    }

    let mut max_rel_dev = 0.0f32;
    for r in radii {
        let rel_dev = (r - mean).abs() / mean;
        if rel_dev > max_rel_dev {
            max_rel_dev = rel_dev;
        }
    }
    max_rel_dev <= 0.18
}

#[inline]
fn blend_premul_over(dst: &mut [u8], src: [u8; 4]) {
    let sa = src[3] as u16;
    if sa == 0 {
        return;
    }
    if sa == 255 {
        dst[0] = src[0];
        dst[1] = src[1];
        dst[2] = src[2];
        dst[3] = 255;
        return;
    }
    let inv = 255u16.saturating_sub(sa);
    dst[0] = (src[0] as u16 + ((dst[0] as u16 * inv + 127) / 255)) as u8;
    dst[1] = (src[1] as u16 + ((dst[1] as u16 * inv + 127) / 255)) as u8;
    dst[2] = (src[2] as u16 + ((dst[2] as u16 * inv + 127) / 255)) as u8;
    dst[3] = (src[3] as u16 + ((dst[3] as u16 * inv + 127) / 255)) as u8;
}

#[inline]
fn blend_premul_over_scaled(dst: &mut [u8], src: [u8; 4], coverage: f32) {
    if coverage <= 0.0 {
        return;
    }
    if coverage >= 0.999 {
        blend_premul_over(dst, src);
        return;
    }
    let cov = coverage.clamp(0.0, 1.0);
    let scaled = [
        (src[0] as f32 * cov + 0.5) as u8,
        (src[1] as f32 * cov + 0.5) as u8,
        (src[2] as f32 * cov + 0.5) as u8,
        (src[3] as f32 * cov + 0.5) as u8,
    ];
    blend_premul_over(dst, scaled);
}

fn should_use_marker_splat_fastpath() -> bool {
    match std::env::var("PLOTIX_MARKER_SPLAT") {
        Ok(v) => {
            let t = v.trim().to_ascii_lowercase();
            !(t == "0" || t == "false" || t == "off" || t == "no")
        }
        Err(_) => true,
    }
}

fn should_use_marker_splat_parallel() -> bool {
    match std::env::var("PLOTIX_MARKER_SPLAT_PARALLEL") {
        Ok(v) => {
            let t = v.trim().to_ascii_lowercase();
            !(t == "0" || t == "false" || t == "off" || t == "no")
        }
        Err(_) => true,
    }
}

fn marker_splat_parallel_min_points() -> usize {
    std::env::var("PLOTIX_MARKER_SPLAT_PARALLEL_MIN_POINTS")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|n| *n >= 50_000)
        .unwrap_or(80_000)
}

fn should_use_marker_stamp_fastpath() -> bool {
    match std::env::var("PLOTIX_MARKER_STAMP") {
        Ok(v) => {
            let t = v.trim().to_ascii_lowercase();
            !(t == "0" || t == "false" || t == "off" || t == "no")
        }
        Err(_) => false,
    }
}

#[inline]
fn splat_dense_circle_raw_chunk<F>(
    data: &mut [u8],
    width: i32,
    height: i32,
    stride: usize,
    raw: &[u8],
    bytes_per_point: usize,
    decode_point: F,
    positions_affine: Option<PositionAffine>,
    combined: Transform,
    src: [u8; 4],
    r_inner2: f32,
    r_outer2: f32,
    denom: f32,
    r_outer: f32,
) where
    F: Fn(&[u8]) -> (f32, f32) + Copy + Send + Sync,
{
    for chunk in raw.chunks_exact(bytes_per_point) {
        let (x_raw, y_raw) = decode_point(chunk);
        let (x, y) = apply_position_affine(positions_affine, x_raw, y_raw);
        let cx = combined.sx.mul_add(x, combined.kx.mul_add(y, combined.tx));
        let cy = combined.ky.mul_add(x, combined.sy.mul_add(y, combined.ty));
        if !cx.is_finite() || !cy.is_finite() {
            continue;
        }

        let min_x = (cx - r_outer - 1.0).floor() as i32;
        let max_x = (cx + r_outer + 1.0).ceil() as i32;
        let min_y = (cy - r_outer - 1.0).floor() as i32;
        let max_y = (cy + r_outer + 1.0).ceil() as i32;

        let x0 = min_x.max(0);
        let x1 = max_x.min(width - 1);
        let y0 = min_y.max(0);
        let y1 = max_y.min(height - 1);
        if x0 > x1 || y0 > y1 {
            continue;
        }

        for py in y0..=y1 {
            let dy = py as f32 + 0.5 - cy;
            let dy2 = dy * dy;
            let row_off = py as usize * stride;
            for px in x0..=x1 {
                let dx = px as f32 + 0.5 - cx;
                let d2 = dx * dx + dy2;
                if d2 > r_outer2 {
                    continue;
                }
                let cov = if d2 <= r_inner2 {
                    1.0
                } else {
                    (r_outer2 - d2) / denom
                };
                let off = row_off + px as usize * 4;
                blend_premul_over_scaled(&mut data[off..off + 4], src, cov);
            }
        }
    }
}

fn try_render_dense_circle_splat_raw<F>(
    pixmap: &mut Pixmap,
    raw: &[u8],
    bytes_per_point: usize,
    decode_point: F,
    count: usize,
    fill_color: [f64; 4],
    parent_alpha: f64,
    combined: Transform,
    marker_scale: f32,
    positions_affine: Option<PositionAffine>,
) -> bool
where
    F: Fn(&[u8]) -> (f32, f32) + Copy + Send + Sync,
{
    if count < 20_000 {
        return false;
    }
    if marker_scale <= 0.0 {
        return false;
    }
    // Keep this path for near-axis-aligned tiny circles only.
    if combined.kx.abs() > 1e-5 || combined.ky.abs() > 1e-5 {
        return false;
    }

    let sx_total = (combined.sx * marker_scale).abs();
    let sy_total = (combined.sy * marker_scale).abs();
    if !sx_total.is_finite() || !sy_total.is_finite() {
        return false;
    }
    let max_axis = sx_total.max(sy_total).max(1e-6);
    let min_axis = sx_total.min(sy_total).max(1e-6);
    if max_axis / min_axis > 1.35 {
        return false;
    }

    // Marker unit-circle has radius 0.5 before marker_scale.
    let radius = 0.25 * (sx_total + sy_total);
    if !(0.20..=2.6).contains(&radius) {
        return false;
    }

    let src = color_to_premul_rgba8(fill_color, parent_alpha);
    if src[3] == 0 {
        return true;
    }

    let width = pixmap.width() as i32;
    let height = pixmap.height() as i32;
    if width <= 0 || height <= 0 {
        return false;
    }

    let r_inner = radius.max(0.2);
    let r_outer = r_inner + 0.75;
    let r_inner2 = r_inner * r_inner;
    let r_outer2 = r_outer * r_outer;
    let denom = (r_outer2 - r_inner2).max(1e-6);

    if should_use_marker_splat_parallel() && count >= marker_splat_parallel_min_points() {
        let max_threads = thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        let target_chunk = markers_parallel_target_chunk(count);
        let desired_workers = count.div_ceil(target_chunk).max(1);
        let workers = max_threads
            .min(markers_parallel_worker_cap(count))
            .min(desired_workers)
            .min(count.max(1));

        if workers > 1 {
            let width_u = pixmap.width();
            let height_u = pixmap.height();
            let layer_bytes = (width_u as usize)
                .saturating_mul(height_u as usize)
                .saturating_mul(4);
            let pixel_count = (width_u as usize).saturating_mul(height_u as usize);
            let max_extra_bytes =
                markers_parallel_max_extra_mb(count, pixel_count).saturating_mul(1024 * 1024);
            let max_workers_by_mem = if layer_bytes == 0 {
                1
            } else {
                (max_extra_bytes / layer_bytes).max(1)
            };
            let workers = workers.min(max_workers_by_mem.max(1));
            if workers > 1 {
                let chunk_size = count.div_ceil(workers);
                let worker_stack = markers_parallel_stack_bytes();
                let mut rendered_any = false;

                thread::scope(|scope| {
                    let mut handles = Vec::new();
                    for worker_idx in 0..workers {
                        let point_start = worker_idx.saturating_mul(chunk_size);
                        if point_start >= count {
                            break;
                        }
                        let point_end = ((worker_idx + 1).saturating_mul(chunk_size)).min(count);
                        let byte_start = point_start.saturating_mul(bytes_per_point);
                        let byte_end = point_end.saturating_mul(bytes_per_point);
                        let raw_chunk = &raw[byte_start..byte_end];

                        let spawned = thread::Builder::new()
                            .stack_size(worker_stack)
                            .spawn_scoped(scope, move || -> Option<Pixmap> {
                                let mut layer = Pixmap::new(width_u, height_u)?;
                                let stride = width_u as usize * 4;
                                splat_dense_circle_raw_chunk(
                                    layer.data_mut(),
                                    width,
                                    height,
                                    stride,
                                    raw_chunk,
                                    bytes_per_point,
                                    decode_point,
                                    positions_affine,
                                    combined,
                                    src,
                                    r_inner2,
                                    r_outer2,
                                    denom,
                                    r_outer,
                                );
                                Some(layer)
                            });
                        if let Ok(handle) = spawned {
                            handles.push(handle);
                        }
                    }

                    for handle in handles {
                        if let Ok(Some(layer)) = handle.join() {
                            rendered_any = true;
                            pixmap.draw_pixmap(
                                0,
                                0,
                                layer.as_ref(),
                                &tiny_skia::PixmapPaint {
                                    opacity: 1.0,
                                    blend_mode: tiny_skia::BlendMode::SourceOver,
                                    quality: tiny_skia::FilterQuality::Nearest,
                                },
                                Transform::identity(),
                                None,
                            );
                        }
                    }
                });
                if rendered_any {
                    return true;
                }
            }
        }
    }

    let stride = width as usize * 4;
    let data = pixmap.data_mut();
    splat_dense_circle_raw_chunk(
        data,
        width,
        height,
        stride,
        raw,
        bytes_per_point,
        decode_point,
        positions_affine,
        combined,
        src,
        r_inner2,
        r_outer2,
        denom,
        r_outer,
    );

    true
}

fn try_render_dense_marker_stamp_raw<F>(
    pixmap: &mut Pixmap,
    path: &tiny_skia::Path,
    raw: &[u8],
    bytes_per_point: usize,
    decode_point: F,
    count: usize,
    fill_color: [f64; 4],
    parent_alpha: f64,
    combined: Transform,
    marker_scale: f32,
) -> bool
where
    F: Fn(&[u8]) -> (f32, f32) + Copy + Send + Sync,
{
    if count < 80_000 {
        return false;
    }

    // Keep this fast-path conservative: tiny, near-isotropic, no shear markers.
    let scale_x = (combined.sx * marker_scale).abs();
    let scale_y = (combined.sy * marker_scale).abs();
    let marker_px = (scale_x + scale_y) * 0.5;
    if !marker_px.is_finite() || marker_px < 0.75 || marker_px > 4.5 {
        return false;
    }
    if (scale_x - scale_y).abs() > marker_px * 0.25 {
        return false;
    }
    if combined.kx.abs() > 1e-4 || combined.ky.abs() > 1e-4 {
        return false;
    }

    let pad = 1.0f32;
    let stamp_w = (marker_px + 2.0 * pad).ceil().max(1.0) as u32;
    let stamp_h = stamp_w;
    let mut stamp = match Pixmap::new(stamp_w, stamp_h) {
        Some(p) => p,
        None => return false,
    };

    let cx = stamp_w as f32 * 0.5;
    let cy = stamp_h as f32 * 0.5;
    let mut paint = make_fill_paint(&fill_color, parent_alpha);
    paint.anti_alias = false;
    let stamp_transform = Transform {
        sx: marker_px,
        ky: 0.0,
        kx: 0.0,
        sy: marker_px,
        tx: cx,
        ty: cy,
    };
    stamp.fill_path(path, &paint, FillRule::Winding, stamp_transform, None);

    if stamp.data().iter().all(|v| *v == 0) {
        return false;
    }

    let pixmap_paint = tiny_skia::PixmapPaint {
        opacity: 1.0,
        blend_mode: tiny_skia::BlendMode::SourceOver,
        quality: tiny_skia::FilterQuality::Nearest,
    };

    for chunk in raw.chunks_exact(bytes_per_point) {
        let (x, y) = decode_point(chunk);
        let px = combined.sx * x + combined.kx * y + combined.tx;
        let py = combined.ky * x + combined.sy * y + combined.ty;
        let dx = (px - cx).round() as i32;
        let dy = (py - cy).round() as i32;
        pixmap.draw_pixmap(
            dx,
            dy,
            stamp.as_ref(),
            &pixmap_paint,
            Transform::identity(),
            None,
        );
    }

    true
}

fn interpolation_quality(name: &str) -> tiny_skia::FilterQuality {
    match name.to_ascii_lowercase().as_str() {
        "nearest" | "none" => tiny_skia::FilterQuality::Nearest,
        _ => tiny_skia::FilterQuality::Bilinear,
    }
}

fn transform_effective_scale(t: Transform) -> f32 {
    let sx = (t.sx * t.sx + t.ky * t.ky).sqrt();
    let sy = (t.kx * t.kx + t.sy * t.sy).sqrt();
    ((sx + sy) * 0.5).max(1e-6)
}

fn resolve_text_oversample(text_transform: Transform, text_w: f32, text_h: f32) -> f32 {
    let auto_scale = transform_effective_scale(text_transform);
    let max_oversample = std::env::var("PLOTIX_TEXT_OVERSAMPLE_MAX")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
        .map(|v| v.clamp(1.0, 6.0))
        .unwrap_or(2.5);

    let mut oversample = match std::env::var("PLOTIX_TEXT_OVERSAMPLE") {
        Ok(v) => {
            let t = v.trim().to_ascii_lowercase();
            if t.is_empty() || t == "auto" {
                auto_scale
            } else {
                v.parse::<f32>().ok().unwrap_or(auto_scale)
            }
        }
        Err(_) => auto_scale,
    }
    .clamp(1.0, max_oversample);

    // Bound peak raster memory for very long strings / high DPI.
    let max_pixels = std::env::var("PLOTIX_TEXT_MAX_PIXELS")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
        .map(|v| v.max(16_384.0))
        .unwrap_or(2_000_000.0);
    let est_w = text_w.max(1.0) * oversample;
    let est_h = text_h.max(1.0) * oversample;
    let est_pixels = est_w * est_h;
    if est_pixels > max_pixels {
        oversample = (oversample * (max_pixels / est_pixels).sqrt()).clamp(1.0, max_oversample);
    }
    oversample
}

fn should_snap_text_to_pixels(rotation_deg: f64) -> bool {
    let default_enabled = true;
    let enabled = match std::env::var("PLOTIX_TEXT_SNAP_PIXELS") {
        Ok(v) => {
            let t = v.trim().to_ascii_lowercase();
            !(t == "0" || t == "false" || t == "off" || t == "no")
        }
        Err(_) => default_enabled,
    };
    enabled && rotation_deg.abs() <= 1e-6
}

fn should_skip_dense_marker_stroke(
    dense_markers: bool,
    size: f64,
    fill: &Option<crate::scene::FillStyle>,
    stroke: &Option<StrokeStyle>,
) -> bool {
    if !dense_markers || size > 3.0 || fill.is_none() {
        return false;
    }
    let Some(stroke_style) = stroke.as_ref() else {
        // Fill-only dense markers can skip stroke path handling entirely.
        return true;
    };
    let Some(fill_style) = fill.as_ref() else {
        return false;
    };
    if stroke_style.color[3] <= 1e-6 {
        return true;
    }
    if stroke_style.width > 1.0 {
        return false;
    }
    let a = fill_style.color;
    let b = stroke_style.color;
    let same_color = (a[0] - b[0]).abs() <= 1e-6
        && (a[1] - b[1]).abs() <= 1e-6
        && (a[2] - b[2]).abs() <= 1e-6
        && (a[3] - b[3]).abs() <= 1e-6;
    if !same_color {
        return false;
    }
    match std::env::var("PLOTIX_DENSE_MARKER_STROKE") {
        Ok(v) => {
            let t = v.trim().to_ascii_lowercase();
            t == "0" || t == "false" || t == "off" || t == "no"
        }
        Err(_) => true,
    }
}

fn should_disable_dense_marker_aa() -> bool {
    match std::env::var("PLOTIX_DENSE_MARKER_AA") {
        Ok(v) => {
            let t = v.trim().to_ascii_lowercase();
            !(t == "1" || t == "true" || t == "on" || t == "yes")
        }
        Err(_) => true,
    }
}

fn should_use_marker_stripes(point_count: usize) -> bool {
    match std::env::var("PLOTIX_MARKERS_STRIPES") {
        Ok(v) => {
            let t = v.trim().to_ascii_lowercase();
            if t == "0" || t == "false" || t == "off" || t == "no" {
                false
            } else if t == "1" || t == "true" || t == "on" || t == "yes" {
                true
            } else {
                point_count >= 2_000_000
            }
        }
        Err(_) => point_count >= 2_000_000,
    }
}

fn markers_parallel_stack_bytes() -> usize {
    let kb = match std::env::var("PLOTIX_MARKERS_PARALLEL_STACK_KB") {
        Ok(v) => v
            .trim()
            .parse::<usize>()
            .ok()
            .filter(|kb| *kb > 0)
            .unwrap_or(256),
        Err(_) => 256,
    };
    kb.clamp(64, 4096).saturating_mul(1024)
}

fn markers_parallel_target_chunk(point_count: usize) -> usize {
    match std::env::var("PLOTIX_MARKERS_PARALLEL_TARGET_CHUNK") {
        Ok(v) => v
            .trim()
            .parse::<usize>()
            .ok()
            .filter(|n| *n > 0)
            .map(|n| n.clamp(2_000, 200_000))
            .unwrap_or(16_000),
        Err(_) => {
            if point_count >= 1_000_000 {
                20_000
            } else if point_count >= 500_000 {
                18_000
            } else if point_count >= 200_000 {
                16_000
            } else {
                24_000
            }
        }
    }
}

fn markers_parallel_worker_cap(point_count: usize) -> usize {
    match std::env::var("PLOTIX_MARKERS_PARALLEL_MAX_WORKERS") {
        Ok(v) => v
            .trim()
            .parse::<usize>()
            .ok()
            .filter(|n| *n > 0)
            .map(|n| n.clamp(1, 32))
            .unwrap_or(6),
        Err(_) => {
            if point_count >= 1_000_000 {
                6
            } else if point_count >= 300_000 {
                5
            } else if point_count >= 100_000 {
                4
            } else {
                3
            }
        }
    }
}

fn markers_parallel_pixel_cap_mb(pixel_count: usize) -> usize {
    if pixel_count >= 8_000_000 {
        8
    } else if pixel_count >= 4_000_000 {
        12
    } else if pixel_count >= 2_000_000 {
        16
    } else if pixel_count >= 1_000_000 {
        24
    } else {
        48
    }
}

fn markers_parallel_max_extra_mb(point_count: usize, pixel_count: usize) -> usize {
    let base_mb = match std::env::var("PLOTIX_MARKERS_PARALLEL_MAX_EXTRA_MB") {
        Ok(v) => v
            .trim()
            .parse::<usize>()
            .ok()
            .filter(|mb| *mb > 0)
            .unwrap_or(32),
        Err(_) => {
            if point_count >= 1_000_000 {
                32
            } else if point_count >= 400_000 {
                24
            } else if point_count >= 100_000 {
                16
            } else {
                12
            }
        }
    };
    let hard_cap = markers_parallel_pixel_cap_mb(pixel_count);
    base_mb.min(hard_cap).clamp(4, 512)
}

fn normalized_dtype(dtype: &str) -> &'static str {
    let d = dtype.to_ascii_lowercase();
    if d == "f64" || d.contains("float64") || d.contains("f8") {
        "f64"
    } else if d == "f32" || d.contains("float32") || d.contains("f4") {
        "f32"
    } else if d == "u8" || d.contains("uint8") || d.contains("u1") || d.contains("ubyte") {
        "u8"
    } else {
        "unknown"
    }
}

fn normalized_index_dtype(dtype: &str) -> &'static str {
    let d = dtype.to_ascii_lowercase();
    if d == "u64" || d.contains("uint64") || d.contains("u8") {
        "u64"
    } else if d == "u32" || d.contains("uint32") || d.contains("u4") {
        "u32"
    } else {
        "unknown"
    }
}

fn normalized_color_dtype(dtype: &str) -> &'static str {
    let d = dtype.to_ascii_lowercase();
    if d == "f64" || d.contains("float64") || d.contains("f8") {
        "f64"
    } else if d == "f32" || d.contains("float32") || d.contains("f4") {
        "f32"
    } else {
        "unknown"
    }
}

fn color_to_premul_rgba8(color: [f64; 4], alpha_factor: f64) -> [u8; 4] {
    let r = color[0].clamp(0.0, 1.0);
    let g = color[1].clamp(0.0, 1.0);
    let b = color[2].clamp(0.0, 1.0);
    let a = (color[3].clamp(0.0, 1.0) * alpha_factor).clamp(0.0, 1.0);
    let a8 = (a * 255.0).round() as u8;
    let pa = a8 as f64 / 255.0;
    [
        (r * pa * 255.0).round() as u8,
        (g * pa * 255.0).round() as u8,
        (b * pa * 255.0).round() as u8,
        a8,
    ]
}

#[derive(Debug, Clone)]
enum NormSpec {
    Linear {
        vmin: f64,
        vmax: f64,
        clip: bool,
    },
    Log {
        vmin: f64,
        vmax: f64,
        clip: bool,
    },
    Power {
        vmin: f64,
        vmax: f64,
        gamma: f64,
        clip: bool,
    },
    TwoSlope {
        vmin: f64,
        vcenter: f64,
        vmax: f64,
    },
    SymLog {
        vmin: f64,
        vmax: f64,
        linthresh: f64,
        linscale: f64,
        clip: bool,
    },
    Boundary {
        boundaries: Vec<f64>,
        ncolors: usize,
        clip: bool,
    },
}

#[derive(Debug, Clone, Copy)]
struct AutoScale {
    vmin: f64,
    vmax: f64,
    pos_vmin: Option<f64>,
    pos_vmax: Option<f64>,
}

fn autoscale_from_iter<I>(values: I) -> Option<AutoScale>
where
    I: Iterator<Item = f64>,
{
    let mut finite_iter = values.filter(|v| v.is_finite());
    let first = finite_iter.next()?;

    let mut min_v = first;
    let mut max_v = first;
    let mut pos_min = if first > 0.0 { Some(first) } else { None };
    let mut pos_max = if first > 0.0 { Some(first) } else { None };

    for v in finite_iter {
        if v < min_v {
            min_v = v;
        }
        if v > max_v {
            max_v = v;
        }
        if v > 0.0 {
            pos_min = Some(match pos_min {
                Some(cur) => cur.min(v),
                None => v,
            });
            pos_max = Some(match pos_max {
                Some(cur) => cur.max(v),
                None => v,
            });
        }
    }

    Some(AutoScale {
        vmin: min_v,
        vmax: max_v,
        pos_vmin: pos_min,
        pos_vmax: pos_max,
    })
}

#[cfg(target_endian = "little")]
fn as_f64_slice_le(raw: &[u8], count: usize) -> Option<&[f64]> {
    if raw.len() != count.checked_mul(8)? {
        return None;
    }
    // SAFETY: We only return the middle aligned section when there is no
    // prefix/suffix, so the returned slice is fully aligned and covers raw.
    let (prefix, body, suffix) = unsafe { raw.align_to::<f64>() };
    if prefix.is_empty() && suffix.is_empty() && body.len() == count {
        Some(body)
    } else {
        None
    }
}

#[cfg(not(target_endian = "little"))]
fn as_f64_slice_le(_raw: &[u8], _count: usize) -> Option<&[f64]> {
    None
}

#[cfg(target_endian = "little")]
fn as_f32_slice_le(raw: &[u8], count: usize) -> Option<&[f32]> {
    if raw.len() != count.checked_mul(4)? {
        return None;
    }
    // SAFETY: We only return the middle aligned section when there is no
    // prefix/suffix, so the returned slice is fully aligned and covers raw.
    let (prefix, body, suffix) = unsafe { raw.align_to::<f32>() };
    if prefix.is_empty() && suffix.is_empty() && body.len() == count {
        Some(body)
    } else {
        None
    }
}

#[cfg(not(target_endian = "little"))]
fn as_f32_slice_le(_raw: &[u8], _count: usize) -> Option<&[f32]> {
    None
}

enum ScalarImageSource<'a> {
    U8(&'a [u8]),
    F32Slice(&'a [f32]),
    F64Slice(&'a [f64]),
    F32Bytes(&'a [u8]),
    F64Bytes(&'a [u8]),
}

impl<'a> ScalarImageSource<'a> {
    #[inline]
    fn value_at(&self, idx: usize) -> f64 {
        match self {
            ScalarImageSource::U8(raw) => raw[idx] as f64,
            ScalarImageSource::F32Slice(values) => values[idx] as f64,
            ScalarImageSource::F64Slice(values) => values[idx],
            ScalarImageSource::F32Bytes(raw) => {
                let off = idx * 4;
                let mut b = [0u8; 4];
                b.copy_from_slice(&raw[off..off + 4]);
                f32::from_le_bytes(b) as f64
            }
            ScalarImageSource::F64Bytes(raw) => {
                let off = idx * 8;
                let mut b = [0u8; 8];
                b.copy_from_slice(&raw[off..off + 8]);
                f64::from_le_bytes(b)
            }
        }
    }

    fn autoscale(&self, count: usize) -> Option<AutoScale> {
        autoscale_from_iter((0..count).map(|idx| self.value_at(idx)))
    }
}

#[inline]
fn render_scalar_image_from_values<F>(
    out: &mut [u8],
    h: usize,
    w: usize,
    flip_y: bool,
    cmap: &Colormap,
    norm_spec: Option<&NormSpec>,
    alpha_factor: f64,
    value_at: F,
) where
    F: Fn(usize) -> f64 + Sync,
{
    #[inline]
    fn build_colormap_lut(cmap: &Colormap, alpha_factor: f64, levels: usize) -> Vec<[u8; 4]> {
        let n = levels.max(2);
        let mut lut = Vec::with_capacity(n);
        let denom = (n - 1) as f64;
        for i in 0..n {
            let t = i as f64 / denom;
            lut.push(color_to_premul_rgba8(cmap.evaluate(t), alpha_factor));
        }
        lut
    }

    #[inline]
    fn lut_lookup(lut: &[[u8; 4]], t: f64) -> [u8; 4] {
        let idx = (t.clamp(0.0, 1.0) * (lut.len() - 1) as f64).round() as usize;
        lut[idx]
    }

    // Use LUT sampling to avoid per-pixel colormap interpolation overhead.
    // 4096 entries is visually smooth for continuous gradients.
    let lut = build_colormap_lut(cmap, alpha_factor, 4096);
    let pixel_count = h.saturating_mul(w);
    let workers = image_parallel_worker_count(pixel_count, h);
    let row_bytes = w * 4;

    if let Some(NormSpec::Linear { vmin, vmax, clip }) = norm_spec {
        let range = *vmax - *vmin;
        let degenerate = range.abs() <= f64::EPSILON;
        if workers > 1 {
            let rows_per_chunk = h.div_ceil(workers);
            thread::scope(|scope| {
                for (chunk_idx, dst_chunk) in out
                    .chunks_mut(rows_per_chunk.saturating_mul(row_bytes))
                    .enumerate()
                {
                    let start_dst_row = chunk_idx.saturating_mul(rows_per_chunk);
                    let lut_ref = &lut;
                    let value_ref = &value_at;
                    scope.spawn(move || {
                        let rows = dst_chunk.len() / row_bytes;
                        for local_row in 0..rows {
                            let dst_row = start_dst_row + local_row;
                            let src_row = if flip_y { h - 1 - dst_row } else { dst_row };
                            let dst_base = local_row * row_bytes;
                            let src_base = src_row * w;
                            for col in 0..w {
                                let src_idx = src_base + col;
                                let v = value_ref(src_idx);
                                let mut t = if !v.is_finite() || degenerate {
                                    0.0
                                } else {
                                    (v - *vmin) / range
                                };
                                if *clip {
                                    t = t.clamp(0.0, 1.0);
                                }
                                let rgba = lut_lookup(lut_ref, t);
                                let dst_idx = dst_base + col * 4;
                                dst_chunk[dst_idx..dst_idx + 4].copy_from_slice(&rgba);
                            }
                        }
                    });
                }
            });
        } else {
            for row in 0..h {
                let dst_row = if flip_y { h - 1 - row } else { row };
                let dst_base = dst_row * w * 4;
                for col in 0..w {
                    let src_idx = row * w + col;
                    let v = value_at(src_idx);
                    let mut t = if !v.is_finite() || degenerate {
                        0.0
                    } else {
                        (v - *vmin) / range
                    };
                    if *clip {
                        t = t.clamp(0.0, 1.0);
                    }
                    let rgba = lut_lookup(&lut, t);
                    let dst_idx = dst_base + col * 4;
                    out[dst_idx..dst_idx + 4].copy_from_slice(&rgba);
                }
            }
        }
        return;
    }

    if let Some(spec) = norm_spec {
        if workers > 1 {
            let rows_per_chunk = h.div_ceil(workers);
            thread::scope(|scope| {
                for (chunk_idx, dst_chunk) in out
                    .chunks_mut(rows_per_chunk.saturating_mul(row_bytes))
                    .enumerate()
                {
                    let start_dst_row = chunk_idx.saturating_mul(rows_per_chunk);
                    let lut_ref = &lut;
                    let spec_ref = spec;
                    let value_ref = &value_at;
                    scope.spawn(move || {
                        let rows = dst_chunk.len() / row_bytes;
                        for local_row in 0..rows {
                            let dst_row = start_dst_row + local_row;
                            let src_row = if flip_y { h - 1 - dst_row } else { dst_row };
                            let dst_base = local_row * row_bytes;
                            let src_base = src_row * w;
                            for col in 0..w {
                                let src_idx = src_base + col;
                                let t = normalize_value(value_ref(src_idx), spec_ref);
                                let rgba = lut_lookup(lut_ref, t);
                                let dst_idx = dst_base + col * 4;
                                dst_chunk[dst_idx..dst_idx + 4].copy_from_slice(&rgba);
                            }
                        }
                    });
                }
            });
        } else {
            for row in 0..h {
                let dst_row = if flip_y { h - 1 - row } else { row };
                let dst_base = dst_row * w * 4;
                for col in 0..w {
                    let src_idx = row * w + col;
                    let t = normalize_value(value_at(src_idx), spec);
                    let rgba = lut_lookup(&lut, t);
                    let dst_idx = dst_base + col * 4;
                    out[dst_idx..dst_idx + 4].copy_from_slice(&rgba);
                }
            }
        }
        return;
    }

    // No finite values: output the t=0 color for all pixels.
    let rgba = lut[0];
    if workers > 1 {
        let rows_per_chunk = h.div_ceil(workers);
        thread::scope(|scope| {
            for dst_chunk in out.chunks_mut(rows_per_chunk.saturating_mul(row_bytes)) {
                scope.spawn(move || {
                    for px in dst_chunk.chunks_exact_mut(4) {
                        px.copy_from_slice(&rgba);
                    }
                });
            }
        });
    } else {
        for row in 0..h {
            let dst_row = if flip_y { h - 1 - row } else { row };
            let dst_base = dst_row * w * 4;
            for col in 0..w {
                let dst_idx = dst_base + col * 4;
                out[dst_idx..dst_idx + 4].copy_from_slice(&rgba);
            }
        }
    }
}

fn parse_norm_spec(norm: Option<&serde_json::Value>, autoscale: AutoScale) -> NormSpec {
    fn finite_f64(value: Option<&serde_json::Value>) -> Option<f64> {
        value.and_then(|v| v.as_f64()).filter(|v| v.is_finite())
    }

    fn bool_or_default(value: Option<&serde_json::Value>, default: bool) -> bool {
        value.and_then(|v| v.as_bool()).unwrap_or(default)
    }

    fn clamp_linear_range(vmin: f64, vmax: f64) -> (f64, f64) {
        if vmin <= vmax {
            (vmin, vmax)
        } else {
            (vmax, vmin)
        }
    }

    fn parse_boundaries(value: Option<&serde_json::Value>) -> Vec<f64> {
        let mut out = Vec::new();
        if let Some(serde_json::Value::Array(arr)) = value {
            for item in arr {
                if let Some(v) = item.as_f64().filter(|v| v.is_finite()) {
                    out.push(v);
                }
            }
        }
        out.sort_by(f64::total_cmp);
        out.dedup_by(|a, b| (*a - *b).abs() <= f64::EPSILON);
        out
    }

    fn kind_from_string(input: &str) -> Option<&'static str> {
        let s = input.to_ascii_lowercase();
        if s.contains("twoslopenorm") || s.contains("two_slope") || s.contains("twoslope") {
            return Some("two_slope");
        }
        if s.contains("symlognorm") || s.contains("sym_log") || s.contains("symlog") {
            return Some("sym_log");
        }
        if s.contains("boundarynorm") || s.contains("boundary") {
            return Some("boundary");
        }
        if s.contains("powernorm") || s.contains("power") {
            return Some("power");
        }
        if s.contains("lognorm") || s == "log" {
            return Some("log");
        }
        if s.contains("normalize") || s.contains("linear") {
            return Some("normalize");
        }
        None
    }

    let default_linear = || NormSpec::Linear {
        vmin: autoscale.vmin,
        vmax: autoscale.vmax,
        clip: false,
    };

    let (kind, obj) = match norm {
        None => return default_linear(),
        Some(serde_json::Value::Object(map)) => {
            let key = map
                .get("kind")
                .or_else(|| map.get("type"))
                .and_then(|v| v.as_str())
                .unwrap_or("normalize");
            (key.to_ascii_lowercase(), Some(map))
        }
        Some(serde_json::Value::String(s)) => {
            (kind_from_string(s).unwrap_or("normalize").to_string(), None)
        }
        _ => return default_linear(),
    };

    let get_vmin = || finite_f64(obj.and_then(|m| m.get("vmin"))).unwrap_or(autoscale.vmin);
    let get_vmax = || finite_f64(obj.and_then(|m| m.get("vmax"))).unwrap_or(autoscale.vmax);
    let clip = bool_or_default(obj.and_then(|m| m.get("clip")), false);

    match kind.as_str() {
        "normalize" | "linear" => {
            let (vmin, vmax) = clamp_linear_range(get_vmin(), get_vmax());
            NormSpec::Linear { vmin, vmax, clip }
        }
        "log" | "lognorm" => {
            let mut vmin = finite_f64(obj.and_then(|m| m.get("vmin")))
                .or(autoscale.pos_vmin)
                .unwrap_or(1.0);
            let mut vmax = finite_f64(obj.and_then(|m| m.get("vmax")))
                .or(autoscale.pos_vmax)
                .unwrap_or(vmin * 10.0);
            if vmin > vmax {
                std::mem::swap(&mut vmin, &mut vmax);
            }
            if vmin <= 0.0 || vmax <= 0.0 {
                return default_linear();
            }
            NormSpec::Log { vmin, vmax, clip }
        }
        "power" | "powernorm" => {
            let (vmin, vmax) = clamp_linear_range(get_vmin(), get_vmax());
            let gamma = finite_f64(obj.and_then(|m| m.get("gamma"))).unwrap_or(1.0);
            let gamma = if gamma <= 0.0 { 1.0 } else { gamma };
            NormSpec::Power {
                vmin,
                vmax,
                gamma,
                clip,
            }
        }
        "two_slope" | "twoslope" | "twoslopenorm" => {
            let (vmin, vmax) = clamp_linear_range(get_vmin(), get_vmax());
            let mut vcenter =
                finite_f64(obj.and_then(|m| m.get("vcenter"))).unwrap_or((vmin + vmax) * 0.5);
            if vcenter < vmin {
                vcenter = vmin;
            } else if vcenter > vmax {
                vcenter = vmax;
            }
            NormSpec::TwoSlope {
                vmin,
                vcenter,
                vmax,
            }
        }
        "sym_log" | "symlog" | "symlognorm" => {
            let (vmin, vmax) = clamp_linear_range(get_vmin(), get_vmax());
            let linthresh = finite_f64(obj.and_then(|m| m.get("linthresh")))
                .filter(|v| *v > 0.0)
                .unwrap_or(1.0);
            let linscale = finite_f64(obj.and_then(|m| m.get("linscale")))
                .filter(|v| *v > 0.0)
                .unwrap_or(1.0);
            NormSpec::SymLog {
                vmin,
                vmax,
                linthresh,
                linscale,
                clip,
            }
        }
        "boundary" | "boundarynorm" => {
            let boundaries = parse_boundaries(obj.and_then(|m| m.get("boundaries")));
            if boundaries.len() < 2 {
                return default_linear();
            }
            let ncolors = obj
                .and_then(|m| m.get("ncolors"))
                .and_then(|v| v.as_u64())
                .map(|v| v as usize)
                .unwrap_or(boundaries.len().saturating_sub(1).max(1));
            NormSpec::Boundary {
                boundaries,
                ncolors,
                clip,
            }
        }
        _ => default_linear(),
    }
}

fn normalize_value(value: f64, spec: &NormSpec) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }

    let out = match spec {
        NormSpec::Linear { vmin, vmax, clip } => {
            if (*vmax - *vmin).abs() <= f64::EPSILON {
                0.0
            } else {
                let mut t = (value - *vmin) / (*vmax - *vmin);
                if *clip {
                    t = t.clamp(0.0, 1.0);
                }
                t
            }
        }
        NormSpec::Log { vmin, vmax, clip } => {
            if *vmin <= 0.0 || *vmax <= 0.0 || value <= 0.0 {
                0.0
            } else {
                let log_vmin = vmin.log10();
                let log_vmax = vmax.log10();
                if (log_vmax - log_vmin).abs() <= f64::EPSILON {
                    0.0
                } else {
                    let mut t = (value.log10() - log_vmin) / (log_vmax - log_vmin);
                    if *clip {
                        t = t.clamp(0.0, 1.0);
                    }
                    t
                }
            }
        }
        NormSpec::Power {
            vmin,
            vmax,
            gamma,
            clip,
        } => {
            if (*vmax - *vmin).abs() <= f64::EPSILON {
                0.0
            } else {
                // Match matplotlib/plotix behaviour: pre-clip to [0, 1] before power.
                let base = ((value - *vmin) / (*vmax - *vmin)).clamp(0.0, 1.0);
                let mut t = base.powf(*gamma);
                if *clip {
                    t = t.clamp(0.0, 1.0);
                }
                t
            }
        }
        NormSpec::TwoSlope {
            vmin,
            vcenter,
            vmax,
        } => {
            if value <= *vcenter {
                if (*vcenter - *vmin).abs() <= f64::EPSILON {
                    0.0
                } else {
                    0.5 * (value - *vmin) / (*vcenter - *vmin)
                }
            } else if (*vmax - *vcenter).abs() <= f64::EPSILON {
                1.0
            } else {
                0.5 + 0.5 * (value - *vcenter) / (*vmax - *vcenter)
            }
        }
        NormSpec::SymLog {
            vmin,
            vmax,
            linthresh,
            linscale,
            clip,
        } => {
            fn symlog(x: f64, linthresh: f64, linscale: f64) -> f64 {
                if x > linthresh {
                    linscale + (x.log10() - linthresh.log10())
                } else if x < -linthresh {
                    -(linscale + ((-x).log10() - linthresh.log10()))
                } else {
                    x / linthresh * linscale
                }
            }

            let t_min = symlog(*vmin, *linthresh, *linscale);
            let t_max = symlog(*vmax, *linthresh, *linscale);
            if (t_max - t_min).abs() <= f64::EPSILON {
                0.0
            } else {
                let mut t = (symlog(value, *linthresh, *linscale) - t_min) / (t_max - t_min);
                if *clip {
                    t = t.clamp(0.0, 1.0);
                }
                t
            }
        }
        NormSpec::Boundary {
            boundaries,
            ncolors,
            clip,
        } => {
            if boundaries.len() < 2 {
                0.0
            } else {
                let n_intervals = boundaries.len() - 1;
                let mut lo = 0usize;
                let mut hi = boundaries.len();
                while lo < hi {
                    let mid = (lo + hi) / 2;
                    if value < boundaries[mid] {
                        hi = mid;
                    } else {
                        lo = mid + 1;
                    }
                }
                let idx = lo.saturating_sub(1).clamp(0, n_intervals.saturating_sub(1));
                let mut t = if *ncolors > 1 {
                    idx as f64 / (*ncolors as f64 - 1.0)
                } else {
                    0.0
                };
                if *clip {
                    t = t.clamp(0.0, 1.0);
                }
                t
            }
        }
    };

    if out.is_finite() {
        out
    } else {
        0.0
    }
}

pub(crate) fn decode_image_data_to_premul_rgba(
    shape: &[usize],
    dtype: &str,
    data_b64: &str,
    cmap_name: &str,
    norm: &Option<serde_json::Value>,
    origin: &str,
    alpha_factor: f64,
) -> Result<(Vec<u8>, u32, u32), String> {
    let raw = base64_decode(data_b64)?;
    decode_image_data_to_premul_rgba_raw(shape, dtype, &raw, cmap_name, norm, origin, alpha_factor)
}

pub(crate) fn decode_image_data_to_premul_rgba_raw(
    shape: &[usize],
    dtype: &str,
    raw: &[u8],
    cmap_name: &str,
    norm: &Option<serde_json::Value>,
    origin: &str,
    alpha_factor: f64,
) -> Result<(Vec<u8>, u32, u32), String> {
    let dtype = normalized_dtype(dtype);
    let flip_y = origin.eq_ignore_ascii_case("lower");

    if shape.len() == 2 {
        let h = shape[0];
        let w = shape[1];
        if h == 0 || w == 0 {
            return Err("image_data shape has zero dimension".to_string());
        }
        let count = h
            .checked_mul(w)
            .ok_or_else(|| "image_data shape overflow".to_string())?;

        let source = match dtype {
            "f64" => {
                let expected = count
                    .checked_mul(8)
                    .ok_or_else(|| "image_data byte-size overflow".to_string())?;
                if raw.len() != expected {
                    return Err(format!(
                        "image_data byte-size mismatch: expected {}, got {}",
                        expected,
                        raw.len()
                    ));
                }
                match as_f64_slice_le(raw, count) {
                    Some(values) => ScalarImageSource::F64Slice(values),
                    None => ScalarImageSource::F64Bytes(raw),
                }
            }
            "f32" => {
                let expected = count
                    .checked_mul(4)
                    .ok_or_else(|| "image_data byte-size overflow".to_string())?;
                if raw.len() != expected {
                    return Err(format!(
                        "image_data byte-size mismatch: expected {}, got {}",
                        expected,
                        raw.len()
                    ));
                }
                match as_f32_slice_le(raw, count) {
                    Some(values) => ScalarImageSource::F32Slice(values),
                    None => ScalarImageSource::F32Bytes(raw),
                }
            }
            "u8" => {
                if raw.len() != count {
                    return Err(format!(
                        "image_data byte-size mismatch: expected {}, got {}",
                        count,
                        raw.len()
                    ));
                }
                ScalarImageSource::U8(raw)
            }
            _ => {
                return Err(format!(
                    "unsupported image_data dtype for 2D array: {}",
                    dtype
                ));
            }
        };

        let norm_spec = source
            .autoscale(count)
            .map(|a| parse_norm_spec(norm.as_ref(), a));

        let cmap = Colormap::from_name(cmap_name)
            .or_else(|| Colormap::from_name("viridis"))
            .ok_or_else(|| "failed to load built-in colormap".to_string())?;

        let mut out = vec![0u8; count * 4];
        let norm_ref = norm_spec.as_ref();
        match source {
            ScalarImageSource::U8(values) => render_scalar_image_from_values(
                &mut out,
                h,
                w,
                flip_y,
                &cmap,
                norm_ref,
                alpha_factor,
                |idx| values[idx] as f64,
            ),
            ScalarImageSource::F32Slice(values) => render_scalar_image_from_values(
                &mut out,
                h,
                w,
                flip_y,
                &cmap,
                norm_ref,
                alpha_factor,
                |idx| values[idx] as f64,
            ),
            ScalarImageSource::F64Slice(values) => render_scalar_image_from_values(
                &mut out,
                h,
                w,
                flip_y,
                &cmap,
                norm_ref,
                alpha_factor,
                |idx| values[idx],
            ),
            ScalarImageSource::F32Bytes(bytes) => render_scalar_image_from_values(
                &mut out,
                h,
                w,
                flip_y,
                &cmap,
                norm_ref,
                alpha_factor,
                |idx| {
                    let off = idx * 4;
                    let mut b = [0u8; 4];
                    b.copy_from_slice(&bytes[off..off + 4]);
                    f32::from_le_bytes(b) as f64
                },
            ),
            ScalarImageSource::F64Bytes(bytes) => render_scalar_image_from_values(
                &mut out,
                h,
                w,
                flip_y,
                &cmap,
                norm_ref,
                alpha_factor,
                |idx| {
                    let off = idx * 8;
                    let mut b = [0u8; 8];
                    b.copy_from_slice(&bytes[off..off + 8]);
                    f64::from_le_bytes(b)
                },
            ),
        }
        return Ok((out, w as u32, h as u32));
    }

    if shape.len() == 3 && (shape[2] == 3 || shape[2] == 4) {
        let h = shape[0];
        let w = shape[1];
        let c = shape[2];
        if h == 0 || w == 0 {
            return Err("image_data shape has zero dimension".to_string());
        }
        let count = h
            .checked_mul(w)
            .and_then(|n| n.checked_mul(c))
            .ok_or_else(|| "image_data shape overflow".to_string())?;
        let mut out = vec![0u8; h * w * 4];
        let pixel_count = h.saturating_mul(w);
        let workers = image_parallel_worker_count(pixel_count, h);
        let row_bytes = w * 4;

        match dtype {
            "u8" => {
                if raw.len() != count {
                    return Err(format!(
                        "image_data byte-size mismatch: expected {}, got {}",
                        count,
                        raw.len()
                    ));
                }
                if workers > 1 {
                    let rows_per_chunk = h.div_ceil(workers);
                    thread::scope(|scope| {
                        for (chunk_idx, dst_chunk) in out
                            .chunks_mut(rows_per_chunk.saturating_mul(row_bytes))
                            .enumerate()
                        {
                            let start_dst_row = chunk_idx.saturating_mul(rows_per_chunk);
                            let raw_ref = raw;
                            scope.spawn(move || {
                                let rows = dst_chunk.len() / row_bytes;
                                for local_row in 0..rows {
                                    let dst_row = start_dst_row + local_row;
                                    let src_row = if flip_y { h - 1 - dst_row } else { dst_row };
                                    let dst_base = local_row * row_bytes;
                                    let src_row_base = src_row * w * c;
                                    for col in 0..w {
                                        let src_base = src_row_base + col * c;
                                        let rf = raw_ref[src_base] as f64 / 255.0;
                                        let gf = raw_ref[src_base + 1] as f64 / 255.0;
                                        let bf = raw_ref[src_base + 2] as f64 / 255.0;
                                        let af = if c == 4 {
                                            raw_ref[src_base + 3] as f64 / 255.0
                                        } else {
                                            1.0
                                        };
                                        let rgba =
                                            color_to_premul_rgba8([rf, gf, bf, af], alpha_factor);
                                        let dst_idx = dst_base + col * 4;
                                        dst_chunk[dst_idx] = rgba[0];
                                        dst_chunk[dst_idx + 1] = rgba[1];
                                        dst_chunk[dst_idx + 2] = rgba[2];
                                        dst_chunk[dst_idx + 3] = rgba[3];
                                    }
                                }
                            });
                        }
                    });
                } else {
                    for row in 0..h {
                        let dst_row = if flip_y { h - 1 - row } else { row };
                        for col in 0..w {
                            let src_base = (row * w + col) * c;
                            let dst_base = (dst_row * w + col) * 4;
                            let rf = raw[src_base] as f64 / 255.0;
                            let gf = raw[src_base + 1] as f64 / 255.0;
                            let bf = raw[src_base + 2] as f64 / 255.0;
                            let af = if c == 4 {
                                raw[src_base + 3] as f64 / 255.0
                            } else {
                                1.0
                            };
                            let rgba = color_to_premul_rgba8([rf, gf, bf, af], alpha_factor);
                            out[dst_base] = rgba[0];
                            out[dst_base + 1] = rgba[1];
                            out[dst_base + 2] = rgba[2];
                            out[dst_base + 3] = rgba[3];
                        }
                    }
                }
            }
            "f32" | "f64" => {
                let stride = if dtype == "f32" { 4 } else { 8 };
                let expected = count
                    .checked_mul(stride)
                    .ok_or_else(|| "image_data byte-size overflow".to_string())?;
                if raw.len() != expected {
                    return Err(format!(
                        "image_data byte-size mismatch: expected {}, got {}",
                        expected,
                        raw.len()
                    ));
                }
                if dtype == "f32" {
                    let pixel_components = h
                        .checked_mul(w)
                        .and_then(|n| n.checked_mul(c))
                        .ok_or_else(|| "image_data shape overflow".to_string())?;
                    if let Some(values) = as_f32_slice_le(raw, pixel_components) {
                        if workers > 1 {
                            let rows_per_chunk = h.div_ceil(workers);
                            thread::scope(|scope| {
                                for (chunk_idx, dst_chunk) in out
                                    .chunks_mut(rows_per_chunk.saturating_mul(row_bytes))
                                    .enumerate()
                                {
                                    let start_dst_row = chunk_idx.saturating_mul(rows_per_chunk);
                                    scope.spawn(move || {
                                        let rows = dst_chunk.len() / row_bytes;
                                        for local_row in 0..rows {
                                            let dst_row = start_dst_row + local_row;
                                            let src_row =
                                                if flip_y { h - 1 - dst_row } else { dst_row };
                                            let dst_base = local_row * row_bytes;
                                            let src_row_base = src_row * w * c;
                                            for col in 0..w {
                                                let src_base = src_row_base + col * c;
                                                let rf = values[src_base] as f64;
                                                let gf = values[src_base + 1] as f64;
                                                let bf = values[src_base + 2] as f64;
                                                let af = if c == 4 {
                                                    values[src_base + 3] as f64
                                                } else {
                                                    1.0
                                                };
                                                let rgba = color_to_premul_rgba8(
                                                    [
                                                        rf.clamp(0.0, 1.0),
                                                        gf.clamp(0.0, 1.0),
                                                        bf.clamp(0.0, 1.0),
                                                        af.clamp(0.0, 1.0),
                                                    ],
                                                    alpha_factor,
                                                );
                                                let dst_idx = dst_base + col * 4;
                                                dst_chunk[dst_idx] = rgba[0];
                                                dst_chunk[dst_idx + 1] = rgba[1];
                                                dst_chunk[dst_idx + 2] = rgba[2];
                                                dst_chunk[dst_idx + 3] = rgba[3];
                                            }
                                        }
                                    });
                                }
                            });
                        } else {
                            for row in 0..h {
                                let dst_row = if flip_y { h - 1 - row } else { row };
                                for col in 0..w {
                                    let src_base = (row * w + col) * c;
                                    let rf = values[src_base] as f64;
                                    let gf = values[src_base + 1] as f64;
                                    let bf = values[src_base + 2] as f64;
                                    let af = if c == 4 {
                                        values[src_base + 3] as f64
                                    } else {
                                        1.0
                                    };
                                    let dst_base = (dst_row * w + col) * 4;
                                    let rgba = color_to_premul_rgba8(
                                        [
                                            rf.clamp(0.0, 1.0),
                                            gf.clamp(0.0, 1.0),
                                            bf.clamp(0.0, 1.0),
                                            af.clamp(0.0, 1.0),
                                        ],
                                        alpha_factor,
                                    );
                                    out[dst_base] = rgba[0];
                                    out[dst_base + 1] = rgba[1];
                                    out[dst_base + 2] = rgba[2];
                                    out[dst_base + 3] = rgba[3];
                                }
                            }
                        }
                    } else {
                        for row in 0..h {
                            let dst_row = if flip_y { h - 1 - row } else { row };
                            for col in 0..w {
                                let pix = row * w + col;
                                let ch_value = |ch: usize| -> f64 {
                                    let idx = (pix * c + ch) * stride;
                                    let mut b = [0u8; 4];
                                    b.copy_from_slice(&raw[idx..idx + 4]);
                                    f32::from_le_bytes(b) as f64
                                };
                                let rf = ch_value(0).clamp(0.0, 1.0);
                                let gf = ch_value(1).clamp(0.0, 1.0);
                                let bf = ch_value(2).clamp(0.0, 1.0);
                                let af = if c == 4 {
                                    ch_value(3).clamp(0.0, 1.0)
                                } else {
                                    1.0
                                };
                                let dst_base = (dst_row * w + col) * 4;
                                let rgba = color_to_premul_rgba8([rf, gf, bf, af], alpha_factor);
                                out[dst_base] = rgba[0];
                                out[dst_base + 1] = rgba[1];
                                out[dst_base + 2] = rgba[2];
                                out[dst_base + 3] = rgba[3];
                            }
                        }
                    }
                } else if let Some(values) = as_f64_slice_le(raw, count) {
                    if workers > 1 {
                        let rows_per_chunk = h.div_ceil(workers);
                        thread::scope(|scope| {
                            for (chunk_idx, dst_chunk) in out
                                .chunks_mut(rows_per_chunk.saturating_mul(row_bytes))
                                .enumerate()
                            {
                                let start_dst_row = chunk_idx.saturating_mul(rows_per_chunk);
                                scope.spawn(move || {
                                    let rows = dst_chunk.len() / row_bytes;
                                    for local_row in 0..rows {
                                        let dst_row = start_dst_row + local_row;
                                        let src_row =
                                            if flip_y { h - 1 - dst_row } else { dst_row };
                                        let dst_base = local_row * row_bytes;
                                        let src_row_base = src_row * w * c;
                                        for col in 0..w {
                                            let src_base = src_row_base + col * c;
                                            let rf = values[src_base].clamp(0.0, 1.0);
                                            let gf = values[src_base + 1].clamp(0.0, 1.0);
                                            let bf = values[src_base + 2].clamp(0.0, 1.0);
                                            let af = if c == 4 {
                                                values[src_base + 3].clamp(0.0, 1.0)
                                            } else {
                                                1.0
                                            };
                                            let rgba = color_to_premul_rgba8(
                                                [rf, gf, bf, af],
                                                alpha_factor,
                                            );
                                            let dst_idx = dst_base + col * 4;
                                            dst_chunk[dst_idx] = rgba[0];
                                            dst_chunk[dst_idx + 1] = rgba[1];
                                            dst_chunk[dst_idx + 2] = rgba[2];
                                            dst_chunk[dst_idx + 3] = rgba[3];
                                        }
                                    }
                                });
                            }
                        });
                    } else {
                        for row in 0..h {
                            let dst_row = if flip_y { h - 1 - row } else { row };
                            for col in 0..w {
                                let src_base = (row * w + col) * c;
                                let rf = values[src_base].clamp(0.0, 1.0);
                                let gf = values[src_base + 1].clamp(0.0, 1.0);
                                let bf = values[src_base + 2].clamp(0.0, 1.0);
                                let af = if c == 4 {
                                    values[src_base + 3].clamp(0.0, 1.0)
                                } else {
                                    1.0
                                };
                                let dst_base = (dst_row * w + col) * 4;
                                let rgba = color_to_premul_rgba8([rf, gf, bf, af], alpha_factor);
                                out[dst_base] = rgba[0];
                                out[dst_base + 1] = rgba[1];
                                out[dst_base + 2] = rgba[2];
                                out[dst_base + 3] = rgba[3];
                            }
                        }
                    }
                } else {
                    for row in 0..h {
                        let dst_row = if flip_y { h - 1 - row } else { row };
                        for col in 0..w {
                            let pix = row * w + col;
                            let ch_value = |ch: usize| -> f64 {
                                let idx = (pix * c + ch) * stride;
                                let mut b = [0u8; 8];
                                b.copy_from_slice(&raw[idx..idx + 8]);
                                f64::from_le_bytes(b)
                            };
                            let rf = ch_value(0).clamp(0.0, 1.0);
                            let gf = ch_value(1).clamp(0.0, 1.0);
                            let bf = ch_value(2).clamp(0.0, 1.0);
                            let af = if c == 4 {
                                ch_value(3).clamp(0.0, 1.0)
                            } else {
                                1.0
                            };
                            let dst_base = (dst_row * w + col) * 4;
                            let rgba = color_to_premul_rgba8([rf, gf, bf, af], alpha_factor);
                            out[dst_base] = rgba[0];
                            out[dst_base + 1] = rgba[1];
                            out[dst_base + 2] = rgba[2];
                            out[dst_base + 3] = rgba[3];
                        }
                    }
                }
            }
            _ => {
                return Err(format!(
                    "unsupported image_data dtype for RGB/RGBA array: {}",
                    dtype
                ));
            }
        }

        return Ok((out, w as u32, h as u32));
    }

    Err(format!(
        "unsupported image_data shape: {:?} (dtype={})",
        shape, dtype
    ))
}

fn base64_decode(input: &str) -> Result<Vec<u8>, String> {
    #[inline]
    fn decode_char(c: u8) -> Result<u8, String> {
        match c {
            b'A'..=b'Z' => Ok(c - b'A'),
            b'a'..=b'z' => Ok(c - b'a' + 26),
            b'0'..=b'9' => Ok(c - b'0' + 52),
            b'+' => Ok(62),
            b'/' => Ok(63),
            _ => Err(format!("invalid base64 character: {}", c as char)),
        }
    }

    let bytes = input.as_bytes();
    let mut output = Vec::with_capacity(bytes.len() * 3 / 4);
    let mut q = [0u8; 4];
    let mut qlen = 0usize;

    for &b in bytes {
        if b == b'=' {
            break;
        }
        if b == b'\n' || b == b'\r' || b == b' ' || b == b'\t' {
            continue;
        }
        q[qlen] = decode_char(b)?;
        qlen += 1;
        if qlen == 4 {
            output.push((q[0] << 2) | (q[1] >> 4));
            output.push((q[1] << 4) | (q[2] >> 2));
            output.push((q[2] << 6) | q[3]);
            qlen = 0;
        }
    }

    match qlen {
        0 => {}
        2 => {
            output.push((q[0] << 2) | (q[1] >> 4));
        }
        3 => {
            output.push((q[0] << 2) | (q[1] >> 4));
            output.push((q[1] << 4) | (q[2] >> 2));
        }
        _ => return Err("invalid base64 length".to_string()),
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::*;

    #[test]
    fn test_render_empty_scene() {
        let scene = Scene::new(100.0, 100.0, 72.0);
        let png = render_to_png(&scene);
        assert!(!png.is_empty());
        // Check PNG magic bytes.
        assert_eq!(&png[..4], &[0x89, b'P', b'N', b'G']);
    }

    #[test]
    fn test_render_with_path() {
        let scene = Scene {
            width: 200.0,
            height: 200.0,
            dpi: 72.0,
            background: [1.0, 1.0, 1.0, 1.0],
            nodes: vec![SceneNode::Path {
                segments: vec![
                    PathSegment {
                        cmd: "M".to_string(),
                        points: vec![10.0, 10.0],
                    },
                    PathSegment {
                        cmd: "L".to_string(),
                        points: vec![190.0, 10.0],
                    },
                    PathSegment {
                        cmd: "L".to_string(),
                        points: vec![190.0, 190.0],
                    },
                    PathSegment {
                        cmd: "L".to_string(),
                        points: vec![10.0, 190.0],
                    },
                    PathSegment {
                        cmd: "Z".to_string(),
                        points: vec![],
                    },
                ],
                fill: Some(FillStyle {
                    color: [0.2, 0.4, 0.8, 1.0],
                }),
                stroke: Some(StrokeStyle {
                    color: [0.0, 0.0, 0.0, 1.0],
                    width: 2.0,
                    line_cap: String::new(),
                    line_join: String::new(),
                    dash_array: vec![],
                    dash_offset: 0.0,
                }),
                transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            }],
        };
        let png = render_to_png(&scene);
        assert!(!png.is_empty());
        assert_eq!(&png[..4], &[0x89, b'P', b'N', b'G']);
    }

    #[test]
    fn test_base64_decode() {
        let encoded = "SGVsbG8=";
        let decoded = base64_decode(encoded).unwrap();
        assert_eq!(&decoded, b"Hello");
    }

    #[test]
    fn test_render_with_polygons_data_blob() {
        let mut points_raw = Vec::new();
        for &(x, y) in &[
            (20.0f32, 20.0f32),
            (80.0f32, 20.0f32),
            (80.0f32, 80.0f32),
            (20.0f32, 80.0f32),
            (120.0f32, 40.0f32),
            (160.0f32, 40.0f32),
            (140.0f32, 90.0f32),
        ] {
            points_raw.extend_from_slice(&x.to_le_bytes());
            points_raw.extend_from_slice(&y.to_le_bytes());
        }

        let mut rings_raw = Vec::new();
        for &n in &[4u32, 3u32] {
            rings_raw.extend_from_slice(&n.to_le_bytes());
        }

        let mut fill_raw = Vec::new();
        for rgba in &[
            [0.2f32, 0.7f32, 0.3f32, 1.0f32],
            [0.8f32, 0.2f32, 0.4f32, 1.0f32],
        ] {
            for c in rgba {
                fill_raw.extend_from_slice(&c.to_le_bytes());
            }
        }

        let blob_refs: Vec<&[u8]> = vec![&points_raw, &rings_raw, &fill_raw];
        let scene = Scene {
            width: 200.0,
            height: 120.0,
            dpi: 72.0,
            background: [1.0, 1.0, 1.0, 1.0],
            nodes: vec![SceneNode::PolygonsData {
                points_data: None,
                points_blob: Some(0),
                points_dtype: "f32".to_string(),
                point_count: 7,
                ring_sizes_data: None,
                ring_sizes_blob: Some(1),
                ring_sizes_dtype: "u32".to_string(),
                polygon_count: 2,
                fill_colors_data: None,
                fill_colors_blob: Some(2),
                fill_colors_dtype: "f32".to_string(),
                fill: None,
                stroke: Some(StrokeStyle {
                    color: [0.0, 0.0, 0.0, 1.0],
                    width: 1.0,
                    line_cap: String::new(),
                    line_join: String::new(),
                    dash_array: vec![],
                    dash_offset: 0.0,
                }),
                transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            }],
        };
        let png = render_to_png_with_blobs(&scene, &blob_refs);
        assert!(!png.is_empty());
        assert_eq!(&png[..4], &[0x89, b'P', b'N', b'G']);
    }
}

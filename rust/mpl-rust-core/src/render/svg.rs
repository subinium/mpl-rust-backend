use crate::render::rasterizer::{
    decode_image_data_to_premul_rgba, decode_image_data_to_premul_rgba_raw,
};
use crate::scene::{PathSegment, Scene, SceneNode, StrokeStyle};
use std::env;
use std::fmt::Write;
use tiny_skia::Pixmap;

/// Render a scene graph to an SVG string (returned as UTF-8 bytes).
pub fn render_to_svg(scene: &Scene) -> Vec<u8> {
    render_to_svg_inner(scene, None)
}

/// Render a scene graph to SVG using optional raw blobs referenced by scene nodes.
pub fn render_to_svg_with_blobs(scene: &Scene, blobs: &[&[u8]]) -> Vec<u8> {
    render_to_svg_inner(scene, Some(blobs))
}

fn render_to_svg_inner(scene: &Scene, blobs: Option<&[&[u8]]>) -> Vec<u8> {
    let scale = scene.dpi / 72.0;
    let px_width = (scene.width * scale).ceil();
    let px_height = (scene.height * scale).ceil();

    let mut svg = String::with_capacity(4096);

    // SVG header
    write!(
        svg,
        r#"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink"
     width="{px_w}" height="{px_h}" viewBox="0 0 {vw} {vh}">
"#,
        px_w = px_width,
        px_h = px_height,
        vw = scene.width,
        vh = scene.height,
    )
    .unwrap();

    // Background
    let bg = &scene.background;
    write!(
        svg,
        r#"<rect width="{}" height="{}" fill="{}"/>"#,
        scene.width,
        scene.height,
        rgba_to_svg_color(bg),
    )
    .unwrap();
    svg.push('\n');

    // Render all nodes
    let mut id_counter = 0usize;
    for node in &scene.nodes {
        render_svg_node(&mut svg, node, 1, &mut id_counter, blobs);
    }

    svg.push_str("</svg>\n");
    svg.into_bytes()
}

fn render_svg_node(
    svg: &mut String,
    node: &SceneNode,
    indent: usize,
    id_counter: &mut usize,
    blobs: Option<&[&[u8]]>,
) {
    let pad = "  ".repeat(indent);
    match node {
        SceneNode::Group {
            transform,
            alpha,
            clip,
            children,
        } => {
            write!(svg, "{pad}<g").unwrap();
            if !is_identity_transform(transform) {
                write!(
                    svg,
                    r#" transform="matrix({},{},{},{},{},{})""#,
                    transform[0],
                    transform[1],
                    transform[2],
                    transform[3],
                    transform[4],
                    transform[5],
                )
                .unwrap();
            }
            if *alpha < 1.0 {
                write!(svg, r#" opacity="{:.3}""#, alpha).unwrap();
            }
            svg.push_str(">\n");

            if let Some(clip_rect) = clip {
                let clip_id = format!("clip_{}", next_svg_id(id_counter));
                write!(
                    svg,
                    r#"{pad}  <defs><clipPath id="{id}"><rect x="{x}" y="{y}" width="{w}" height="{h}"/></clipPath></defs>
{pad}  <g clip-path="url(#{id})">
"#,
                    id = clip_id,
                    x = clip_rect.x,
                    y = clip_rect.y,
                    w = clip_rect.width,
                    h = clip_rect.height,
                )
                .unwrap();

                for child in children {
                    render_svg_node(svg, child, indent + 2, id_counter, blobs);
                }
                write!(svg, "{pad}  </g>\n").unwrap();
            } else {
                for child in children {
                    render_svg_node(svg, child, indent + 1, id_counter, blobs);
                }
            }

            write!(svg, "{pad}</g>\n").unwrap();
        }

        SceneNode::Path {
            segments,
            fill,
            stroke,
            transform,
        } => {
            let d = segments_to_svg_path(segments);
            if d.is_empty() {
                return;
            }

            write!(svg, r#"{pad}<path d="{d}""#).unwrap();
            if !is_identity_transform(transform) {
                write!(
                    svg,
                    r#" transform="matrix({},{},{},{},{},{})""#,
                    transform[0],
                    transform[1],
                    transform[2],
                    transform[3],
                    transform[4],
                    transform[5],
                )
                .unwrap();
            }

            if let Some(fill_style) = fill {
                write!(svg, r#" fill="{}""#, rgba_to_svg_color(&fill_style.color)).unwrap();
                if fill_style.color[3] < 1.0 {
                    write!(svg, r#" fill-opacity="{:.3}""#, fill_style.color[3]).unwrap();
                }
            } else {
                svg.push_str(r#" fill="none""#);
            }

            if let Some(stroke_style) = stroke {
                write_stroke_attrs(svg, stroke_style);
            }

            svg.push_str("/>\n");
        }

        SceneNode::Text {
            content,
            x,
            y,
            font_size,
            font_family,
            font_weight,
            color,
            rotation,
            ha,
            va,
            ..
        } => {
            if content.is_empty() {
                return;
            }

            let text_anchor = match ha.as_str() {
                "center" => "middle",
                "right" => "end",
                _ => "start",
            };

            let dominant_baseline = match va.as_str() {
                "top" => "hanging",
                "center" => "central",
                "bottom" => "text-bottom",
                _ => "auto",
            };

            write!(svg, r#"{pad}<text x="{x:.2}" y="{y:.2}""#, x = x, y = y).unwrap();
            write!(svg, r#" font-family="{font_family}""#).unwrap();
            write!(svg, r#" font-size="{font_size:.1}""#).unwrap();
            if *font_weight != 400 {
                write!(svg, r#" font-weight="{font_weight}""#).unwrap();
            }
            write!(svg, r#" text-anchor="{text_anchor}""#).unwrap();
            write!(svg, r#" dominant-baseline="{dominant_baseline}""#).unwrap();
            write!(svg, r#" fill="{}""#, rgba_to_svg_color(color)).unwrap();
            if color[3] < 1.0 {
                write!(svg, r#" fill-opacity="{:.3}""#, color[3]).unwrap();
            }
            if rotation.abs() > 1e-6 {
                write!(
                    svg,
                    r#" transform="rotate({:.2},{:.2},{:.2})""#,
                    -rotation, x, y
                )
                .unwrap();
            }
            // Escape XML special characters
            let escaped = xml_escape(content);
            write!(svg, ">{escaped}</text>\n").unwrap();
        }

        SceneNode::Markers {
            path: marker_path,
            positions,
            size,
            fill,
            stroke,
            positions_transform,
            transform,
            ..
        } => {
            if positions.is_empty() {
                return;
            }

            let ms = *size as f64;
            if !ms.is_finite() || ms <= 0.0 {
                return;
            }
            let d = segments_to_svg_path_scaled(marker_path, ms);
            if d.is_empty() {
                return;
            }
            let marker_id = format!("marker_{}", next_svg_id(id_counter));

            write!(svg, "{pad}<g").unwrap();
            if !is_identity_transform(transform) {
                write_svg_transform_attr(svg, transform);
            }
            if let Some(fill_style) = fill {
                write!(svg, r#" fill="{}""#, rgba_to_svg_color(&fill_style.color)).unwrap();
                if fill_style.color[3] < 1.0 {
                    write!(svg, r#" fill-opacity="{:.3}""#, fill_style.color[3]).unwrap();
                }
            } else {
                svg.push_str(r#" fill="none""#);
            }
            if let Some(stroke_style) = stroke {
                write_stroke_attrs(svg, stroke_style);
            }
            svg.push_str(">\n");
            write!(
                svg,
                r#"{pad}  <defs><path id="{id}" d="{d}"/></defs>"#,
                id = marker_id
            )
            .unwrap();
            svg.push('\n');
            let marker_precision = svg_marker_position_precision(positions.len());
            let compact_uses = svg_marker_compact_uses(positions.len());
            let per_use_hint = if compact_uses { 26 } else { 34 };
            svg.reserve(positions.len().saturating_mul(per_use_hint));
            for pos in positions {
                let (px, py) = apply_affine_2d(pos[0], pos[1], positions_transform.as_ref());
                write_marker_use(
                    svg,
                    &pad,
                    &marker_id,
                    px,
                    py,
                    marker_precision,
                    compact_uses,
                );
            }
            if compact_uses {
                svg.push('\n');
            }

            write!(svg, "{pad}</g>\n").unwrap();
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
            ..
        } => {
            let raw_owned = if positions_blob.is_some() {
                None
            } else if let Some(positions_data) = positions_data.as_ref() {
                match base64_decode(positions_data) {
                    Ok(raw) => Some(raw),
                    Err(_) => return,
                }
            } else {
                return;
            };
            let raw: &[u8] = if let Some(blob_idx) = positions_blob {
                let Some(all) = blobs else {
                    return;
                };
                let Some(bytes) = all.get(*blob_idx) else {
                    return;
                };
                bytes
            } else if let Some(ref owned) = raw_owned {
                owned.as_slice()
            } else {
                return;
            };

            let ms = *size as f64;
            if !ms.is_finite() || ms <= 0.0 {
                return;
            }
            let d = segments_to_svg_path_scaled(marker_path, ms);
            if d.is_empty() {
                return;
            }
            let marker_id = format!("marker_{}", next_svg_id(id_counter));

            write!(svg, "{pad}<g").unwrap();
            if !is_identity_transform(transform) {
                write_svg_transform_attr(svg, transform);
            }
            if let Some(fill_style) = fill {
                write!(svg, r#" fill="{}""#, rgba_to_svg_color(&fill_style.color)).unwrap();
                if fill_style.color[3] < 1.0 {
                    write!(svg, r#" fill-opacity="{:.3}""#, fill_style.color[3]).unwrap();
                }
            } else {
                svg.push_str(r#" fill="none""#);
            }
            if let Some(stroke_style) = stroke {
                write_stroke_attrs(svg, stroke_style);
            }
            svg.push_str(">\n");
            write!(
                svg,
                r#"{pad}  <defs><path id="{id}" d="{d}"/></defs>"#,
                id = marker_id
            )
            .unwrap();
            svg.push('\n');
            let marker_precision = svg_marker_position_precision(*count);
            let compact_uses = svg_marker_compact_uses(*count);
            let per_use_hint = if compact_uses { 26 } else { 34 };
            svg.reserve(count.saturating_mul(per_use_hint));
            if write_marker_uses_from_raw(
                svg,
                &pad,
                &marker_id,
                raw,
                positions_dtype,
                *count,
                positions_transform.as_ref(),
                marker_precision,
                compact_uses,
            )
            .is_err()
            {
                write!(svg, "{pad}</g>\n").unwrap();
                return;
            }
            if compact_uses {
                svg.push('\n');
            }

            write!(svg, "{pad}</g>\n").unwrap();
        }

        SceneNode::PolylineData {
            points_data,
            points_blob,
            points_dtype,
            count,
            stroke,
            transform,
        } => {
            let raw_owned = if points_blob.is_some() {
                None
            } else if let Some(points_data) = points_data.as_ref() {
                match base64_decode(points_data) {
                    Ok(raw) => Some(raw),
                    Err(_) => return,
                }
            } else {
                return;
            };
            let raw: &[u8] = if let Some(blob_idx) = points_blob {
                let Some(all) = blobs else {
                    return;
                };
                let Some(bytes) = all.get(*blob_idx) else {
                    return;
                };
                bytes
            } else if let Some(ref owned) = raw_owned {
                owned.as_slice()
            } else {
                return;
            };

            let simplify_max_points = svg_polyline_simplify_max_points(*count);
            let target_count = simplify_max_points.map(|m| m.min(*count)).unwrap_or(*count);
            let mut d = String::with_capacity(target_count.saturating_mul(12));
            let precision = svg_polyline_coord_precision(*count);
            let path_result = if let Some(max_points) = simplify_max_points {
                decode_polyline_points_raw(raw, points_dtype, *count).and_then(|points| {
                    let simplified = simplify_polyline_points_minmax(&points, max_points);
                    write_polyline_path_from_points(&mut d, &simplified, precision)
                })
            } else {
                write_polyline_path_from_raw(&mut d, raw, points_dtype, *count, precision)
            };
            if path_result.is_err() {
                return;
            }
            if d.is_empty() {
                return;
            }

            write!(svg, r#"{pad}<path d="{}" fill="none""#, d.trim_end()).unwrap();
            if !is_identity_transform(transform) {
                write_svg_transform_attr(svg, transform);
            }
            if let Some(stroke_style) = stroke {
                write_stroke_attrs(svg, stroke_style);
            }
            svg.push_str("/>\n");
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
            if *polygon_count == 0 || *point_count == 0 {
                return;
            }

            let points_raw_owned = if points_blob.is_some() {
                None
            } else if let Some(points_data) = points_data.as_ref() {
                match base64_decode(points_data) {
                    Ok(raw) => Some(raw),
                    Err(_) => return,
                }
            } else {
                return;
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
            } else if let Some(ring_sizes_data) = ring_sizes_data.as_ref() {
                match base64_decode(ring_sizes_data) {
                    Ok(raw) => Some(raw),
                    Err(_) => return,
                }
            } else {
                return;
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

            let points = match decode_polyline_points_raw(points_raw, points_dtype, *point_count) {
                Ok(v) => v,
                Err(_) => return,
            };
            let ring_sizes = match decode_polygon_ring_sizes_raw(
                ring_sizes_raw,
                ring_sizes_dtype,
                *polygon_count,
            ) {
                Ok(v) => v,
                Err(_) => return,
            };
            let total_ring_points = ring_sizes
                .iter()
                .fold(0usize, |acc, &v| acc.saturating_add(v));
            if total_ring_points != *point_count {
                return;
            }
            let fill_colors = if let Some(raw) = fill_colors_raw {
                match decode_polygon_fill_colors_raw(raw, fill_colors_dtype, *polygon_count) {
                    Ok(v) => Some(v),
                    Err(_) => return,
                }
            } else {
                None
            };

            let precision = svg_polyline_coord_precision(*point_count);
            let mut cursor = 0usize;
            for (poly_i, &ring_size) in ring_sizes.iter().enumerate() {
                if ring_size < 3 {
                    cursor = cursor.saturating_add(ring_size);
                    continue;
                }
                let end = cursor.saturating_add(ring_size);
                if end > points.len() {
                    return;
                }
                let ring = &points[cursor..end];
                cursor = end;

                let mut d = String::with_capacity(ring_size.saturating_mul(12));
                if write_polygon_path_from_points(&mut d, ring, precision).is_err() || d.is_empty()
                {
                    continue;
                }

                write!(svg, r#"{pad}<path d="{}""#, d.trim_end()).unwrap();
                if !is_identity_transform(transform) {
                    write_svg_transform_attr(svg, transform);
                }

                if let Some(colors) = fill_colors.as_ref() {
                    if let Some(color) = colors.get(poly_i) {
                        write!(svg, r#" fill="{}""#, rgba_to_svg_color(color)).unwrap();
                        if color[3] < 1.0 {
                            write!(svg, r#" fill-opacity="{:.3}""#, color[3]).unwrap();
                        }
                    } else if let Some(fill_style) = fill {
                        write!(svg, r#" fill="{}""#, rgba_to_svg_color(&fill_style.color)).unwrap();
                        if fill_style.color[3] < 1.0 {
                            write!(svg, r#" fill-opacity="{:.3}""#, fill_style.color[3]).unwrap();
                        }
                    } else {
                        svg.push_str(r#" fill="none""#);
                    }
                } else if let Some(fill_style) = fill {
                    write!(svg, r#" fill="{}""#, rgba_to_svg_color(&fill_style.color)).unwrap();
                    if fill_style.color[3] < 1.0 {
                        write!(svg, r#" fill-opacity="{:.3}""#, fill_style.color[3]).unwrap();
                    }
                } else {
                    svg.push_str(r#" fill="none""#);
                }

                if let Some(stroke_style) = stroke {
                    write_stroke_attrs(svg, stroke_style);
                }
                svg.push_str("/>\n");
            }
        }

        SceneNode::Image {
            data,
            x,
            y,
            width,
            height,
            transform,
        } => {
            // Embed as base64 PNG
            write!(
                svg,
                r#"{pad}<image x="{x}" y="{y}" width="{w}" height="{h}" preserveAspectRatio="none" href="data:image/png;base64,{data}""#,
                x = x,
                y = y,
                w = width,
                h = height,
                data = data,
            )
            .unwrap();
            if !is_identity_transform(transform) {
                write_svg_transform_attr(svg, transform);
            }
            svg.push_str("/>");
            svg.push('\n');
        }

        SceneNode::ImageData {
            shape,
            dtype,
            data,
            data_blob,
            cmap,
            origin,
            interpolation,
            norm,
            alpha,
            x,
            y,
            width,
            height,
            transform,
            ..
        } => {
            let alpha_factor = alpha.unwrap_or(1.0).clamp(0.0, 1.0);
            let decoded = if let Some(blob_idx) = data_blob {
                let Some(all) = blobs else {
                    return;
                };
                let Some(raw) = all.get(*blob_idx) else {
                    return;
                };
                decode_image_data_to_premul_rgba_raw(
                    shape,
                    dtype,
                    raw,
                    cmap,
                    norm,
                    origin,
                    alpha_factor,
                )
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
                return;
            };
            let Ok((premul_rgba, src_w, src_h)) = decoded else {
                return;
            };

            let Some(size) = tiny_skia::IntSize::from_wh(src_w, src_h) else {
                return;
            };
            let Some(pixmap) = Pixmap::from_vec(premul_rgba, size) else {
                return;
            };
            let Ok(png_bytes) = pixmap.encode_png() else {
                return;
            };
            let encoded = base64_encode(&png_bytes);
            let img_w = width.unwrap_or(src_w as f64).max(1.0);
            let img_h = height.unwrap_or(src_h as f64).max(1.0);

            write!(
                svg,
                r#"{pad}<image x="{x}" y="{y}" width="{w}" height="{h}" preserveAspectRatio="none" href="data:image/png;base64,{data}""#,
                x = x,
                y = y,
                w = img_w,
                h = img_h,
                data = encoded,
            )
            .unwrap();
            if let Some(rendering) = svg_image_rendering(interpolation) {
                write!(svg, r#" image-rendering="{rendering}""#).unwrap();
            }
            if !is_identity_transform(transform) {
                write_svg_transform_attr(svg, transform);
            }
            svg.push_str("/>");
            svg.push('\n');
        }
    }
}

/// Convert path segments to an SVG path data string.
fn segments_to_svg_path(segments: &[PathSegment]) -> String {
    segments_to_svg_path_with_scale(segments, 1.0)
}

fn segments_to_svg_path_scaled(segments: &[PathSegment], scale: f64) -> String {
    segments_to_svg_path_with_scale(segments, scale)
}

fn segments_to_svg_path_with_scale(segments: &[PathSegment], scale: f64) -> String {
    if !scale.is_finite() || scale <= 0.0 {
        return String::new();
    }
    let mut d = String::new();
    for seg in segments {
        match seg.cmd.as_str() {
            "M" if seg.points.len() >= 2 => {
                write!(
                    d,
                    "M{:.2},{:.2} ",
                    seg.points[0] * scale,
                    seg.points[1] * scale
                )
                .unwrap();
            }
            "L" if seg.points.len() >= 2 => {
                write!(
                    d,
                    "L{:.2},{:.2} ",
                    seg.points[0] * scale,
                    seg.points[1] * scale
                )
                .unwrap();
            }
            "C" if seg.points.len() >= 6 => {
                write!(
                    d,
                    "C{:.2},{:.2} {:.2},{:.2} {:.2},{:.2} ",
                    seg.points[0] * scale,
                    seg.points[1] * scale,
                    seg.points[2] * scale,
                    seg.points[3] * scale,
                    seg.points[4] * scale,
                    seg.points[5] * scale,
                )
                .unwrap();
            }
            "Q" if seg.points.len() >= 4 => {
                write!(
                    d,
                    "Q{:.2},{:.2} {:.2},{:.2} ",
                    seg.points[0] * scale,
                    seg.points[1] * scale,
                    seg.points[2] * scale,
                    seg.points[3] * scale,
                )
                .unwrap();
            }
            "Z" => {
                d.push_str("Z ");
            }
            _ => {}
        }
    }
    d.trim_end().to_string()
}

#[inline]
fn apply_affine_2d(x: f64, y: f64, transform: Option<&[f64; 6]>) -> (f64, f64) {
    if let Some(t) = transform {
        (
            t[0].mul_add(x, t[2].mul_add(y, t[4])),
            t[1].mul_add(x, t[3].mul_add(y, t[5])),
        )
    } else {
        (x, y)
    }
}

const SVG_POW10: [f64; 7] = [1.0, 10.0, 100.0, 1_000.0, 10_000.0, 100_000.0, 1_000_000.0];

#[inline]
fn write_svg_number(svg: &mut String, value: f64, precision: usize) {
    let p = precision.min(6);
    let scale = SVG_POW10[p];
    let mut rounded = if p == 0 {
        value.round()
    } else {
        (value * scale).round() / scale
    };
    if rounded == -0.0 {
        rounded = 0.0;
    }
    let start = svg.len();
    write!(svg, "{:.*}", p, rounded).unwrap();
    if p > 0 {
        while matches!(svg.as_bytes().last(), Some(b'0')) {
            svg.pop();
        }
        if matches!(svg.as_bytes().last(), Some(b'.')) {
            svg.pop();
        }
    }
    if svg.len() == start + 2 && &svg[start..] == "-0" {
        svg.truncate(start);
        svg.push('0');
    }
}

#[inline]
fn write_marker_use(
    svg: &mut String,
    pad: &str,
    marker_id: &str,
    x: f64,
    y: f64,
    precision: usize,
    compact_uses: bool,
) {
    if !compact_uses {
        svg.push_str(pad);
        svg.push_str("  ");
    }
    svg.push_str("<use href='#");
    svg.push_str(marker_id);
    svg.push_str("' x='");
    write_svg_number(svg, x, precision);
    svg.push_str("' y='");
    write_svg_number(svg, y, precision);
    svg.push_str("'/>");
    if !compact_uses {
        svg.push('\n');
    }
}

fn rgba_to_svg_color(color: &[f64; 4]) -> String {
    let r = (color[0] * 255.0).round() as u8;
    let g = (color[1] * 255.0).round() as u8;
    let b = (color[2] * 255.0).round() as u8;
    format!("rgb({r},{g},{b})")
}

#[inline]
fn write_svg_transform_attr(svg: &mut String, transform: &[f64; 6]) {
    write!(
        svg,
        r#" transform="matrix({},{},{},{},{},{})""#,
        transform[0], transform[1], transform[2], transform[3], transform[4], transform[5],
    )
    .unwrap();
}

#[inline]
fn next_svg_id(counter: &mut usize) -> usize {
    let out = *counter;
    *counter = counter.saturating_add(1);
    out
}

fn svg_image_rendering(interpolation: &str) -> Option<&'static str> {
    let key = interpolation.to_ascii_lowercase();
    if key == "nearest" || key == "none" {
        Some("pixelated")
    } else {
        None
    }
}

fn normalized_point_dtype(dtype: &str) -> &'static str {
    let d = dtype.to_ascii_lowercase();
    if d == "f64" || d.contains("float64") || d.contains("f8") {
        "f64"
    } else if d == "f32" || d.contains("float32") || d.contains("f4") {
        "f32"
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

fn write_marker_uses_from_raw(
    svg: &mut String,
    pad: &str,
    marker_id: &str,
    raw: &[u8],
    dtype: &str,
    count: usize,
    positions_transform: Option<&[f64; 6]>,
    precision: usize,
    compact_uses: bool,
) -> Result<(), String> {
    match normalized_point_dtype(dtype) {
        "f32" => {
            let expected = count
                .checked_mul(2)
                .and_then(|n| n.checked_mul(4))
                .ok_or_else(|| "markers_data byte-size overflow".to_string())?;
            if raw.len() != expected {
                return Err(format!(
                    "markers_data byte-size mismatch: expected {}, got {}",
                    expected,
                    raw.len()
                ));
            }
            for chunk in raw.chunks_exact(8) {
                let mut bx = [0u8; 4];
                let mut by = [0u8; 4];
                bx.copy_from_slice(&chunk[0..4]);
                by.copy_from_slice(&chunk[4..8]);
                let (px, py) = apply_affine_2d(
                    f32::from_le_bytes(bx) as f64,
                    f32::from_le_bytes(by) as f64,
                    positions_transform,
                );
                write_marker_use(svg, pad, marker_id, px, py, precision, compact_uses);
            }
        }
        "f64" => {
            let expected = count
                .checked_mul(2)
                .and_then(|n| n.checked_mul(8))
                .ok_or_else(|| "markers_data byte-size overflow".to_string())?;
            if raw.len() != expected {
                return Err(format!(
                    "markers_data byte-size mismatch: expected {}, got {}",
                    expected,
                    raw.len()
                ));
            }
            for chunk in raw.chunks_exact(16) {
                let mut bx = [0u8; 8];
                let mut by = [0u8; 8];
                bx.copy_from_slice(&chunk[0..8]);
                by.copy_from_slice(&chunk[8..16]);
                let (px, py) = apply_affine_2d(
                    f64::from_le_bytes(bx),
                    f64::from_le_bytes(by),
                    positions_transform,
                );
                write_marker_use(svg, pad, marker_id, px, py, precision, compact_uses);
            }
        }
        other => return Err(format!("unsupported markers_data dtype: {}", other)),
    }
    Ok(())
}

fn write_polyline_path_from_raw(
    d: &mut String,
    raw: &[u8],
    dtype: &str,
    count: usize,
    precision: usize,
) -> Result<(), String> {
    if count == 0 {
        return Ok(());
    }
    let mut started = false;
    let mut in_line_cmd = false;
    match normalized_point_dtype(dtype) {
        "f32" => {
            let expected = count
                .checked_mul(2)
                .and_then(|n| n.checked_mul(4))
                .ok_or_else(|| "polyline_data byte-size overflow".to_string())?;
            if raw.len() != expected {
                return Err(format!(
                    "polyline_data byte-size mismatch: expected {}, got {}",
                    expected,
                    raw.len()
                ));
            }
            for chunk in raw.chunks_exact(8) {
                let mut bx = [0u8; 4];
                let mut by = [0u8; 4];
                bx.copy_from_slice(&chunk[0..4]);
                by.copy_from_slice(&chunk[4..8]);
                let x = f32::from_le_bytes(bx) as f64;
                let y = f32::from_le_bytes(by) as f64;
                if !x.is_finite() || !y.is_finite() {
                    started = false;
                    in_line_cmd = false;
                    continue;
                }
                if !started {
                    if !d.is_empty() {
                        d.push(' ');
                    }
                    d.push('M');
                    write_svg_number(d, x, precision);
                    d.push(' ');
                    write_svg_number(d, y, precision);
                    started = true;
                    in_line_cmd = false;
                } else {
                    if !in_line_cmd {
                        d.push(' ');
                        d.push('L');
                        in_line_cmd = true;
                    } else {
                        d.push(' ');
                    }
                    write_svg_number(d, x, precision);
                    d.push(' ');
                    write_svg_number(d, y, precision);
                }
            }
        }
        "f64" => {
            let expected = count
                .checked_mul(2)
                .and_then(|n| n.checked_mul(8))
                .ok_or_else(|| "polyline_data byte-size overflow".to_string())?;
            if raw.len() != expected {
                return Err(format!(
                    "polyline_data byte-size mismatch: expected {}, got {}",
                    expected,
                    raw.len()
                ));
            }
            for chunk in raw.chunks_exact(16) {
                let mut bx = [0u8; 8];
                let mut by = [0u8; 8];
                bx.copy_from_slice(&chunk[0..8]);
                by.copy_from_slice(&chunk[8..16]);
                let x = f64::from_le_bytes(bx);
                let y = f64::from_le_bytes(by);
                if !x.is_finite() || !y.is_finite() {
                    started = false;
                    in_line_cmd = false;
                    continue;
                }
                if !started {
                    if !d.is_empty() {
                        d.push(' ');
                    }
                    d.push('M');
                    write_svg_number(d, x, precision);
                    d.push(' ');
                    write_svg_number(d, y, precision);
                    started = true;
                    in_line_cmd = false;
                } else {
                    if !in_line_cmd {
                        d.push(' ');
                        d.push('L');
                        in_line_cmd = true;
                    } else {
                        d.push(' ');
                    }
                    write_svg_number(d, x, precision);
                    d.push(' ');
                    write_svg_number(d, y, precision);
                }
            }
        }
        other => return Err(format!("unsupported polyline_data dtype: {}", other)),
    }
    Ok(())
}

fn decode_polyline_points_raw(
    raw: &[u8],
    dtype: &str,
    count: usize,
) -> Result<Vec<[f64; 2]>, String> {
    if count == 0 {
        return Ok(Vec::new());
    }
    let mut points = Vec::with_capacity(count);
    match normalized_point_dtype(dtype) {
        "f32" => {
            let expected = count
                .checked_mul(2)
                .and_then(|n| n.checked_mul(4))
                .ok_or_else(|| "polyline_data byte-size overflow".to_string())?;
            if raw.len() != expected {
                return Err(format!(
                    "polyline_data byte-size mismatch: expected {}, got {}",
                    expected,
                    raw.len()
                ));
            }
            for chunk in raw.chunks_exact(8) {
                let mut bx = [0u8; 4];
                let mut by = [0u8; 4];
                bx.copy_from_slice(&chunk[0..4]);
                by.copy_from_slice(&chunk[4..8]);
                points.push([f32::from_le_bytes(bx) as f64, f32::from_le_bytes(by) as f64]);
            }
        }
        "f64" => {
            let expected = count
                .checked_mul(2)
                .and_then(|n| n.checked_mul(8))
                .ok_or_else(|| "polyline_data byte-size overflow".to_string())?;
            if raw.len() != expected {
                return Err(format!(
                    "polyline_data byte-size mismatch: expected {}, got {}",
                    expected,
                    raw.len()
                ));
            }
            for chunk in raw.chunks_exact(16) {
                let mut bx = [0u8; 8];
                let mut by = [0u8; 8];
                bx.copy_from_slice(&chunk[0..8]);
                by.copy_from_slice(&chunk[8..16]);
                points.push([f64::from_le_bytes(bx), f64::from_le_bytes(by)]);
            }
        }
        other => return Err(format!("unsupported polyline_data dtype: {}", other)),
    }
    Ok(points)
}

fn decode_polygon_ring_sizes_raw(
    raw: &[u8],
    dtype: &str,
    count: usize,
) -> Result<Vec<usize>, String> {
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

fn decode_polygon_fill_colors_raw(
    raw: &[u8],
    dtype: &str,
    count: usize,
) -> Result<Vec<[f64; 4]>, String> {
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

fn write_polyline_path_from_points(
    d: &mut String,
    points: &[[f64; 2]],
    precision: usize,
) -> Result<(), String> {
    if points.is_empty() {
        return Ok(());
    }
    if svg_polyline_relative_enabled(points.len()) {
        write_polyline_path_points_rel(
            d,
            points,
            precision,
            svg_polyline_relative_reset_interval(),
        );
        return Ok(());
    }
    write_polyline_path_points_abs(d, points, precision);
    Ok(())
}

fn write_polygon_path_from_points(
    d: &mut String,
    points: &[[f64; 2]],
    precision: usize,
) -> Result<(), String> {
    if points.is_empty() {
        return Ok(());
    }
    let mut started = false;
    for point in points {
        let x = point[0];
        let y = point[1];
        if !x.is_finite() || !y.is_finite() {
            started = false;
            continue;
        }
        if !started {
            d.push('M');
            write_svg_number(d, x, precision);
            d.push(' ');
            write_svg_number(d, y, precision);
            started = true;
        } else {
            d.push(' ');
            d.push('L');
            write_svg_number(d, x, precision);
            d.push(' ');
            write_svg_number(d, y, precision);
        }
    }
    if started {
        d.push(' ');
        d.push('Z');
    }
    Ok(())
}

fn write_polyline_path_points_abs(d: &mut String, points: &[[f64; 2]], precision: usize) {
    let mut started = false;
    let mut in_line_cmd = false;
    for point in points {
        let x = point[0];
        let y = point[1];
        if !x.is_finite() || !y.is_finite() {
            started = false;
            in_line_cmd = false;
            continue;
        }
        if !started {
            if !d.is_empty() {
                d.push(' ');
            }
            d.push('M');
            write_svg_number(d, x, precision);
            d.push(' ');
            write_svg_number(d, y, precision);
            started = true;
            in_line_cmd = false;
        } else {
            if !in_line_cmd {
                d.push(' ');
                d.push('L');
                in_line_cmd = true;
            } else {
                d.push(' ');
            }
            write_svg_number(d, x, precision);
            d.push(' ');
            write_svg_number(d, y, precision);
        }
    }
}

fn write_polyline_path_points_rel(
    d: &mut String,
    points: &[[f64; 2]],
    precision: usize,
    reset_interval: usize,
) {
    let mut started = false;
    let mut in_rel_cmd = false;
    let mut since_abs = 0usize;
    let mut prev_x = 0.0f64;
    let mut prev_y = 0.0f64;

    for point in points {
        let x = point[0];
        let y = point[1];
        if !x.is_finite() || !y.is_finite() {
            started = false;
            in_rel_cmd = false;
            since_abs = 0;
            continue;
        }

        if !started {
            if !d.is_empty() {
                d.push(' ');
            }
            d.push('M');
            write_svg_number(d, x, precision);
            d.push(' ');
            write_svg_number(d, y, precision);
            started = true;
            in_rel_cmd = false;
            since_abs = 0;
            prev_x = x;
            prev_y = y;
            continue;
        }

        if since_abs >= reset_interval {
            d.push(' ');
            d.push('L');
            write_svg_number(d, x, precision);
            d.push(' ');
            write_svg_number(d, y, precision);
            in_rel_cmd = false;
            since_abs = 0;
            prev_x = x;
            prev_y = y;
            continue;
        }

        let dx = x - prev_x;
        let dy = y - prev_y;
        if !in_rel_cmd {
            d.push(' ');
            d.push('l');
            in_rel_cmd = true;
        } else {
            d.push(' ');
        }
        write_svg_number(d, dx, precision);
        d.push(' ');
        write_svg_number(d, dy, precision);
        prev_x = x;
        prev_y = y;
        since_abs = since_abs.saturating_add(1);
    }
}

fn simplify_polyline_points_minmax(points: &[[f64; 2]], max_points: usize) -> Vec<[f64; 2]> {
    let n = points.len();
    if n <= 2 || max_points <= 2 || n <= max_points {
        return points.to_vec();
    }
    let bucket_count = (max_points / 2).max(1);
    let bucket_size = n.div_ceil(bucket_count);
    if bucket_size <= 1 {
        return points.to_vec();
    }

    let mut keep_idx = Vec::with_capacity(bucket_count.saturating_mul(2).saturating_add(2));
    keep_idx.push(0usize);

    let mut start = 0usize;
    while start < n {
        let end = (start + bucket_size).min(n);
        let mut min_idx: Option<usize> = None;
        let mut max_idx: Option<usize> = None;
        let mut min_y = f64::INFINITY;
        let mut max_y = f64::NEG_INFINITY;

        for i in start..end {
            let x = points[i][0];
            let y = points[i][1];
            if !x.is_finite() || !y.is_finite() {
                continue;
            }
            if y < min_y {
                min_y = y;
                min_idx = Some(i);
            }
            if y > max_y {
                max_y = y;
                max_idx = Some(i);
            }
        }

        if let (Some(a), Some(b)) = (min_idx, max_idx) {
            let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
            if keep_idx.last().copied() != Some(lo) {
                keep_idx.push(lo);
            }
            if keep_idx.last().copied() != Some(hi) {
                keep_idx.push(hi);
            }
        }

        start = end;
    }

    if keep_idx.last().copied() != Some(n - 1) {
        keep_idx.push(n - 1);
    }

    keep_idx.sort_unstable();
    keep_idx.dedup();

    if keep_idx.len() > max_points {
        let first = keep_idx[0];
        let last = *keep_idx.last().unwrap_or(&first);
        let interior = keep_idx.len().saturating_sub(2);
        let interior_budget = max_points.saturating_sub(2);
        if interior_budget == 0 {
            keep_idx = vec![first, last];
        } else {
            let step = interior.div_ceil(interior_budget).max(1);
            let mut reduced = Vec::with_capacity(max_points);
            reduced.push(first);
            let mut idx = 1usize;
            while idx + 1 < keep_idx.len() && reduced.len() + 1 < max_points {
                reduced.push(keep_idx[idx]);
                idx += step;
            }
            if reduced.last().copied() != Some(last) {
                reduced.push(last);
            }
            reduced.dedup();
            keep_idx = reduced;
        }
    }

    let mut out = Vec::with_capacity(keep_idx.len());
    for idx in keep_idx {
        out.push(points[idx]);
    }
    out
}

fn svg_polyline_simplify_max_points(count: usize) -> Option<usize> {
    if count <= 2 {
        return None;
    }

    let mode = env::var("PLOTIX_SVG_POLYLINE_SIMPLIFY")
        .unwrap_or_else(|_| "auto".to_string())
        .trim()
        .to_ascii_lowercase();

    let threshold = env::var("PLOTIX_SVG_POLYLINE_SIMPLIFY_AUTO_THRESHOLD")
        .ok()
        .and_then(|s| s.trim().parse::<usize>().ok())
        .map(|v| v.max(10_000))
        .unwrap_or(100_000);

    let max_points = env::var("PLOTIX_SVG_POLYLINE_MAX_POINTS")
        .ok()
        .and_then(|s| s.trim().parse::<usize>().ok())
        .map(|v| v.max(256))
        .unwrap_or(2_000);

    let enabled = match mode.as_str() {
        "1" | "true" | "on" | "yes" => true,
        "0" | "false" | "off" | "no" => false,
        _ => count >= threshold,
    };
    if !enabled || count <= max_points {
        return None;
    }
    Some(max_points)
}

fn svg_polyline_relative_enabled(count: usize) -> bool {
    let mode = env::var("PLOTIX_SVG_POLYLINE_RELATIVE")
        .unwrap_or_else(|_| "auto".to_string())
        .trim()
        .to_ascii_lowercase();
    match mode.as_str() {
        "1" | "true" | "on" | "yes" => true,
        "0" | "false" | "off" | "no" => false,
        _ => {
            let _ = count;
            false
        }
    }
}

fn svg_polyline_relative_reset_interval() -> usize {
    env::var("PLOTIX_SVG_POLYLINE_REL_RESET")
        .ok()
        .and_then(|s| s.trim().parse::<usize>().ok())
        .map(|v| v.clamp(8, 1024))
        .unwrap_or(64)
}

fn svg_marker_position_precision(count: usize) -> usize {
    if let Ok(raw) = env::var("PLOTIX_SVG_MARKER_POS_PRECISION") {
        let value = raw.trim().to_ascii_lowercase();
        if value == "auto" || value.is_empty() {
            return svg_marker_position_precision_auto(count);
        }
        if let Ok(parsed) = value.parse::<usize>() {
            return parsed.min(6);
        }
    }
    svg_marker_position_precision_auto(count)
}

#[inline]
fn svg_marker_position_precision_auto(count: usize) -> usize {
    if count >= 1_000_000 {
        0
    } else if count >= 100_000 {
        1
    } else {
        2
    }
}

fn svg_marker_compact_uses(count: usize) -> bool {
    if let Ok(raw) = env::var("PLOTIX_SVG_MARKER_COMPACT_USES") {
        let value = raw.trim().to_ascii_lowercase();
        return matches!(value.as_str(), "1" | "true" | "on" | "yes" | "auto" | "")
            || (!matches!(value.as_str(), "0" | "false" | "off" | "no") && count >= 50_000);
    }
    count >= 50_000
}

fn svg_polyline_coord_precision(count: usize) -> usize {
    if let Ok(raw) = env::var("PLOTIX_SVG_POLYLINE_PRECISION") {
        let value = raw.trim().to_ascii_lowercase();
        if value == "auto" || value.is_empty() {
            return svg_polyline_coord_precision_auto(count);
        }
        if let Ok(parsed) = value.parse::<usize>() {
            return parsed.min(6);
        }
    }
    svg_polyline_coord_precision_auto(count)
}

#[inline]
fn svg_polyline_coord_precision_auto(count: usize) -> usize {
    if count >= 1_000_000 {
        1
    } else {
        2
    }
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
        2 => output.push((q[0] << 2) | (q[1] >> 4)),
        3 => {
            output.push((q[0] << 2) | (q[1] >> 4));
            output.push((q[1] << 4) | (q[2] >> 2));
        }
        _ => return Err("invalid base64 length".to_string()),
    }

    Ok(output)
}

fn base64_encode(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);

        out.push(TABLE[(b0 >> 2) as usize] as char);
        out.push(TABLE[((b0 & 0b0000_0011) << 4 | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((b1 & 0b0000_1111) << 2 | (b2 >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(b2 & 0b0011_1111) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

fn write_stroke_attrs(svg: &mut String, style: &StrokeStyle) {
    write!(
        svg,
        r#" stroke="{}" stroke-width="{:.2}""#,
        rgba_to_svg_color(&style.color),
        style.width,
    )
    .unwrap();
    if style.color[3] < 1.0 {
        write!(svg, r#" stroke-opacity="{:.3}""#, style.color[3]).unwrap();
    }
    if !style.line_cap.is_empty() {
        write!(svg, r#" stroke-linecap="{}""#, style.line_cap).unwrap();
    }
    if !style.line_join.is_empty() {
        write!(svg, r#" stroke-linejoin="{}""#, style.line_join).unwrap();
    }
    if !style.dash_array.is_empty() {
        let dash_str: Vec<String> = style
            .dash_array
            .iter()
            .map(|d| format!("{:.1}", d))
            .collect();
        write!(svg, r#" stroke-dasharray="{}""#, dash_str.join(",")).unwrap();
        if style.dash_offset != 0.0 {
            write!(svg, r#" stroke-dashoffset="{:.1}""#, style.dash_offset).unwrap();
        }
    }
}

fn is_identity_transform(t: &[f64; 6]) -> bool {
    (t[0] - 1.0).abs() < 1e-9
        && t[1].abs() < 1e-9
        && t[2].abs() < 1e-9
        && (t[3] - 1.0).abs() < 1e-9
        && t[4].abs() < 1e-9
        && t[5].abs() < 1e-9
}

fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::*;

    #[test]
    fn test_render_empty_svg() {
        let scene = Scene::new(100.0, 100.0, 72.0);
        let svg = render_to_svg(&scene);
        let s = String::from_utf8(svg).unwrap();
        assert!(s.contains("<svg"));
        assert!(s.contains("</svg>"));
        assert!(s.contains("width=\"100\""));
    }

    #[test]
    fn test_render_svg_with_path() {
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
        let svg = render_to_svg(&scene);
        let s = String::from_utf8(svg).unwrap();
        assert!(s.contains("<path"));
        assert!(s.contains("fill="));
        assert!(s.contains("stroke="));
    }

    #[test]
    fn test_render_svg_with_text() {
        let scene = Scene {
            width: 200.0,
            height: 100.0,
            dpi: 72.0,
            background: [1.0, 1.0, 1.0, 1.0],
            nodes: vec![SceneNode::Text {
                content: "Hello".to_string(),
                x: 50.0,
                y: 50.0,
                font_size: 12.0,
                font_family: "DejaVu Sans".to_string(),
                font_weight: 400,
                color: [0.0, 0.0, 0.0, 1.0],
                rotation: 0.0,
                ha: "left".to_string(),
                va: "baseline".to_string(),
                transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            }],
        };
        let svg = render_to_svg(&scene);
        let s = String::from_utf8(svg).unwrap();
        assert!(s.contains("<text"));
        assert!(s.contains("Hello"));
    }

    #[test]
    fn test_render_svg_markers_use_defs() {
        let scene = Scene {
            width: 100.0,
            height: 100.0,
            dpi: 72.0,
            background: [1.0, 1.0, 1.0, 1.0],
            nodes: vec![SceneNode::Markers {
                path: vec![
                    PathSegment {
                        cmd: "M".to_string(),
                        points: vec![-0.5, -0.5],
                    },
                    PathSegment {
                        cmd: "L".to_string(),
                        points: vec![0.5, -0.5],
                    },
                    PathSegment {
                        cmd: "L".to_string(),
                        points: vec![0.5, 0.5],
                    },
                    PathSegment {
                        cmd: "L".to_string(),
                        points: vec![-0.5, 0.5],
                    },
                    PathSegment {
                        cmd: "Z".to_string(),
                        points: vec![],
                    },
                ],
                positions: vec![[10.0, 10.0], [20.0, 20.0]],
                size: 3.0,
                fill: Some(FillStyle {
                    color: [0.2, 0.4, 0.8, 1.0],
                }),
                stroke: None,
                positions_transform: None,
                transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            }],
        };
        let svg = render_to_svg(&scene);
        let s = String::from_utf8(svg).unwrap();
        assert!(s.contains("<defs><path id=\"marker_"));
        assert!(s.contains("<use href='#marker_"));
        assert!(!s.contains("xlink:href"));
        assert_eq!(s.matches("<defs><path id=\"marker_").count(), 1);
    }

    #[test]
    fn test_render_svg_markers_data_blob_with_defs_use() {
        let mut raw = Vec::new();
        raw.extend_from_slice(&1.0f32.to_le_bytes());
        raw.extend_from_slice(&2.0f32.to_le_bytes());
        raw.extend_from_slice(&3.0f32.to_le_bytes());
        raw.extend_from_slice(&4.0f32.to_le_bytes());
        let blob_refs: Vec<&[u8]> = vec![raw.as_slice()];
        let scene = Scene {
            width: 100.0,
            height: 100.0,
            dpi: 72.0,
            background: [1.0, 1.0, 1.0, 1.0],
            nodes: vec![SceneNode::MarkersData {
                path: vec![
                    PathSegment {
                        cmd: "M".to_string(),
                        points: vec![-0.5, -0.5],
                    },
                    PathSegment {
                        cmd: "L".to_string(),
                        points: vec![0.5, -0.5],
                    },
                    PathSegment {
                        cmd: "L".to_string(),
                        points: vec![0.5, 0.5],
                    },
                    PathSegment {
                        cmd: "L".to_string(),
                        points: vec![-0.5, 0.5],
                    },
                    PathSegment {
                        cmd: "Z".to_string(),
                        points: vec![],
                    },
                ],
                positions_data: None,
                positions_blob: Some(0),
                positions_dtype: "f32".to_string(),
                count: 2,
                size: 3.0,
                fill: Some(FillStyle {
                    color: [0.2, 0.4, 0.8, 1.0],
                }),
                stroke: None,
                positions_transform: None,
                transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            }],
        };
        let svg = render_to_svg_with_blobs(&scene, &blob_refs);
        let s = String::from_utf8(svg).unwrap();
        assert!(s.contains("<defs><path id=\"marker_"));
        assert!(s.contains("<use href='#marker_"));
        assert!(!s.contains("xlink:href"));
    }

    #[test]
    fn test_render_svg_polyline_data_emits_transform_attr() {
        let mut raw = Vec::new();
        for &(x, y) in &[(0.0f32, 0.0f32), (1.0f32, 2.0f32), (2.0f32, 3.0f32)] {
            raw.extend_from_slice(&x.to_le_bytes());
            raw.extend_from_slice(&y.to_le_bytes());
        }
        let scene = Scene {
            width: 100.0,
            height: 100.0,
            dpi: 72.0,
            background: [1.0, 1.0, 1.0, 1.0],
            nodes: vec![SceneNode::PolylineData {
                points_data: Some(base64_encode(&raw)),
                points_blob: None,
                points_dtype: "f32".to_string(),
                count: 3,
                stroke: Some(StrokeStyle {
                    color: [0.1, 0.2, 0.3, 1.0],
                    width: 1.2,
                    line_cap: String::new(),
                    line_join: String::new(),
                    dash_array: vec![],
                    dash_offset: 0.0,
                }),
                transform: [2.0, 0.0, 0.0, 2.0, 1.0, 1.0],
            }],
        };
        let svg = render_to_svg(&scene);
        let s = String::from_utf8(svg).unwrap();
        assert!(s.contains("transform=\"matrix(2,0,0,2,1,1)\""));
    }

    #[test]
    fn test_render_svg_imagedata_embeds_png_data_uri() {
        let scene = Scene {
            width: 50.0,
            height: 50.0,
            dpi: 72.0,
            background: [1.0, 1.0, 1.0, 1.0],
            nodes: vec![SceneNode::ImageData {
                shape: vec![1, 1, 4],
                dtype: "u8".to_string(),
                data: Some("/wAA/w==".to_string()),
                data_blob: None,
                cmap: "viridis".to_string(),
                origin: "upper".to_string(),
                interpolation: "nearest".to_string(),
                norm: None,
                extent: None,
                alpha: Some(1.0),
                x: 5.0,
                y: 6.0,
                width: Some(10.0),
                height: Some(12.0),
                transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            }],
        };
        let svg = render_to_svg(&scene);
        let s = String::from_utf8(svg).unwrap();
        assert!(s.contains("<image "));
        assert!(s.contains("href=\"data:image/png;base64,"));
        assert!(!s.contains("fill=\"rgb(204,204,204)\""));
    }

    #[test]
    fn test_render_svg_imagedata_blob_embeds_png_data_uri() {
        let raw: Vec<u8> = vec![255, 0, 0, 255];
        let blob_refs: Vec<&[u8]> = vec![raw.as_slice()];
        let scene = Scene {
            width: 50.0,
            height: 50.0,
            dpi: 72.0,
            background: [1.0, 1.0, 1.0, 1.0],
            nodes: vec![SceneNode::ImageData {
                shape: vec![1, 1, 4],
                dtype: "u8".to_string(),
                data: None,
                data_blob: Some(0),
                cmap: "viridis".to_string(),
                origin: "upper".to_string(),
                interpolation: "nearest".to_string(),
                norm: None,
                extent: None,
                alpha: Some(1.0),
                x: 5.0,
                y: 6.0,
                width: Some(10.0),
                height: Some(12.0),
                transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            }],
        };
        let svg = render_to_svg_with_blobs(&scene, &blob_refs);
        let s = String::from_utf8(svg).unwrap();
        assert!(s.contains("<image "));
        assert!(s.contains("href=\"data:image/png;base64,"));
    }

    #[test]
    fn test_render_svg_polygons_data_blob() {
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
        let svg = render_to_svg_with_blobs(&scene, &blob_refs);
        let s = String::from_utf8(svg).unwrap();
        assert!(s.contains("<path d=\"M20"));
        assert!(s.contains("stroke="));
    }

    #[test]
    fn test_svg_polyline_coord_precision_auto_threshold() {
        assert_eq!(svg_polyline_coord_precision_auto(10), 2);
        assert_eq!(svg_polyline_coord_precision_auto(1_000_000), 1);
    }

    #[test]
    fn test_simplify_polyline_points_minmax_caps_count() {
        let points: Vec<[f64; 2]> = (0..500)
            .map(|i| {
                let x = i as f64;
                let y = (i as f64 * 0.05).sin();
                [x, y]
            })
            .collect();
        let simplified = simplify_polyline_points_minmax(&points, 32);
        assert!(simplified.len() <= 32);
        assert_eq!(simplified.first().copied(), points.first().copied());
        assert_eq!(simplified.last().copied(), points.last().copied());
    }
}

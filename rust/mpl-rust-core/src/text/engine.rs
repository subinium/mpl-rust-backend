/// Text rendering engine using fontdue.
///
/// Embeds DejaVu Sans as the default font and rasterises glyphs onto
/// tiny-skia pixmaps for compositing into the scene.
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use fontdue::{Font, FontSettings};

/// Embedded DejaVu Sans Regular font bytes.
static DEJAVU_SANS: &[u8] = include_bytes!("../../../../fonts/DejaVuSans.ttf");

/// Embedded DejaVu Sans Bold font bytes.
static DEJAVU_SANS_BOLD: &[u8] = include_bytes!("../../../../fonts/DejaVuSans-Bold.ttf");

/// Global regular font instance (initialised once).
fn default_font() -> &'static Font {
    static FONT: OnceLock<Font> = OnceLock::new();
    FONT.get_or_init(|| {
        Font::from_bytes(DEJAVU_SANS, FontSettings::default())
            .expect("failed to parse embedded DejaVu Sans font")
    })
}

/// Global bold font instance (initialised once).
/// Falls back to regular if the bold font fails to parse.
fn bold_font() -> &'static Font {
    static FONT: OnceLock<Font> = OnceLock::new();
    FONT.get_or_init(|| {
        Font::from_bytes(DEJAVU_SANS_BOLD, FontSettings::default()).unwrap_or_else(|_| {
            Font::from_bytes(DEJAVU_SANS, FontSettings::default())
                .expect("failed to parse embedded DejaVu Sans font")
        })
    })
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
struct GlyphCacheKey {
    font_weight: u32,
    px_size_bits: u32,
    ch: char,
}

#[derive(Clone)]
struct GlyphCacheValue {
    metrics: fontdue::Metrics,
    bitmap: Arc<[u8]>,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct TextPixmapCacheKey {
    font_weight: u32,
    px_size_bits: u32,
    color_bits: [u64; 4],
    text: String,
}

#[derive(Clone)]
struct TextPixmapCacheValue {
    width: u32,
    height: u32,
    pixels: Arc<[u8]>,
}

#[derive(Default)]
struct TextPixmapCache {
    map: HashMap<TextPixmapCacheKey, TextPixmapCacheValue>,
    bytes: usize,
}

fn glyph_cache() -> &'static Mutex<HashMap<GlyphCacheKey, GlyphCacheValue>> {
    static CACHE: OnceLock<Mutex<HashMap<GlyphCacheKey, GlyphCacheValue>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn text_pixmap_cache() -> &'static Mutex<TextPixmapCache> {
    static CACHE: OnceLock<Mutex<TextPixmapCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(TextPixmapCache::default()))
}

fn glyph_cache_limit() -> usize {
    std::env::var("PLOTIX_TEXT_GLYPH_CACHE_MAX")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|n| *n >= 1024)
        .unwrap_or(65_536)
}

fn text_pixmap_cache_limit() -> usize {
    std::env::var("PLOTIX_TEXT_PIXMAP_CACHE_MAX")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|n| *n >= 64)
        .unwrap_or(2_048)
}

fn text_pixmap_cache_enabled() -> bool {
    match std::env::var("PLOTIX_TEXT_PIXMAP_CACHE") {
        Ok(v) => !matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "off" | "no"
        ),
        Err(_) => true,
    }
}

fn text_pixmap_cache_byte_limit() -> usize {
    std::env::var("PLOTIX_TEXT_PIXMAP_CACHE_MAX_BYTES")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|n| *n >= 1_048_576)
        .unwrap_or(64 * 1024 * 1024)
}

fn glyph_cache_key(ch: char, px_size: f32, font_weight: u32) -> GlyphCacheKey {
    GlyphCacheKey {
        font_weight,
        px_size_bits: px_size.to_bits(),
        ch,
    }
}

fn rasterize_glyph_cached(
    font: &Font,
    ch: char,
    px_size: f32,
    font_weight: u32,
) -> (fontdue::Metrics, Arc<[u8]>) {
    let key = glyph_cache_key(ch, px_size, font_weight);
    if let Ok(cache) = glyph_cache().lock() {
        if let Some(hit) = cache.get(&key) {
            return (hit.metrics, Arc::clone(&hit.bitmap));
        }
    }

    let (metrics, bitmap_vec) = font.rasterize(ch, px_size);
    let bitmap: Arc<[u8]> = Arc::from(bitmap_vec.into_boxed_slice());

    if let Ok(mut cache) = glyph_cache().lock() {
        let limit = glyph_cache_limit();
        if cache.len() >= limit {
            cache.clear();
        }
        cache.insert(
            key,
            GlyphCacheValue {
                metrics,
                bitmap: Arc::clone(&bitmap),
            },
        );
    }

    (metrics, bitmap)
}

fn text_pixmap_cache_key(
    text: &str,
    px_size: f32,
    color: [f64; 4],
    font_weight: u32,
) -> TextPixmapCacheKey {
    TextPixmapCacheKey {
        font_weight,
        px_size_bits: px_size.to_bits(),
        color_bits: [
            color[0].to_bits(),
            color[1].to_bits(),
            color[2].to_bits(),
            color[3].to_bits(),
        ],
        text: text.to_owned(),
    }
}

fn text_pixmap_cache_get(key: &TextPixmapCacheKey) -> Option<TextPixmapCacheValue> {
    let cache = text_pixmap_cache().lock().ok()?;
    cache.map.get(key).cloned()
}

fn text_pixmap_cache_insert(key: TextPixmapCacheKey, pixmap: &tiny_skia::Pixmap) {
    let width = pixmap.width();
    let height = pixmap.height();
    let pixels: Arc<[u8]> = Arc::from(pixmap.data().to_vec().into_boxed_slice());
    let value = TextPixmapCacheValue {
        width,
        height,
        pixels,
    };

    if let Ok(mut cache) = text_pixmap_cache().lock() {
        let entry_limit = text_pixmap_cache_limit();
        let byte_limit = text_pixmap_cache_byte_limit();
        let new_bytes = value.pixels.len();

        if let Some(prev) = cache.map.remove(&key) {
            cache.bytes = cache.bytes.saturating_sub(prev.pixels.len());
        }
        if cache.map.len() >= entry_limit || cache.bytes.saturating_add(new_bytes) > byte_limit {
            cache.map.clear();
            cache.bytes = 0;
        }
        cache.bytes = cache.bytes.saturating_add(new_bytes);
        cache.map.insert(key, value);
    }
}

fn pixmap_from_cached(value: &TextPixmapCacheValue) -> Option<tiny_skia::Pixmap> {
    let mut pixmap = tiny_skia::Pixmap::new(value.width.max(1), value.height.max(1))?;
    if pixmap.data().len() != value.pixels.len() {
        return None;
    }
    pixmap.data_mut().copy_from_slice(&value.pixels);
    Some(pixmap)
}

/// Select the appropriate font based on CSS-style numeric font weight.
///
/// Weights >= 600 (semibold and above) use the bold variant;
/// everything else uses the regular variant.
pub fn get_font_for_weight(weight: u32) -> &'static Font {
    if weight >= 600 {
        bold_font()
    } else {
        default_font()
    }
}

/// Measure the bounding box of a text string at the given font size.
///
/// Returns `(width, height, descent)` in scene units (points).
/// Uses the regular font for measurement. For weight-aware measurement,
/// use `measure_text_weighted`.
pub fn measure_text(text: &str, font_size: f64) -> (f64, f64, f64) {
    measure_text_weighted(text, font_size, 400)
}

/// Measure the bounding box of a text string at the given font size and weight.
///
/// Returns `(width, height, descent)` in scene units (points).
pub fn measure_text_weighted(text: &str, font_size: f64, font_weight: u32) -> (f64, f64, f64) {
    let font = get_font_for_weight(font_weight);
    let px_size = font_size as f32;

    let metrics = font
        .horizontal_line_metrics(px_size)
        .unwrap_or(fontdue::LineMetrics {
            ascent: px_size * 0.8,
            descent: px_size * -0.2,
            line_gap: 0.0,
            new_line_size: px_size * 1.2,
        });

    let mut width: f32 = 0.0;
    let mut prev_char: Option<char> = None;
    for ch in text.chars() {
        if let Some(prev) = prev_char {
            width += font.horizontal_kern(prev, ch, px_size).unwrap_or(0.0);
        }
        let m = font.metrics(ch, px_size);
        width += m.advance_width;
        prev_char = Some(ch);
    }

    let ascent = metrics.ascent;
    let descent = -metrics.descent; // fontdue descent is negative
    let height = ascent + descent;

    (width as f64, height as f64, descent as f64)
}

/// Render a text string into a tiny-skia `Pixmap`.
///
/// The pixmap is sized to fit the text bounding box. Glyphs are drawn
/// with the given RGBA colour. Returns `None` if the text is empty.
pub fn render_text_to_pixmap(
    text: &str,
    font_size: f64,
    color: [f64; 4],
) -> Option<tiny_skia::Pixmap> {
    render_text_to_pixmap_scaled(text, font_size, color, 1.0)
}

/// Render a text string into a tiny-skia `Pixmap` with the given font weight.
///
/// The pixmap is sized to fit the text bounding box. Glyphs are drawn
/// with the given RGBA colour. Returns `None` if the text is empty.
pub fn render_text_to_pixmap_weighted(
    text: &str,
    font_size: f64,
    color: [f64; 4],
    font_weight: u32,
) -> Option<tiny_skia::Pixmap> {
    render_text_to_pixmap_scaled_weighted(text, font_size, color, 1.0, font_weight)
}

/// Render text with an oversampling factor for improved visual quality.
///
/// `oversample` > 1.0 rasterizes glyphs at higher resolution; caller should
/// apply a reciprocal transform to keep final text size unchanged.
pub fn render_text_to_pixmap_scaled(
    text: &str,
    font_size: f64,
    color: [f64; 4],
    oversample: f32,
) -> Option<tiny_skia::Pixmap> {
    render_text_to_pixmap_scaled_weighted(text, font_size, color, oversample, 400)
}

/// Render text with an oversampling factor and font weight.
///
/// `oversample` > 1.0 rasterizes glyphs at higher resolution; caller should
/// apply a reciprocal transform to keep final text size unchanged.
/// `font_weight` selects the font variant (>= 600 for bold).
pub fn render_text_to_pixmap_scaled_weighted(
    text: &str,
    font_size: f64,
    color: [f64; 4],
    oversample: f32,
    font_weight: u32,
) -> Option<tiny_skia::Pixmap> {
    if text.is_empty() {
        return None;
    }

    let font = get_font_for_weight(font_weight);
    let ratio = oversample.max(1.0);
    let px_size = (font_size as f32 * ratio).max(1.0);
    let use_pixmap_cache = text_pixmap_cache_enabled();
    let cache_key = if use_pixmap_cache {
        let key = text_pixmap_cache_key(text, px_size, color, font_weight);
        if let Some(hit) = text_pixmap_cache_get(&key) {
            if let Some(pixmap) = pixmap_from_cached(&hit) {
                return Some(pixmap);
            }
        }
        Some(key)
    } else {
        None
    };

    let metrics = font
        .horizontal_line_metrics(px_size)
        .unwrap_or(fontdue::LineMetrics {
            ascent: px_size * 0.8,
            descent: px_size * -0.2,
            line_gap: 0.0,
            new_line_size: px_size * 1.2,
        });

    let ascent = metrics.ascent;
    let descent = -metrics.descent;
    let line_height = (ascent + descent).ceil() as u32 + 2; // +2 for padding

    // First pass: measure total width.
    let mut total_width: f32 = 0.0;
    let mut glyph_data: Vec<(fontdue::Metrics, Arc<[u8]>, f32)> = Vec::new();
    let mut prev_char: Option<char> = None;
    for ch in text.chars() {
        if let Some(prev) = prev_char {
            total_width += font.horizontal_kern(prev, ch, px_size).unwrap_or(0.0);
        }
        let (m, bitmap) = rasterize_glyph_cached(font, ch, px_size, font_weight);
        let x_pos = total_width;
        total_width += m.advance_width;
        glyph_data.push((m, bitmap, x_pos));
        prev_char = Some(ch);
    }

    let px_width = total_width.ceil() as u32;
    if px_width == 0 || line_height == 0 {
        return None;
    }

    let mut pixmap = tiny_skia::Pixmap::new(px_width.max(1), line_height.max(1))?;

    let base_r = (color[0].clamp(0.0, 1.0) * 255.0).round() as u16;
    let base_g = (color[1].clamp(0.0, 1.0) * 255.0).round() as u16;
    let base_b = (color[2].clamp(0.0, 1.0) * 255.0).round() as u16;
    let a_base = color[3].clamp(0.0, 1.0);
    let px_width_usize = px_width as usize;
    let line_height_usize = line_height as usize;
    let pixels = pixmap.data_mut();

    // Second pass: composite each glyph.
    for (m, bitmap, x_pos) in &glyph_data {
        if bitmap.is_empty() || m.width == 0 || m.height == 0 {
            continue;
        }

        // Glyph origin: xmin from left, baseline offset from top.
        let gx = (*x_pos + m.xmin as f32).round() as i32;
        let gy = (ascent - m.height as f32 - m.ymin as f32).round() as i32;

        // Clip glyph bounds once and iterate only valid pixels.
        let src_x0 = if gx < 0 { (-gx) as usize } else { 0 };
        let src_y0 = if gy < 0 { (-gy) as usize } else { 0 };
        let dst_x0 = gx.max(0) as usize;
        let dst_y0 = gy.max(0) as usize;
        if dst_x0 >= px_width_usize || dst_y0 >= line_height_usize {
            continue;
        }
        let cols = m
            .width
            .saturating_sub(src_x0)
            .min(px_width_usize.saturating_sub(dst_x0));
        let rows = m
            .height
            .saturating_sub(src_y0)
            .min(line_height_usize.saturating_sub(dst_y0));
        if cols == 0 || rows == 0 {
            continue;
        }

        for row_off in 0..rows {
            let src_row = src_y0 + row_off;
            let dst_row = dst_y0 + row_off;
            let bitmap_row_base = src_row * m.width + src_x0;
            let mut idx = (dst_row * px_width_usize + dst_x0) * 4;
            for col_off in 0..cols {
                let coverage = bitmap[bitmap_row_base + col_off] as f64 / 255.0;
                let src_a = (a_base * coverage * 255.0).round() as u8;
                if src_a == 0 {
                    idx += 4;
                    continue;
                }

                // Compute source color in premultiplied-alpha form.
                let src_a_u16 = src_a as u16;
                let src_r = ((base_r * src_a_u16 + 127) / 255) as u8;
                let src_g = ((base_g * src_a_u16 + 127) / 255) as u8;
                let src_b = ((base_b * src_a_u16 + 127) / 255) as u8;
                blend_premul_source_over(pixels, idx, src_r, src_g, src_b, src_a);
                idx += 4;
            }
        }
    }

    if let Some(key) = cache_key {
        text_pixmap_cache_insert(key, &pixmap);
    }
    Some(pixmap)
}

/// Blend a premultiplied source pixel onto premultiplied destination pixel.
fn blend_premul_source_over(
    dst: &mut [u8],
    idx: usize,
    src_r: u8,
    src_g: u8,
    src_b: u8,
    src_a: u8,
) {
    if src_a == 0 {
        return;
    }
    if src_a == 255 {
        dst[idx] = src_r;
        dst[idx + 1] = src_g;
        dst[idx + 2] = src_b;
        dst[idx + 3] = 255;
        return;
    }

    let inv_src_a = 255u16 - src_a as u16;

    let out_r = src_r as u16 + ((dst[idx] as u16 * inv_src_a + 127) / 255);
    let out_g = src_g as u16 + ((dst[idx + 1] as u16 * inv_src_a + 127) / 255);
    let out_b = src_b as u16 + ((dst[idx + 2] as u16 * inv_src_a + 127) / 255);
    let out_a = src_a as u16 + ((dst[idx + 3] as u16 * inv_src_a + 127) / 255);

    dst[idx] = out_r.min(255) as u8;
    dst[idx + 1] = out_g.min(255) as u8;
    dst[idx + 2] = out_b.min(255) as u8;
    dst[idx + 3] = out_a.min(255) as u8;
}

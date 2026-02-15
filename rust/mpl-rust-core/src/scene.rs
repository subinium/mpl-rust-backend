use serde::{Deserialize, Serialize};

/// The top-level scene graph. Represents a complete visualization that can be
/// rendered to a raster image (PNG) or vector format (SVG, PDF — future).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scene {
    pub width: f64,
    pub height: f64,
    pub dpi: f64,
    pub background: [f64; 4], // RGBA 0.0-1.0
    pub nodes: Vec<SceneNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SceneNode {
    #[serde(rename = "group")]
    Group {
        #[serde(default = "default_transform")]
        transform: [f64; 6], // [a, b, c, d, tx, ty] affine
        #[serde(default = "default_alpha")]
        alpha: f64,
        #[serde(default)]
        clip: Option<ClipRect>,
        children: Vec<SceneNode>,
    },
    #[serde(rename = "path")]
    Path {
        segments: Vec<PathSegment>,
        #[serde(default)]
        fill: Option<FillStyle>,
        #[serde(default)]
        stroke: Option<StrokeStyle>,
        #[serde(default = "default_transform")]
        transform: [f64; 6],
    },
    #[serde(rename = "text")]
    Text {
        content: String,
        x: f64,
        y: f64,
        #[serde(default = "default_font_size")]
        font_size: f64,
        #[serde(default = "default_font_family")]
        font_family: String,
        #[serde(default = "default_font_weight")]
        font_weight: u32,
        #[serde(default = "default_color")]
        color: [f64; 4],
        #[serde(default)]
        rotation: f64,
        #[serde(default = "default_ha")]
        ha: String,
        #[serde(default = "default_va")]
        va: String,
        #[serde(default = "default_transform")]
        transform: [f64; 6],
    },
    #[serde(rename = "markers")]
    Markers {
        path: Vec<PathSegment>,
        positions: Vec<[f64; 2]>,
        size: f64,
        #[serde(default)]
        fill: Option<FillStyle>,
        #[serde(default)]
        stroke: Option<StrokeStyle>,
        #[serde(default)]
        positions_transform: Option<[f64; 6]>,
        #[serde(default = "default_transform")]
        transform: [f64; 6],
    },
    #[serde(rename = "markers_data")]
    MarkersData {
        path: Vec<PathSegment>,
        #[serde(default)]
        positions_data: Option<String>, // base64 encoded interleaved XY bytes
        #[serde(default)]
        positions_blob: Option<usize>, // raw interleaved XY blob index
        #[serde(default = "default_point_dtype")]
        positions_dtype: String, // "f32" | "f64"
        count: usize,
        size: f64,
        #[serde(default)]
        fill: Option<FillStyle>,
        #[serde(default)]
        stroke: Option<StrokeStyle>,
        #[serde(default)]
        positions_transform: Option<[f64; 6]>,
        #[serde(default = "default_transform")]
        transform: [f64; 6],
    },
    #[serde(rename = "polyline_data")]
    PolylineData {
        #[serde(default)]
        points_data: Option<String>, // base64 encoded interleaved XY bytes
        #[serde(default)]
        points_blob: Option<usize>, // raw interleaved XY blob index
        #[serde(default = "default_point_dtype")]
        points_dtype: String, // "f32" | "f64"
        count: usize,
        #[serde(default)]
        stroke: Option<StrokeStyle>,
        #[serde(default = "default_transform")]
        transform: [f64; 6],
    },
    #[serde(rename = "polygons_data")]
    PolygonsData {
        #[serde(default)]
        points_data: Option<String>, // base64 encoded interleaved XY bytes
        #[serde(default)]
        points_blob: Option<usize>, // raw interleaved XY blob index
        #[serde(default = "default_point_dtype")]
        points_dtype: String, // "f32" | "f64"
        point_count: usize,
        #[serde(default)]
        ring_sizes_data: Option<String>, // base64 encoded polygon ring sizes
        #[serde(default)]
        ring_sizes_blob: Option<usize>, // raw ring-size blob index
        #[serde(default = "default_index_dtype")]
        ring_sizes_dtype: String, // "u32" | "u64"
        polygon_count: usize,
        #[serde(default)]
        fill_colors_data: Option<String>, // base64 encoded per-polygon RGBA rows
        #[serde(default)]
        fill_colors_blob: Option<usize>, // raw per-polygon RGBA blob index
        #[serde(default = "default_color_dtype")]
        fill_colors_dtype: String, // "f32" | "f64"
        #[serde(default)]
        fill: Option<FillStyle>, // optional uniform fallback fill
        #[serde(default)]
        stroke: Option<StrokeStyle>, // optional uniform stroke
        #[serde(default = "default_transform")]
        transform: [f64; 6],
    },
    #[serde(rename = "path_data")]
    PathData {
        #[serde(default)]
        vertices_data: Option<String>,
        #[serde(default)]
        vertices_blob: Option<usize>,
        #[serde(default = "default_point_dtype")]
        vertices_dtype: String,
        #[serde(default)]
        codes_data: Option<String>,
        #[serde(default)]
        codes_blob: Option<usize>,
        count: usize,
        #[serde(default)]
        snap: bool,
        #[serde(default)]
        fill: Option<FillStyle>,
        #[serde(default)]
        stroke: Option<StrokeStyle>,
        #[serde(default = "default_transform")]
        transform: [f64; 6],
    },
    #[serde(rename = "image_blob")]
    ImageBlob {
        data_blob: usize,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        #[serde(default = "default_transform")]
        transform: [f64; 6],
    },
    #[serde(rename = "image")]
    Image {
        data: String, // base64 encoded RGBA bytes
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        #[serde(default = "default_transform")]
        transform: [f64; 6],
    },
    #[serde(rename = "image_data")]
    ImageData {
        shape: Vec<usize>,
        dtype: String,
        #[serde(default)]
        data: Option<String>, // base64 encoded ndarray bytes
        #[serde(default)]
        data_blob: Option<usize>, // raw ndarray blob index
        #[serde(default = "default_cmap")]
        cmap: String,
        #[serde(default = "default_origin")]
        origin: String,
        #[serde(default = "default_interpolation")]
        interpolation: String,
        #[serde(default)]
        norm: Option<serde_json::Value>,
        #[serde(default)]
        extent: Option<[f64; 4]>, // data-coord extent (optional metadata)
        #[serde(default)]
        alpha: Option<f64>,
        #[serde(default)]
        x: f64,
        #[serde(default)]
        y: f64,
        #[serde(default)]
        width: Option<f64>,
        #[serde(default)]
        height: Option<f64>,
        #[serde(default = "default_transform")]
        transform: [f64; 6],
    },
}

fn default_font_family() -> String {
    "DejaVu Sans".to_string()
}

fn default_font_weight() -> u32 {
    400
}

fn default_ha() -> String {
    "left".to_string()
}

fn default_va() -> String {
    "baseline".to_string()
}

fn default_cmap() -> String {
    "viridis".to_string()
}

fn default_origin() -> String {
    "upper".to_string()
}

fn default_interpolation() -> String {
    "nearest".to_string()
}

fn default_point_dtype() -> String {
    "f32".to_string()
}

fn default_index_dtype() -> String {
    "u32".to_string()
}

fn default_color_dtype() -> String {
    "f32".to_string()
}

fn default_transform() -> [f64; 6] {
    [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]
}

fn default_alpha() -> f64 {
    1.0
}

fn default_color() -> [f64; 4] {
    [0.0, 0.0, 0.0, 1.0]
}

fn default_font_size() -> f64 {
    10.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathSegment {
    pub cmd: String, // "M", "L", "C", "Q", "Z"
    #[serde(default)]
    pub points: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FillStyle {
    pub color: [f64; 4],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrokeStyle {
    pub color: [f64; 4],
    pub width: f64,
    #[serde(default)]
    pub line_cap: String,
    #[serde(default)]
    pub line_join: String,
    #[serde(default)]
    pub dash_array: Vec<f64>,
    #[serde(default)]
    pub dash_offset: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Scene {
    /// Create a new empty scene with default white background.
    pub fn new(width: f64, height: f64, dpi: f64) -> Self {
        Self {
            width,
            height,
            dpi,
            background: [1.0, 1.0, 1.0, 1.0],
            nodes: Vec::new(),
        }
    }
}

pub mod color;
pub mod geometry;
pub mod render;
pub mod scene;
pub mod text;

/// Parse scene JSON bytes into a Scene struct.
pub fn parse_scene(json: &[u8]) -> Result<scene::Scene, String> {
    serde_json::from_slice(json).map_err(|e| format!("Invalid scene JSON: {}", e))
}

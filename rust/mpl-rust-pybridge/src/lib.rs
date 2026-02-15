use pyo3::prelude::*;
use pyo3::pybacked::PyBackedBytes;
use pyo3::types::PyBytes;

/// Parse a scene packet with format: PXPK | json_len(u32) | blob_count(u32) | json | (blob_len(u32) | blob_data)*
struct ScenePacketView {
    json_start: usize,
    json_end: usize,
    blob_ranges: Vec<(usize, usize)>,
}

fn parse_scene_packet(packet: &[u8]) -> Result<ScenePacketView, String> {
    if packet.len() < 12 {
        return Err("scene packet too short".to_string());
    }
    if &packet[0..4] != b"PXPK" {
        return Err("invalid scene packet magic".to_string());
    }

    let json_len = u32::from_le_bytes([packet[4], packet[5], packet[6], packet[7]]) as usize;
    let blob_count = u32::from_le_bytes([packet[8], packet[9], packet[10], packet[11]]) as usize;

    let mut cursor = 12usize;
    if cursor
        .checked_add(json_len)
        .filter(|end| *end <= packet.len())
        .is_none()
    {
        return Err("scene packet json length out of bounds".to_string());
    }
    let json_end = cursor + json_len;
    cursor = json_end;

    let mut blob_ranges: Vec<(usize, usize)> = Vec::with_capacity(blob_count);
    for _ in 0..blob_count {
        if cursor
            .checked_add(4)
            .filter(|end| *end <= packet.len())
            .is_none()
        {
            return Err("scene packet blob header out of bounds".to_string());
        }
        let blob_len = u32::from_le_bytes([
            packet[cursor],
            packet[cursor + 1],
            packet[cursor + 2],
            packet[cursor + 3],
        ]) as usize;
        cursor += 4;

        if cursor
            .checked_add(blob_len)
            .filter(|end| *end <= packet.len())
            .is_none()
        {
            return Err("scene packet blob data out of bounds".to_string());
        }
        let end = cursor + blob_len;
        blob_ranges.push((cursor, end));
        cursor = end;
    }

    if cursor != packet.len() {
        return Err("scene packet has trailing bytes".to_string());
    }
    Ok(ScenePacketView {
        json_start: 12usize,
        json_end,
        blob_ranges,
    })
}

/// Render a scene described by UTF-8 JSON bytes to the specified output format.
///
/// The GIL is released during parse+render.
#[pyfunction]
fn render_scene_bytes(
    py: Python<'_>,
    scene_json: PyBackedBytes,
    format: &str,
) -> PyResult<PyObject> {
    let format_owned = format.to_string();

    let result = py.allow_threads(move || -> Result<Vec<u8>, String> {
        let scene = mpl_rust_core::parse_scene(scene_json.as_ref())?;

        match format_owned.as_str() {
            "png" => Ok(mpl_rust_core::render::rasterizer::render_to_png(&scene)),
            "svg" => Ok(mpl_rust_core::render::svg::render_to_svg(&scene)),
            other => Err(format!("Unsupported format: {}", other)),
        }
    });

    match result {
        Ok(bytes) => Ok(PyBytes::new(py, &bytes).into()),
        Err(message) => Err(pyo3::exceptions::PyValueError::new_err(message)),
    }
}

/// Render scene JSON plus a side-channel blob list to the specified format.
#[pyfunction]
fn render_scene_with_blobs(
    py: Python<'_>,
    scene_json: PyBackedBytes,
    blobs: Vec<PyBackedBytes>,
    format: &str,
) -> PyResult<PyObject> {
    let format_owned = format.to_string();

    let result = py.allow_threads(move || -> Result<Vec<u8>, String> {
        let scene = mpl_rust_core::parse_scene(scene_json.as_ref())?;
        let blob_slices: Vec<&[u8]> = blobs.iter().map(|b| b.as_ref()).collect();

        match format_owned.as_str() {
            "png" => Ok(mpl_rust_core::render::rasterizer::render_to_png_with_blobs(
                &scene,
                &blob_slices,
            )),
            "svg" => Ok(mpl_rust_core::render::svg::render_to_svg_with_blobs(
                &scene,
                &blob_slices,
            )),
            other => Err(format!("Unsupported format: {}", other)),
        }
    });

    match result {
        Ok(bytes) => Ok(PyBytes::new(py, &bytes).into()),
        Err(message) => Err(pyo3::exceptions::PyValueError::new_err(message)),
    }
}

/// Render a scene packet (JSON + raw blobs in PXPK format) to the specified output format.
#[pyfunction]
fn render_scene_packet(
    py: Python<'_>,
    scene_packet: PyBackedBytes,
    format: &str,
) -> PyResult<PyObject> {
    let format_owned = format.to_string();

    let result = py.allow_threads(move || -> Result<Vec<u8>, String> {
        let packet = scene_packet.as_ref();
        let packet_view = parse_scene_packet(packet)
            .map_err(|e| format!("Invalid scene packet: {}", e))?;
        let scene_json = &packet[packet_view.json_start..packet_view.json_end];
        let scene = mpl_rust_core::parse_scene(scene_json)?;
        let blobs: Vec<&[u8]> = packet_view
            .blob_ranges
            .iter()
            .map(|(start, end)| &packet[*start..*end])
            .collect();

        match format_owned.as_str() {
            "png" => Ok(mpl_rust_core::render::rasterizer::render_to_png_with_blobs(
                &scene, &blobs,
            )),
            "svg" => Ok(mpl_rust_core::render::svg::render_to_svg_with_blobs(
                &scene, &blobs,
            )),
            other => Err(format!("Unsupported format: {}", other)),
        }
    });

    match result {
        Ok(bytes) => Ok(PyBytes::new(py, &bytes).into()),
        Err(message) => Err(pyo3::exceptions::PyValueError::new_err(message)),
    }
}

/// Measure text bounding box: returns (width, height, descent) in points.
#[pyfunction]
fn measure_text_whd(text: &str, font_size: f64, font_weight: u32) -> (f64, f64, f64) {
    mpl_rust_core::text::measure_text_weighted(text, font_size, font_weight)
}

/// The Python module definition.
#[pymodule]
fn _rust(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(render_scene_bytes, m)?)?;
    m.add_function(wrap_pyfunction!(render_scene_with_blobs, m)?)?;
    m.add_function(wrap_pyfunction!(render_scene_packet, m)?)?;
    m.add_function(wrap_pyfunction!(measure_text_whd, m)?)?;
    Ok(())
}

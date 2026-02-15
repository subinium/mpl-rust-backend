/// Normalize a floating-point dtype string (e.g. "float64", "<f8") to "f32", "f64", "u8", or "unknown".
pub(crate) fn normalized_dtype(dtype: &str) -> &'static str {
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

/// Normalize an index (unsigned integer) dtype string to "u32", "u64", or "unknown".
pub(crate) fn normalized_index_dtype(dtype: &str) -> &'static str {
    let d = dtype.to_ascii_lowercase();
    if d == "u64" || d.contains("uint64") || d.contains("u8") {
        "u64"
    } else if d == "u32" || d.contains("uint32") || d.contains("u4") {
        "u32"
    } else {
        "unknown"
    }
}

/// Normalize a color component dtype string to "f32", "f64", or "unknown".
pub(crate) fn normalized_color_dtype(dtype: &str) -> &'static str {
    let d = dtype.to_ascii_lowercase();
    if d == "f64" || d.contains("float64") || d.contains("f8") {
        "f64"
    } else if d == "f32" || d.contains("float32") || d.contains("f4") {
        "f32"
    } else {
        "unknown"
    }
}

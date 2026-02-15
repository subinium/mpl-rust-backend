use crate::color::palettes;

/// A colormap that maps scalar values in `[0, 1]` to RGBA colors.
#[derive(Debug, Clone)]
pub struct Colormap {
    /// Name of the colormap (e.g., "viridis", "tab10").
    pub name: String,
    /// The color table: each entry is `[R, G, B, A]` in `[0.0, 1.0]`.
    colors: Vec<[f64; 4]>,
    /// Whether this is a categorical (qualitative) colormap.
    /// Categorical colormaps use nearest-index lookup instead of interpolation.
    categorical: bool,
}

impl Colormap {
    /// Create a new continuous (interpolated) colormap from an RGBA table.
    pub fn new(name: &str, colors: Vec<[f64; 4]>) -> Self {
        Self {
            name: name.to_string(),
            colors,
            categorical: false,
        }
    }

    /// Create a new categorical (discrete) colormap.
    pub fn new_categorical(name: &str, colors: Vec<[f64; 4]>) -> Self {
        Self {
            name: name.to_string(),
            colors,
            categorical: true,
        }
    }

    /// Number of colors in the table.
    pub fn len(&self) -> usize {
        self.colors.len()
    }

    /// Whether the color table is empty.
    pub fn is_empty(&self) -> bool {
        self.colors.is_empty()
    }

    /// Evaluate the colormap at parameter `t` in `[0, 1]`.
    ///
    /// - For continuous colormaps: linearly interpolates between the two
    ///   nearest table entries.
    /// - For categorical colormaps: returns the color at the nearest index.
    /// - Values outside `[0, 1]` are clamped.
    pub fn evaluate(&self, t: f64) -> [f64; 4] {
        if self.colors.is_empty() {
            return [0.0, 0.0, 0.0, 1.0];
        }

        let t = t.clamp(0.0, 1.0);
        let n = self.colors.len();

        if self.categorical {
            let idx = (t * n as f64).floor() as usize;
            let idx = idx.min(n - 1);
            return self.colors[idx];
        }

        // Continuous interpolation
        if n == 1 {
            return self.colors[0];
        }

        let scaled = t * (n - 1) as f64;
        let lo = (scaled.floor() as usize).min(n - 2);
        let hi = lo + 1;
        let frac = scaled - lo as f64;

        let c0 = &self.colors[lo];
        let c1 = &self.colors[hi];

        [
            c0[0] + (c1[0] - c0[0]) * frac,
            c0[1] + (c1[1] - c0[1]) * frac,
            c0[2] + (c1[2] - c0[2]) * frac,
            c0[3] + (c1[3] - c0[3]) * frac,
        ]
    }

    /// Look up a built-in colormap by name.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "viridis" => Some(Self::new("viridis", palettes::viridis())),
            "plasma" => Some(Self::new("plasma", palettes::plasma())),
            "inferno" => Some(Self::new("inferno", palettes::inferno())),
            "magma" => Some(Self::new("magma", palettes::magma())),
            "cividis" => Some(Self::new("cividis", palettes::cividis())),
            "hot" => Some(Self::new("hot", palettes::hot())),
            "cool" => Some(Self::new("cool", palettes::cool())),
            "spring" => Some(Self::new("spring", palettes::spring())),
            "summer" => Some(Self::new("summer", palettes::summer())),
            "autumn" => Some(Self::new("autumn", palettes::autumn())),
            "winter" => Some(Self::new("winter", palettes::winter())),
            "gray" | "grey" => Some(Self::new("gray", palettes::gray())),
            "jet" => Some(Self::new("jet", palettes::jet())),
            "coolwarm" => Some(Self::new("coolwarm", palettes::coolwarm())),
            "RdBu" | "rdbu" => Some(Self::new("RdBu", palettes::rdbu())),
            "tab10" => Some(Self::new_categorical("tab10", palettes::tab10())),
            "tab20" => Some(Self::new_categorical("tab20", palettes::tab20())),
            _ => None,
        }
    }

    /// Return a list of all built-in colormap names.
    pub fn builtin_names() -> &'static [&'static str] {
        &[
            "viridis", "plasma", "inferno", "magma", "cividis", "hot", "cool", "spring", "summer",
            "autumn", "winter", "gray", "jet", "coolwarm", "RdBu", "tab10", "tab20",
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_evaluate_endpoints() {
        let cmap = Colormap::from_name("viridis").unwrap();
        let c0 = cmap.evaluate(0.0);
        let c1 = cmap.evaluate(1.0);
        // viridis starts dark purple, ends yellow
        assert!(c0[0] < 0.4); // low red at start
        assert!(c1[1] > 0.8); // high green at end
    }

    #[test]
    fn test_clamp() {
        let cmap = Colormap::from_name("viridis").unwrap();
        let c_neg = cmap.evaluate(-1.0);
        let c_zero = cmap.evaluate(0.0);
        assert_eq!(c_neg, c_zero);
    }

    #[test]
    fn test_categorical() {
        let cmap = Colormap::from_name("tab10").unwrap();
        assert_eq!(cmap.len(), 10);
        // First color of tab10 is a blue
        let c = cmap.evaluate(0.0);
        assert!(c[2] > 0.5); // blue component
    }
}

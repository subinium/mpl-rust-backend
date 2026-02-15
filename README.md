# mpl-rust-backend

Drop-in matplotlib backend powered by a Rust rendering engine.

Replaces matplotlib's C++ Agg rasterizer with a Rust pipeline built on **tiny-skia** (CPU rasterizer) and **FreeType** text-as-paths. Every `draw_path`, `draw_markers`, `draw_image`, and `draw_text` call is translated into a JSON scene graph, sent to the Rust core, and rasterized in a single pass.

## Quick Start

```bash
# Build (requires Rust toolchain + maturin)
pip install maturin
maturin develop --release

# Use as matplotlib backend
python -c "
import matplotlib
matplotlib.use('module://mpl_rust_backend')
import matplotlib.pyplot as plt

plt.plot([1, 2, 3], [1, 4, 2])
plt.savefig('test.png')
"
```

Or use the canvas directly:

```python
from matplotlib.figure import Figure
from mpl_rust_backend._canvas import FigureCanvasRust

fig = Figure()
canvas = FigureCanvasRust(fig)
ax = fig.add_subplot(111)
ax.plot([1, 2, 3], [1, 4, 2])
fig.savefig("output.png")
```

## Architecture

```
matplotlib draw calls (Python)
  │
  ├── draw_path()      ─┐
  ├── draw_markers()    │  RendererRust (_renderer.py)
  ├── draw_text()       │  Translates to JSON scene graph
  └── draw_image()     ─┘
          │
          ▼
    JSON scene graph ──► Rust mpl-rust-core
                              │
                              ├── serde_json::from_slice()
                              ├── tiny-skia rasterizer
                              └── PNG encoder
                                    │
                                    ▼
                              PNG bytes → Python
```

| Layer | Language | Role |
|-------|----------|------|
| `mpl_rust_backend` | Python | matplotlib `RendererBase` implementation, scene graph builder |
| `mpl-rust-pybridge` | Rust (PyO3) | FFI bridge, accepts JSON bytes, returns PNG bytes |
| `mpl-rust-core` | Rust | Scene graph parser, tiny-skia rasterizer, PNG encoder |

## Project Structure

```
├── python/mpl_rust_backend/
│   ├── __init__.py          # Backend registration
│   ├── _canvas.py           # FigureCanvasRust (print_png, print_svg)
│   ├── _renderer.py         # RendererRust (draw_path, draw_markers, ...)
│   ├── _scene_builder.py    # JSON scene graph accumulator
│   └── _translators.py      # matplotlib → scene graph type converters
├── rust/
│   ├── mpl-rust-core/       # Rendering engine (tiny-skia, scene, text)
│   └── mpl-rust-pybridge/   # PyO3 FFI bridge
├── benchmarks/
│   └── bench_compare.py     # Speed + visual quality benchmarks
├── examples/
│   └── basic_plot.py        # Usage demo
└── tests/
```

## Benchmark: Pixel Accuracy (vs matplotlib Agg)

Average **94.1%** exact pixel match across all test cases. Differences come from anti-aliasing algorithm differences between Agg (256-level scanline AA) and tiny-skia (analytic AA).

### Core Plot Types

| Plot Type | Exact Match | MSE | Notes |
|-----------|-------------|-----|-------|
| Empty figure | 100% | 0 | Baseline |
| Pie chart | 99.0% | 14 | Best non-trivial |
| Axes frame | 98.8% | 4 | Spine AA fringe |
| Heatmap (imshow) | 98.5% | 60 | Pixel-exact images |
| Bar chart | 98.4% | 41 | Edge AA |
| Histogram | 98.4% | 38 | Many bar edges |
| Line plot (1K pts) | 96.9% | 139 | Line edge AA |
| Errorbar | 96.9% | 98 | Markers + caps |
| Step plot | 96.7% | 179 | Rectilinear |
| Text rendering | 96.6% | 142 | Glyph edge AA |
| Subplots (2x2) | 95.4% | 157 | Cumulative AA |
| Polar plot | 92.6% | 253 | Grid curves |
| Stackplot | 92.1% | 43 | Curved boundaries |
| fill_between | 89.9% | 105 | Curved boundary AA |
| Scatter (500 pts) | 87.9% | 66 | Circle edge AA |

### Patches

| Patch Type | Exact Match |
|------------|-------------|
| Wedge | 97.6% |
| Circle | 97.5% |
| FancyBboxPatch | 97.3% |
| Polygon | 97.3% |
| Rectangle | 96.0% |

### Hard Cases (20 complex plots)

| Plot Type | Exact Match | Notes |
|-----------|-------------|-------|
| Grouped horizontal bar | 97.4% | Best hard case |
| Fancy arrows | 96.8% | |
| Annotated heatmap | 95.6% | |
| Filled contour | 94.2% | |
| Multi-line styles | 93.3% | |
| Stem plot | 93.1% | |
| Scatter + colorbar | 92.4% | |
| Hexbin | 92.3% | |
| Boxplot | 91.4% | |
| Twinx axes | 91.3% | |
| Violin plot | 90.6% | |
| Math text | 90.3% | |
| GridSpec layout | 89.5% | |
| Quiver | 88.8% | |
| Contour lines | 88.1% | |
| Gantt chart | 86.9% | |
| 3D surface | 84.9% | |
| 3D wireframe | 84.4% | |
| Stacked histogram | 83.2% | |
| Log-log scale | 82.0% | Worst hard case |

**Average across 20 hard cases: 91.6%**

### Why Not 100%?

The remaining ~6% gap is fundamental — Agg and tiny-skia use different anti-aliasing algorithms:

- **Agg**: Area-based scanline rasterizer, 256 coverage levels
- **tiny-skia**: Analytic AA (Skia-derived), different edge coverage computation

Every shape edge (lines, circles, text glyphs, filled boundaries) produces slightly different sub-pixel coverage values. This affects ~2-7% of pixels per figure depending on complexity.

Reaching 99%+ would require replacing tiny-skia with [`agg-rust`](https://crates.io/crates/agg) (a pure Rust port of the same AGG C++ engine matplotlib uses).

## Benchmark: Speed

Measured as median of 3 runs after 1 warmup. `savefig(format="png")` end-to-end.

| Case | Agg (ms) | Rust (ms) | Ratio |
|------|----------|-----------|-------|
| Bar (10) | 29 | 38 | 0.8x |
| Heatmap (64x64) | 28 | 33 | 0.8x |
| Line (1K pts) | 30 | 37 | 0.8x |
| Scatter (500 pts) | 33 | 42 | 0.8x |
| Text heavy | 39 | 52 | 0.8x |
| Subplots (2x2) | 57 | 80 | 0.7x |
| fill_between (1K) | 31 | 42 | 0.7x |
| Line (10K pts) | 34 | 52 | 0.7x |
| Scatter (5K pts) | 55 | 104 | 0.5x |
| Line (100K pts) | 72 | 242 | 0.3x |
| Scatter (50K pts) | 183 | 630 | 0.3x |

Current Rust backend is **0.3-0.8x** the speed of Agg. The bottleneck is the Python → JSON → Rust data pipeline, not rasterization itself.

### Known Bottlenecks

| Bottleneck | Impact | Potential Fix |
|------------|--------|---------------|
| `json.dumps()` in Python | High for large data | Replace with `orjson` (3-5x faster) or `pythonize` (skip JSON entirely) |
| `path_to_segments()` Python loop | High for many paths | Move path translation to Rust via `rust-numpy` |
| `serde_json::from_slice()` in Rust | Medium | Use `pythonize` crate for direct Python dict → Rust struct |
| Per-marker re-rasterization | Medium for scatter | Implement stamp-once pattern (rasterize marker once, blit at positions) |
| Base64 image encoding | Low | Pass raw bytes via PyO3 buffer protocol |

## Implemented Fixes (vs naive Rust backend)

| Fix | Before → After |
|-----|----------------|
| Y-axis flip | Upside-down → Correct |
| Stroke +0.5px offset | Split-pixel spines → Crisp lines |
| Path snapping (rectilinear) | Axes frame 96% → 98.8% |
| Text-as-FreeType-paths | Wrong glyph shapes → Shape-perfect |
| Image flip + Nearest filter | Heatmap 40% → 98.5% |
| Marker direction encoding | Wrong tick direction → Correct |
| Clip rectangle handling | Content bleed → Proper clipping |
| Color quantization rounding | Polygon 70.7% → 97.3% |

## Development

```bash
# Build in development mode
maturin develop

# Run tests
pytest tests/

# Run benchmarks (generates artifacts in benchmarks/artifacts/)
python benchmarks/bench_compare.py
```

## Requirements

- Python >= 3.9
- Rust toolchain (rustup)
- matplotlib >= 3.5
- maturin >= 1.0

## License

MIT

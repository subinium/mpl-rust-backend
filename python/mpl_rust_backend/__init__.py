"""mpl_rust_backend — Drop-in Matplotlib backend powered by a Rust rendering engine.

Usage::

    import matplotlib
    matplotlib.use('module://mpl_rust_backend')

    import matplotlib.pyplot as plt
    plt.plot([1, 2, 3])
    plt.savefig('test.png')
"""

from ._canvas import FigureCanvasRust
from matplotlib.backend_bases import FigureManagerBase

# matplotlib requires these module-level names
FigureCanvas = FigureCanvasRust
FigureManager = FigureManagerBase

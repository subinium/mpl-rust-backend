"""Basic demo: render a matplotlib plot using the Rust backend."""

import matplotlib

matplotlib.use("module://mpl_rust_backend")

import matplotlib.pyplot as plt
import numpy as np

# Line plot
fig, axes = plt.subplots(2, 2, figsize=(10, 8))

# 1. Line plot
x = np.linspace(0, 2 * np.pi, 100)
axes[0, 0].plot(x, np.sin(x), label="sin(x)")
axes[0, 0].plot(x, np.cos(x), label="cos(x)")
axes[0, 0].set_title("Line Plot")
axes[0, 0].legend()

# 2. Scatter plot
np.random.seed(42)
axes[0, 1].scatter(np.random.randn(100), np.random.randn(100), alpha=0.6)
axes[0, 1].set_title("Scatter Plot")

# 3. Bar chart
categories = ["A", "B", "C", "D", "E"]
values = [23, 45, 12, 67, 34]
axes[1, 0].bar(
    categories, values, color=["#1f77b4", "#ff7f0e", "#2ca02c", "#d62728", "#9467bd"]
)
axes[1, 0].set_title("Bar Chart")

# 4. Fill between
x = np.linspace(0, 4, 100)
y1 = np.sin(x)
y2 = np.sin(x) * 0.5
axes[1, 1].fill_between(x, y1, y2, alpha=0.3)
axes[1, 1].plot(x, y1, label="Upper")
axes[1, 1].plot(x, y2, label="Lower")
axes[1, 1].set_title("Fill Between")
axes[1, 1].legend()

fig.suptitle("mpl_rust_backend Demo", fontsize=14)
fig.tight_layout()
fig.savefig("test_output.png", dpi=150)
print(f"Saved test_output.png")

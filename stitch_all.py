"""Stitch all comparison images into one tall summary image."""

import os
from PIL import Image

dirs = [
    ("compare", "Basic (8)"),
    ("compare_extended", "Extended (18)"),
]

images = []
for d, label in dirs:
    path = os.path.join(os.path.dirname(__file__), d)
    if not os.path.isdir(path):
        continue
    files = sorted(f for f in os.listdir(path) if f.endswith(".png"))
    for f in files:
        images.append(Image.open(os.path.join(path, f)))

if not images:
    print("No images found")
    exit(1)

# Normalize widths to the max width
max_w = max(im.width for im in images)
resized = []
for im in images:
    if im.width != max_w:
        ratio = max_w / im.width
        new_h = int(im.height * ratio)
        im = im.resize((max_w, new_h), Image.LANCZOS)
    resized.append(im)

total_h = sum(im.height for im in resized)
canvas = Image.new("RGBA", (max_w, total_h), (255, 255, 255, 255))

y = 0
for im in resized:
    canvas.paste(im, (0, y))
    y += im.height

out = os.path.join(os.path.dirname(__file__), "compare_all.png")
canvas.save(out, optimize=True)
print(f"Saved {out} ({max_w}x{total_h})")

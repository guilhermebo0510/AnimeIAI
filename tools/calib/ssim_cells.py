"""SSIM por célula (leitura, skimage em luminância): onde a estrutura difere. uso: python ssim_cells.py ref.png render.png [cols rows]"""
import sys
import numpy as np
from PIL import Image
from skimage.metrics import structural_similarity

a = np.asarray(Image.open(sys.argv[1]).convert("L")).astype(np.float64)
b = np.asarray(Image.open(sys.argv[2]).convert("L")).astype(np.float64)
cols = int(sys.argv[3]) if len(sys.argv) > 3 else 8
rows = int(sys.argv[4]) if len(sys.argv) > 4 else 6
m, smap = structural_similarity(a, b, data_range=255, full=True, gaussian_weights=True, sigma=1.5, use_sample_covariance=False)
print(f"SSIM (skimage, luma) {m:.4f}")
h, w = a.shape
cells = []
for r in range(rows):
    out = []
    for c in range(cols):
        y0, y1, x0, x1 = r * h // rows, (r + 1) * h // rows, c * w // cols, (c + 1) * w // cols
        v = smap[y0:y1, x0:x1].mean()
        out.append(f"{v:.3f}")
        cells.append((v, x0, y0, x1, y1))
    print(" ".join(out))
print("piores:", [(round(v, 3), (x0, y0, x1, y1)) for v, x0, y0, x1, y1 in sorted(cells)[:6]])

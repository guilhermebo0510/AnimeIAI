"""Bordas faltando (referência sem par no render a ≤5 px) e sobrando (render sem par), por célula e em mapa (leitura).
uso: python missspur.py ref.png render.png mapa.png [cols rows]"""
import sys
import numpy as np
from PIL import Image, ImageFilter
from skimage.feature import canny
from scipy.ndimage import distance_transform_edt

a = np.asarray(Image.open(sys.argv[1]).convert("L")).astype(float)
b = np.asarray(Image.open(sys.argv[2]).convert("L")).astype(float)
cols = int(sys.argv[4]) if len(sys.argv) > 4 else 8
rows = int(sys.argv[5]) if len(sys.argv) > 5 else 6
ea = canny(a, sigma=2, low_threshold=15, high_threshold=35)
eb = canny(b, sigma=2, low_threshold=15, high_threshold=35)
miss = ea & (distance_transform_edt(~eb) > 5)
spur = eb & (distance_transform_edt(~ea) > 5)
print(f"bordas ref {ea.sum()} render {eb.sum()} | faltando {miss.sum()} ({miss.sum() / ea.sum():.1%}) sobrando {spur.sum()} ({spur.sum() / eb.sum():.1%})")
h, w = a.shape
cells = []
for r in range(rows):
    line = []
    for c in range(cols):
        y0, y1, x0, x1 = r * h // rows, (r + 1) * h // rows, c * w // cols, (c + 1) * w // cols
        m, s = miss[y0:y1, x0:x1].sum(), spur[y0:y1, x0:x1].sum()
        line.append(f"{m:4d}/{s:<4d}")
        cells.append((m + s, m, s, x0, y0))
    print(" ".join(line))
print("piores:", [(x0, y0, m, s) for _, m, s, x0, y0 in sorted(cells, reverse=True)[:6]])
o = np.stack([a * 0.4 + 150] * 3, -1).astype(np.uint8)
o[miss] = (0, 160, 0)
o[spur] = (230, 0, 0)
Image.fromarray(o).filter(ImageFilter.MinFilter(3)).resize((w // 2, h // 2), Image.LANCZOS).save(sys.argv[3])

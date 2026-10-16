"""Erro por célula (leitura): média |ΔRGB| e viés com sinal (render − ref) numa grade, para achar onde corrigir.
uso: python tools/calib/err_grid.py ref.png render.png [cols] [rows]"""
import sys
import numpy as np
from PIL import Image

ref = np.asarray(Image.open(sys.argv[1]).convert("RGB")).astype(np.float32)
ren = np.asarray(Image.open(sys.argv[2]).convert("RGB")).astype(np.float32)
cols = int(sys.argv[3]) if len(sys.argv) > 3 else 8
rows = int(sys.argv[4]) if len(sys.argv) > 4 else 6
h, w, _ = ref.shape
diff = ren - ref
mag = np.sqrt((diff ** 2).sum(-1))
print(f"média |Δ| global {mag.mean():.1f}")
cells = []
for r in range(rows):
    line = []
    for c in range(cols):
        y0, y1, x0, x1 = r * h // rows, (r + 1) * h // rows, c * w // cols, (c + 1) * w // cols
        m = mag[y0:y1, x0:x1].mean()
        b = diff[y0:y1, x0:x1].reshape(-1, 3).mean(0)
        line.append(f"{m:5.1f}")
        cells.append((m, x0, y0, x1, y1, b))
    print(" ".join(line))
print("piores células (x0,y0,x1,y1) viés RGB:")
for m, x0, y0, x1, y1, b in sorted(cells, reverse=True)[:8]:
    print(f"  {m:5.1f} ({x0},{y0},{x1},{y1}) viés {b.round(1).tolist()}")

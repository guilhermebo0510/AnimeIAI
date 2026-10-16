"""Bordas não casadas por célula (leitura do *_bordas.png do COMPARE): verde = só na referência (faltando),
vermelho = só no render (sobrando), amarelo = casadas. uso: python edge_cells.py bordas.png [cols rows]"""
import sys
import numpy as np
from PIL import Image

b = np.asarray(Image.open(sys.argv[1]).convert("RGB")).astype(int)
cols = int(sys.argv[2]) if len(sys.argv) > 2 else 8
rows = int(sys.argv[3]) if len(sys.argv) > 3 else 6
Y = (b[..., 0] == 255) & (b[..., 1] == 220) & (b[..., 2] == 0)
R = (b[..., 0] == 230) & (b[..., 1] == 0) & (b[..., 2] == 0)
G = (b[..., 0] == 0) & (b[..., 1] == 200) & (b[..., 2] == 0)
print(f"casadas {Y.sum()}  faltando {G.sum()}  sobrando {R.sum()}")
h, w = Y.shape
cells = []
for r in range(rows):
    line = []
    for c in range(cols):
        y0, y1, x0, x1 = r * h // rows, (r + 1) * h // rows, c * w // cols, (c + 1) * w // cols
        g, rr = G[y0:y1, x0:x1].sum(), R[y0:y1, x0:x1].sum()
        line.append(f"{g:5d}/{rr:<5d}")
        cells.append((g + rr, g, rr, x0, y0, x1, y1))
    print(" ".join(line))
print("piores (faltando/sobrando):")
for t, g, rr, x0, y0, x1, y1 in sorted(cells, reverse=True)[:8]:
    print(f"  ({x0},{y0},{x1},{y1}) faltando {g} sobrando {rr}")

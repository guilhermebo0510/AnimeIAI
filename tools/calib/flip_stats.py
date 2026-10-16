"""Onde está o FLIP (leitura): contribuição ao FLIP médio por classe (perto de linha da referência × interior) e por célula.
uso: python tools/calib/flip_stats.py ref.png flip_bruto.png [tinta_hex] [cols rows]"""
import sys
import numpy as np
from PIL import Image, ImageFilter

ref = Image.open(sys.argv[1]).convert("RGB")
flip = np.asarray(Image.open(sys.argv[2]).convert("L")).astype(np.float32) / 255.0
ink = sys.argv[3] if len(sys.argv) > 3 else "5c2e2e"
cols = int(sys.argv[4]) if len(sys.argv) > 4 else 8
rows = int(sys.argv[5]) if len(sys.argv) > 5 else 6
a = np.asarray(ref).astype(np.float32)
c = np.array([int(ink[i:i + 2], 16) for i in (0, 2, 4)], np.float32)
line = np.sqrt(((a - c) ** 2).sum(-1)) < 45
near = np.asarray(Image.fromarray((line * 255).astype(np.uint8)).filter(ImageFilter.MaxFilter(7))) > 0
edge = near & ~line
n = flip.size
print(f"FLIP médio {flip.mean():.4f}")
for name, m in [("tinta (linha)", line), ("borda da linha (±3 px)", edge), ("interior", ~near)]:
    print(f"  {name:24s} área {m.mean():6.1%}  FLIP médio {flip[m].mean():.3f}  contribuição {flip[m].sum() / n:.4f}")
h, w = flip.shape
print("contribuição por célula (×1000):")
cells = []
for r in range(rows):
    out = []
    for q in range(cols):
        y0, y1, x0, x1 = r * h // rows, (r + 1) * h // rows, q * w // cols, (q + 1) * w // cols
        v = flip[y0:y1, x0:x1].sum() / n * 1000
        out.append(f"{v:5.1f}")
        cells.append((v, x0, y0, x1, y1))
    print(" ".join(out))

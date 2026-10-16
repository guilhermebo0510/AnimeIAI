"""Leitura (paquímetro manual): trechos contínuos de uma cor (±tol) por linha ou coluna numa caixa da referência.
uso: python runs.py quadro.png x0 y0 x1 y1 hex tol [passo] [--colunas]"""
import math, sys
from PIL import Image

f, x0, y0, x1, y1, hx, tol = sys.argv[1], *map(int, sys.argv[2:6]), sys.argv[6], float(sys.argv[7])
step = int(sys.argv[8]) if len(sys.argv) > 8 and not sys.argv[8].startswith("--") else 4
cols = "--colunas" in sys.argv
im = Image.open(f).convert("RGB")
x1, y1 = min(x1, im.width), min(y1, im.height)
c = tuple(int(hx.lstrip("#")[i:i + 2], 16) for i in (0, 2, 4))
outer = range(x0, x1, step) if cols else range(y0, y1, step)
for o in outer:
    inner = range(y0, y1) if cols else range(x0, x1)
    hits = [i for i in inner if math.dist(im.getpixel((o, i) if cols else (i, o)), c) < tol]
    runs, s, p = [], None, None
    for i in hits:
        if s is None:
            s = p = i
        elif i == p + 1:
            p = i
        else:
            runs.append((s, p)); s = p = i
    if s is not None:
        runs.append((s, p))
    print(o, [r for r in runs if r[1] - r[0] >= 1])

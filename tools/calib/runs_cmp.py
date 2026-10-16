"""Leitura: trechos de uma cor por linha/coluna na referência e no render, lado a lado (acha deslocamentos em números).
uso: python runs_cmp.py ref.png render.png x0 y0 x1 y1 hex tol [passo] [--colunas]"""
import math, sys
from PIL import Image

ref_p, ren_p = sys.argv[1], sys.argv[2]
x0, y0, x1, y1 = map(int, sys.argv[3:7])
hx, tol = sys.argv[7], float(sys.argv[8])
step = int(sys.argv[9]) if len(sys.argv) > 9 and not sys.argv[9].startswith("--") else 4
cols = "--colunas" in sys.argv
c = tuple(int(hx.lstrip("#")[i:i + 2], 16) for i in (0, 2, 4))


def runs(im, o):
    inner = range(y0, min(y1, im.height)) if cols else range(x0, min(x1, im.width))
    hits = [i for i in inner if math.dist(im.getpixel((o, i) if cols else (i, o)), c) < tol]
    out, s, p = [], None, None
    for i in hits:
        if s is None:
            s = p = i
        elif i == p + 1:
            p = i
        else:
            out.append((s, p)); s = p = i
    if s is not None:
        out.append((s, p))
    return [r for r in out if r[1] - r[0] >= 1]


A, B = Image.open(ref_p).convert("RGB"), Image.open(ren_p).convert("RGB")
for o in (range(x0, x1, step) if cols else range(y0, y1, step)):
    ra, rb = runs(A, o), runs(B, o)
    flag = "" if ra == rb else "  <-"
    print(f"{o:5d} ref {ra}  ren {rb}{flag}")

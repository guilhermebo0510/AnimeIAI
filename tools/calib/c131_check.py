"""Leitura: para cada linha medida, fração dos centros que caem em tinta na referência (luma mais escura que a vizinhança a
±6 px na normal por ≥ 12 níveis). Linhas com fração baixa estão mal estimadas → reler. uso: python c131_check.py"""
import os, sys
import numpy as np
from PIL import Image
sys.path.insert(0, os.path.dirname(__file__))
import meas
from c131 import d, REF, FLOOR
from c131_rosas import LINES

L = np.asarray(Image.open(REF).convert("L")).astype(float)
H, W = L.shape


def at(x, y):
    return L[min(H - 1, max(0, int(round(y)))), min(W - 1, max(0, int(round(x))))]


rows = []
for name, (poly, w, closed, *opt) in LINES.items():
    ink, tol = (opt + [None, None])[:2]
    pts, ws = meas.measure_band(d, REF, meas.densify(poly, 3, closed=closed), ink or "#1a0508", tol or 0.12, closed=closed, passes=(16.0, 10.0), smooth=1)
    ns = meas._normals(pts, closed)
    ok = 0
    for (x, y), (nx, ny), wv in zip(pts, ns, ws):
        o = max(6.0, (wv or w) / 2 + 3)
        side = min(at(x + nx * o, y + ny * o), at(x - nx * o, y - ny * o))
        ok += at(x, y) < side - 12
    rows.append((ok / max(1, len(pts)), name, len(pts)))
rows.sort()
for f, n, k in rows[:40]:
    print(f"{n:22s} na tinta {f:.2f}  ({k} pts)")

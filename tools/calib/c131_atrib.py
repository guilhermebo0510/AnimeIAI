"""Leitura: bordas sem par do COMPARE perto de cada linha estimada (±r px), para escolher o que reler. uso: python c131_atrib.py tag [raio]"""
import os, sys
import numpy as np
from PIL import Image, ImageDraw
sys.path.insert(0, os.path.dirname(__file__))
import meas
from c131_rosas import LINES
from c131_caules import STEMS

tag = sys.argv[1]
r = int(sys.argv[2]) if len(sys.argv) > 2 else 8
b = np.asarray(Image.open(rf"C:\AnimeIAI\sprints\registro\calibracao\C131\tentativas\{tag}_q15\q0015_bordas.png").convert("RGB")).astype(int)
G = (b[..., 0] == 0) & (b[..., 1] == 200) & (b[..., 2] == 0)
R = (b[..., 0] == 230) & (b[..., 1] == 0) & (b[..., 2] == 0)
Y = (b[..., 0] == 255) & (b[..., 1] == 220) & (b[..., 2] == 0)
rows = []
for name, (poly, w, closed, *_) in list(LINES.items()) + [(k, (p, w, False)) for k, (p, w) in STEMS.items()]:
    m = Image.new("L", (1920, 1080), 0)
    q = [tuple(p) for p in meas.densify([list(p) for p in poly], 4, closed=closed)]
    if closed:
        q.append(q[0])
    ImageDraw.Draw(m).line(q, fill=255, width=2 * r + int(w))
    k = np.asarray(m) > 0
    rows.append((int(G[k].sum()), int(R[k].sum()), int(Y[k].sum()), name))
rows.sort(key=lambda t: -(t[0] + t[1]))
print("faltando total", int(G.sum()), "sobrando", int(R.sum()), "casadas", int(Y.sum()))
for g, rr, y, n in rows[:35]:
    print(f"{n:22s} faltando {g:5d} sobrando {rr:5d} casadas {y:5d}")

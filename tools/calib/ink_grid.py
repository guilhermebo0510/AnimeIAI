"""Leitura: só a tinta (distância à cor ≤ tol) em preto sobre branco, num recorte ampliado com grade rotulada — para o agente
LER os caminhos das linhas (as estimativas são digitadas à mão; nada é convertido sozinho).
uso: python ink_grid.py quadro.png x0 y0 x1 y1 saida.png [--ink 1a0508] [--tol 70] [--step 20] [--scale 3]"""
import argparse
import numpy as np
from PIL import Image, ImageDraw

ap = argparse.ArgumentParser()
ap.add_argument("frame"); ap.add_argument("x0", type=int); ap.add_argument("y0", type=int)
ap.add_argument("x1", type=int); ap.add_argument("y1", type=int); ap.add_argument("out")
ap.add_argument("--ink", default="1a0508"); ap.add_argument("--tol", type=float, default=70)
ap.add_argument("--step", type=int, default=20); ap.add_argument("--scale", type=int, default=3)
a = ap.parse_args()
img = np.asarray(Image.open(a.frame).convert("RGB")).astype(float)[a.y0:a.y1, a.x0:a.x1]
c = np.array([int(a.ink[i:i + 2], 16) for i in (0, 2, 4)], float)
ink = np.sqrt(((img - c) ** 2).sum(-1)) <= a.tol
o = np.where(ink[..., None], np.array([30, 30, 30]), np.array([255, 255, 255])).astype(np.uint8)
im = Image.fromarray(o).resize(((a.x1 - a.x0) * a.scale, (a.y1 - a.y0) * a.scale), Image.NEAREST)
dr = ImageDraw.Draw(im)
for x in range((a.x0 // a.step + 1) * a.step, a.x1, a.step):
    X = (x - a.x0) * a.scale
    dr.line([(X, 0), (X, im.height)], fill=(255, 120, 0) if x % 100 == 0 else (120, 200, 255), width=1)
    if x % 100 == 0:
        dr.text((X + 2, 2), str(x), fill=(255, 0, 0))
for y in range((a.y0 // a.step + 1) * a.step, a.y1, a.step):
    Y = (y - a.y0) * a.scale
    dr.line([(0, Y), (im.width, Y)], fill=(255, 120, 0) if y % 100 == 0 else (120, 200, 255), width=1)
    if y % 100 == 0:
        dr.text((2, Y + 2), str(y), fill=(255, 0, 0))
im.save(a.out)
print(a.out, im.size)

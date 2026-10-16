"""Leitura: bordas da REFERÊNCIA como o COMPARE as vê (Canny replicado) sobre a imagem esmaecida, recorte ampliado com grade
rotulada — para o agente ler onde estão as arestas que contam (as estimativas continuam digitadas à mão).
uso: python edge_grid.py quadro.png x0 y0 x1 y1 saida.png [--step 10] [--scale 3] [--render render.png]"""
import argparse, sys
import numpy as np
from PIL import Image, ImageDraw
ap = argparse.ArgumentParser()
ap.add_argument("frame"); ap.add_argument("x0", type=int); ap.add_argument("y0", type=int)
ap.add_argument("x1", type=int); ap.add_argument("y1", type=int); ap.add_argument("out")
ap.add_argument("--step", type=int, default=10); ap.add_argument("--scale", type=int, default=3); ap.add_argument("--render", default=None)
a = ap.parse_args()
argv = sys.argv
sys.argv = ["x", "a", "b", "0,0,1,1", "o"]
exec(open(__file__.replace("edge_grid.py", "canny_pair.py"), encoding="utf-8").read().split("ref, ren, box")[0])
mf, _, _ = grad(a.frame)
high = max(float(np.percentile(mf, 95)), 10.0)
E, _ = canny(a.frame, high, 0.4 * high)
img = np.asarray(Image.open(a.frame).convert("RGB")).astype(float)
o = (img * 0.35 + 165).astype(np.uint8)
o[E] = [0, 140, 0]
if a.render:
    Et, _ = canny(a.render, high, 0.4 * high)
    o[Et & ~E] = [220, 0, 0]
    o[Et & E] = [200, 170, 0]
c = Image.fromarray(o[a.y0:a.y1, a.x0:a.x1]).resize(((a.x1 - a.x0) * a.scale, (a.y1 - a.y0) * a.scale), Image.NEAREST)
dr = ImageDraw.Draw(c)
for x in range((a.x0 // a.step + 1) * a.step, a.x1, a.step):
    X = (x - a.x0) * a.scale
    dr.line([(X, 0), (X, c.height)], fill=(255, 120, 0) if x % 100 == 0 else (150, 210, 255), width=1)
    if x % 50 == 0:
        dr.text((X + 2, 2), str(x), fill=(255, 0, 0))
for y in range((a.y0 // a.step + 1) * a.step, a.y1, a.step):
    Y = (y - a.y0) * a.scale
    dr.line([(0, Y), (c.width, Y)], fill=(255, 120, 0) if y % 100 == 0 else (150, 210, 255), width=1)
    if y % 50 == 0:
        dr.text((2, Y + 2), str(y), fill=(255, 0, 0))
c.save(a.out)
print(a.out, c.size)

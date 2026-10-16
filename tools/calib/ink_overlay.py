"""Leitura de posição: pixels de tinta do RENDER (perto da cor da linha) em ciano semitransparente sobre a referência.
uso: python tools/calib/ink_overlay.py ref.png render.png x0 y0 x1 y1 saida.png [--ink 5c2e2e] [--tol 40] [--scale 1] [--step 50]"""
import argparse
import numpy as np
from PIL import Image, ImageDraw

ap = argparse.ArgumentParser()
ap.add_argument("ref"); ap.add_argument("render")
for k in ("x0", "y0", "x1", "y1"):
    ap.add_argument(k, type=int)
ap.add_argument("out"); ap.add_argument("--ink", default="5c2e2e"); ap.add_argument("--tol", type=float, default=40)
ap.add_argument("--scale", type=int, default=1); ap.add_argument("--step", type=int, default=50)
a = ap.parse_args()
box = (a.x0, a.y0, a.x1, a.y1)
ref = np.asarray(Image.open(a.ref).convert("RGB").crop(box)).astype(np.float32)
ren = np.asarray(Image.open(a.render).convert("RGB").crop(box)).astype(np.float32)
c = np.array([int(a.ink[i:i + 2], 16) for i in (0, 2, 4)], np.float32)
mask = np.sqrt(((ren - c) ** 2).sum(-1)) < a.tol
out = ref.copy()
out[mask] = out[mask] * 0.35 + np.array([0, 255, 255], np.float32) * 0.65
im = Image.fromarray(out.clip(0, 255).astype(np.uint8))
im = im.resize((im.width * a.scale, im.height * a.scale), Image.NEAREST)
d = ImageDraw.Draw(im)
for x in range((a.x0 // a.step + 1) * a.step, a.x1, a.step):
    sx = (x - a.x0) * a.scale
    d.line([(sx, 0), (sx, 6)], fill=(255, 255, 0)); d.text((sx + 2, 0), str(x), fill=(255, 255, 0))
for y in range((a.y0 // a.step + 1) * a.step, a.y1, a.step):
    sy = (y - a.y0) * a.scale
    d.line([(0, sy), (6, sy)], fill=(255, 255, 0)); d.text((0, sy + 2), str(y), fill=(255, 255, 0))
im.save(a.out)
print(a.out, int(mask.sum()), "px de tinta")

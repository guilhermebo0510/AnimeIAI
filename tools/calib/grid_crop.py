"""Recorte ampliado de um quadro com grade de coordenadas rotulada (para o agente LER posições; não desenha nada).

uso: python tools/calib/grid_crop.py quadro.png x0 y0 x1 y1 saida.png [--step 20] [--scale 3] [--overlay render.png]
--overlay sobrepõe (50%) um render do app no mesmo recorte para comparar posições.
"""
import argparse
from PIL import Image, ImageDraw, ImageFont

ap = argparse.ArgumentParser()
ap.add_argument("frame"); ap.add_argument("x0", type=int); ap.add_argument("y0", type=int)
ap.add_argument("x1", type=int); ap.add_argument("y1", type=int); ap.add_argument("out")
ap.add_argument("--step", type=int, default=20); ap.add_argument("--scale", type=int, default=3)
ap.add_argument("--overlay")
ap.add_argument("--lines", type=float, default=0.0, help="alto contraste: pixels com luminância < valor ficam pretos, o resto branco (só para leitura)")
ap.add_argument("--near", help="cor da linha (hex): pixels a até --tol dessa cor ficam pretos (só para leitura)")
ap.add_argument("--tol", type=float, default=35.0)
a = ap.parse_args()
im = Image.open(a.frame).convert("RGB").crop((a.x0, a.y0, a.x1, a.y1))
if a.overlay:
    ov = Image.open(a.overlay).convert("RGB").crop((a.x0, a.y0, a.x1, a.y1))
    im = Image.blend(im, ov, 0.5)
if a.lines > 0:
    g = im.convert("L")
    im = g.point(lambda v: 0 if v < a.lines * 255 else 235).convert("RGB")
if a.near:
    c = tuple(int(a.near.lstrip("#")[i:i + 2], 16) for i in (0, 2, 4))
    px = im.load()
    for yy in range(im.height):
        for xx in range(im.width):
            r, g, b = px[xx, yy]
            px[xx, yy] = (0, 0, 0) if ((r - c[0]) ** 2 + (g - c[1]) ** 2 + (b - c[2]) ** 2) ** 0.5 < a.tol else (235, 235, 235)
im = im.resize((im.width * a.scale, im.height * a.scale), Image.NEAREST)
d = ImageDraw.Draw(im)
try:
    font = ImageFont.truetype("arial.ttf", 11)
except OSError:
    font = ImageFont.load_default()
for x in range((a.x0 // a.step) * a.step, a.x1 + 1, a.step):
    if x < a.x0: continue
    sx = (x - a.x0) * a.scale
    major = x % (a.step * 5) == 0
    d.line([(sx, 0), (sx, im.height)], fill=(0, 255, 255) if major else (0, 140, 140), width=2 if major else 1)
    if major:
        d.rectangle([sx + 1, 1, sx + 30, 14], fill=(0, 0, 0))
        d.text((sx + 2, 2), str(x), fill=(255, 255, 0), font=font)
for y in range((a.y0 // a.step) * a.step, a.y1 + 1, a.step):
    if y < a.y0: continue
    sy = (y - a.y0) * a.scale
    major = y % (a.step * 5) == 0
    d.line([(0, sy), (im.width, sy)], fill=(0, 255, 255) if major else (0, 140, 140), width=2 if major else 1)
    if major:
        d.rectangle([1, sy + 1, 30, sy + 14], fill=(0, 0, 0))
        d.text((2, sy + 2), str(y), fill=(255, 255, 0), font=font)
im.save(a.out)
print(a.out, im.size)

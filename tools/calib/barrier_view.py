"""Leitura: referência com as barreiras do balde (linhas + fecho, já com a folga) por cima em ciano, recorte ampliado com
grade — mostra onde uma pétala vaza para a vizinha. uso: python barrier_view.py x0 y0 x1 y1 saida.png [escala] [gap]"""
import sys
import numpy as np
from PIL import Image, ImageDraw
from scipy.ndimage import binary_dilation
x0, y0, x1, y1 = (int(v) for v in sys.argv[1:5])
out = sys.argv[5]
sc = int(sys.argv[6]) if len(sys.argv) > 6 else 3
gap = float(sys.argv[7]) if len(sys.argv) > 7 else 8.0
ref = np.asarray(Image.open(r"C:\AnimeIAI\calibracao\ref\cut_131\frames\f_0016.png").convert("RGB")).copy()
w = np.asarray(Image.open(r"C:\AnimeIAI\calibracao\tmp\c131_barreira.png").convert("RGBA"))[..., 3] > 40
wd = binary_dilation(w, iterations=max(1, int(gap / 2)))
o = ref.copy()
o[wd & ~w] = (o[wd & ~w] * 0.5 + np.array([0, 255, 255]) * 0.5).astype(np.uint8)
o[w] = [0, 160, 160]
c = Image.fromarray(o[y0:y1, x0:x1]).resize(((x1 - x0) * sc, (y1 - y0) * sc), Image.NEAREST)
dr = ImageDraw.Draw(c)
for x in range((x0 // 10 + 1) * 10, x1, 10):
    X = (x - x0) * sc
    dr.line([(X, 0), (X, c.height)], fill=(255, 255, 255) if x % 50 else (255, 255, 0), width=1)
    if x % 50 == 0:
        dr.text((X + 2, 2), str(x), fill=(255, 255, 0))
for y in range((y0 // 10 + 1) * 10, y1, 10):
    Y = (y - y0) * sc
    dr.line([(0, Y), (c.width, Y)], fill=(255, 255, 255) if y % 50 else (255, 255, 0), width=1)
    if y % 50 == 0:
        dr.text((2, Y + 2), str(y), fill=(255, 255, 0))
c.save(out)
print(out)

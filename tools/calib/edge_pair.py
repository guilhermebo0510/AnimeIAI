"""Leitura: bordas (Canny) do render (ciano) e da referência (verde) lado a lado num recorte ampliado.
uso: python edge_pair.py ref.png render.png x0 y0 x1 y1 saida.png [escala]"""
import sys
import numpy as np
from PIL import Image
from skimage.feature import canny

F = Image.open(sys.argv[1]).convert("RGB")
R = Image.open(sys.argv[2]).convert("RGB")
x0, y0, x1, y1 = map(int, sys.argv[3:7])
out = sys.argv[7]
s = int(sys.argv[8]) if len(sys.argv) > 8 else 3
ea = canny(np.asarray(F.convert("L")).astype(float), sigma=2, low_threshold=15, high_threshold=35)
eb = canny(np.asarray(R.convert("L")).astype(float), sigma=2, low_threshold=15, high_threshold=35)


def panel(img, e, col):
    o = np.asarray(img).astype(np.uint8)[y0:y1, x0:x1].copy()
    o[e[y0:y1, x0:x1]] = col
    return o


p = np.concatenate([panel(R, eb, (0, 255, 255)), np.zeros((y1 - y0, 4, 3), np.uint8), panel(F, ea, (0, 255, 0))], 1)
Image.fromarray(p).resize((p.shape[1] * s, p.shape[0] * s), Image.NEAREST).save(out)
print(out)

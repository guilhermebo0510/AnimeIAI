"""Leitura: réplica do Canny do COMPARE (luma Rec.709, gaussiana σ 1, Sobel/8, NMS em 4 direções, histerese com limiares
da REFERÊNCIA: alto = max(P95, 10), baixo = 0,4·alto) num recorte — referência (verde) e render (vermelho) lado a lado.
Mostra quais bordas cada imagem tem antes do casamento a 2 px. uso: python canny_pair.py ref.png render.png x0,y0,x1,y1 saida.png [escala]"""
import sys
import numpy as np
from PIL import Image
from scipy.ndimage import gaussian_filter, label


def grad(p):
    a = np.asarray(Image.open(p).convert("RGB")).astype(np.float32)
    L = 0.2126 * a[..., 0] + 0.7152 * a[..., 1] + 0.0722 * a[..., 2]
    b = gaussian_filter(L, 1.0, mode="nearest", truncate=3.0)
    P = np.pad(b, 1, mode="edge")
    sx = ((P[:-2, 2:] + 2 * P[1:-1, 2:] + P[2:, 2:]) - (P[:-2, :-2] + 2 * P[1:-1, :-2] + P[2:, :-2])) / 8
    sy = ((P[2:, :-2] + 2 * P[2:, 1:-1] + P[2:, 2:]) - (P[:-2, :-2] + 2 * P[:-2, 1:-1] + P[:-2, 2:])) / 8
    return np.hypot(sx, sy), sx, sy


def canny(p, high, low):
    m, gx, gy = grad(p)
    h, w = m.shape
    ang = np.degrees(np.arctan2(gy, gx)) % 180
    dx = np.where((ang < 22.5) | (ang >= 157.5), 1, np.where(ang < 67.5, 1, np.where(ang < 112.5, 0, -1)))
    dy = np.where((ang < 22.5) | (ang >= 157.5), 0, 1)
    yy, xx = np.mgrid[0:h, 0:w]

    def s(ox, oy):
        X, Y = xx + ox, yy + oy
        ok = (X >= 0) & (Y >= 0) & (X < w) & (Y < h)
        out = np.zeros_like(m)
        out[ok] = m[Y[ok], X[ok]]
        return out
    keep = (m >= low) & (m > s(dx, dy)) & (m >= s(-dx, -dy))
    lab, _ = label(keep, structure=np.ones((3, 3)))
    return np.isin(lab, np.unique(lab[keep & (m >= high)])) & keep, m


ref, ren, box, out = sys.argv[1], sys.argv[2], [int(v) for v in sys.argv[3].split(",")], sys.argv[4]
sc = int(sys.argv[5]) if len(sys.argv) > 5 else 5
mf, _, _ = grad(ref)
high = max(float(np.percentile(mf, 95)), 10.0)
Er, mr = canny(ref, high, 0.4 * high)
Et, mt = canny(ren, high, 0.4 * high)
x0, y0, x1, y1 = box
o = np.full((y1 - y0, (x1 - x0) * 2 + 4, 3), 255, np.uint8)
o[:, :x1 - x0][Er[y0:y1, x0:x1]] = (0, 160, 0)
o[:, x1 - x0 + 4:][Et[y0:y1, x0:x1]] = (200, 0, 0)
Image.fromarray(o).resize(((2 * (x1 - x0) + 4) * sc, (y1 - y0) * sc), Image.NEAREST).save(out)
print("limiar alto", round(high, 2), "| bordas ref", int(Er[y0:y1, x0:x1].sum()), "render", int(Et[y0:y1, x0:x1].sum()),
      "| px >= alto ref", int((mr[y0:y1, x0:x1] >= high).sum()), "render", int((mt[y0:y1, x0:x1] >= high).sum()))

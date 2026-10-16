"""Leitura: para a região do balde que contém (x, y), pinta os pixels pela família de cor da referência (2 grupos) e mostra as
barreiras — para ver onde falta a linha que separa salmão de vermelho. uso: python region_split.py x y saida.png [gap] [escala]"""
import sys
import numpy as np
from PIL import Image, ImageDraw
from scipy.ndimage import label, binary_dilation
from sklearn.cluster import KMeans
x, y, out = int(sys.argv[1]), int(sys.argv[2]), sys.argv[3]
gap = float(sys.argv[4]) if len(sys.argv) > 4 else 8.0
sc = int(sys.argv[5]) if len(sys.argv) > 5 else 2
w = np.asarray(Image.open(r"C:\AnimeIAI\calibracao\tmp\c131_barreira.png").convert("RGBA"))[..., 3] > 40
wd = binary_dilation(w, iterations=max(1, int(gap / 2)))
lab, n = label(~wd)
m = lab == lab[y, x]
ref = np.asarray(Image.open(r"C:\AnimeIAI\calibracao\ref\cut_131\frames\f_0016.png").convert("RGB")).astype(float)
px = ref[m]
km = KMeans(2, n_init=2, random_state=0).fit(px[::max(1, len(px) // 4000)])
lb = km.predict(px)
o = (ref * 0.3 + 170).astype(np.uint8)
cols = np.array([[0, 90, 255], [255, 140, 0]], np.uint8)
o[m] = cols[lb]
o[w] = [0, 0, 0]
ys, xs = np.where(m)
x0, y0, x1, y1 = max(0, xs.min() - 10), max(0, ys.min() - 10), min(1920, xs.max() + 10), min(1080, ys.max() + 10)
c = Image.fromarray(o[y0:y1, x0:x1]).resize(((x1 - x0) * sc, (y1 - y0) * sc), Image.NEAREST)
dr = ImageDraw.Draw(c)
for gx in range((x0 // 20 + 1) * 20, x1, 20):
    dr.line([((gx - x0) * sc, 0), ((gx - x0) * sc, c.height)], fill=(200, 200, 200) if gx % 100 else (255, 0, 0))
    if gx % 100 == 0:
        dr.text(((gx - x0) * sc + 2, 2), str(gx), fill=(255, 0, 0))
for gy in range((y0 // 20 + 1) * 20, y1, 20):
    dr.line([(0, (gy - y0) * sc), (c.width, (gy - y0) * sc)], fill=(200, 200, 200) if gy % 100 else (255, 0, 0))
    if gy % 100 == 0:
        dr.text((2, (gy - y0) * sc + 2), str(gy), fill=(255, 0, 0))
c.save(out)
print(out, "cores", ["#%02x%02x%02x" % tuple(v.astype(int)) for v in km.cluster_centers_], "caixa", (x0, y0, x1, y1))

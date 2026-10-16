"""Leitura: regiões do balde (mesma barreira e folga do c131_fill) cuja cor na referência é misturada — duas famílias de cor
(ex.: salmão e vermelho) com ≥ 15 % cada: sinal de vazamento entre pétalas; mostra as caixas para fechar à mão.
uso: python c131_mixed.py saida.png [gap]"""
import os, sys
import numpy as np
from PIL import Image, ImageDraw
from scipy.ndimage import label, binary_dilation, find_objects
from sklearn.cluster import KMeans
sys.path.insert(0, os.path.dirname(__file__))
from c131 import d, ids, F, REF

out = sys.argv[1]
gap = float(sys.argv[2]) if len(sys.argv) > 2 else 8.0
TMP = r"C:\AnimeIAI\calibracao\tmp\c131_barreira.png"
d.call("RENDER", {"cut": ids["cut"], "frame": F, "out": TMP, "layers": [ids["moldura_linha"], ids["fecho"]]})
wall = binary_dilation(np.asarray(Image.open(TMP).convert("RGBA"))[..., 3] > 40, iterations=max(1, int(gap / 2)))
lab, n = label(~wall)
ref = np.asarray(Image.open(REF).convert("RGB")).astype(float)
rows = []
for i, sl in enumerate(find_objects(lab)):
    m = lab[sl] == i + 1
    a = int(m.sum())
    if a < 300 or a > 400000:
        continue
    px = ref[sl][m]
    sub = px[np.random.default_rng(0).choice(len(px), min(3000, len(px)), replace=False)]
    km = KMeans(2, n_init=2, random_state=0).fit(sub)
    c = km.cluster_centers_
    frac = np.bincount(km.labels_, minlength=2) / len(sub)
    dist = float(np.sqrt(((c[0] - c[1]) ** 2).sum()))
    if frac.min() >= 0.15 and dist > 45:
        rows.append((int(a * frac.min()), a, dist, sl[1].start, sl[0].start, sl[1].stop, sl[0].stop,
                     "#%02x%02x%02x" % tuple(c[0].astype(int)), "#%02x%02x%02x" % tuple(c[1].astype(int))))
rows.sort(reverse=True)
im = Image.open(REF).convert("RGB").point(lambda v: v // 2 + 100)
dr = ImageDraw.Draw(im)
w = np.asarray(Image.open(TMP).convert("RGBA"))[..., 3] > 40
o = np.asarray(im).copy()
o[w] = [0, 0, 0]
im = Image.fromarray(o)
dr = ImageDraw.Draw(im)
for r in rows[:25]:
    dr.rectangle(r[3:7], outline=(0, 0, 255), width=3)
    dr.text((r[3] + 3, r[4] + 3), str(r[0]), fill=(0, 0, 255))
im.resize((960, 540)).save(out)
for r in rows[:25]:
    print(r)

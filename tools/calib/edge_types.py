"""Leitura: que tipo de fronteira concentra as bordas sem par (classes de cor da referência ao redor de cada pixel).
uso: python edge_types.py ref.png bordas.png"""
import sys
import numpy as np
from collections import Counter
from PIL import Image

ref = np.asarray(Image.open(sys.argv[1]).convert("RGB")).astype(float)
b = np.asarray(Image.open(sys.argv[2]).convert("RGB")).astype(int)
R = (b[..., 0] == 230) & (b[..., 1] == 0) & (b[..., 2] == 0)
G = (b[..., 0] == 0) & (b[..., 1] == 200) & (b[..., 2] == 0)
CL = {"pele": (248, 211, 203), "tinta": (90, 44, 44), "cabelo": (160, 28, 56), "realce": (244, 155, 145), "franja": (218, 78, 80),
      "fundo": (190, 155, 125), "fundo_esc": (70, 35, 22), "branco": (245, 243, 245), "boca": (234, 117, 130), "amarelo": (234, 240, 124),
      "laranja": (160, 70, 30), "creme": (240, 244, 210), "rosa": (214, 90, 166), "sombra": (95, 10, 32)}
names = list(CL)
cols = np.array([CL[n] for n in names], float)
d = ((ref[:, :, None, :] - cols[None, None]) ** 2).sum(-1)
lab = d.argmin(-1)
for nm, m in [("faltando", G), ("sobrando", R)]:
    ys, xs = np.nonzero(m)
    cnt = Counter()
    for y, x in zip(ys, xs):
        win = lab[max(0, y - 4):y + 5, max(0, x - 4):x + 5].ravel()
        c = Counter(win).most_common(2)
        key = "/".join(sorted(names[k] for k, _ in c)) if len(c) > 1 else names[c[0][0]]
        cnt[key] += 1
    print(nm, m.sum())
    for k, v in cnt.most_common(12):
        print(f"   {v:5d} {k}")

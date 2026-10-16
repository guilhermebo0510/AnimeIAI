"""Leitura: mapa reduzido das bordas sem par do COMPARE (verde = faltando, vermelho = sobrando), engrossadas para ver.
uso: python edgemap.py bordas.png saida.png [escala]"""
import sys
import numpy as np
from PIL import Image
from scipy.ndimage import maximum_filter
b = np.asarray(Image.open(sys.argv[1]).convert("RGB")).astype(int)
G = (b[..., 0] == 0) & (b[..., 1] == 200) & (b[..., 2] == 0)
R = (b[..., 0] == 230) & (b[..., 1] == 0) & (b[..., 2] == 0)
g2, r2 = maximum_filter(G, 3), maximum_filter(R, 3)
o = np.full(b.shape, 255, np.uint8)
o[g2] = [0, 170, 0]
o[r2 & ~g2] = [220, 0, 0]
sc = float(sys.argv[3]) if len(sys.argv) > 3 else 0.5
Image.fromarray(o).resize((int(b.shape[1] * sc), int(b.shape[0] * sc))).save(sys.argv[2])
print("faltando", int(G.sum()), "sobrando", int(R.sum()))

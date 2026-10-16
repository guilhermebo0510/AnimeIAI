"""Leitura: agrupamentos de bordas sem par no *_bordas.png do COMPARE (verde = só na referência, vermelho = só no render).
uso: python f1_clusters.py bordas.png [N]"""
import sys
import numpy as np
from PIL import Image
from skimage.measure import label, regionprops
from scipy.ndimage import binary_dilation

b = np.asarray(Image.open(sys.argv[1]).convert("RGB")).astype(int)
n = int(sys.argv[2]) if len(sys.argv) > 2 else 12
Y = (b[..., 0] == 255) & (b[..., 1] == 220) & (b[..., 2] == 0)
R = (b[..., 0] == 230) & (b[..., 1] == 0) & (b[..., 2] == 0)
G = (b[..., 0] == 0) & (b[..., 1] == 200) & (b[..., 2] == 0)
print(f"casadas {Y.sum()} faltando {G.sum()} sobrando {R.sum()}")
both = G | R
lab = label(binary_dilation(both, iterations=6))
rows = []
for r in regionprops(lab):
    m = lab == r.label
    rows.append((int((G & m).sum() + (R & m).sum()), int((G & m).sum()), int((R & m).sum()), (r.bbox[1], r.bbox[0], r.bbox[3], r.bbox[2])))
for t, g, rr, box in sorted(rows, reverse=True)[:n]:
    print(f"  {t:4d} faltando {g:4d} sobrando {rr:4d} caixa {box}")

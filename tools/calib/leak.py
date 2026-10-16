"""Leitura: onde o balde do fundo (a partir do centro) invade pixels que na referência não são creme (vazamentos da moldura).
uso: python leak.py linhas.png referencia.png saida.png [gap]"""
import sys
import numpy as np
from PIL import Image
from scipy.ndimage import label, binary_dilation
L = np.asarray(Image.open(sys.argv[1]).convert("RGBA"))[..., 3] > 40
g = int(float(sys.argv[4]) / 2) if len(sys.argv) > 4 else 2
L = binary_dilation(L, iterations=g)
lab, n = label(~L)
bg = lab == lab[540, 960]
F = np.asarray(Image.open(sys.argv[2]).convert("RGB")).astype(float)
cream = np.array([[240, 235, 194], [238, 217, 183], [225, 192, 168], [242, 245, 202]], float)
dist = np.min(np.sqrt(((F[..., None, :] - cream[None, None]) ** 2).sum(-1)), axis=-1)
leak = bg & (dist > 45)
o = (F * 0.5 + 127).astype(np.uint8)
o[leak] = [0, 200, 255]
o[L] = [0, 0, 0]
Image.fromarray(o).resize((960, 540)).save(sys.argv[3])
print("vazamento px", int(leak.sum()))

"""Leitura: deslocamento local entre as referências de dois desenhos em pontos escolhidos (correlação de janela, busca
limitada). Serve para levar estimativas lidas num desenho para o seguinte; a geometria final vem das medições.
uso: python d218_desloca.py desenho_a desenho_b x,y [x,y ...] [--meia 28] [--busca 30]"""
import sys
import numpy as np
from PIL import Image
from skimage.feature import match_template

FIRST = {1: 0, 2: 6, 3: 9, 4: 14, 5: 54, 6: 57, 7: 60, 8: 63}
a, b = int(sys.argv[1]), int(sys.argv[2])
half = int(sys.argv[sys.argv.index("--meia") + 1]) if "--meia" in sys.argv else 28
search = int(sys.argv[sys.argv.index("--busca") + 1]) if "--busca" in sys.argv else 30
P = r"C:\AnimeIAI\calibracao\ref\cut_218\frames\f_%04d.png"
A = np.asarray(Image.open(P % (FIRST[a] + 1)).convert("L")).astype(float)
B = np.asarray(Image.open(P % (FIRST[b] + 1)).convert("L")).astype(float)
for arg in sys.argv[3:]:
    if "," not in arg:
        continue
    x, y = (int(float(v)) for v in arg.split(","))
    h = min(half, x - search, 1920 - search - x, y - search, 1080 - search - y)
    t = A[y - h:y + h, x - h:x + h]
    s = B[y - h - search:y + h + search, x - h - search:x + h + search]
    r = match_template(s, t)
    iy, ix = np.unravel_index(np.argmax(r), r.shape)
    print(f"{x},{y} -> dx {ix - search:+d} dy {iy - search:+d} ncc {r[iy, ix]:.3f}")

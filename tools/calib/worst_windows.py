"""Leitura: as N janelas de maior perda de SSIM (luma), lado a lado render | referência, numa imagem só.
uso: python worst_windows.py ref.png render.png saida.png [N] [tamanho] [escala]"""
import sys
import numpy as np
from PIL import Image, ImageDraw
from skimage.metrics import structural_similarity

ref_p, ren_p, out = sys.argv[1:4]
n = int(sys.argv[4]) if len(sys.argv) > 4 else 6
size = int(sys.argv[5]) if len(sys.argv) > 5 else 100
scale = int(sys.argv[6]) if len(sys.argv) > 6 else 2
F, R = Image.open(ref_p).convert("RGB"), Image.open(ren_p).convert("RGB")
a = np.asarray(F.convert("L")).astype(float)
b = np.asarray(R.convert("L")).astype(float)
_, S = structural_similarity(a, b, data_range=255, full=True, gaussian_weights=True, sigma=1.5, use_sample_covariance=False)
L = 1 - S
H, W = L.shape
cells = []
step = size // 2
for y in range(0, H - size + 1, step):
    for x in range(0, W - size + 1, step):
        cells.append((L[y:y + size, x:x + size].mean(), x, y))
cells.sort(reverse=True)
picked = []
for v, x, y in cells:
    if all(abs(x - px) >= size or abs(y - py) >= size for _, px, py in picked):
        picked.append((v, x, y))
    if len(picked) == n:
        break
tw, th = (size * 2 + 4) * scale, size * scale + 14
o = Image.new("RGB", (tw * 2 + 8, th * ((n + 1) // 2)), (255, 255, 255))
dr = ImageDraw.Draw(o)
for i, (v, x, y) in enumerate(picked):
    box = (x, y, x + size, y + size)
    c = Image.new("RGB", (size * 2 + 4, size), (0, 255, 255))
    c.paste(R.crop(box), (0, 0))
    c.paste(F.crop(box), (size + 4, 0))
    c = c.resize((c.width * scale, c.height * scale), Image.NEAREST)
    ox, oy = (i % 2) * (tw + 8), (i // 2) * th
    o.paste(c, (ox, oy + 14))
    dr.text((ox + 2, oy + 1), f"#{i + 1} ({x},{y}) perda {v:.2f}", fill=(0, 0, 0))
o.save(out)
print(out, [(round(v, 2), x, y) for v, x, y in picked])

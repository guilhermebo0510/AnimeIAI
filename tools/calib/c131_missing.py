"""Leitura: bordas da referência a > 8 px de qualquer borda do render (estrutura que falta desenhar), em aglomerados e num mapa.
uso: python c131_missing.py tag saida.png [n]"""
import sys
import numpy as np
from PIL import Image, ImageDraw
from scipy.ndimage import distance_transform_edt, label, find_objects, binary_dilation
args = sys.argv[1:]
sys.argv = ["x", "a", "b", "0,0,1,1", "o"]
exec(open(__file__.replace("c131_missing.py", "canny_pair.py"), encoding="utf-8").read().split("ref, ren, box")[0])
tag, out = args[0], args[1]
n = int(args[2]) if len(args) > 2 else 30
Fp = r"C:\AnimeIAI\calibracao\ref\cut_131\frames\f_0016.png"
Rp = rf"C:\AnimeIAI\sprints\registro\calibracao\C131\tentativas\{tag}_q15\q0015_render.png"
mf, _, _ = grad(Fp)
high = max(float(np.percentile(mf, 95)), 10.0)
Er, _ = canny(Fp, high, 0.4 * high)
Et, _ = canny(Rp, high, 0.4 * high)
miss = Er & (distance_transform_edt(~Et) > 8)
lab, k = label(binary_dilation(miss, iterations=6))
cl = []
for i, sl in enumerate(find_objects(lab)):
    c = int((miss[sl] & (lab[sl] == i + 1)).sum())
    cl.append((c, sl[1].start, sl[0].start, sl[1].stop, sl[0].stop))
cl.sort(reverse=True)
print("faltando >8 px", int(miss.sum()))
for c in cl[:n]:
    print(c)
F = np.asarray(Image.open(Fp).convert("RGB").point(lambda v: v // 2 + 100)).copy()
F[binary_dilation(miss)] = [0, 150, 0]
im = Image.fromarray(F)
dr = ImageDraw.Draw(im)
for c in cl[:n]:
    dr.rectangle(c[1:5], outline=(0, 0, 255), width=3)
    dr.text((c[1] + 3, c[2] + 3), str(c[0]), fill=(0, 0, 255))
im.resize((960, 540)).save(out)

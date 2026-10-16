"""Leitura: aglomerados de bordas da referência sem par a > 2 px (faltando) e do render sem par (sobrando), com o Canny replicado;
lista as caixas maiores para reler. uso: python c131_far.py tag [n] [saida.png]"""
import sys
import numpy as np
from PIL import Image, ImageDraw
from scipy.ndimage import distance_transform_edt, label, find_objects, binary_dilation
sys.argv_ = sys.argv[:]
sys.argv = ["x", "a", "b", "0,0,1,1", "o"]
exec(open(__file__.replace("c131_far.py", "canny_pair.py"), encoding="utf-8").read().split("ref, ren, box")[0])
sys.argv = sys.argv_
tag = sys.argv[1]
n = int(sys.argv[2]) if len(sys.argv) > 2 else 25
Fp = r"C:\AnimeIAI\calibracao\ref\cut_131\frames\f_0016.png"
Rp = rf"C:\AnimeIAI\sprints\registro\calibracao\C131\tentativas\{tag}_q15\q0015_render.png"
mf, _, _ = grad(Fp)
high = max(float(np.percentile(mf, 95)), 10.0)
Er, _ = canny(Fp, high, 0.4 * high)
Et, _ = canny(Rp, high, 0.4 * high)
miss = Er & (distance_transform_edt(~Et) > 2)
extra = Et & (distance_transform_edt(~Er) > 2)
both = miss | extra
lab, k = label(binary_dilation(both, iterations=4))
cl = []
for i, sl in enumerate(find_objects(lab)):
    m = lab[sl] == i + 1
    a, b = int((miss[sl] & m).sum()), int((extra[sl] & m).sum())
    cl.append((a + b, a, b, sl[1].start, sl[0].start, sl[1].stop, sl[0].stop))
cl.sort(reverse=True)
print("faltando", int(miss.sum()), "sobrando", int(extra.sum()))
for c in cl[:n]:
    print(c)
if len(sys.argv) > 3:
    F = Image.open(Fp).convert("RGB").point(lambda v: v // 2 + 100)
    o = np.asarray(F).copy()
    o[binary_dilation(miss)] = [0, 160, 0]
    o[binary_dilation(extra)] = [220, 0, 0]
    im = Image.fromarray(o)
    dr = ImageDraw.Draw(im)
    for c in cl[:n]:
        dr.rectangle(c[3:7], outline=(0, 0, 255), width=3)
        dr.text((c[3] + 3, c[4] + 3), str(c[0]), fill=(0, 0, 255))
    im.resize((960, 540)).save(sys.argv[3])

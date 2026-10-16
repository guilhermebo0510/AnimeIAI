"""Leitura: atribui a perda (bordas sem par do COMPARE + perda de SSIM em luma) a cada elemento desenhado do desenho —
linhas do cabelo/diretas (pelo traçado estimado levado pelo campo) e formas da roupa/detalhes — numa faixa de ±r px em volta.
Só ordena onde olhar; não mexe em nada. uso: python d218_atrib.py desenho tag quadro [raio]"""
import json, os, sys
import numpy as np
from PIL import Image, ImageDraw
from skimage.metrics import structural_similarity
sys.path.insert(0, os.path.dirname(__file__))
os.environ["D218_DES"] = sys.argv[1]
import meas
import d218
import d218_hair as H, d218_low as LO, d218_details as DT, d218_rim as RM

tag, fr = sys.argv[2], int(sys.argv[3])
r = int(sys.argv[4]) if len(sys.argv) > 4 else 8
base = rf"C:\AnimeIAI\sprints\registro\calibracao\D218\tentativas\{tag}_q{fr:02d}"
b = np.asarray(Image.open(os.path.join(base, f"q{fr:04d}_bordas.png")).convert("RGB")).astype(int)
bad = ((b[..., 0] == 230) & (b[..., 1] == 0)) | ((b[..., 0] == 0) & (b[..., 1] == 200) & (b[..., 2] == 0))
ref = np.asarray(Image.open(rf"C:\AnimeIAI\calibracao\ref\cut_218\frames\f_{fr + 1:04d}.png").convert("L")).astype(float)
ren = np.asarray(Image.open(os.path.join(base, f"q{fr:04d}_render.png")).convert("L")).astype(float)
_, S = structural_similarity(ref, ren, data_range=255, full=True, gaussian_weights=True, sigma=1.5, use_sample_covariance=False)
loss = 1 - S


def score(pts, closed=False):
    m = Image.new("L", (1920, 1080), 0)
    dr = ImageDraw.Draw(m)
    q = [tuple(p) for p in meas.densify([list(p) for p in pts], 4, closed=closed)]
    if closed:
        q = q + [q[0]]
    dr.line(q, fill=255, width=2 * r)
    k = np.asarray(m) > 0
    return int(bad[k].sum()), float(loss[k].sum())


rows = []
hd = H.HAIR_DES.get(int(sys.argv[1]), {})
for n, p in H.HAIR.items():
    rows.append(("linha " + n,) + score(p if n in hd else d218.W(p)))
for n, (p, w) in H.DIRECT.items():
    rows.append(("direta " + n,) + score(p if n in H.RAW else d218.W(p)))
for src, name in ((LO, "roupa"), (DT, "detalhe")):
    raw = src.SHAPES_DES.get(int(sys.argv[1]), {})
    for n, (c, p, m) in {**src.SHAPES, **raw}.items():
        rows.append((f"{name} {n}",) + score(p if n in raw else d218.W(p), closed=True))
rawc = RM.CENTERS_DES.get(int(sys.argv[1]), {})
for n, p in {**RM.CENTERS, **rawc}.items():
    if p:
        rows.append(("realce " + n,) + score(p if n in rawc else d218.W(p)))
rows.sort(key=lambda t: -(t[1] + 40 * t[2]))
print("bordas sem par total", int(bad.sum()), "| perda SSIM total", round(float(loss.sum()), 1))
for n, e, s in rows[:30]:
    print(f"{n:28s} bordas {e:5d}  perda SSIM {s:8.1f}")

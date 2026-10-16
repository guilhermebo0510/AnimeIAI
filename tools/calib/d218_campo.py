"""Leitura: campo de deslocamento esparso entre o desenho 1 e um redesenho de tremor (7/8). Em pontos de controle escolhidos
pelo agente (ao longo das estimativas das linhas e no rosto), mede o deslocamento local por correlação de uma janela da
referência 1 dentro da referência do desenho (busca limitada). Só move as ESTIMATIVAS do desenho 1 para perto das linhas
do outro desenho; a geometria final continua vindo das medições de sempre (paquímetro, bordas).
uso: python d218_campo.py desenho [--mapa saida.png] [--busca px] [--tol px]   → grava d218_campo_<desenho>.json"""
import json, os, sys
import numpy as np
from PIL import Image, ImageDraw
from skimage.feature import match_template
sys.path.insert(0, os.path.dirname(__file__))
import meas

os.environ["D218_DES"] = "1"
import d218_hair as H
import d218_rim as RM
import d218_low as LO
import d218_details as DT
from d218 import FIRST

k = int(sys.argv[1])
STEP = int(sys.argv[sys.argv.index("--passo") + 1]) if "--passo" in sys.argv else 30
TOL = float(sys.argv[sys.argv.index("--tol") + 1]) if "--tol" in sys.argv else 8.0
HALF, SEARCH, MIN_NCC = 32, int(sys.argv[sys.argv.index("--busca") + 1]) if "--busca" in sys.argv else 26, 0.85
P = r"C:\AnimeIAI\calibracao\ref\cut_218\frames\f_%04d.png"
A = np.asarray(Image.open(P % 1).convert("L")).astype(float)
B = np.asarray(Image.open(P % (FIRST[k] + 1)).convert("L")).astype(float)


def control_points():
    pts = []
    shapes = [p for c, p, m in list(LO.SHAPES.values()) + list(DT.SHAPES.values())]
    for poly in list(H.HAIR.values()) + [p for p, w in H.DIRECT.values()] + list(RM.CENTERS.values()) + shapes:
        pts += meas.densify(list(poly), STEP)
    for cx, cy, ro in [(772, 446, 117), (1288, 389, 112)]:  # anéis dos olhos
        pts += meas.circle(cx, cy, ro, n=12)
    out = []
    m = 20 + SEARCH  # perto da borda do quadro a janela encolhe (até 32 px)
    for x, y in pts:  # um ponto por célula de 40 px
        if m <= x < 1920 - m and m <= y < 1080 - m:
            if all(abs(x - a) >= STEP or abs(y - b) >= STEP for a, b in out):
                out.append((x, y))
    return out


def shift_at(x, y):
    x, y = round(x), round(y)
    h = min(HALF, x - SEARCH, 1920 - SEARCH - x, y - SEARCH, 1080 - SEARCH - y)
    t = A[y - h:y + h, x - h:x + h]
    if t.std() < 12:
        return None
    s = B[y - h - SEARCH:y + h + SEARCH, x - h - SEARCH:x + h + SEARCH]
    r = match_template(s, t)
    iy, ix = np.unravel_index(np.argmax(r), r.shape)
    peak = r[iy, ix]
    if peak < MIN_NCC or ix in (0, r.shape[1] - 1) or iy in (0, r.shape[0] - 1):
        return None
    # refinamento subpixel (parábola)
    def sub(a, b, c):
        den = a - 2 * b + c
        return 0.0 if abs(den) < 1e-9 else 0.5 * (a - c) / den
    dx = ix - SEARCH + sub(r[iy, ix - 1], peak, r[iy, ix + 1])
    dy = iy - SEARCH + sub(r[iy - 1, ix], peak, r[iy + 1, ix])
    return float(dx), float(dy), float(peak)


rows = []
for x, y in control_points():
    s = shift_at(x, y)
    if s:
        rows.append([round(x, 1), round(y, 1), round(s[0], 2), round(s[1], 2), round(s[2], 3)])
# descarta pontos que discordam muito dos vizinhos (janela pegou outra linha parecida)
arr = np.array(rows)
keep = []
for i, (x, y, dx, dy, c) in enumerate(arr):
    dd = np.hypot(arr[:, 0] - x, arr[:, 1] - y)
    nb = (dd < 160) & (dd > 0)
    if nb.sum() >= 2:
        mx, my = np.median(arr[nb, 2]), np.median(arr[nb, 3])
        if np.hypot(dx - mx, dy - my) > TOL:
            continue
    keep.append(rows[i])
out = os.path.join(os.path.dirname(__file__), f"d218_campo_{k}.json")
json.dump(keep, open(out, "w"))
print(len(rows), "medidos,", len(keep), "mantidos →", out)
if "--mapa" in sys.argv:
    im = Image.open(P % (FIRST[k] + 1)).convert("RGB")
    dr = ImageDraw.Draw(im)
    for x, y, dx, dy, c in keep:
        dr.line([(x, y), (x + dx * 4, y + dy * 4)], fill=(0, 255, 255), width=3)
        dr.ellipse([x - 3, y - 3, x + 3, y + 3], fill=(255, 255, 0))
    im.resize((960, 540)).save(sys.argv[sys.argv.index("--mapa") + 1])

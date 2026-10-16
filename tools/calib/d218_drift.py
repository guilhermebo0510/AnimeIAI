"""Leitura (diagnóstico): quanto cada estimativa do desenho 1 (tabelas do cabelo e do realce) fica longe da tinta / do realce
na referência de outro desenho. Só aponta quais curvas o agente precisa reler; não gera geometria.
uso: python d218_drift.py desenho [limiar_px]"""
import os, sys
import numpy as np
from PIL import Image
from scipy.ndimage import distance_transform_edt
sys.path.insert(0, os.path.dirname(__file__))
import meas

os.environ.setdefault("D218_DES", "1")
import d218_hair as H
import d218_rim as RM
from d218 import FIRST

k = int(sys.argv[1])
lim = float(sys.argv[2]) if len(sys.argv) > 2 else 6.0


def dist_map(frame, color, tol):
    a = np.asarray(Image.open(rf"C:\AnimeIAI\calibracao\ref\cut_218\frames\f_{frame + 1:04d}.png").convert("RGB")).astype(float)
    c = np.array([int(color[i:i + 2], 16) for i in (1, 3, 5)], float)
    mask = np.sqrt(((a - c) ** 2).sum(-1)) < tol
    return distance_transform_edt(~mask)


def report(table, dm0, dmk, title):
    print("--", title)
    rows = []
    for name, poly in table.items():
        pts = meas.densify(list(poly), 4)
        d0 = np.array([dm0[min(1079, max(0, round(y))), min(1919, max(0, round(x)))] for x, y in pts])
        dk = np.array([dmk[min(1079, max(0, round(y))), min(1919, max(0, round(x)))] for x, y in pts])
        rows.append((float(np.percentile(dk, 90)), name, round(float(np.median(d0)), 1), round(float(np.median(dk)), 1), round(float(dk.max()), 1)))
    for p90, name, m0, mk, mx in sorted(rows, reverse=True):
        flag = "RELER" if p90 > lim else ""
        print(f"{name:22s} des1 med {m0:5} | des{k} med {mk:5} p90 {p90:5.1f} max {mx:5} {flag}")


ink0, inkk = dist_map(0, "#5c2e2e", 55), dist_map(FIRST[k], "#5c2e2e", 55)
report(H.HAIR, ink0, inkk, "cabelo (tabela HAIR)")
report({n: p for n, (p, w) in H.DIRECT.items()}, ink0, inkk, "linhas diretas")
rim0, rimk = dist_map(0, RM.RIM, 40), dist_map(FIRST[k], RM.RIM, 40)
report(RM.CENTERS, rim0, rimk, "realces")
